//! Audio transport for the Rust application.
//!
//! One worker owns every music object and every operation that can read the
//! card or reopen the device. The UI only submits bounded commands and takes
//! a nonblocking snapshot. SDL_mixer's native decoder callback never touches
//! either channel or the snapshot lock.

use std::cell::RefCell;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use libc::{c_char, c_double, c_int, c_void};
use sdl2::sys;

const RAM_LIMIT: u64 = 48 * 1024 * 1024;
const COMMAND_CAPACITY: usize = 32;
const VISUALIZER_SAMPLES: usize = 32768;

#[repr(C)]
struct MixMusic {
    _opaque: [u8; 0],
}

// The library is the same patched SDL_mixer shipped with stable betterPod.
// Keeping FFI here also makes the music and RAM-buffer lifetime explicit.
#[cfg_attr(target_os = "macos", link(name = "SDL2_mixer", kind = "framework"))]
#[cfg_attr(not(target_os = "macos"), link(name = "SDL2_mixer"))]
extern "C" {
    fn Mix_Init(flags: c_int) -> c_int;
    fn Mix_Quit();
    fn Mix_OpenAudio(rate: c_int, format: u16, channels: c_int, samples: c_int) -> c_int;
    fn Mix_CloseAudio();
    fn Mix_QuerySpec(rate: *mut c_int, format: *mut u16, channels: *mut c_int) -> c_int;
    fn Mix_LoadMUS(path: *const c_char) -> *mut MixMusic;
    fn Mix_LoadMUS_RW(rw: *mut sys::SDL_RWops, free_source: c_int) -> *mut MixMusic;
    fn Mix_FreeMusic(music: *mut MixMusic);
    fn Mix_PlayMusic(music: *mut MixMusic, loops: c_int) -> c_int;
    fn Mix_HaltMusic() -> c_int;
    fn Mix_PauseMusic();
    fn Mix_ResumeMusic();
    fn Mix_PlayingMusic() -> c_int;
    fn Mix_PausedMusic() -> c_int;
    fn Mix_SetMusicPosition(seconds: c_double) -> c_int;
    fn Mix_GetMusicPosition(music: *mut MixMusic) -> c_double;
    fn Mix_MusicDuration(music: *mut MixMusic) -> c_double;
    fn Mix_VolumeMusic(volume: c_int) -> c_int;
    fn Mix_SetPostMix(
        callback: Option<unsafe extern "C" fn(*mut c_void, *mut u8, c_int)>,
        data: *mut c_void,
    );
}

#[derive(Debug, Clone)]
pub enum AudioCommand {
    Play {
        path: PathBuf,
        sample_rate: u32,
        start: f64,
    },
    /// Cache the next small encoded file while the current one is playing.
    /// No decoder or output device changes are made by this command.
    Prepare {
        path: PathBuf,
    },
    Pause,
    Resume,
    TogglePause,
    Stop,
    Seek(f64),
    Volume(f32),
}

#[derive(Debug, Clone)]
pub struct AudioSnapshot {
    pub path: Option<PathBuf>,
    pub playing: bool,
    pub paused: bool,
    pub loading: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub rate: u32,
    pub error: Option<String>,
    /// Increments once for a natural track end, never for an explicit stop.
    pub ended: u64,
}

impl Default for AudioSnapshot {
    fn default() -> Self {
        Self {
            path: None,
            playing: false,
            paused: false,
            loading: false,
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            rate: 44100,
            error: None,
            ended: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AudioTelemetry {
    pub callbacks: u64,
    pub last_interval_ms: f64,
    pub max_interval_ms: f64,
    pub rate: u32,
    pub chunk_frames: u32,
    pub underruns: u64,
}

// The callback publishes only numeric atomics. There are no allocations,
// locks, file operations or commands in this real-time path.
static VISUALIZER: [AtomicU32; VISUALIZER_SAMPLES] =
    [const { AtomicU32::new(0) }; VISUALIZER_SAMPLES];
static WRITE_POSITION: AtomicU64 = AtomicU64::new(0);
static CALLBACK_START: AtomicU64 = AtomicU64::new(0);
static CALLBACK_TICK: AtomicU64 = AtomicU64::new(0);
static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_INTERVAL: AtomicU64 = AtomicU64::new(0);
static MAX_INTERVAL: AtomicU64 = AtomicU64::new(0);
static CLOCK_FREQUENCY: AtomicU64 = AtomicU64::new(1);
static OUTPUT_FLOAT: AtomicBool = AtomicBool::new(true);
static OUTPUT_CHANNELS: AtomicU32 = AtomicU32::new(2);
static OUTPUT_RATE: AtomicU32 = AtomicU32::new(44100);
static OUTPUT_CHUNK: AtomicU32 = AtomicU32::new(4096);
static ENGINE_ACTIVE: AtomicBool = AtomicBool::new(false);
static PCM_UNDERRUNS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn observe_pcm(samples: &mut [f32]) {
    unsafe {
        postmix(
            std::ptr::null_mut(),
            samples.as_mut_ptr().cast(),
            (samples.len() * 4) as c_int,
        );
    }
}
pub(crate) fn reset_callback_clock() {
    CALLBACK_TICK.store(0, Ordering::Release);
}
pub(crate) fn configure_pcm(rate: u32, chunk: u32) {
    OUTPUT_FLOAT.store(true, Ordering::Relaxed);
    OUTPUT_CHANNELS.store(2, Ordering::Relaxed);
    OUTPUT_RATE.store(rate, Ordering::Relaxed);
    OUTPUT_CHUNK.store(chunk, Ordering::Relaxed);
    PCM_UNDERRUNS.store(0, Ordering::Relaxed);
    reset_callback_clock();
}

unsafe extern "C" fn postmix(_data: *mut c_void, stream: *mut u8, len: c_int) {
    if stream.is_null() || len <= 0 {
        return;
    }
    let now = sys::SDL_GetPerformanceCounter();
    let previous = CALLBACK_TICK.load(Ordering::Relaxed);
    if previous != 0 {
        let interval = now.saturating_sub(previous);
        LAST_INTERVAL.store(interval, Ordering::Relaxed);
        MAX_INTERVAL.fetch_max(interval, Ordering::Relaxed);
    }
    let channels = OUTPUT_CHANNELS.load(Ordering::Relaxed).max(1) as usize;
    let start = WRITE_POSITION.load(Ordering::Relaxed);
    let frames;
    if OUTPUT_FLOAT.load(Ordering::Relaxed) {
        let samples = std::slice::from_raw_parts(stream.cast::<f32>(), len as usize / 4);
        frames = samples.len() / channels;
        for frame in 0..frames {
            let i = frame * channels;
            let mono = if channels > 1 {
                (samples[i] + samples[i + 1]) * 0.5
            } else {
                samples[i]
            };
            VISUALIZER[(start as usize + frame) & (VISUALIZER_SAMPLES - 1)]
                .store(mono.to_bits(), Ordering::Relaxed);
        }
    } else {
        let samples = std::slice::from_raw_parts(stream.cast::<i16>(), len as usize / 2);
        frames = samples.len() / channels;
        for frame in 0..frames {
            let i = frame * channels;
            let mono = if channels > 1 {
                (samples[i] as f32 + samples[i + 1] as f32) / 65536.0
            } else {
                samples[i] as f32 / 32768.0
            };
            VISUALIZER[(start as usize + frame) & (VISUALIZER_SAMPLES - 1)]
                .store(mono.to_bits(), Ordering::Relaxed);
        }
    }
    CALLBACK_START.store(start, Ordering::Relaxed);
    WRITE_POSITION.store(start + frames as u64, Ordering::Release);
    CALLBACK_TICK.store(now, Ordering::Release);
    CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub struct AudioEngine {
    commands: mpsc::SyncSender<AudioCommand>,
    shared: Arc<RwLock<AudioSnapshot>>,
    cached: RefCell<AudioSnapshot>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl AudioEngine {
    pub fn new() -> Result<Self, String> {
        if ENGINE_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("Only one audio engine may own SDL_mixer".into());
        }
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (ready, started) = mpsc::sync_channel(1);
        let shared = Arc::new(RwLock::new(AudioSnapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_state = Arc::clone(&shared);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("betterpod-audio".into())
            .spawn(move || worker(receiver, worker_state, worker_stop, ready))
            .map_err(|e| {
                ENGINE_ACTIVE.store(false, Ordering::Release);
                format!("Cannot start audio worker: {e}")
            })?;
        match started.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                commands,
                shared,
                cached: RefCell::new(AudioSnapshot::default()),
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                // A backend stuck while opening must not freeze the UI's
                // startup. The worker owns its resources until it exits.
                Err(format!("Audio initialization did not complete: {error}"))
            }
        }
    }

    /// False means the bounded queue is full/disconnected. Callers must retain
    /// important commands (e.g. Play) and retry instead of blocking the UI.
    pub fn command(&self, command: AudioCommand) -> bool {
        self.commands.try_send(command).is_ok()
    }

    pub fn snapshot(&self) -> AudioSnapshot {
        if let Ok(state) = self.shared.try_read() {
            *self.cached.borrow_mut() = state.clone();
        }
        self.cached.borrow().clone()
    }

    pub fn telemetry(&self) -> AudioTelemetry {
        let scale = 1000.0 / CLOCK_FREQUENCY.load(Ordering::Relaxed).max(1) as f64;
        AudioTelemetry {
            callbacks: CALLBACK_COUNT.load(Ordering::Relaxed),
            last_interval_ms: LAST_INTERVAL.load(Ordering::Relaxed) as f64 * scale,
            max_interval_ms: MAX_INTERVAL.load(Ordering::Relaxed) as f64 * scale,
            rate: OUTPUT_RATE.load(Ordering::Relaxed),
            chunk_frames: OUTPUT_CHUNK.load(Ordering::Relaxed),
            underruns: PCM_UNDERRUNS.load(Ordering::Relaxed),
        }
    }

    pub fn visualizer(&self, output: &mut [f32]) -> usize {
        let tick = CALLBACK_TICK.load(Ordering::Acquire);
        let frequency = CLOCK_FREQUENCY.load(Ordering::Relaxed).max(1);
        let elapsed = unsafe { sys::SDL_GetPerformanceCounter() }.saturating_sub(tick);
        if tick == 0 || elapsed > frequency / 2 || output.len() > VISUALIZER_SAMPLES / 2 {
            output.fill(0.0);
            return 0;
        }
        let start = CALLBACK_START.load(Ordering::Relaxed);
        let written = WRITE_POSITION.load(Ordering::Acquire);
        let end = (start
            + elapsed.saturating_mul(OUTPUT_RATE.load(Ordering::Relaxed) as u64) / frequency)
            .min(written);
        let count = output.len();
        for (i, sample) in output.iter_mut().enumerate() {
            let at = end.wrapping_sub(count as u64).wrapping_add(i as u64) as usize;
            *sample =
                f32::from_bits(VISUALIZER[at & (VISUALIZER_SAMPLES - 1)].load(Ordering::Relaxed));
        }
        output.len()
    }

    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn chunk_for(rate: u32) -> c_int {
    if let Some(value) = std::env::var("BETTERPOD_RUST_AUDIO_CHUNK")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        if (256..=65536).contains(&value) {
            return value;
        }
    }
    if rate <= 48000 {
        4096
    } else if rate <= 96000 {
        8192
    } else {
        16384
    }
}

fn target_rate(rate: u32) -> u32 {
    if (8000..=384000).contains(&rate) {
        rate
    } else {
        44100
    }
}

fn track_rate(path: &Path, hint: u32) -> u32 {
    use lofty::file::AudioFile;
    if (8000..=384000).contains(&hint) {
        return hint;
    }
    // Old Go library caches may leave the rate at zero until first play.
    // Inspect properties on this worker without decoding embedded artwork.
    let rate = lofty::probe::Probe::open(path)
        .ok()
        .and_then(|probe| probe.guess_file_type().ok())
        .and_then(|probe| {
            probe
                .options(
                    lofty::config::ParseOptions::new()
                        .read_tags(false)
                        .read_cover_art(false),
                )
                .read()
                .ok()
        })
        .and_then(|file| file.properties().sample_rate());
    target_rate(rate.unwrap_or(44100))
}

fn sdl_error() -> String {
    unsafe {
        let error = sys::SDL_GetError();
        if error.is_null() {
            "Unknown SDL error".into()
        } else {
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .into_owned()
        }
    }
}

fn path_cstring(path: &Path) -> Result<CString, String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "Path contains a NUL byte".into())
    }
    #[cfg(not(unix))]
    {
        CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "Path contains a NUL byte".into())
    }
}

struct LoadedMusic {
    pointer: *mut MixMusic,
    // Vec owns the stable allocation for as long as SDL_mixer's RWops uses it.
    _encoded: Option<Vec<u8>>,
}

impl Drop for LoadedMusic {
    fn drop(&mut self) {
        unsafe {
            Mix_FreeMusic(self.pointer);
        }
    }
}

fn memory_candidate(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|s| s.is_file() && s.len() > 0 && s.len() < RAM_LIMIT)
        .unwrap_or(false)
}

fn load_music(
    path: &Path,
    prepared: &mut Option<(PathBuf, Vec<u8>)>,
) -> Result<LoadedMusic, String> {
    let bytes = if prepared.as_ref().map(|p| p.0.as_path()) == Some(path) {
        prepared.take().map(|(_, bytes)| bytes)
    } else if memory_candidate(path) {
        std::fs::read(path).ok()
    } else {
        None
    };
    if let Some(bytes) = bytes {
        if !bytes.is_empty() && bytes.len() < RAM_LIMIT as usize {
            let rw =
                unsafe { sys::SDL_RWFromConstMem(bytes.as_ptr().cast(), bytes.len() as c_int) };
            if !rw.is_null() {
                // The mixer owns and closes RWops even on failure; Rust
                // retains the encoded allocation until the decoder is freed.
                let pointer = unsafe { Mix_LoadMUS_RW(rw, 1) };
                if !pointer.is_null() {
                    return Ok(LoadedMusic {
                        pointer,
                        _encoded: Some(bytes),
                    });
                }
                eprintln!("Audio RAM load failed, trying file stream: {}", sdl_error());
            }
        }
    }
    let path = path_cstring(path)?;
    let pointer = unsafe { Mix_LoadMUS(path.as_ptr()) };
    if pointer.is_null() {
        Err(sdl_error())
    } else {
        Ok(LoadedMusic {
            pointer,
            _encoded: None,
        })
    }
}

fn open_output(wanted: u32) -> Result<u32, String> {
    let mut actual_rate = wanted;
    let mut chunk = chunk_for(wanted);
    let mut open = unsafe {
        Mix_OpenAudio(
            wanted as c_int,
            sys::AUDIO_F32SYS as u16,
            2,
            chunk_for(wanted),
        )
    };
    if open < 0 {
        eprintln!(
            "Audio {wanted} Hz float output unavailable: {}",
            sdl_error()
        );
        actual_rate = 44100;
        chunk = 4096;
        open = unsafe { Mix_OpenAudio(44100, sys::AUDIO_F32SYS as u16, 2, 4096) };
    }
    if open < 0 {
        eprintln!(
            "Audio float output unavailable, using signed 16-bit: {}",
            sdl_error()
        );
        open = unsafe { Mix_OpenAudio(44100, sys::AUDIO_S16SYS as u16, 2, 4096) };
    }
    if open < 0 {
        return Err(format!("Cannot open audio output: {}", sdl_error()));
    }
    let mut rate = 0;
    let mut channels = 0;
    let mut format = 0;
    unsafe {
        Mix_QuerySpec(&mut rate, &mut format, &mut channels);
    }
    if rate > 0 {
        actual_rate = rate as u32;
    }
    OUTPUT_FLOAT.store(format == sys::AUDIO_F32SYS as u16, Ordering::Relaxed);
    OUTPUT_CHANNELS.store(channels.max(1) as u32, Ordering::Relaxed);
    OUTPUT_RATE.store(actual_rate, Ordering::Relaxed);
    OUTPUT_CHUNK.store(chunk as u32, Ordering::Relaxed);
    CLOCK_FREQUENCY.store(
        unsafe { sys::SDL_GetPerformanceFrequency() }.max(1),
        Ordering::Relaxed,
    );
    CALLBACK_TICK.store(0, Ordering::Release);
    LAST_INTERVAL.store(0, Ordering::Relaxed);
    // Rate changes are intentional discontinuities, not callback underruns.
    MAX_INTERVAL.store(0, Ordering::Relaxed);
    unsafe {
        Mix_SetPostMix(Some(postmix), std::ptr::null_mut());
    }
    eprintln!(
        "Audio output: {actual_rate} Hz, format 0x{format:04x}, {channels} channels, {} frames",
        chunk
    );
    Ok(actual_rate)
}

fn publish(shared: &RwLock<AudioSnapshot>, state: &AudioSnapshot) {
    // The reader uses try_read. This clone is off the UI and audio callback.
    if let Ok(mut target) = shared.write() {
        *target = state.clone();
    }
}

fn worker(
    receiver: mpsc::Receiver<AudioCommand>,
    shared: Arc<RwLock<AudioSnapshot>>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    struct ActiveEngine;
    impl Drop for ActiveEngine {
        fn drop(&mut self) {
            ENGINE_ACTIVE.store(false, Ordering::Release);
        }
    }
    let _engine = ActiveEngine;
    if unsafe { sys::SDL_InitSubSystem(sys::SDL_INIT_AUDIO) } < 0 {
        let _ = ready.send(Err(format!(
            "SDL audio initialization failed: {}",
            sdl_error()
        )));
        return;
    }
    // A decoder absent from a desktop build need not prevent supported
    // formats from playing; the bundled Knulli mixer supplies AAC/ALAC too.
    let codecs = unsafe { Mix_Init(0x01 | 0x08 | 0x10 | 0x40) };
    eprintln!("SDL_mixer decoder flags: 0x{codecs:02x}");
    let rate = match open_output(44100) {
        Ok(rate) => rate,
        Err(error) => {
            let _ = ready.send(Err(error));
            unsafe {
                Mix_Quit();
                sys::SDL_QuitSubSystem(sys::SDL_INIT_AUDIO);
            }
            return;
        }
    };
    let mut state = AudioSnapshot {
        rate,
        ..AudioSnapshot::default()
    };
    let mut current: Option<LoadedMusic> = None;
    let mut rust_track: Option<crate::pcm::RustTrack> = None;
    let mut mix_open = true;
    let mut prepared: Option<(PathBuf, Vec<u8>)> = None;
    let mut started = false;
    let mut position_anchor = Instant::now();
    let mut anchor_seconds = 0.0;
    publish(&shared, &state);
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Acquire) {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(command) => match command {
                AudioCommand::Play {
                    path,
                    sample_rate,
                    start,
                } => {
                    state.loading = true;
                    state.error = None;
                    publish(&shared, &state);
                    // The worker handles all blocking load/device operations.
                    started = false;
                    if mix_open {
                        unsafe {
                            Mix_HaltMusic();
                        }
                    }
                    current.take();
                    rust_track.take();
                    state.playing = false;
                    state.paused = false;
                    state.position = 0.0;
                    state.duration = 0.0;
                    let wanted = track_rate(&path, sample_rate);
                    let output = if !mix_open || state.rate != wanted {
                        if mix_open {
                            unsafe {
                                Mix_SetPostMix(None, std::ptr::null_mut());
                                Mix_CloseAudio();
                            }
                        }
                        let result = open_output(wanted);
                        mix_open = result.is_ok();
                        result
                    } else {
                        Ok(state.rate)
                    };
                    match output.and_then(|rate| {
                        state.rate = rate;
                        load_music(&path, &mut prepared)
                    }) {
                        Ok(music) => {
                            unsafe {
                                Mix_VolumeMusic(
                                    (state.volume.clamp(0.0, 1.0) * 128.0).round() as c_int
                                );
                            }
                            if unsafe { Mix_PlayMusic(music.pointer, 0) } == 0 {
                                state.path = Some(path.clone());
                                state.duration =
                                    unsafe { Mix_MusicDuration(music.pointer) }.max(0.0);
                                let seek = if start.is_finite() {
                                    start.max(0.0)
                                } else {
                                    0.0
                                };
                                if seek > 0.0 && unsafe { Mix_SetMusicPosition(seek) } != 0 {
                                    eprintln!("Audio could not restore position: {}", sdl_error());
                                    anchor_seconds = 0.0;
                                } else {
                                    anchor_seconds = seek;
                                }
                                position_anchor = Instant::now();
                                state.playing = true;
                                started = true;
                                current = Some(music);
                            } else {
                                state.error = Some(format!("Cannot play: {}", sdl_error()));
                            }
                        }
                        Err(error) => {
                            if mix_open {
                                unsafe {
                                    Mix_SetPostMix(None, std::ptr::null_mut());
                                    Mix_CloseAudio();
                                }
                                mix_open = false;
                            }
                            match crate::pcm::RustTrack::open(&path, start) {
                                Ok(track) => {
                                    eprintln!("Rust decoder fallback for {}", path.display());
                                    track.volume(state.volume);
                                    state.rate = track.output.rate;
                                    state.duration = track.duration;
                                    state.path = Some(path);
                                    state.position = start;
                                    state.playing = true;
                                    state.paused = false;
                                    started = true;
                                    rust_track = Some(track);
                                }
                                Err(fallback) => {
                                    state.error = Some(format!(
                                        "Cannot load {}: {error}; Rust decoder: {fallback}",
                                        path.display()
                                    ))
                                }
                            }
                        }
                    }
                    state.loading = false;
                }
                AudioCommand::Prepare { path } => {
                    if state.path.as_ref() != Some(&path) && memory_candidate(&path) {
                        if let Ok(bytes) = std::fs::read(&path) {
                            if bytes.len() < RAM_LIMIT as usize {
                                prepared = Some((path, bytes));
                            }
                        }
                    }
                }
                AudioCommand::Pause => unsafe {
                    if let Some(track) = rust_track.as_mut() {
                        track.pause(true);
                        state.paused = true;
                        state.playing = false;
                    } else if started && Mix_PausedMusic() == 0 {
                        Mix_PauseMusic();
                        anchor_seconds = state.position;
                        position_anchor = Instant::now();
                    }
                },
                AudioCommand::Resume => unsafe {
                    if let Some(track) = rust_track.as_mut() {
                        track.pause(false);
                        state.paused = false;
                        state.playing = true;
                    } else if started && Mix_PausedMusic() != 0 {
                        Mix_ResumeMusic();
                        position_anchor = Instant::now();
                    }
                },
                AudioCommand::TogglePause => unsafe {
                    if let Some(track) = rust_track.as_mut() {
                        state.paused = !state.paused;
                        track.pause(state.paused);
                        state.playing = !state.paused;
                    } else if started {
                        if Mix_PausedMusic() != 0 {
                            Mix_ResumeMusic();
                            position_anchor = Instant::now();
                        } else {
                            Mix_PauseMusic();
                            anchor_seconds = state.position;
                            position_anchor = Instant::now();
                        }
                    }
                },
                AudioCommand::Stop => {
                    started = false;
                    if mix_open {
                        unsafe {
                            Mix_HaltMusic();
                        }
                    }
                    current.take();
                    rust_track.take();
                    state.playing = false;
                    state.paused = false;
                    state.loading = false;
                    state.position = 0.0;
                    state.duration = 0.0;
                    state.path = None;
                }
                AudioCommand::Seek(position) => {
                    if started && position.is_finite() {
                        let position = position.max(0.0).min(if state.duration > 0.0 {
                            state.duration
                        } else {
                            f64::MAX
                        });
                        if let Some(old) = rust_track.take() {
                            let path = old.path.clone();
                            drop(old);
                            match crate::pcm::RustTrack::open(&path, position) {
                                Ok(mut track) => {
                                    track.volume(state.volume);
                                    track.pause(state.paused);
                                    state.position = position;
                                    rust_track = Some(track);
                                }
                                Err(error) => {
                                    state.error = Some(format!("Cannot seek: {error}"));
                                    started = false;
                                    state.playing = false;
                                }
                            }
                        } else if unsafe { Mix_SetMusicPosition(position) } == 0 {
                            anchor_seconds = position;
                            position_anchor = Instant::now();
                            state.position = position;
                        } else {
                            state.error = Some(format!("Cannot seek: {}", sdl_error()));
                        }
                    }
                }
                AudioCommand::Volume(volume) => {
                    if volume.is_finite() {
                        state.volume = volume.clamp(0.0, 1.0);
                        if let Some(track) = rust_track.as_ref() {
                            track.volume(state.volume);
                        } else if mix_open {
                            unsafe {
                                Mix_VolumeMusic((state.volume * 128.0).round() as c_int);
                            }
                        }
                    }
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(track) = rust_track.as_mut() {
            track.poll();
            state.position = track.position().min(if state.duration > 0.0 {
                state.duration
            } else {
                f64::MAX
            });
            PCM_UNDERRUNS.store(
                track.output.ring.underruns.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            if track.output.ring.failed.load(Ordering::Acquire) {
                state.error = Some("Rust decoder stopped after a decode error".into());
            }
            if started && track.ended() && !state.paused {
                started = false;
                state.playing = false;
                state.ended = state.ended.wrapping_add(1);
            }
        } else if let Some(music) = current.as_ref() {
            let playing = unsafe { Mix_PlayingMusic() != 0 };
            let paused = unsafe { Mix_PausedMusic() != 0 };
            state.playing = playing && !paused;
            state.paused = paused;
            let position = unsafe { Mix_GetMusicPosition(music.pointer) };
            state.position = if position.is_finite() && position >= 0.0 {
                position
            } else if paused || !playing {
                anchor_seconds
            } else {
                anchor_seconds + position_anchor.elapsed().as_secs_f64()
            };
            if state.duration > 0.0 {
                state.position = state.position.min(state.duration);
            }
            if started && !playing && !paused {
                started = false;
                state.ended = state.ended.wrapping_add(1);
                state.position = state.duration;
                anchor_seconds = state.duration;
            }
        }
        publish(&shared, &state);
    }
    if mix_open {
        unsafe {
            Mix_HaltMusic();
        }
    }
    current.take();
    rust_track.take();
    unsafe {
        if mix_open {
            Mix_SetPostMix(None, std::ptr::null_mut());
            Mix_CloseAudio();
        }
        Mix_Quit();
        sys::SDL_QuitSubSystem(sys::SDL_INIT_AUDIO);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rate_chunk_policy_preserves_callback_headroom() {
        // Ignore the environment override while checking policy boundaries.
        if std::env::var_os("BETTERPOD_RUST_AUDIO_CHUNK").is_some() {
            return;
        }
        assert_eq!(chunk_for(44100), 4096);
        assert_eq!(chunk_for(48000), 4096);
        assert_eq!(chunk_for(88200), 8192);
        assert_eq!(chunk_for(96000), 8192);
        assert_eq!(chunk_for(192000), 16384);
    }

    #[test]
    fn invalid_rate_metadata_cannot_open_an_invalid_device() {
        assert_eq!(target_rate(0), 44100);
        assert_eq!(target_rate(1_000_000), 44100);
        assert_eq!(target_rate(96000), 96000);
        assert_eq!(target_rate(384000), 384000);
    }
}
