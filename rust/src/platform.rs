//! Knulli integration owned by the experimental Rust application.
//! All state is restored independently of the stable player's launcher.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_signal: libc::c_int) {
    QUIT.store(true, Ordering::Relaxed);
}

pub fn install_signal_handlers() {
    QUIT.store(false, Ordering::Relaxed);
    #[cfg(unix)]
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_signal as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = 0;
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}

pub fn quit_requested() -> bool {
    QUIT.load(Ordering::Relaxed)
}

struct VolumeState {
    available: AtomicBool,
    current: AtomicU32,
    wanted: AtomicU32,
    pending: AtomicBool,
    stop: AtomicBool,
    /// Counts volume changes made outside the app (Knulli's volume keys),
    /// so the UI can show them as it shows its own.
    external: AtomicU32,
    /// The `pactl subscribe` child, killed on shutdown.
    subscriber: std::sync::Mutex<Option<std::process::Child>>,
}

/// All PipeWire subprocesses run on this worker, never on the drawing loop
/// or audio transport thread. A burst of volume keys keeps only its last value.
pub struct SystemVolume {
    state: Arc<VolumeState>,
    wake: mpsc::SyncSender<()>,
    worker: Option<JoinHandle<()>>,
}

impl SystemVolume {
    pub fn new() -> Self {
        let state = Arc::new(VolumeState {
            available: AtomicBool::new(false),
            current: AtomicU32::new(1.0f32.to_bits()),
            wanted: AtomicU32::new(1.0f32.to_bits()),
            pending: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            external: AtomicU32::new(0),
            subscriber: std::sync::Mutex::new(None),
        });
        let (wake, receiver) = mpsc::sync_channel(1);
        let wake_events = wake.clone();
        let shared = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("system-volume".into())
            .spawn(move || {
                let Some(value) = read_system_volume() else {
                    return;
                };
                shared.current.store(value.to_bits(), Ordering::Release);
                shared.available.store(true, Ordering::Release);
                // Knulli owns the volume keys: follow PipeWire's sink events
                // (as the Go app's `pactl subscribe`) instead of waiting for
                // the 2-second poll, so a key press shows at once.
                spawn_subscriber(&shared, wake_events.clone());
                while !shared.stop.load(Ordering::Acquire) {
                    let _ = receiver.recv_timeout(Duration::from_secs(2));
                    if shared.stop.load(Ordering::Acquire) {
                        break;
                    }
                    if shared.pending.swap(false, Ordering::AcqRel) {
                        let value = f32::from_bits(shared.wanted.load(Ordering::Acquire));
                        let percent = format!("{}%", (value * 100.0).round() as u32);
                        if pactl_output(&["set-sink-volume", "@DEFAULT_SINK@", &percent]).is_none()
                        {
                            eprintln!("System volume could not be changed");
                        }
                    } else {
                        let before = shared.current.load(Ordering::Acquire);
                        if let Some(value) = read_system_volume() {
                            if !shared.pending.load(Ordering::Acquire)
                                && value.to_bits() != before
                                && shared
                                    .current
                                    .compare_exchange(
                                        before,
                                        value.to_bits(),
                                        Ordering::AcqRel,
                                        Ordering::Acquire,
                                    )
                                    .is_ok()
                            {
                                shared.external.fetch_add(1, Ordering::AcqRel);
                            }
                        }
                    }
                }
                shared.available.store(false, Ordering::Release);
            })
            .ok();
        Self {
            state,
            wake,
            worker,
        }
    }

    pub fn get(&self) -> Option<f32> {
        self.state
            .available
            .load(Ordering::Acquire)
            .then(|| f32::from_bits(self.state.current.load(Ordering::Acquire)))
    }

    pub fn set(&self, volume: f32) -> bool {
        if self.get().is_none() || !volume.is_finite() {
            return false;
        }
        let value = volume.clamp(0.0, 1.0).to_bits();
        self.state.current.store(value, Ordering::Release);
        self.state.wanted.store(value, Ordering::Release);
        self.state.pending.store(true, Ordering::Release);
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => true,
            Err(mpsc::TrySendError::Disconnected(())) => false,
        }
    }

    /// How many outside volume changes were seen so far.
    pub fn external_changes(&self) -> u32 {
        self.state.external.load(Ordering::Acquire)
    }

    pub fn shutdown(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        if let Ok(mut child) = self.state.subscriber.lock() {
            if let Some(mut child) = child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let _ = self.wake.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Default for SystemVolume {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for SystemVolume {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn pactl_output(arguments: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut command = Command::new("pactl");
    command
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if Path::new("/etc/idlewatcher").exists() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            command.env("XDG_RUNTIME_DIR", "/var/run");
        }
        if std::env::var_os("PULSE_SERVER").is_none() {
            command.env("PULSE_SERVER", "unix:/var/run/pulse/native");
        }
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_millis(750);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut output = String::new();
    child
        .stdout
        .take()?
        .take(32768)
        .read_to_string(&mut output)
        .ok()?;
    Some(output)
}

/// `pactl subscribe` in a thread of its own: each sink or server change
/// wakes the volume worker, which reads the volume again.
fn spawn_subscriber(state: &Arc<VolumeState>, wake: mpsc::SyncSender<()>) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let mut command = Command::new("pactl");
    command
        .arg("subscribe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if Path::new("/etc/idlewatcher").exists() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            command.env("XDG_RUNTIME_DIR", "/var/run");
        }
        if std::env::var_os("PULSE_SERVER").is_none() {
            command.env("PULSE_SERVER", "unix:/var/run/pulse/native");
        }
    }
    let Ok(mut child) = command.spawn() else {
        return; // the 2-second poll still follows the volume
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return;
    };
    if let Ok(mut slot) = state.subscriber.lock() {
        *slot = Some(child);
    }
    let shared = Arc::clone(state);
    let _ = thread::Builder::new()
        .name("volume-events".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if shared.stop.load(Ordering::Acquire) {
                    break;
                }
                if line.contains("'change' on sink") || line.contains("'change' on server") {
                    let _ = wake.try_send(());
                }
            }
        });
}

fn read_system_volume() -> Option<f32> {
    let output = pactl_output(&["get-sink-volume", "@DEFAULT_SINK@"])?;
    let percent = output.find('%')?;
    let before = &output[..percent];
    let number = before.rsplit(|c: char| !c.is_ascii_digit()).next()?;
    Some(number.parse::<f32>().ok()?.clamp(0.0, 100.0) / 100.0)
}

pub struct IdleGuard {
    path: Option<PathBuf>,
    held: bool,
    last_active: Option<Instant>,
    closed: bool,
}

impl IdleGuard {
    pub fn new() -> Self {
        let path = std::env::var_os("BETTERPOD_RUST_IDLE_FILE")
            .map(PathBuf::from)
            .or_else(|| {
                Path::new("/etc/idlewatcher").exists().then(|| {
                    PathBuf::from(format!(
                        "/var/run/battery-saver/betterpod-rust-{}.pause",
                        std::process::id()
                    ))
                })
            });
        Self {
            path,
            held: false,
            last_active: None,
            closed: false,
        }
    }

    pub fn update(&mut self, active: bool, paused: bool) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        let now = Instant::now();
        if active {
            self.last_active = Some(now);
        }
        let want = active
            || (self.held
                && !paused
                && self
                    .last_active
                    .map(|t| now.duration_since(t) < Duration::from_secs(2))
                    .unwrap_or(false));
        if want == self.held {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        if want {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Cannot create idle marker folder: {e}"))?;
            }
            // create_new prevents this app from owning/deleting another app's
            // marker. A launcher should choose a unique marker for each run.
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| format!("Cannot hold Knulli idle marker {}: {e}", path.display()))?;
            drop(file);
            self.held = true;
        } else {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("Cannot release idle marker: {error}")),
            }
            self.held = false;
        }
        Ok(())
    }

    pub fn close(&mut self) {
        self.closed = true;
        if self.held {
            if let Some(path) = &self.path {
                let _ = fs::remove_file(path);
            }
            self.held = false;
        }
    }
}

impl Default for IdleGuard {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for IdleGuard {
    fn drop(&mut self) {
        self.close();
    }
}

fn governors() -> Vec<(PathBuf, String)> {
    let mut values = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name
                .strip_prefix("cpu")
                .map(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                .unwrap_or(false)
            {
                continue;
            }
            let path = entry.path().join("cpufreq/scaling_governor");
            if let Ok(value) = fs::read_to_string(&path) {
                values.push((path, value.trim().to_owned()));
            }
        }
    }
    values
}

fn write_governors(values: &[(PathBuf, String)]) -> Result<(), String> {
    let mut error = None;
    for (path, value) in values {
        if let Err(e) = fs::write(path, value) {
            error = Some(format!("Cannot set CPU governor {}: {e}", path.display()));
        }
    }
    error.map_or(Ok(()), Err)
}

fn set_governors(value: &str) -> Result<(), String> {
    write_governors(
        &governors()
            .into_iter()
            .map(|(path, _)| (path, value.to_owned()))
            .collect::<Vec<_>>(),
    )
}

#[cfg(target_os = "linux")]
fn disp_brightness(value: Option<i32>) -> Option<i32> {
    use std::os::fd::AsRawFd;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/disp")
        .ok()?;
    let mut args = [0 as libc::c_ulong; 4];
    let result = unsafe {
        if let Some(value) = value {
            args[1] = value.clamp(0, 255) as libc::c_ulong;
            libc::ioctl(file.as_raw_fd(), 0x102, args.as_mut_ptr())
        } else {
            libc::ioctl(file.as_raw_fd(), 0x103, args.as_mut_ptr())
        }
    };
    if result < 0 {
        None
    } else {
        Some(result)
    }
}

#[cfg(not(target_os = "linux"))]
fn disp_brightness(_value: Option<i32>) -> Option<i32> {
    None
}

fn sysfs_backlight() -> Option<(PathBuf, i32)> {
    for entry in fs::read_dir("/sys/class/backlight").ok()?.flatten() {
        let Some(max) = fs::read_to_string(entry.path().join("max_brightness"))
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
        else {
            continue;
        };
        if max > 0 {
            return Some((entry.path(), max));
        }
    }
    None
}

fn read_brightness() -> Option<i32> {
    if let Some(raw) = disp_brightness(None) {
        return Some((raw * 100 + 127) / 255);
    }
    let (path, max) = sysfs_backlight()?;
    let raw = fs::read_to_string(path.join("brightness"))
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()?;
    Some((raw * 100 + max / 2) / max)
}

fn write_brightness(percent: i32) -> Result<(), String> {
    let percent = percent.clamp(0, 100);
    let raw = if percent == 0 {
        0
    } else {
        ((percent * 255 + 50) / 100).max(1)
    };
    if disp_brightness(Some(raw)).is_some() {
        return Ok(());
    }
    if let Some((path, max)) = sysfs_backlight() {
        let raw = if percent == 0 {
            0
        } else {
            ((percent * max + 50) / 100).max(1)
        };
        fs::write(path.join("brightness"), raw.to_string())
            .map_err(|e| format!("Cannot set backlight: {e}"))?;
    }
    // Desktop runs have no backlight device owned by the player.
    Ok(())
}

pub struct Platform {
    initial_brightness: Option<i32>,
    initial_governors: Vec<(PathBuf, String)>,
    awake_brightness: Option<i32>,
    screen_off: bool,
}

impl Platform {
    pub fn new() -> Self {
        Self {
            initial_brightness: read_brightness(),
            initial_governors: governors(),
            awake_brightness: None,
            screen_off: false,
        }
    }

    pub fn brightness(&self) -> Option<i32> {
        if self.screen_off {
            self.awake_brightness
        } else {
            read_brightness()
        }
    }

    pub fn set_brightness(&mut self, percent: i32) -> Result<(), String> {
        if self.screen_off {
            self.awake_brightness = Some(percent.clamp(1, 100));
            Ok(())
        } else {
            write_brightness(percent)
        }
    }

    /// Opt-in: the caller decides when a performance benchmark needs this.
    /// Disabling it restores the original policy rather than forcing one.
    pub fn set_performance(&mut self, enabled: bool) -> Result<(), String> {
        if enabled {
            set_governors("performance")
        } else {
            write_governors(&self.initial_governors)
        }
    }

    pub fn set_screen_off(&mut self, off: bool) -> Result<(), String> {
        if off == self.screen_off {
            return Ok(());
        }
        if off {
            self.awake_brightness = read_brightness();
            write_brightness(0)?;
            self.screen_off = true;
            // Display blanking must not downclock the decoder or Bluetooth
            // transport. The app's CPU policy is restored separately on exit.
            Ok(())
        } else {
            let brightness = self.awake_brightness.unwrap_or(70).clamp(1, 100);
            write_brightness(brightness)?;
            self.screen_off = false;
            Ok(())
        }
    }
}

impl Default for Platform {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Platform {
    fn drop(&mut self) {
        if let Some(brightness) = self.initial_brightness {
            let _ = write_brightness(brightness.max(1));
        }
        let _ = write_governors(&self.initial_governors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_guard_never_removes_another_owners_marker() {
        let path =
            std::env::temp_dir().join(format!("betterpod-rust-owned-{}.pause", std::process::id()));
        fs::write(&path, "other owner").unwrap();
        let mut guard = IdleGuard {
            path: Some(path.clone()),
            held: false,
            last_active: None,
            closed: false,
        };
        assert!(guard.update(true, false).is_err());
        guard.close();
        assert_eq!(fs::read_to_string(&path).unwrap(), "other owner");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn explicit_pause_releases_owned_marker_and_resume_recreates_it() {
        let path =
            std::env::temp_dir().join(format!("betterpod-rust-pause-{}.pause", std::process::id()));
        let _ = fs::remove_file(&path);
        let mut guard = IdleGuard {
            path: Some(path.clone()),
            held: false,
            last_active: None,
            closed: false,
        };
        guard.update(true, false).unwrap();
        assert!(path.exists());
        guard.update(false, true).unwrap();
        assert!(!path.exists());
        guard.update(true, false).unwrap();
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn natural_transition_keeps_marker_until_grace_expires() {
        let path = std::env::temp_dir().join(format!(
            "betterpod-rust-transition-{}.pause",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let mut guard = IdleGuard {
            path: Some(path.clone()),
            held: false,
            last_active: None,
            closed: false,
        };
        guard.update(true, false).unwrap();
        guard.update(false, false).unwrap();
        assert!(path.exists(), "A track transition must remain protected");
        guard.last_active = Some(Instant::now() - Duration::from_secs(3));
        guard.update(false, false).unwrap();
        assert!(
            !path.exists(),
            "A stopped/ended source must eventually release idle protection"
        );
    }
}
