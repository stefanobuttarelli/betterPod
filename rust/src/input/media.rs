//! Read headset media keys without grabbing devices or intercepting Knulli's
//! volume/power buttons. Discovery and poll/read syscalls stay off the UI.
use super::Action;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
const KEY_BYTES: usize = 96;
const EVIOCGBIT_KEY: libc::c_ulong =
    0x8000_0000 | ((KEY_BYTES as libc::c_ulong) << 16) | (b'E' as libc::c_ulong) << 8 | 0x21;
fn action(code: u16, jack: bool) -> Option<Action> {
    match code {
        164 => Some(Action::MediaPlayPause),
        258 => Some(Action::MediaVolumeUp),
        259 => Some(Action::MediaVolumeDown),
        115 if jack => Some(Action::MediaVolumeUp),
        114 if jack => Some(Action::MediaVolumeDown),
        207 => Some(Action::MediaPlay),
        119 => Some(Action::MediaPause),
        163 => Some(Action::MediaNext),
        165 => Some(Action::MediaPrevious),
        _ => None,
    }
}
#[derive(Default)]
struct HookClicks {
    count: u8,
    last: Option<Instant>,
}
impl HookClicks {
    fn click(&mut self, now: Instant) {
        self.count = self.count.saturating_add(1);
        self.last = Some(now);
    }
    fn ready(&mut self, now: Instant) -> Option<Action> {
        if !self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(450))
        {
            return None;
        }
        let count = std::mem::take(&mut self.count);
        self.last = None;
        Some(if count == 1 {
            Action::MediaPlayPause
        } else if count == 2 {
            Action::MediaNext
        } else {
            Action::MediaPrevious
        })
    }
}
fn hook(code: u16) -> bool {
    matches!(code, 226 | 169 | 256 | 582)
}
fn ioctl_bits(file: &File, number: libc::c_ulong, bits: &mut [u8]) -> bool {
    let request =
        0x8000_0000 | ((bits.len() as libc::c_ulong) << 16) | (b'E' as libc::c_ulong) << 8 | number;
    unsafe { libc::ioctl(file.as_raw_fd(), request, bits.as_mut_ptr()) >= 0 }
}
struct Device {
    path: PathBuf,
    inode: u64,
    file: File,
    jack: bool,
    bluetooth: bool,
    inserted: Option<bool>,
}
pub(super) struct MediaInput {
    rx: mpsc::Receiver<Action>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl MediaInput {
    pub(super) fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker = if std::env::var("BETTERPOD_RUST_MEDIA_KEYS").map_or(true, |v| v != "0") {
            let stop = stop.clone();
            thread::Builder::new()
                .name("headset-input".into())
                .spawn(move || run(tx, stop))
                .ok()
        } else {
            None
        };
        Self { rx, stop, worker }
    }
    pub(super) fn drain(&self, out: &mut Vec<Action>) {
        while let Ok(event) = self.rx.try_recv() {
            out.push(event);
        }
    }
}
impl Drop for MediaInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn run(tx: mpsc::SyncSender<Action>, stop: Arc<AtomicBool>) {
    let mut devices: Vec<Device> = Vec::with_capacity(16);
    let mut skipped = HashMap::<PathBuf, u64>::new();
    let mut next_scan = Instant::now();
    let mut hook_clicks = HookClicks::default();
    let mut last_media: Option<(u8, Instant)> = None;
    let mut polls = Vec::<libc::pollfd>::with_capacity(16);
    while !stop.load(Ordering::Acquire) {
        if Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(2);
            discover(&mut devices, &mut skipped, &tx);
        }
        if let Some(action) = hook_clicks.ready(Instant::now()) {
            let _ = tx.try_send(action);
        }
        polls.clear();
        for d in &devices {
            polls.push(libc::pollfd {
                fd: d.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let ready = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, 100) };
        if ready <= 0 {
            continue;
        }
        let mut remove = Vec::new();
        for (index, poll) in polls.iter().enumerate() {
            if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                remove.push(index);
                continue;
            }
            if poll.revents & libc::POLLIN == 0 {
                continue;
            }
            // A Linux input_event has a native timeval followed by type/code/value.
            let event_size = std::mem::size_of::<libc::timeval>() + 8;
            let mut buffer = [0u8; 24 * 16];
            loop {
                match devices[index].file.read(&mut buffer) {
                    Ok(0) => {
                        remove.push(index);
                        break;
                    }
                    Ok(n) => {
                        for raw in buffer[..n].chunks_exact(event_size) {
                            let at = std::mem::size_of::<libc::timeval>();
                            let kind = u16::from_ne_bytes(raw[at..at + 2].try_into().unwrap());
                            let code = u16::from_ne_bytes(raw[at + 2..at + 4].try_into().unwrap());
                            let value = i32::from_ne_bytes(raw[at + 4..at + 8].try_into().unwrap());
                            if kind == 1 && value == 1 {
                                if hook(code) {
                                    hook_clicks.click(Instant::now());
                                    continue;
                                }
                                if let Some(event) = action(code, devices[index].jack) {
                                    let class = match event {
                                        Action::MediaNext => 2,
                                        Action::MediaPrevious => 3,
                                        Action::MediaVolumeUp | Action::MediaVolumeDown => 0,
                                        _ => 1,
                                    };
                                    let now = Instant::now();
                                    if class != 0
                                        && last_media.is_some_and(|(last, at)| {
                                            class == last
                                                && now.saturating_duration_since(at)
                                                    < Duration::from_millis(400)
                                        })
                                    {
                                        continue;
                                    }
                                    if class != 0 {
                                        last_media = Some((class, now));
                                    }
                                    let _ = tx.try_send(event);
                                }
                            } else if kind == 5 && code == 2 && devices[index].jack {
                                let inserted = value != 0;
                                let old = devices[index].inserted.replace(inserted);
                                if old != Some(inserted) {
                                    let _ = tx.try_send(Action::Headphones(inserted));
                                    if !inserted && old == Some(true) {
                                        let _ = tx.try_send(Action::MediaUnplug);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        remove.push(index);
                        break;
                    }
                }
            }
        }
        remove.sort_unstable();
        remove.dedup();
        for index in remove.into_iter().rev() {
            let device = devices.remove(index);
            if device.bluetooth || device.inserted == Some(true) {
                let _ = tx.try_send(Action::MediaUnplug);
            }
            if device.jack {
                let _ = tx.try_send(Action::Headphones(false));
            }
        }
    }
}
fn discover(
    devices: &mut Vec<Device>,
    skipped: &mut HashMap<PathBuf, u64>,
    tx: &mpsc::SyncSender<Action>,
) {
    devices.retain(|device| {
        let retained = fs::metadata(&device.path).is_ok_and(|m| m.ino() == device.inode);
        if !retained {
            if device.bluetooth || device.inserted == Some(true) {
                let _ = tx.try_send(Action::MediaUnplug);
            }
            if device.jack {
                let _ = tx.try_send(Action::Headphones(false));
            }
        }
        retained
    });
    let Ok(entries) = fs::read_dir("/dev/input") else {
        return;
    };
    for entry in entries.flatten() {
        if devices.len() >= 16 {
            break;
        }
        if !entry.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let inode = meta.ino();
        if devices.iter().any(|d| d.path == path && d.inode == inode)
            || skipped.get(&path) == Some(&inode)
        {
            continue;
        }
        let Ok(file) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&path)
        else {
            continue;
        };
        let mut bits = [0u8; KEY_BYTES];
        let read = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGBIT_KEY, bits.as_mut_ptr()) };
        let media = read >= 0
            && [164usize, 226, 207, 119, 163, 165, 169, 582]
                .iter()
                .any(|code| bits[code / 8] & (1 << (code % 8)) != 0);
        let mut switches = [0u8; 8];
        let jack = ioctl_bits(&file, 0x25, &mut switches) && switches[0] & 4 != 0;
        if media || jack {
            let mut id = [0u8; 8];
            let bluetooth =
                ioctl_bits(&file, 0x02, &mut id) && u16::from_ne_bytes([id[0], id[1]]) == 5;
            let mut state = [0u8; 8];
            let inserted = if jack && ioctl_bits(&file, 0x1b, &mut state) {
                Some(state[0] & 4 != 0)
            } else {
                None
            };
            if let Some(inserted) = inserted {
                let _ = tx.try_send(Action::Headphones(inserted));
            }
            devices.push(Device {
                path,
                inode,
                file,
                jack,
                bluetooth,
                inserted,
            });
        } else {
            skipped.insert(path, inode);
        }
    }
    if skipped.len() > 128 {
        skipped.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_hook_click_window_and_wired_volume_filter() {
        let start = Instant::now();
        let mut clicks = HookClicks::default();
        clicks.click(start);
        assert_eq!(clicks.ready(start + Duration::from_millis(449)), None);
        assert_eq!(
            clicks.ready(start + Duration::from_millis(450)),
            Some(Action::MediaPlayPause)
        );
        clicks.click(start);
        clicks.click(start + Duration::from_millis(100));
        assert_eq!(
            clicks.ready(start + Duration::from_millis(550)),
            Some(Action::MediaNext)
        );
        for offset in [0, 100, 200] {
            clicks.click(start + Duration::from_millis(offset));
        }
        assert_eq!(
            clicks.ready(start + Duration::from_millis(650)),
            Some(Action::MediaPrevious)
        );
        assert_eq!(action(115, false), None);
        assert_eq!(action(115, true), Some(Action::MediaVolumeUp));
        assert_eq!(action(114, true), Some(Action::MediaVolumeDown));
        assert!(!hook(164));
        assert!(hook(226));
    }
    #[test]
    fn hardware_volume_and_power_are_not_intercepted() {
        for code in [114, 115, 116, 113] {
            assert_eq!(action(code, false), None);
        }
        assert_eq!(action(207, false), Some(Action::MediaPlay));
        assert_eq!(action(119, false), Some(Action::MediaPause));
        assert_eq!(action(164, false), Some(Action::MediaPlayPause));
        assert_eq!(action(163, false), Some(Action::MediaNext));
    }
}
