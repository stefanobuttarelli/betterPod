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
fn action(code: u16) -> Option<Action> {
    match code {
        164 | 226 => Some(Action::MediaPlayPause),
        207 => Some(Action::MediaPlay),
        119 => Some(Action::MediaPause),
        163 => Some(Action::MediaNext),
        165 => Some(Action::MediaPrevious),
        _ => None,
    }
}
struct Device {
    path: PathBuf,
    inode: u64,
    file: File,
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
    let mut polls = Vec::<libc::pollfd>::with_capacity(16);
    while !stop.load(Ordering::Acquire) {
        if Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(2);
            discover(&mut devices, &mut skipped);
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
                                if let Some(event) = action(code) {
                                    let _ = tx.try_send(event);
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
            devices.remove(index);
        }
    }
}
fn discover(devices: &mut Vec<Device>, skipped: &mut HashMap<PathBuf, u64>) {
    devices.retain(|d| fs::metadata(&d.path).map_or(false, |m| m.ino() == d.inode));
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
            && [164usize, 226, 207, 119, 163, 165]
                .iter()
                .any(|code| bits[code / 8] & (1 << (code % 8)) != 0);
        if media {
            devices.push(Device { path, inode, file });
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
    fn hardware_volume_and_power_are_not_intercepted() {
        for code in [114, 115, 116, 113] {
            assert_eq!(action(code), None);
        }
        assert_eq!(action(207), Some(Action::MediaPlay));
        assert_eq!(action(119), Some(Action::MediaPause));
        assert_eq!(action(164), Some(Action::MediaPlayPause));
        assert_eq!(action(163), Some(Action::MediaNext));
    }
}
