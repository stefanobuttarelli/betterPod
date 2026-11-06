//! Video and Listen share a bounded PCM transport. Audio consumption is the
//! presentation clock. Network, probing and decode never run on the UI thread.
use crate::online::StreamSource;
use crate::pcm::{PcmOutput, Ring};
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex, RwLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum VideoCommand {
    Open {
        source: StreamSource,
        start: f64,
        audio_only: bool,
    },
    Pause(bool),
    Seek(f64),
    Volume(f32),
    Stop,
}
#[derive(Clone, Debug, Default)]
pub struct VideoSnapshot {
    pub playing: bool,
    pub paused: bool,
    pub loading: bool,
    pub ended: bool,
    pub error: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub title: String,
    pub audio_only: bool,
    pub generation: u64,
    pub underruns: u64,
}
pub struct VideoFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub pts: f64,
    pub generation: u64,
    recycle: mpsc::SyncSender<Vec<u8>>,
}
impl Drop for VideoFrame {
    fn drop(&mut self) {
        let _ = self.recycle.try_send(std::mem::take(&mut self.pixels));
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct VideoTelemetry {
    pub playing: bool,
    pub loading: bool,
    pub failed: bool,
    pub position: f64,
    pub frames: u64,
    pub frame_pts: f64,
    pub underruns: u64,
    pub generation: u64,
}
pub struct VideoPlayer {
    tx: mpsc::SyncSender<(u64, VideoCommand)>,
    rx: mpsc::Receiver<VideoFrame>,
    shared: Arc<RwLock<VideoSnapshot>>,
    cached: RefCell<VideoSnapshot>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    generation: Arc<AtomicU64>,
    mute: Arc<AtomicBool>,
    control: Arc<Mutex<()>>,
    desired_pause: Arc<AtomicBool>,
    shown: Cell<u64>,
    shown_pts: Cell<f64>,
}
impl VideoPlayer {
    pub fn new() -> Result<Self, String> {
        Self::start(None)
    }
    /// Resolve the optional private decoder on the worker for every opening,
    /// including after installing the tool while this transport is alive.
    pub fn with_data(data: PathBuf) -> Result<Self, String> {
        Self::start(Some(data))
    }
    fn start(data: Option<PathBuf>) -> Result<Self, String> {
        let (tx, commands) = mpsc::sync_channel(16);
        let (frames, rx) = mpsc::sync_channel(2);
        let shared = Arc::new(RwLock::new(VideoSnapshot::default()));
        let published = shared.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let requested = generation.clone();
        let mute = Arc::new(AtomicBool::new(true));
        let gate = mute.clone();
        let control = Arc::new(Mutex::new(()));
        let worker_control = control.clone();
        let desired_pause = Arc::new(AtomicBool::new(false));
        let worker_pause = desired_pause.clone();
        let worker = thread::Builder::new()
            .name("video-control".into())
            .spawn(move || {
                worker(
                    commands,
                    frames,
                    published,
                    stopped,
                    requested,
                    gate,
                    worker_control,
                    worker_pause,
                    data,
                )
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx,
            rx,
            shared,
            cached: RefCell::new(VideoSnapshot::default()),
            stop,
            worker: Some(worker),
            generation,
            mute,
            control,
            desired_pause,
            shown: Cell::new(0),
            shown_pts: Cell::new(0.),
        })
    }
    pub fn command(&self, command: VideoCommand) -> bool {
        if self.stop.load(Ordering::Acquire)
            || match &command {
                VideoCommand::Open { start, .. } | VideoCommand::Seek(start) => {
                    !start.is_finite() || *start < 0.0
                }
                VideoCommand::Volume(volume) => !volume.is_finite(),
                _ => false,
            }
        {
            return false;
        }
        // The worker takes this tiny gate before observing the accepted
        // generation. A failed enqueue never changes the running transport.
        let Ok(_guard) = self.control.try_lock() else {
            return false;
        };
        let interrupts = matches!(
            &command,
            VideoCommand::Open { .. } | VideoCommand::Stop | VideoCommand::Seek(_)
        );
        let fresh = matches!(&command, VideoCommand::Open { .. } | VideoCommand::Stop);
        let pause = if let VideoCommand::Pause(p) = &command {
            Some(*p)
        } else {
            None
        };
        let generation = self
            .generation
            .load(Ordering::Acquire)
            .wrapping_add(u64::from(interrupts));
        if self.tx.try_send((generation, command)).is_err() {
            return false;
        }
        if interrupts {
            self.generation.store(generation, Ordering::Release);
            self.mute.store(true, Ordering::Release);
        }
        if fresh {
            self.desired_pause.store(false, Ordering::Release);
        }
        if let Some(paused) = pause {
            self.desired_pause.store(paused, Ordering::Release);
            self.mute.store(paused, Ordering::Release);
        }
        true
    }
    pub fn snapshot(&self) -> VideoSnapshot {
        if let Ok(s) = self.shared.try_read() {
            *self.cached.borrow_mut() = s.clone();
        }
        self.cached.borrow().clone()
    }
    pub fn frame(&self) -> Option<VideoFrame> {
        let generation = self.snapshot().generation;
        let mut latest = None;
        while let Ok(frame) = self.rx.try_recv() {
            if frame.generation == generation {
                latest = Some(frame);
            }
        }
        if let Some(frame) = latest.as_ref() {
            self.shown.set(self.shown.get().wrapping_add(1));
            self.shown_pts.set(frame.pts);
        }
        latest
    }
    pub fn telemetry(&self) -> VideoTelemetry {
        let make = |s: &VideoSnapshot| VideoTelemetry {
            playing: s.playing,
            loading: s.loading,
            failed: s.error.is_some(),
            position: s.position,
            frames: self.shown.get(),
            frame_pts: self.shown_pts.get(),
            underruns: s.underruns,
            generation: s.generation,
        };
        if let Ok(s) = self.shared.try_read() {
            make(&s)
        } else {
            make(&self.cached.borrow())
        }
    }
    pub fn shutdown(&mut self) {
        self.mute.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for VideoPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}
struct Session {
    source: StreamSource,
    start: f64,
    audio_only: bool,
    output: Option<PcmOutput>,
    audio_child: Option<Child>,
    video_child: Option<Child>,
    cancel: Arc<AtomicBool>,
    audio_worker: Option<JoinHandle<()>>,
    stderr_worker: Option<JoinHandle<()>>,
    audio_error: Arc<Mutex<Vec<u8>>>,
    video_worker: Option<JoinHandle<()>>,
    frames: mpsc::Receiver<VideoFrame>,
    pending: Option<VideoFrame>,
    proxies: Vec<crate::stream_proxy::StreamProxy>,
    clock: Instant,
    pause_at: f64,
    paused: bool,
    video_eof: Arc<AtomicBool>,
}
impl Session {
    fn open(
        source: StreamSource,
        start: f64,
        audio_only: bool,
        generation: u64,
        volume: f32,
        stop: &AtomicBool,
        requested: &AtomicU64,
        mute: Arc<AtomicBool>,
        executable: &Path,
    ) -> Result<Self, String> {
        if !start.is_finite() || start < 0.0 {
            return Err("Invalid media start time".into());
        }
        let start = if source.duration.is_finite() && source.duration > 0.0 {
            start.min(source.duration)
        } else {
            start
        };
        let (frame_tx, frames) = mpsc::sync_channel(8);
        let (free_tx, free_rx) = mpsc::sync_channel::<Vec<u8>>(12);
        // Construct the owner before spawning anything. Every subsequent '?'
        // kills/reaps children and joins readers through Session::drop.
        let mut session = Self {
            source,
            start,
            audio_only,
            output: None,
            audio_child: None,
            video_child: None,
            cancel: Arc::new(AtomicBool::new(false)),
            audio_worker: None,
            stderr_worker: None,
            audio_error: Arc::new(Mutex::new(Vec::new())),
            video_worker: None,
            frames,
            pending: None,
            proxies: Vec::new(),
            clock: Instant::now(),
            pause_at: start,
            paused: true,
            video_eof: Arc::new(AtomicBool::new(false)),
        };
        let video = proxy_url(
            &session.source.video_url,
            &session.source.headers,
            &mut session.proxies,
        )?;
        let audio_source = session
            .source
            .audio_url
            .as_deref()
            .unwrap_or(&session.source.video_url);
        let audio = if audio_source == session.source.video_url {
            video.clone()
        } else {
            proxy_url(audio_source, &session.source.headers, &mut session.proxies)?
        };
        let mut command = Command::new(executable);
        input_args(&mut command, &audio, start);
        command
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-sn",
                "-dn",
                "-af",
                "aresample=async=1:first_pts=0",
                "-ac",
                "2",
                "-ar",
                "48000",
                "-f",
                "f32le",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        session.audio_child = Some(
            command
                .spawn()
                .map_err(|e| format!("Video decoder unavailable: {e}"))?,
        );
        let child = session.audio_child.as_mut().unwrap();
        let mut stdout = child.stdout.take().ok_or("Audio pipe unavailable")?;
        let mut stderr = child.stderr.take().ok_or("Audio error pipe unavailable")?;
        let errors = session.audio_error.clone();
        session.stderr_worker = Some(
            thread::Builder::new()
                .name("video-audio-errors".into())
                .spawn(move || {
                    let mut bytes = [0u8; 1024];
                    loop {
                        match stderr.read(&mut bytes) {
                            Ok(0) => break,
                            Ok(n) => {
                                if let Ok(mut output) = errors.lock() {
                                    let room = 16384usize.saturating_sub(output.len());
                                    output.extend_from_slice(&bytes[..n.min(room)]);
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|e| e.to_string())?,
        );
        let output = PcmOutput::new_gated(48000, mute)?;
        output.ring.volume(volume);
        let ring = output.ring.clone();
        let cancelled = session.cancel.clone();
        session.output = Some(output);
        session.audio_worker = Some(
            thread::Builder::new()
                .name("video-audio".into())
                .spawn(move || read_pcm(&mut stdout, &ring, &cancelled))
                .map_err(|e| e.to_string())?,
        );
        if !audio_only {
            let mut command = Command::new(&executable);
            input_args(&mut command, &video, start);
            command.args(["-map","0:v:0","-an","-sn","-dn","-vf","scale=640:480:force_original_aspect_ratio=decrease,pad=640:480:(ow-iw)/2:(oh-ih)/2:color=black,fps=30","-pix_fmt","rgba","-f","rawvideo","pipe:1"])
                .stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
            session.video_child = Some(command.spawn().map_err(|e| e.to_string())?);
            let mut stdout = session
                .video_child
                .as_mut()
                .unwrap()
                .stdout
                .take()
                .ok_or("Video pipe unavailable")?;
            let cancelled = session.cancel.clone();
            let eof = session.video_eof.clone();
            session.video_worker = Some(
                thread::Builder::new()
                    .name("video-frames".into())
                    .spawn(move || {
                        let mut index = 0u64;
                        while !cancelled.load(Ordering::Acquire) {
                            let mut pixels = free_rx
                                .try_recv()
                                .unwrap_or_else(|_| vec![0u8; 640 * 480 * 4]);
                            pixels.resize(640 * 480 * 4, 0);
                            if stdout.read_exact(&mut pixels).is_err() {
                                break;
                            }
                            let mut frame = VideoFrame {
                                pixels,
                                width: 640,
                                height: 480,
                                pts: start + index as f64 / 30.0,
                                generation,
                                recycle: free_tx.clone(),
                            };
                            index += 1;
                            loop {
                                match frame_tx.try_send(frame) {
                                    Ok(()) => break,
                                    Err(mpsc::TrySendError::Full(returned)) => {
                                        frame = returned;
                                        if cancelled.load(Ordering::Acquire) {
                                            return;
                                        }
                                        thread::sleep(Duration::from_millis(3));
                                    }
                                    Err(_) => return,
                                }
                            }
                        }
                        eof.store(true, Ordering::Release);
                    })
                    .map_err(|e| e.to_string())?,
            );
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while session.output.as_ref().is_some_and(|o| !o.ready()) {
            if stop.load(Ordering::Acquire) || requested.load(Ordering::Acquire) != generation {
                return Err("Media opening was cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Media did not buffer in time".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        if session
            .output
            .as_ref()
            .is_some_and(|o| o.ring.buffered() == 0)
        {
            // A deliberately silent video is allowed only when ffmpeg proves
            // that the muxed source has no audio stream. An invalid separate
            // audio URL or a decoder/network failure must be visible.
            let deadline = Instant::now() + Duration::from_millis(500);
            let status = loop {
                match session.audio_child.as_mut().unwrap().try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Err(e) => return Err(e.to_string()),
                    Ok(None) => {
                        if Instant::now() >= deadline
                            || stop.load(Ordering::Acquire)
                            || requested.load(Ordering::Acquire) != generation
                        {
                            break None;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            };
            if status.is_some() {
                if let Some(worker) = session.stderr_worker.take() {
                    let _ = worker.join();
                }
            }
            let diagnostic = String::from_utf8_lossy(
                &session
                    .audio_error
                    .lock()
                    .map_err(|_| "Audio error state unavailable")?,
            )
            .into_owned();
            let silent = !audio_only
                && session.source.audio_url.is_none()
                && diagnostic.contains("matches no streams");
            if !silent {
                return Err(format!(
                    "No audio samples were decoded{}",
                    if diagnostic.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", diagnostic.trim())
                    }
                ));
            }
            session.output = None;
        }
        if session
            .output
            .as_ref()
            .is_some_and(|o| o.ring.failed.load(Ordering::Acquire))
        {
            return Err("Audio decode failed while buffering".into());
        }
        if session.output.is_some() {
            if let Some(status) = session
                .audio_child
                .as_mut()
                .unwrap()
                .try_wait()
                .map_err(|e| e.to_string())?
            {
                if !status.success() {
                    return Err("Audio decoder stopped while buffering".into());
                }
            }
        }
        Ok(session)
    }
    fn position(&self) -> f64 {
        if self.paused {
            self.pause_at
        } else if let Some(output) = &self.output {
            self.start + output.position()
        } else {
            self.start + self.clock.elapsed().as_secs_f64()
        }
    }
    fn pause(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        if paused {
            self.pause_at = self.position();
        } else {
            self.start = self.pause_at;
            self.clock = Instant::now();
            if self.output.is_some() {
                /* PCM retains its own sample position. */
                self.start -= self.output.as_ref().unwrap().position();
            }
        }
        self.paused = paused;
        if let Some(output) = &mut self.output {
            output.pause(paused);
        }
    }
    fn due(&mut self) -> Option<VideoFrame> {
        if self.paused {
            return None;
        }
        let position = self.position();
        let mut due = None;
        for _ in 0..12 {
            let frame = match self.pending.take() {
                Some(frame) => frame,
                None => match self.frames.try_recv() {
                    Ok(frame) => frame,
                    Err(_) => break,
                },
            };
            if frame.pts <= position + 0.003 {
                due = Some(frame);
            } else {
                self.pending = Some(frame);
                break;
            }
        }
        due
    }
    fn ended(&mut self) -> bool {
        if let Some(output) = &self.output {
            output.ended()
        } else if self.video_eof.load(Ordering::Acquire) && self.pending.is_none() {
            match self.frames.try_recv() {
                Ok(frame) => {
                    self.pending = Some(frame);
                    false
                }
                Err(_) => true,
            }
        } else {
            false
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(output) = &mut self.output {
            output.pause(true);
        }
        for child in [&mut self.audio_child, &mut self.video_child]
            .into_iter()
            .flatten()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.audio_worker.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_worker.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.video_worker.take() {
            let _ = reader.join();
        }
        self.proxies.clear();
    }
}
fn worker(
    commands: mpsc::Receiver<(u64, VideoCommand)>,
    frames: mpsc::SyncSender<VideoFrame>,
    shared: Arc<RwLock<VideoSnapshot>>,
    stop: Arc<AtomicBool>,
    requested: Arc<AtomicU64>,
    mute: Arc<AtomicBool>,
    control: Arc<Mutex<()>>,
    desired_pause: Arc<AtomicBool>,
    data: Option<PathBuf>,
) {
    let mut state = VideoSnapshot::default();
    let mut session: Option<Session> = None;
    let mut volume = 1.0;
    let mut last_source: Option<(StreamSource, bool)> = None;
    while !stop.load(Ordering::Acquire) {
        let received = commands.recv_timeout(Duration::from_millis(8));
        // Synchronize with successful enqueue/generation commit; never hold
        // this gate across probing, decoding, process cleanup or network I/O.
        let current = {
            let _guard = control.lock().unwrap_or_else(|e| e.into_inner());
            requested.load(Ordering::Acquire)
        };
        let mut opening = None;
        match received {
            Ok((
                generation,
                VideoCommand::Open {
                    source,
                    start,
                    audio_only,
                },
            )) => {
                // Preserve the source even if a newer queued Seek superseded
                // this opening before the worker could start it.
                last_source = Some((source.clone(), audio_only));
                if generation == current {
                    session.take();
                    opening = Some((source, start, audio_only, generation));
                }
            }
            Ok((generation, VideoCommand::Seek(position))) => {
                if generation == current {
                    let source = last_source
                        .clone()
                        .or_else(|| session.as_ref().map(|s| (s.source.clone(), s.audio_only)));
                    session.take();
                    if let Some((source, audio_only)) = source {
                        opening = Some((source, position, audio_only, generation));
                    }
                }
            }
            Ok((generation, VideoCommand::Pause(paused))) => {
                if generation == current {
                    if let Some(session) = &mut session {
                        session.pause(paused);
                        state.paused = paused;
                        state.playing = !paused;
                    }
                }
            }
            Ok((_, VideoCommand::Volume(value))) => {
                volume = value.clamp(0.0, 1.0);
                if let Some(output) = session.as_ref().and_then(|s| s.output.as_ref()) {
                    output.ring.volume(volume);
                }
            }
            Ok((generation, VideoCommand::Stop)) => {
                if generation == current {
                    session.take();
                    last_source = None;
                    state = VideoSnapshot {
                        generation,
                        ..VideoSnapshot::default()
                    };
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some((source, start, audio_only, generation)) = opening {
            state = VideoSnapshot {
                title: source.title.clone(),
                duration: if source.duration.is_finite() {
                    source.duration.max(0.0)
                } else {
                    0.0
                },
                loading: true,
                audio_only,
                generation,
                ..VideoSnapshot::default()
            };
            publish(&shared, &state);
            let executable = data
                .as_deref()
                .map(crate::downloads::ffmpeg_path)
                .unwrap_or_else(|| {
                    std::env::var_os("BETTERPOD_RUST_FFMPEG")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
                });
            match Session::open(
                source,
                start,
                audio_only,
                generation,
                volume,
                &stop,
                &requested,
                mute.clone(),
                &executable,
            ) {
                Ok(mut open) => {
                    let accepted = {
                        let _guard = control.lock().unwrap_or_else(|e| e.into_inner());
                        if generation == requested.load(Ordering::Acquire)
                            && !stop.load(Ordering::Acquire)
                        {
                            let paused = desired_pause.load(Ordering::Acquire);
                            open.pause(paused);
                            state.loading = false;
                            state.paused = paused;
                            state.playing = !paused;
                            mute.store(paused, Ordering::Release);
                            true
                        } else {
                            false
                        }
                    };
                    if accepted {
                        session = Some(open);
                    }
                }
                Err(error) => {
                    state.playing = false;
                    if generation == requested.load(Ordering::Acquire) {
                        state.loading = false;
                        state.error = Some(error);
                    } else {
                        state.loading = true;
                        state.error = None;
                    }
                }
            }
        }
        if let Some(session) = &mut session {
            state.position = session.position();
            if state.duration > 0.0 {
                state.position = state.position.min(state.duration);
            }
            if let Some(frame) = session.due() {
                let _ = frames.try_send(frame);
            }
            if let Some(output) = &session.output {
                state.underruns = output.ring.underruns.load(Ordering::Relaxed);
                if output.ring.failed.load(Ordering::Acquire) {
                    state.error = Some("Audio decode failed".into());
                    state.playing = false;
                    mute.store(true, Ordering::Release);
                }
            }
            if !state.paused && session.ended() {
                state.ended = true;
                state.playing = false;
            }
            if let Some(child) = &mut session.video_child {
                if let Ok(Some(status)) = child.try_wait() {
                    if !status.success() && !session.cancel.load(Ordering::Acquire) {
                        state.error =
                            Some("Video decoder stopped; refresh the stream or try again".into());
                        state.playing = false;
                        mute.store(true, Ordering::Release);
                    }
                }
            }
            if let Some(child) = &mut session.audio_child {
                if let Ok(Some(status)) = child.try_wait() {
                    if !status.success()
                        && session.output.is_some()
                        && !session.cancel.load(Ordering::Acquire)
                    {
                        state.error =
                            Some("Audio decoder stopped; refresh the stream or try again".into());
                        state.playing = false;
                        mute.store(true, Ordering::Release);
                    }
                }
            }
        }
        publish(&shared, &state);
    }
    session.take();
}
fn publish(shared: &RwLock<VideoSnapshot>, state: &VideoSnapshot) {
    if let Ok(mut snapshot) = shared.write() {
        *snapshot = state.clone();
    }
}
fn input_args(command: &mut Command, url: &str, start: f64) {
    command.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-threads",
        "2",
    ]);
    if start > 0.0 {
        command.args(["-ss", &format!("{start:.6}")]);
    }
    if url.starts_with("http://") {
        command.args([
            "-reconnect",
            "1",
            "-reconnect_streamed",
            "1",
            "-reconnect_delay_max",
            "2",
        ]);
    }
    command.args(["-i", url]);
}
fn read_pcm(stdout: &mut impl Read, ring: &Ring, cancel: &AtomicBool) {
    let mut bytes = [0u8; 32768];
    let mut carry = Vec::with_capacity(32776);
    let mut samples = Vec::with_capacity(8192);
    while !cancel.load(Ordering::Acquire) {
        let n = match stdout.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                ring.failed.store(true, Ordering::Release);
                break;
            }
        };
        carry.extend_from_slice(&bytes[..n]);
        let usable = carry.len() / 8 * 8;
        samples.clear();
        for value in carry[..usable].chunks_exact(4) {
            samples.push(f32::from_le_bytes(value.try_into().unwrap()));
        }
        if !ring.write(&samples, cancel) {
            break;
        }
        carry.drain(..usable);
    }
    if !carry.is_empty() && !cancel.load(Ordering::Acquire) {
        ring.failed.store(true, Ordering::Release);
    }
    ring.eof.store(true, Ordering::Release);
}
fn proxy_url(
    url: &str,
    headers: &[(String, String)],
    proxies: &mut Vec<crate::stream_proxy::StreamProxy>,
) -> Result<String, String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        let proxy = crate::stream_proxy::StreamProxy::new(url.to_owned(), headers.to_vec())?;
        let local = proxy.url();
        proxies.push(proxy);
        Ok(local)
    } else {
        Ok(url.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn player() -> (VideoPlayer, mpsc::Receiver<(u64, VideoCommand)>) {
        let (tx, commands) = mpsc::sync_channel(1);
        let (_frames, rx) = mpsc::sync_channel(1);
        (
            VideoPlayer {
                tx,
                rx,
                shared: Arc::new(RwLock::new(VideoSnapshot::default())),
                cached: RefCell::new(VideoSnapshot::default()),
                stop: Arc::new(AtomicBool::new(false)),
                worker: None,
                generation: Arc::new(AtomicU64::new(7)),
                mute: Arc::new(AtomicBool::new(false)),
                control: Arc::new(Mutex::new(())),
                desired_pause: Arc::new(AtomicBool::new(false)),
                shown: Cell::new(0),
                shown_pts: Cell::new(0.),
            },
            commands,
        )
    }
    #[test]
    fn rejected_queue_command_never_mutes_or_changes_generation() {
        let (p, _commands) = player();
        assert!(p.command(VideoCommand::Volume(0.5)));
        assert!(!p.command(VideoCommand::Seek(2.0)));
        assert!(!p.command(VideoCommand::Pause(true)));
        assert!(!p.command(VideoCommand::Stop));
        assert_eq!(p.generation.load(Ordering::Acquire), 7);
        assert!(!p.mute.load(Ordering::Acquire));
        assert!(!p.desired_pause.load(Ordering::Acquire));
    }
    #[test]
    fn contention_and_invalid_values_leave_transport_unchanged() {
        let (p, _commands) = player();
        let _guard = p.control.lock().unwrap();
        assert!(!p.command(VideoCommand::Stop));
        drop(_guard);
        for command in [
            VideoCommand::Seek(f64::NAN),
            VideoCommand::Seek(f64::INFINITY),
            VideoCommand::Seek(-1.0),
            VideoCommand::Volume(f32::NAN),
        ] {
            assert!(!p.command(command));
        }
        assert_eq!(p.generation.load(Ordering::Acquire), 7);
        assert!(!p.mute.load(Ordering::Acquire));
    }
    #[test]
    fn accepted_commands_publish_matching_generation_and_pause_intent() {
        let (p, commands) = player();
        assert!(p.command(VideoCommand::Seek(1.0)));
        assert_eq!(commands.try_recv().unwrap().0, 8);
        assert_eq!(p.generation.load(Ordering::Acquire), 8);
        assert!(p.mute.load(Ordering::Acquire));
        assert!(p.command(VideoCommand::Pause(true)));
        assert_eq!(commands.try_recv().unwrap().0, 8);
        assert!(p.desired_pause.load(Ordering::Acquire));
    }
}
