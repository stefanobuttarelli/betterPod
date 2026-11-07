//! Bounded stereo PCM transport for Rust decoders and video. The SDL callback
//! only reads atomics and samples; all decoding and I/O runs on producer threads.
use sdl2::sys;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

const CAPACITY: usize = 262144; // samples, not bytes: about 1.5s stereo at 88.2kHz
pub struct Ring {
    samples: Box<[AtomicU32]>,
    read: AtomicU64,
    write: AtomicU64,
    pub eof: AtomicBool,
    pub failed: AtomicBool,
    pub consumed: AtomicU64,
    pub underruns: AtomicU64,
    volume: AtomicU32,
    observe: AtomicBool,
    callback_start: AtomicU64,
    callback_tick: AtomicU64,
    mute: Arc<AtomicBool>,
}
impl Ring {
    pub fn new() -> Arc<Self> {
        Self::with_gate(Arc::new(AtomicBool::new(false)))
    }
    fn with_gate(mute: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            samples: (0..CAPACITY).map(|_| AtomicU32::new(0)).collect(),
            read: AtomicU64::new(0),
            write: AtomicU64::new(0),
            eof: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            consumed: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            volume: AtomicU32::new(1.0f32.to_bits()),
            observe: AtomicBool::new(false),
            callback_start: AtomicU64::new(0),
            callback_tick: AtomicU64::new(0),
            mute,
        })
    }
    pub fn buffered(&self) -> usize {
        self.write
            .load(Ordering::Acquire)
            .saturating_sub(self.read.load(Ordering::Acquire)) as usize
    }
    pub fn volume(&self, value: f32) {
        self.volume
            .store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn write(&self, samples: &[f32], cancel: &AtomicBool) -> bool {
        let mut offset = 0;
        while offset < samples.len() {
            if cancel.load(Ordering::Acquire) {
                return false;
            }
            let read = self.read.load(Ordering::Acquire);
            let write = self.write.load(Ordering::Relaxed);
            let free = CAPACITY.saturating_sub((write - read) as usize);
            let n = free.min(samples.len() - offset);
            if n == 0 {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            for (i, value) in samples[offset..offset + n].iter().enumerate() {
                self.samples[(write as usize + i) & (CAPACITY - 1)]
                    .store(value.to_bits(), Ordering::Relaxed);
            }
            self.write.store(write + n as u64, Ordering::Release);
            offset += n;
        }
        true
    }
    fn fill(&self, output: &mut [f32]) {
        self.callback_start
            .store(self.consumed.load(Ordering::Relaxed), Ordering::Relaxed);
        self.callback_tick.store(
            unsafe { sys::SDL_GetPerformanceCounter() },
            Ordering::Release,
        );
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        let n = (write - read) as usize;
        let n = n.min(output.len()) & !1;
        let volume = if self.mute.load(Ordering::Relaxed) {
            0.0
        } else {
            f32::from_bits(self.volume.load(Ordering::Relaxed))
        };
        for (i, value) in output[..n].iter_mut().enumerate() {
            *value = f32::from_bits(
                self.samples[(read as usize + i) & (CAPACITY - 1)].load(Ordering::Relaxed),
            ) * volume;
        }
        output[n..].fill(0.0);
        self.read.store(read + n as u64, Ordering::Release);
        self.consumed.fetch_add((n / 2) as u64, Ordering::Relaxed);
        if n < output.len() && !self.eof.load(Ordering::Acquire) {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}
unsafe extern "C" fn callback(data: *mut libc::c_void, bytes: *mut u8, len: libc::c_int) {
    if data.is_null() || bytes.is_null() || len <= 0 {
        return;
    }
    let ring = &*(data as *const Arc<Ring>);
    let output = std::slice::from_raw_parts_mut(bytes.cast::<f32>(), len as usize / 4);
    ring.fill(output);
    if ring.observe.load(Ordering::Relaxed) {
        crate::audio::observe_pcm(output);
    }
}
pub struct PcmOutput {
    pub ring: Arc<Ring>,
    device: sys::SDL_AudioDeviceID,
    userdata: *mut Arc<Ring>,
    pub rate: u32,
    pub chunk_frames: u32,
    started: bool,
    paused_position: f64,
}
impl PcmOutput {
    pub fn new(rate: u32) -> Result<Self, String> {
        Self::new_gated(rate, Arc::new(AtomicBool::new(false)))
    }
    pub fn new_gated(rate: u32, mute: Arc<AtomicBool>) -> Result<Self, String> {
        if !(8000..=384000).contains(&rate) {
            return Err("Unsupported PCM sample rate".into());
        }
        let ring = Ring::with_gate(mute);
        let userdata = Box::into_raw(Box::new(ring.clone()));
        let mut desired: sys::SDL_AudioSpec = unsafe { std::mem::zeroed() };
        let mut obtained = desired;
        desired.freq = rate as i32;
        desired.format = sys::AUDIO_F32SYS as u16;
        desired.channels = 2;
        desired.samples = if rate > 96000 {
            16384
        } else if rate > 48000 {
            8192
        } else {
            4096
        };
        desired.callback = Some(callback);
        desired.userdata = userdata.cast();
        let device =
            unsafe { sys::SDL_OpenAudioDevice(std::ptr::null(), 0, &desired, &mut obtained, 0) };
        if device == 0 {
            unsafe {
                drop(Box::from_raw(userdata));
            }
            return Err(format!("SDL PCM output: {}", sdl2::get_error()));
        }
        Ok(Self {
            ring,
            device,
            userdata,
            rate: obtained.freq as u32,
            chunk_frames: obtained.samples as u32,
            started: false,
            paused_position: 0.0,
        })
    }
    pub fn start(&mut self) {
        unsafe {
            sys::SDL_PauseAudioDevice(self.device, 0);
        }
        self.started = true;
    }
    pub fn pause(&mut self, value: bool) {
        if value && self.started {
            self.paused_position = self.position();
        }
        if !value && !self.started {
            // Paused wall time is not media time. Until the next callback,
            // interpolate from the exact position captured before pausing.
            self.ring.callback_start.store(
                (self.paused_position * self.rate as f64).round() as u64,
                Ordering::Relaxed,
            );
            self.ring.callback_tick.store(
                unsafe { sys::SDL_GetPerformanceCounter() },
                Ordering::Release,
            );
        }
        unsafe {
            sys::SDL_PauseAudioDevice(self.device, i32::from(value));
        }
        self.started = !value;
        if self.ring.observe.load(Ordering::Relaxed) {
            crate::audio::reset_callback_clock();
        }
    }
    pub fn position(&self) -> f64 {
        if !self.started {
            return self.paused_position;
        }
        let tick = self.ring.callback_tick.load(Ordering::Acquire);
        if tick == 0 {
            return 0.0;
        }
        let start = self.ring.callback_start.load(Ordering::Relaxed);
        let elapsed = unsafe { sys::SDL_GetPerformanceCounter() }.saturating_sub(tick) as f64
            / unsafe { sys::SDL_GetPerformanceFrequency() }.max(1) as f64;
        let consumed = self.ring.consumed.load(Ordering::Acquire) as f64;
        (start as f64 + elapsed * self.rate as f64).min(consumed) / self.rate as f64
    }
    pub fn ended(&self) -> bool {
        self.started
            && self.ring.eof.load(Ordering::Acquire)
            && self.ring.buffered() == 0
            && self.position() + 0.0001
                >= self.ring.consumed.load(Ordering::Acquire) as f64 / self.rate as f64
    }
    pub fn ready(&self) -> bool {
        self.ring.buffered() >= self.chunk_frames as usize * 4
            || self.ring.eof.load(Ordering::Acquire)
    }
}
impl Drop for PcmOutput {
    fn drop(&mut self) {
        unsafe {
            sys::SDL_CloseAudioDevice(self.device);
            drop(Box::from_raw(self.userdata));
        }
    }
}

pub struct RustTrack {
    pub output: PcmOutput,
    pub path: PathBuf,
    pub duration: f64,
    pub start: f64,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    child: Option<std::process::Child>,
}
impl RustTrack {
    pub fn open(path: &Path, start: f64) -> Result<Self, String> {
        Self::open_symphonia(path, start).or_else(|native| {
            Self::open_ffmpeg(path, start).map_err(|fallback| format!("{native}; {fallback}"))
        })
    }
    fn open_symphonia(path: &Path, start: f64) -> Result<Self, String> {
        let mut source = open_source(path)?;
        let track = source.default_track().ok_or("No audio track")?;
        let id = track.id;
        let parameters = track.codec_params.clone();
        let rate = parameters.sample_rate.ok_or("Missing native sample rate")?;
        let channels = parameters
            .channels
            .map(|c| c.count())
            .or_else(|| {
                if parameters.codec == symphonia::core::codecs::CODEC_TYPE_ALAC {
                    parameters
                        .extra_data
                        .as_ref()
                        .and_then(|c| c.get(9))
                        .copied()
                        .map(usize::from)
                } else {
                    None
                }
            })
            .filter(|n| *n > 0)
            .ok_or("Missing audio channel layout")?;
        if channels > 2 {
            return Err("Rust lossless decoder currently requires mono or stereo".into());
        }
        let duration = parameters
            .n_frames
            .map_or(0.0, |frames| frames as f64 / rate as f64);
        let start = start
            .max(0.0)
            .min(if duration > 0.0 { duration } else { f64::MAX });
        if start > 0.0 {
            source
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: Time::from(start),
                        track_id: Some(id),
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        let decoder = symphonia::default::get_codecs()
            .make(&parameters, &DecoderOptions::default())
            .map_err(|e| e.to_string())?;
        let mut output = PcmOutput::new(rate)?;
        let ring = output.ring.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let worker = thread::Builder::new()
            .name("rust-audio-decode".into())
            .spawn(move || {
                let mut decoder = decoder;
                let mut samples: Option<SampleBuffer<f32>> = None;
                let mut stereo = Vec::with_capacity(32768);
                loop {
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let packet = match source.next_packet() {
                        Ok(p) => p,
                        Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            break
                        }
                        Err(_) => {
                            ring.failed.store(true, Ordering::Release);
                            break;
                        }
                    };
                    if packet.track_id() != id {
                        continue;
                    }
                    let packet_start = parameters
                        .time_base
                        .map(|tb| {
                            let t = tb.calc_time(packet.ts);
                            t.seconds as f64 + t.frac
                        })
                        .unwrap_or(start);
                    let decoded = match decoder.decode(&packet) {
                        Ok(d) => d,
                        Err(Error::DecodeError(_)) => continue,
                        Err(_) => {
                            ring.failed.store(true, Ordering::Release);
                            break;
                        }
                    };
                    let spec = *decoded.spec();
                    let frames = decoded.frames();
                    if samples.as_ref().map_or(true, |s| {
                        s.capacity() < decoded.capacity() * spec.channels.count()
                    }) {
                        samples = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
                    }
                    let buffer = samples.as_mut().unwrap();
                    buffer.copy_interleaved_ref(decoded);
                    let input = buffer.samples();
                    let skip = if packet_start < start {
                        ((start - packet_start) * rate as f64).round() as usize
                    } else {
                        0
                    };
                    if skip >= frames {
                        continue;
                    }
                    stereo.clear();
                    for frame in input.chunks_exact(channels).skip(skip) {
                        stereo.push(frame[0]);
                        stereo.push(if channels == 1 { frame[0] } else { frame[1] });
                    }
                    if !ring.write(&stereo, &cancelled) {
                        break;
                    }
                }
                ring.eof.store(true, Ordering::Release);
            })
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !output.ready() {
            if Instant::now() >= deadline {
                cancel.store(true, Ordering::Release);
                let _ = worker.join();
                return Err("Rust decoder did not prebuffer in time".into());
            }
            thread::sleep(Duration::from_millis(2));
        }
        if output.ring.failed.load(Ordering::Acquire) && output.ring.buffered() == 0 {
            cancel.store(true, Ordering::Release);
            let _ = worker.join();
            return Err("Rust decoder failed before producing samples".into());
        }
        output.ring.observe.store(true, Ordering::Relaxed);
        crate::audio::configure_pcm(output.rate, output.chunk_frames);
        output.start();
        Ok(Self {
            output,
            path: path.to_path_buf(),
            duration,
            start,
            cancel,
            worker: Some(worker),
            child: None,
        })
    }
    fn open_ffmpeg(path: &Path, start: f64) -> Result<Self, String> {
        use lofty::file::AudioFile;
        use std::io::Read;
        use std::process::{Command, Stdio};
        let metadata = lofty::read_from_path(path).map_err(|e| e.to_string())?;
        let rate = metadata.properties().sample_rate().unwrap_or(44100);
        let duration = metadata.properties().duration().as_secs_f64();
        let executable =
            std::env::var_os("BETTERPOD_RUST_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
        let mut child = Command::new(executable)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-ss",
                &format!("{:.6}", start.max(0.0)),
                "-i",
            ])
            .arg(path)
            .args([
                "-vn",
                "-f",
                "f32le",
                "-acodec",
                "pcm_f32le",
                "-ac",
                "2",
                "-ar",
                &rate.to_string(),
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Native codec unavailable: {e}"))?;
        let mut stdout = child.stdout.take().ok_or("Decoder stdout unavailable")?;
        let mut output = PcmOutput::new(rate)?;
        let ring = output.ring.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let worker = thread::Builder::new()
            .name("pcm-codec".into())
            .spawn(move || {
                let mut bytes = [0u8; 32768];
                let mut samples = Vec::with_capacity(8192);
                let mut carry = Vec::with_capacity(32776);
                while !cancelled.load(Ordering::Acquire) {
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
                    if !ring.write(&samples, &cancelled) {
                        break;
                    }
                    carry.drain(..usable);
                }
                ring.eof.store(true, Ordering::Release);
            })
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while !output.ready() {
            if Instant::now() >= deadline {
                cancel.store(true, Ordering::Release);
                let _ = child.kill();
                let _ = child.wait();
                let _ = worker.join();
                return Err("Codec did not prebuffer in time".into());
            }
            thread::sleep(Duration::from_millis(2));
        }
        if output.ring.buffered() == 0 {
            cancel.store(true, Ordering::Release);
            let _ = child.kill();
            let _ = child.wait();
            let _ = worker.join();
            return Err("Codec produced no audio".into());
        }
        output.ring.observe.store(true, Ordering::Relaxed);
        crate::audio::configure_pcm(output.rate, output.chunk_frames);
        output.start();
        Ok(Self {
            output,
            path: path.to_path_buf(),
            duration,
            start: start.max(0.0),
            cancel,
            worker: Some(worker),
            child: Some(child),
        })
    }
    pub fn poll(&mut self) {
        if let Some(child) = &mut self.child {
            if let Ok(Some(status)) = child.try_wait() {
                if !status.success() {
                    self.output.ring.failed.store(true, Ordering::Release);
                }
            }
        }
    }
    pub fn position(&self) -> f64 {
        self.start + self.output.position()
    }
    pub fn ended(&self) -> bool {
        self.output.ended()
    }
    pub fn pause(&mut self, value: bool) {
        self.output.pause(value);
    }
    pub fn volume(&self, value: f32) {
        self.output.ring.volume(value);
    }
}
impl Drop for RustTrack {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.output.pause(true);
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn open_source(path: &Path) -> Result<Box<dyn symphonia::core::formats::FormatReader>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map(|p| p.format)
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_preserves_stereo_and_never_blocks_callback() {
        let r = Ring::new();
        let cancel = AtomicBool::new(false);
        assert!(r.write(&[0.25, -0.5, 0.75, -1.0], &cancel));
        let mut out = [0.0; 6];
        r.fill(&mut out);
        assert_eq!(out, [0.25, -0.5, 0.75, -1.0, 0.0, 0.0]);
        assert_eq!(r.consumed.load(Ordering::Acquire), 2);
        assert_eq!(r.underruns.load(Ordering::Acquire), 1);
    }
    #[test]
    fn eof_silence_is_not_an_underrun() {
        let r = Ring::new();
        r.eof.store(true, Ordering::Release);
        r.fill(&mut [1.0; 8]);
        assert_eq!(r.underruns.load(Ordering::Acquire), 0);
    }
}
