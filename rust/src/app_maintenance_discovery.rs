//! Folder/card information for maintenance pages, always off the UI thread.
use super::m;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};
#[derive(Clone, Debug)]
pub(super) struct Card {
    pub name: String,
    pub root: PathBuf,
    pub music: PathBuf,
    pub free: u64,
    pub total: u64,
    pub here: bool,
    pub has_music: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Destination {
    pub path: PathBuf,
    pub writable: bool,
    pub owned: bool,
    pub foreign: bool,
    pub has_files: bool,
}
pub(super) enum Query {
    Cards(m::Config),
    Inspect(PathBuf),
    Create {
        path: PathBuf,
        protected: Vec<PathBuf>,
    },
}
pub(super) enum Event {
    Cards(Vec<Card>),
    Destination(Destination),
    Created(PathBuf),
    Error(String),
}
pub(super) struct Discovery {
    tx: Option<mpsc::SyncSender<Query>>,
    rx: Option<mpsc::Receiver<Event>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Discovery {
    pub fn new() -> Self {
        let (tx, jobs) = mpsc::sync_channel(4);
        let (results, rx) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::Builder::new()
            .name("maintenance-view".into())
            .spawn(move || {
                while let Ok(query) = jobs.recv() {
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let event = match query {
                        Query::Cards(config) => Event::Cards(cards(&config)),
                        Query::Inspect(path) => Event::Destination(inspect(path)),
                        Query::Create { path, protected } => match create_folder(&path, &protected)
                        {
                            Ok(()) => Event::Created(path),
                            Err(e) => Event::Error(e),
                        },
                    };
                    if results.send(event).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self {
            tx: Some(tx),
            rx: Some(rx),
            stop,
            worker,
        }
    }
    pub fn submit(&self, query: Query) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.try_send(query).is_ok())
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.as_ref()?.try_recv().ok()
    }
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Discovery {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn roots(config: &m::Config) -> Vec<PathBuf> {
    let mut roots = vec![config.music.parent().unwrap_or(&config.music).to_path_buf()];
    if Path::new("/userdata").is_dir() {
        roots.push("/userdata".into());
    }
    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
        roots.extend(media_mounts(&mounts));
    }
    roots
}
// A directory under /media can belong to its temporary RAM filesystem.
// Only offer mounted storage, never an unmounted placeholder directory.
fn media_mounts(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .take(4096)
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().take(4).collect();
            if fields.len() < 4
                || matches!(
                    fields[2],
                    "tmpfs" | "ramfs" | "devtmpfs" | "overlay" | "squashfs"
                )
                || !fields[3].split(',').any(|option| option == "rw")
            {
                return None;
            }
            let mount = fields[1]
                .replace("\\040", " ")
                .replace("\\011", "\t")
                .replace("\\012", "\n")
                .replace("\\134", "\\");
            let path = PathBuf::from(mount);
            (path.starts_with("/media") && path != Path::new("/media")).then_some(path)
        })
        .collect()
}
fn cards(config: &m::Config) -> Vec<Card> {
    cards_for_roots(config, roots(config))
}
fn cards_for_roots(config: &m::Config, roots: Vec<PathBuf>) -> Vec<Card> {
    let mut out = Vec::new();
    let mut devices = HashSet::new();
    let current = fs::canonicalize(&config.music).ok();
    let current_device = current.as_ref().and_then(|p| device(p));
    for root in roots {
        if root.file_name().is_some_and(|s| {
            s.to_string_lossy().eq_ignore_ascii_case("BATOCERA")
                || s.to_string_lossy().eq_ignore_ascii_case("boot")
        }) {
            continue;
        }
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        if !writable(&root) {
            continue;
        }
        let Some(dev) = device(&root) else { continue };
        if !devices.insert(dev) {
            continue;
        }
        let (free, total) = space(&root).unwrap_or((0, 0));
        let music = if config
            .music
            .parent()
            .is_some_and(|p| fs::canonicalize(p).ok().as_ref() == Some(&root))
        {
            config.music.clone()
        } else {
            root.join("music")
        };
        let name = card_name(&root);
        let here = Some(dev) == current_device
            || current.as_ref() == fs::canonicalize(&music).ok().as_ref();
        let has_music = fs::read_dir(&music).is_ok_and(|mut e| e.next().is_some());
        out.push(Card {
            name,
            root,
            music,
            free,
            total,
            here,
            has_music,
        });
    }
    out
}
pub(super) fn card_name(root: &Path) -> String {
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut best = (0, "".to_owned());
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let Some(dev) = fields.next() else { continue };
        let Some(mount) = fields.next() else { continue };
        let mount = Path::new(mount);
        if root.starts_with(mount) && mount.as_os_str().len() > best.0 {
            best = (mount.as_os_str().len(), dev.into());
        }
    }
    if best.1.contains("mmcblk0") {
        "SD 1".into()
    } else if best.1.contains("mmcblk1") {
        "SD 2".into()
    } else {
        root.file_name()
            .map_or_else(|| "This card".into(), |s| s.to_string_lossy().into_owned())
    }
}
#[cfg(unix)]
fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| m.dev())
}
#[cfg(not(unix))]
fn device(_path: &Path) -> Option<u64> {
    Some(0)
}
#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}
#[cfg(not(unix))]
fn writable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
}
#[cfg(unix)]
fn space(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    let s = unsafe { stats.assume_init() };
    Some((
        s.f_bavail as u64 * s.f_frsize as u64,
        s.f_blocks as u64 * s.f_frsize as u64,
    ))
}
#[cfg(not(unix))]
fn space(_path: &Path) -> Option<(u64, u64)> {
    None
}
fn inspect(path: PathBuf) -> Destination {
    let marker = path.join(".betterpod-rust.json");
    let owner = fs::symlink_metadata(&marker)
        .ok()
        .filter(|m| m.is_file() && !m.file_type().is_symlink() && m.len() <= 4096)
        .and_then(|_| fs::read(&marker).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let owned = owner.as_ref().is_some_and(|v| {
        v.get("application").and_then(|v| v.as_str()) == Some("betterpod-rust")
            && v.get("ready").and_then(|v| v.as_bool()) == Some(true)
    });
    let has_files = fs::read_dir(&path).is_ok_and(|mut entries| entries.next().is_some());
    let foreign = !owned && has_files;
    let writable = writable(&path) || !path.exists() && path.parent().is_some_and(writable);
    Destination {
        path,
        writable,
        owned,
        foreign,
        has_files,
    }
}
fn create_folder(path: &Path, protected: &[PathBuf]) -> Result<(), String> {
    if path.file_name().and_then(|s| s.to_str()) != Some("betterpod-rust") {
        return Err("Choose the new app folder".into());
    }
    let parent = path.parent().ok_or("Missing parent folder")?;
    let meta = fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Choose a regular folder".into());
    }
    let parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
    let target = parent.join("betterpod-rust");
    if protected
        .iter()
        .filter_map(|p| fs::canonicalize(p).ok())
        .any(|p| target.starts_with(p))
    {
        return Err("This folder belongs to the running application".into());
    }
    match fs::create_dir(&target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let m = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
            if m.is_dir() && !m.file_type().is_symlink() {
                Ok(())
            } else {
                Err("That name already exists".into())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_cards_require_a_writable_persistent_mount() {
        let mounts = "tmpfs /media tmpfs rw 0 0\n/dev/mmcblk1p1 /media/SD\\040CARD exfat rw,nosuid 0 0\ntmpfs /media/SHARE tmpfs rw 0 0\n/dev/loop0 /media/BOOT squashfs ro 0 0\n/dev/sda1 /media/LOCKED ext4 ro 0 0\n/dev/mmcblk0p2 /userdata ext4 rw 0 0\n";
        assert_eq!(media_mounts(mounts), vec![PathBuf::from("/media/SD CARD")]);
        assert!(media_mounts("invalid\n").is_empty());
    }
    #[test]
    fn destination_probe_distinguishes_owned_and_foreign_data() {
        let root =
            std::env::temp_dir().join(format!("betterpod-maint-view-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(!inspect(root.clone()).foreign);
        fs::write(root.join("settings.json"), b"{}").unwrap();
        assert!(inspect(root.clone()).foreign);
        fs::write(
            root.join(".betterpod-rust.json"),
            br#"{"application":"betterpod-rust","ready":true}"#,
        )
        .unwrap();
        assert!(inspect(root.clone()).owned);
        fs::remove_dir_all(root).unwrap();
    }
}
