//! Bounded, cancellable file operations. Browser navigation and deletion never
//! run on the rendering thread. Deletion requires explicit UI confirmation.
use crate::library::Library;
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::CString,
    fs::{self, File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub audio: bool,
    pub size: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Space {
    pub total: u64,
    pub free: u64,
}
#[derive(Clone, Debug)]
pub struct ViewRow {
    pub title: String,
    pub subtitle: String,
    pub entry: Option<Entry>,
}
fn text(title: impl Into<String>, subtitle: impl Into<String>) -> ViewRow {
    ViewRow {
        title: title.into(),
        subtitle: subtitle.into(),
        entry: None,
    }
}
#[derive(Clone, Debug)]
pub enum Command {
    Browse(PathBuf),
    Stats,
    StatsLibrary(Arc<Library>),
    Scanned(Arc<Library>),
    Delete { path: PathBuf, confirmed: bool },
}
#[derive(Clone, Debug)]
pub enum Event {
    Listed {
        path: PathBuf,
        rows: Arc<Vec<ViewRow>>,
    },
    Info {
        scanned: bool,
        rows: Arc<Vec<ViewRow>>,
    },
    Space(Space),
    Deleted,
    Error(String),
}
pub struct FileClient {
    tx: Option<mpsc::SyncSender<Command>>,
    rx: Option<mpsc::Receiver<Event>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl FileClient {
    pub fn new(music: PathBuf, data: PathBuf) -> Self {
        let (tx, commands) = mpsc::sync_channel(8);
        let (events, rx) = mpsc::sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let worker = thread::Builder::new()
            .name("file-browser".into())
            .spawn(move || {
                let roots = match Roots::new(music, data) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = events.send(Event::Error(e));
                        return;
                    }
                };
                let mut stats_cache: Option<(Arc<Library>, Arc<Vec<ViewRow>>)> = None;
                let mut scanned_cache: Option<(Arc<Library>, Arc<Vec<ViewRow>>)> = None;
                let mut retired = VecDeque::new();
                while let Ok(command) = commands.recv() {
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let result = match command {
                        Command::Browse(path) => {
                            roots
                                .browse(&path, &flag)
                                .map(|(path, entries)| Event::Listed {
                                    path,
                                    rows: Arc::new(
                                        entries
                                            .into_iter()
                                            .map(|entry| ViewRow {
                                                title: entry.name.clone(),
                                                subtitle: if entry.directory {
                                                    "Folder".into()
                                                } else {
                                                    format_size(entry.size)
                                                },
                                                entry: Some(entry),
                                            })
                                            .collect(),
                                    ),
                                })
                        }
                        Command::Stats => space(&roots.music).map(Event::Space),
                        Command::StatsLibrary(library) => {
                            if !stats_cache
                                .as_ref()
                                .is_some_and(|(old, _)| Arc::ptr_eq(old, &library))
                            {
                                stats_cache =
                                    Some((library.clone(), Arc::new(statistics(&library, &flag))));
                            }
                            let mut rows = stats_cache.as_ref().unwrap().1.as_ref().clone();
                            if let Ok(space) = space(&roots.music) {
                                rows.push(text(
                                    "Music Storage",
                                    format!(
                                        "{} free of {}",
                                        format_size(space.free),
                                        format_size(space.total)
                                    ),
                                ));
                            }
                            Ok(Event::Info {
                                scanned: false,
                                rows: Arc::new(rows),
                            })
                        }
                        Command::Scanned(library) => {
                            if !scanned_cache
                                .as_ref()
                                .is_some_and(|(old, _)| Arc::ptr_eq(old, &library))
                            {
                                let rows = library
                                    .scan_folders
                                    .iter()
                                    .map(|folder| {
                                        text(
                                            folder.path.to_string_lossy().into_owned(),
                                            folder.skipped.clone().unwrap_or_else(|| {
                                                format!("{} songs scanned", folder.tracks)
                                            }),
                                        )
                                    })
                                    .collect();
                                scanned_cache = Some((library, Arc::new(rows)));
                            }
                            Ok(Event::Info {
                                scanned: true,
                                rows: scanned_cache.as_ref().unwrap().1.clone(),
                            })
                        }
                        Command::Delete { path, confirmed } => roots
                            .delete(&path, confirmed, &flag)
                            .map(|_| Event::Deleted),
                    };
                    if let Ok(Event::Listed { rows, .. } | Event::Info { rows, .. }) = &result {
                        retired.push_back(rows.clone());
                        while retired.len() > 3 {
                            retired.pop_front();
                        }
                    }
                    if events.send(result.unwrap_or_else(Event::Error)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| eprintln!("File worker: {e}"))
            .ok();
        Self {
            tx: Some(tx),
            rx: Some(rx),
            cancelled,
            worker,
        }
    }
    pub fn submit(&self, command: Command) -> Result<(), String> {
        self.tx
            .as_ref()
            .ok_or("File browser is shut down")?
            .try_send(command)
            .map_err(|_| "File browser is busy".into())
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.as_ref()?.try_recv().ok()
    }
    pub fn shutdown(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.tx.take();
        self.rx.take();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
impl Drop for FileClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}
struct Roots {
    music: PathBuf,
    data: PathBuf,
    downloads: PathBuf,
    protected: Vec<PathBuf>,
}
impl Roots {
    fn new(music: PathBuf, data: PathBuf) -> Result<Self, String> {
        let music = fs::canonicalize(music).map_err(|e| e.to_string())?;
        let data = fs::canonicalize(data).map_err(|e| e.to_string())?;
        let mut protected = vec![data.clone()];
        for name in ["BETTERPOD_RUST_IMPORT_CACHE", "BETTERPOD_RUST_IMPORT_TOOLS"] {
            if let Some(path) = std::env::var_os(name) {
                let path = PathBuf::from(path);
                if let Ok(path) = fs::canonicalize(path) {
                    protected.push(if path.is_file() {
                        path.parent().unwrap_or(&path).to_path_buf()
                    } else {
                        path
                    });
                }
            }
        }
        let downloads = data.join("downloads");
        Ok(Self {
            music,
            data,
            downloads,
            protected,
        })
    }
    fn allowed(&self, path: &Path) -> Result<PathBuf, String> {
        if fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Symbolic links are not followed".into());
        }
        let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let download_root = fs::symlink_metadata(&self.downloads)
            .ok()
            .filter(|m| m.is_dir() && !m.file_type().is_symlink())
            .and_then(|_| fs::canonicalize(&self.downloads).ok())
            .filter(|r| r.starts_with(&self.data));
        if !canonical.starts_with(&self.music)
            && !download_root
                .as_ref()
                .is_some_and(|r| canonical.starts_with(r))
        {
            return Err("Path is outside the music and private downloads folders".into());
        }
        Ok(canonical)
    }
    fn browse(&self, path: &Path, cancel: &AtomicBool) -> Result<(PathBuf, Vec<Entry>), String> {
        let path = self.allowed(path)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
            check(cancel)?;
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || kind.is_symlink() || !kind.is_dir() && !kind.is_file() {
                continue;
            }
            let child = entry.path();
            if self.protected.iter().any(|p| {
                !(p == &self.data && child.starts_with(&self.downloads))
                    && (child == *p || child.starts_with(p))
            }) {
                continue;
            }
            let audio = audio(&child);
            if !kind.is_dir() && !audio {
                continue;
            }
            let size = if kind.is_file() {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            entries.push(Entry {
                path: child,
                name,
                directory: kind.is_dir(),
                audio,
                size,
            });
            if entries.len() > 100000 {
                return Err("Folder has too many entries".into());
            }
        }
        if path == self.music
            && !self.downloads.starts_with(&self.music)
            && fs::symlink_metadata(&self.downloads)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        {
            entries.push(Entry {
                path: self.downloads.clone(),
                name: "betterPod Rust Downloads".into(),
                directory: true,
                audio: false,
                size: 0,
            });
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok((path, entries))
    }
    fn protected(&self, path: &Path) -> bool {
        let root = if path.starts_with(&self.downloads) {
            &self.downloads
        } else {
            &self.music
        };
        path == self.music || path == self.downloads
            || self.protected.iter().any(|p| !(p == &self.data && path.starts_with(&self.downloads)) && (path.starts_with(p) || p.starts_with(path)))
            || path.strip_prefix(root).is_ok_and(|relative| relative.components().any(|c| {
                matches!(c, Component::Normal(n) if n.to_string_lossy().to_ascii_lowercase().starts_with("betterpod"))
            }))
    }
    fn delete(&self, path: &Path, confirmed: bool, cancel: &AtomicBool) -> Result<(), String> {
        if !confirmed {
            return Err("Confirm deletion in the file browser first".into());
        }
        let path = self.allowed(path)?;
        if self.protected(&path) {
            return Err("Application data and library roots cannot be deleted".into());
        }
        let root = if path.starts_with(&self.downloads) {
            self.downloads.clone()
        } else {
            self.music.clone()
        };
        let handle = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&root)
            .map_err(|e| e.to_string())?;
        let mut pending = vec![path.clone()];
        let mut entries = Vec::new();
        while let Some(current) = pending.pop() {
            check(cancel)?;
            if self.protected(&current) {
                return Err("Folder contains protected application data".into());
            }
            let metadata = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() && !metadata.is_file() {
                return Err("Folder contains a symlink or special file; nothing deleted".into());
            }
            entries.push((current.clone(), metadata.is_dir()));
            if entries.len() > 500000 {
                return Err("Too many files for one deletion".into());
            }
            if metadata.is_dir() {
                for child in fs::read_dir(&current).map_err(|e| e.to_string())? {
                    pending.push(child.map_err(|e| e.to_string())?.path());
                }
            }
        }
        for (entry, directory) in entries.into_iter().rev() {
            check(cancel)?;
            remove_relative(
                &handle,
                entry.strip_prefix(&root).map_err(|e| e.to_string())?,
                directory,
            )?;
        }
        Ok(())
    }
}
fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}
fn remove_relative(root: &File, path: &Path, directory: bool) -> Result<(), String> {
    let parts: Vec<_> = path
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => Err("Unsafe deletion path"),
        })
        .collect::<Result<_, _>>()?;
    let (name, parents) = parts.split_last().ok_or("Cannot delete a library root")?;
    let mut parent = root.try_clone().map_err(|e| e.to_string())?;
    for part in parents {
        let name = CString::new(part.as_bytes()).map_err(|e| e.to_string())?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    let name = CString::new(name.as_bytes()).map_err(|e| e.to_string())?;
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
fn space(path: &Path) -> Result<Space, String> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let stat = unsafe { stat.assume_init() };
    Ok(Space {
        total: (stat.f_blocks as u64).saturating_mul(stat.f_frsize as u64),
        free: (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64),
    })
}
fn audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "mp3"
                | "m4a"
                | "m4b"
                | "flac"
                | "wav"
                | "aiff"
                | "aif"
                | "ogg"
                | "oga"
                | "opus"
                | "aac"
                | "ape"
                | "wv"
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_delete_is_confirmed_root_bound_and_refuses_symlink_escape() {
        let root = std::env::temp_dir().join(format!("betterpod-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let music = root.join("music");
        let data = root.join("rust");
        fs::create_dir_all(music.join("Album")).unwrap();
        fs::create_dir_all(&data).unwrap();
        let song = music.join("Album/song.flac");
        fs::write(&song, b"audio").unwrap();
        let outside = root.join("outside.flac");
        fs::write(&outside, b"protected").unwrap();
        let roots = Roots::new(music.clone(), data).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(roots.delete(&song, false, &cancel).is_err());
        std::os::unix::fs::symlink(&outside, music.join("Album/escape.flac")).unwrap();
        assert!(roots.delete(&music.join("Album"), true, &cancel).is_err());
        assert!(song.exists());
        assert_eq!(fs::read(&outside).unwrap(), b"protected");
        fs::remove_file(music.join("Album/escape.flac")).unwrap();
        roots.delete(&music.join("Album"), true, &cancel).unwrap();
        assert!(!song.exists());
        assert!(roots.delete(&music, true, &cancel).is_err());
        let own = roots.downloads.join("Shared/own.wav");
        fs::create_dir_all(own.parent().unwrap()).unwrap();
        fs::write(&own, b"owned audio").unwrap();
        let (_, listed) = roots.browse(own.parent().unwrap(), &cancel).unwrap();
        assert_eq!(listed.len(), 1);
        roots.delete(&own, true, &cancel).unwrap();
        assert!(!own.exists());
        fs::remove_dir_all(root).unwrap();
    }
}

fn statistics(library: &Library, cancel: &AtomicBool) -> Vec<ViewRow> {
    let mut rows = vec![
        text("Songs", library.tracks.len().to_string()),
        text("Albums", library.albums.len().to_string()),
        text("Artists", library.artists.len().to_string()),
    ];
    let mut seconds = 0.0;
    let mut formats = BTreeMap::<String, usize>::new();
    for track in &library.tracks {
        if cancel.load(Ordering::Acquire) {
            return rows;
        }
        seconds += track.duration.max(0.0);
        let format = track
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("Unknown")
            .to_uppercase();
        *formats.entry(format).or_default() += 1;
    }
    let seconds = seconds as u64;
    rows.push(text(
        "Total Playing Time",
        format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
    ));
    for (format, count) in formats {
        rows.push(text(format, format!("{count} songs")));
    }
    rows
}
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes / 1024)
    }
}
