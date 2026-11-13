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
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    /// Recursive bytes and direct child count arrive from the file worker.
    pub folder: Option<(u64, usize)>,
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
    pub value: String,
    pub entry: Option<Entry>,
}
fn text(title: impl Into<String>, subtitle: impl Into<String>) -> ViewRow {
    ViewRow {
        title: title.into(),
        subtitle: subtitle.into(),
        value: String::new(),
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
        generation: u64,
        initial: bool,
    },
    Info {
        scanned: bool,
        rows: Arc<Vec<ViewRow>>,
    },
    Space(Space),
    Deleted,
    Error(String),
}
struct Request {
    command: Command,
    generation: u64,
}
pub struct FileClient {
    tx: Option<mpsc::SyncSender<Request>>,
    rx: Option<mpsc::Receiver<Event>>,
    cancelled: Arc<AtomicBool>,
    browse_generation: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}
impl FileClient {
    pub fn new(music: PathBuf, data: PathBuf) -> Self {
        let (tx, commands) = mpsc::sync_channel::<Request>(8);
        let (events, rx) = mpsc::sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let browse_generation = Arc::new(AtomicU64::new(0));
        let epoch = browse_generation.clone();
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
                let mut folders = BTreeMap::<PathBuf, (u64, usize)>::new();
                while let Ok(Request {
                    command,
                    generation,
                }) = commands.recv()
                {
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let result = match command {
                        Command::Browse(path) => {
                            if epoch.load(Ordering::Acquire) != generation {
                                continue;
                            }
                            match roots.browse(&path, &flag) {
                                Err(_) if epoch.load(Ordering::Acquire) != generation => continue,
                                Err(error) => Err(error),
                                Ok((path, mut entries)) => {
                                    for entry in &mut entries {
                                        if entry.directory {
                                            entry.folder = folders.get(&entry.path).copied();
                                        }
                                    }
                                    let rows = Arc::new(browser_rows(&entries));
                                    retain_rows(&mut retired, &rows);
                                    if events
                                        .send(Event::Listed {
                                            path: path.clone(),
                                            rows,
                                            generation,
                                            initial: true,
                                        })
                                        .is_err()
                                    {
                                        break;
                                    }
                                    let mut changed = false;
                                    let mut last = std::time::Instant::now();
                                    for index in 0..entries.len() {
                                        if flag.load(Ordering::Acquire)
                                            || epoch.load(Ordering::Acquire) != generation
                                        {
                                            break;
                                        }
                                        if entries[index].directory
                                            && entries[index].folder.is_none()
                                        {
                                            if let Ok(info) = roots
                                                .measure(&entries[index].path, &|| {
                                                    check_browse(&flag, &epoch, generation)
                                                })
                                            {
                                                folders.insert(entries[index].path.clone(), info);
                                                entries[index].folder = Some(info);
                                                changed = true;
                                            }
                                        }
                                        if changed
                                            && (last.elapsed()
                                                >= std::time::Duration::from_millis(250)
                                                || index + 1 == entries.len())
                                        {
                                            let rows = Arc::new(browser_rows(&entries));
                                            retain_rows(&mut retired, &rows);
                                            if events
                                                .send(Event::Listed {
                                                    path: path.clone(),
                                                    rows,
                                                    generation,
                                                    initial: false,
                                                })
                                                .is_err()
                                            {
                                                return;
                                            }
                                            changed = false;
                                            last = std::time::Instant::now();
                                        }
                                    }
                                    continue;
                                }
                            }
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
                                rows.push(value(
                                    "Free space",
                                    format!(
                                        "{} of {}",
                                        format_size(space.free),
                                        format_size(space.total)
                                    ),
                                ));
                            }
                            if let Ok(bytes) = database_size(&roots.data, &|| check(&flag)) {
                                rows.push(value("Library data", format_size(bytes)));
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
                                let rows = scanned_rows(&library, &roots.music);
                                scanned_cache = Some((library, Arc::new(rows)));
                            }
                            Ok(Event::Info {
                                scanned: true,
                                rows: scanned_cache.as_ref().unwrap().1.clone(),
                            })
                        }
                        Command::Delete { path, confirmed } => {
                            let result = roots.delete(&path, confirmed, &flag);
                            folders.retain(|cached, _| {
                                !cached.starts_with(&path) && !path.starts_with(cached)
                            });
                            result.map(|_| Event::Deleted)
                        }
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
            browse_generation,
            worker,
        }
    }
    pub fn submit(&self, command: Command) -> Result<(), String> {
        let generation = if matches!(command, Command::Browse(_)) {
            self.browse_generation.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            self.browse_generation.load(Ordering::Acquire)
        };
        self.tx
            .as_ref()
            .ok_or("File browser is shut down")?
            .try_send(Request {
                command,
                generation,
            })
            .map_err(|_| "File browser is busy".into())
    }
    pub fn browse_generation(&self) -> u64 {
        self.browse_generation.load(Ordering::Acquire)
    }
    pub fn cancel_browse(&self) {
        self.browse_generation.fetch_add(1, Ordering::AcqRel);
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
            if kind.is_symlink() || !kind.is_dir() && !kind.is_file() {
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
                folder: None,
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
                folder: None,
            });
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok((path, entries))
    }
    fn measure(
        &self,
        path: &Path,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<(u64, usize), String> {
        let path = self.allowed(path)?;
        measure_tree(&path, check, &|child| {
            self.protected.iter().any(|p| {
                !(p == &self.data && child.starts_with(&self.downloads))
                    && (child == p || child.starts_with(p))
            })
        })
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
        total: (stat.f_blocks as u64).saturating_mul(stat.f_frsize),
        free: (stat.f_bavail as u64).saturating_mul(stat.f_frsize),
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
        value("Songs", library.tracks.len().to_string()),
        value("Albums", library.albums.len().to_string()),
        value("Artists", library.artists.len().to_string()),
    ];
    if cancel.load(Ordering::Acquire) {
        return rows;
    }
    if let Some(stats) = &library.scan_stats {
        rows.extend([
            value("Last scan", format_scan_date(stats.when_unix)),
            value("Scan time", format!("{:.1} s", stats.took_seconds)),
            value("Files in folder", stats.files.to_string()),
            value("Audio files", stats.audio_files.to_string()),
        ]);
        if !stats.by_format.is_empty() {
            rows.push(text("Formats", format_list(&stats.by_format)));
        }
        let skipped: usize = stats.skipped_formats.values().sum();
        rows.push(value("Not supported", skipped.to_string()));
        if skipped > 0 {
            rows.push(text("Skipped formats", format_list(&stats.skipped_formats)));
        }
        rows.push(value("Music size", format_size(stats.audio_bytes)));
    }
    rows
}
fn value(title: impl Into<String>, value: impl Into<String>) -> ViewRow {
    let mut row = text(title, "");
    row.value = value.into();
    row
}
fn format_list(formats: &BTreeMap<String, usize>) -> String {
    let mut entries: Vec<_> = formats.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    entries
        .iter()
        .map(|(key, count)| format!("{key} {count}"))
        .collect::<Vec<_>>()
        .join(" · ")
}
fn format_scan_date(seconds: u64) -> String {
    let stamp = seconds.min(i64::MAX as u64) as libc::time_t;
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    if unsafe { libc::localtime_r(&stamp, local.as_mut_ptr()) }.is_null() {
        return String::new();
    }
    let local = unsafe { local.assume_init() };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{:02} {} {}, {:02}:{:02}",
        local.tm_mday,
        months.get(local.tm_mon as usize).unwrap_or(&""),
        local.tm_year + 1900,
        local.tm_hour,
        local.tm_min
    )
}
fn scanned_rows(library: &Library, root: &Path) -> Vec<ViewRow> {
    let mut folders: Vec<_> = library.scan_folders.iter().collect();
    folders.sort_by_key(|f| f.path.to_string_lossy().to_lowercase());
    let parents: std::collections::HashSet<_> =
        folders.iter().filter_map(|f| f.path.parent()).collect();
    folders
        .iter()
        .map(|folder| {
            let relative = folder.path.strip_prefix(root).unwrap_or(&folder.path);
            let title = if relative.as_os_str().is_empty() {
                "(music folder)".into()
            } else {
                relative.to_string_lossy().into_owned()
            };
            let subtitle = if let Some(note) = &folder.skipped {
                note.clone()
            } else if folder.other > 0 {
                format!(
                    "{} other {}",
                    folder.other,
                    if folder.other == 1 { "file" } else { "files" }
                )
            } else if folder.tracks == 0 {
                if parents.contains(folder.path.as_path()) {
                    "Only subfolders"
                } else {
                    "Empty"
                }
                .into()
            } else {
                String::new()
            };
            let mut row = text(title, subtitle);
            if folder.tracks > 0 {
                row.value = format!(
                    "{} {}",
                    folder.tracks,
                    if folder.tracks == 1 { "song" } else { "songs" }
                );
            }
            row
        })
        .collect()
}
fn database_size(data: &Path, check: &dyn Fn() -> Result<(), String>) -> Result<u64, String> {
    let mut size = 0u64;
    for name in crate::maintenance::DATA_ITEMS {
        check()?;
        let path = data.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        let bytes = if metadata.is_dir() {
            measure_tree(&path, check, &|_| false)?.0
        } else if metadata.is_file() {
            metadata.len()
        } else {
            continue;
        };
        size = size.checked_add(bytes).ok_or("Database size overflow")?;
    }
    Ok(size)
}
fn retain_rows(retired: &mut VecDeque<Arc<Vec<ViewRow>>>, rows: &Arc<Vec<ViewRow>>) {
    retired.push_back(rows.clone());
    while retired.len() > 3 {
        retired.pop_front();
    }
}
fn browser_rows(entries: &[Entry]) -> Vec<ViewRow> {
    entries
        .iter()
        .cloned()
        .map(|entry| {
            let subtitle = match entry.folder {
                Some((_, count)) => format!(
                    "Folder · {count} {}",
                    if count == 1 { "item" } else { "items" }
                ),
                None if entry.directory => "Folder".into(),
                None => String::new(),
            };
            let value = if entry.directory {
                entry
                    .folder
                    .map(|(bytes, _)| format_size(bytes))
                    .unwrap_or_else(|| "…".into())
            } else {
                format_size(entry.size)
            };
            ViewRow {
                title: entry.name.clone(),
                subtitle,
                value,
                entry: Some(entry),
            }
        })
        .collect()
}
fn check_browse(cancel: &AtomicBool, epoch: &AtomicU64, generation: u64) -> Result<(), String> {
    check(cancel)?;
    if epoch.load(Ordering::Acquire) != generation {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}
/// Read-only traversal never follows links or enters protected application data.
/// Errors stay unknown instead of reporting a silently truncated byte count.
fn measure_tree(
    path: &Path,
    check: &dyn Fn() -> Result<(), String>,
    protected: &dyn Fn(&Path) -> bool,
) -> Result<(u64, usize), String> {
    let mut pending = vec![path.to_path_buf()];
    let mut bytes = 0u64;
    let mut direct = 0usize;
    while let Some(directory) = pending.pop() {
        check()?;
        for child in fs::read_dir(&directory).map_err(|e| e.to_string())? {
            check()?;
            let child = child.map_err(|e| e.to_string())?;
            let kind = child.file_type().map_err(|e| e.to_string())?;
            let childpath = child.path();
            if protected(&childpath) || kind.is_symlink() || !kind.is_dir() && !kind.is_file() {
                continue;
            }
            if directory == path {
                direct += 1;
            }
            if kind.is_dir() {
                pending.push(childpath);
            } else {
                bytes = bytes
                    .checked_add(child.metadata().map_err(|e| e.to_string())?.len())
                    .ok_or("Folder size overflow")?;
            }
        }
    }
    Ok((bytes, direct))
}
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.0} KB", bytes as f64 / (1u64 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    static ID: AtomicU64 = AtomicU64::new(0);
    fn fixture() -> (PathBuf, Roots) {
        let root = std::env::temp_dir().join(format!(
            "betterpod-files-parity-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let music = root.join("music");
        let data = music.join("betterpod-rust");
        fs::create_dir_all(music.join("Album/sub")).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::write(music.join("Album/song.flac"), b"song").unwrap();
        fs::write(music.join("Album/cover.jpg"), b"cover").unwrap();
        fs::write(music.join("Album/.lyrics.lrc"), b"lyrics").unwrap();
        fs::write(music.join("Album/sub/note.txt"), b"note").unwrap();
        let roots = Roots::new(music, data).unwrap();
        (root, roots)
    }
    #[test]
    fn browse_lists_non_audio_and_hidden_files_then_measures_without_following_links() {
        let (root, roots) = fixture();
        let flag = AtomicBool::new(false);
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), b"not counted").unwrap();
        std::os::unix::fs::symlink(&outside, roots.music.join("Album/linked")).unwrap();
        let (_, entries) = roots.browse(&roots.music.join("Album"), &flag).unwrap();
        assert_eq!(entries.len(), 4);
        assert!(entries[0].directory);
        assert!(entries.iter().any(|e| e.name == ".lyrics.lrc"));
        assert!(entries.iter().any(|e| e.name == "cover.jpg" && !e.audio));
        assert_eq!(
            roots
                .measure(&roots.music.join("Album"), &|| Ok(()))
                .unwrap(),
            (19, 4)
        );
        assert!(roots
            .browse(&roots.music, &flag)
            .unwrap()
            .1
            .iter()
            .all(|e| e.path != roots.data));
        let epoch = AtomicU64::new(2);
        assert!(roots
            .measure(&roots.music.join("Album"), &|| check_browse(
                &flag, &epoch, 1
            ))
            .is_err());
        assert!(outside.join("secret").exists());
        fs::write(roots.data.join("settings.json"), b"{}").unwrap();
        fs::write(roots.data.join("unrelated-file"), b"must not count").unwrap();
        assert_eq!(database_size(&roots.data, &|| Ok(())).unwrap(), 2);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn source_scan_rows_keep_counts_format_order_and_folder_reasons() {
        let mut library = Library::default();
        let root = PathBuf::from("/music");
        library.scan_folders = vec![
            crate::library::ScanFolder {
                path: root.clone(),
                ..Default::default()
            },
            crate::library::ScanFolder {
                path: root.join("Album"),
                tracks: 2,
                other: 1,
                skipped: None,
            },
            crate::library::ScanFolder {
                path: root.join("Empty"),
                ..Default::default()
            },
            crate::library::ScanFolder {
                path: root.join("Link"),
                skipped: Some("Symbolic link".into()),
                ..Default::default()
            },
        ];
        library.scan_stats = Some(crate::library::ScanStatistics {
            files: 7,
            audio_files: 2,
            audio_bytes: 1024,
            by_format: BTreeMap::from([("FLAC".into(), 2), ("MP3".into(), 4), ("AAC".into(), 2)]),
            skipped_formats: BTreeMap::from([("XYZ".into(), 1)]),
            ..Default::default()
        });
        let rows = scanned_rows(&library, &root);
        assert_eq!(rows[0].title, "(music folder)");
        assert_eq!(rows[0].subtitle, "Only subfolders");
        assert_eq!(rows[1].value, "2 songs");
        assert_eq!(rows[1].subtitle, "1 other file");
        assert_eq!(rows[2].subtitle, "Empty");
        assert_eq!(rows[3].subtitle, "Symbolic link");
        let rows = statistics(&library, &AtomicBool::new(false));
        assert_eq!(
            rows.iter().find(|r| r.title == "Formats").unwrap().subtitle,
            "MP3 4 · AAC 2 · FLAC 2"
        );
        assert_eq!(
            rows.iter()
                .find(|r| r.title == "Not supported")
                .unwrap()
                .value,
            "1"
        );
        assert!(!rows.iter().any(|r| r.title == "Total Playing Time"));
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1536), "2 KB");
    }
    #[test]
    fn worker_emits_fast_snapshot_then_measured_snapshot_and_drops_old_generation() {
        let (root, roots) = fixture();
        let mut client = FileClient::new(roots.music.clone(), roots.data.clone());
        client.submit(Command::Browse(roots.music.clone())).unwrap();
        let expected = client.browse_generation();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let (mut first, mut measured) = (false, false);
        while std::time::Instant::now() < deadline && !measured {
            if let Some(Event::Listed {
                rows,
                generation,
                initial,
                ..
            }) = client.poll()
            {
                assert_eq!(generation, expected);
                let album = rows.iter().find(|r| r.title == "Album").unwrap();
                if initial {
                    first = true;
                    assert_eq!(album.value, "…");
                } else {
                    measured = true;
                    assert_eq!(album.value, "19 B");
                    assert_eq!(album.subtitle, "Folder · 4 items");
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        assert!(first && measured);
        client.cancel_browse();
        assert_ne!(expected, client.browse_generation());
        client.shutdown();
        fs::remove_dir_all(root).unwrap();
    }
}
