//! Private Rust-app state. Ordinary saves never wait for filesystem I/O.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_WAIT: Duration = Duration::from_secs(2);
const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub volume: f32,
    pub theme: String,
    pub shuffle: bool,
    pub shuffle_mode: String,
    pub beta_updates: bool,
    pub repeat: String,
    pub screensaver_seconds: u32,
    pub apple: bool,
    pub hide_podcasts: bool,
    pub youtube: bool,
    pub screensaver_style: String,
    pub screensaver_effect: u32,
    pub screensaver_colors: u32,
    pub game_clock: Vec<String>,
    pub hold_brightness: u8,
    pub screensaver_dim: u8,
    pub sharing_name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 0.7,
            theme: "light".into(),
            shuffle: false,
            shuffle_mode: "off".into(),
            beta_updates: false,
            repeat: "off".into(),
            screensaver_seconds: 0,
            apple: false,
            hide_podcasts: false,
            youtube: false,
            screensaver_style: "cover".into(),
            screensaver_effect: 0,
            screensaver_colors: 0,
            game_clock: Vec::new(),
            hold_brightness: 15,
            screensaver_dim: 0,
            sharing_name: String::new(),
        }
    }
}

impl Settings {
    fn normalize(mut self) -> Self {
        if !self.volume.is_finite() {
            self.volume = Self::default().volume;
        }
        self.volume = self.volume.clamp(0.0, 1.0);
        if !matches!(self.theme.as_str(), "light" | "dark" | "auto") {
            self.theme = "light".into();
        }
        if !matches!(self.shuffle_mode.as_str(), "off" | "songs" | "albums") {
            self.shuffle_mode = "off".into();
        }
        if self.shuffle && self.shuffle_mode == "off" {
            self.shuffle_mode = "songs".into();
        }
        if !self.shuffle {
            self.shuffle_mode = "off".into();
        }
        if !matches!(self.repeat.as_str(), "off" | "all" | "one") {
            self.repeat = "off".into();
        }
        if !matches!(self.screensaver_style.as_str(), "cover" | "viz") {
            self.screensaver_style = "cover".into();
        }
        self.hold_brightness = self.hold_brightness.clamp(1, 100);
        self.screensaver_dim = self.screensaver_dim.min(100);
        self.screensaver_effect = self.screensaver_effect.min(31);
        self.screensaver_colors = self.screensaver_colors.min(31);
        self.game_clock.truncate(32);
        self.game_clock
            .retain(|name| !name.is_empty() && name.len() <= 80);
        self.sharing_name = self
            .sharing_name
            .chars()
            .filter(|c| !c.is_control())
            .take(32)
            .collect::<String>()
            .trim()
            .to_string();
        self
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Playback {
    pub queue: Vec<PathBuf>,
    pub index: usize,
    pub position: f64,
    pub paused: bool,
    /// Physical list before shuffle, needed to restore Shuffle Off after restart.
    pub original_queue: Option<Vec<PathBuf>>,
}

impl Playback {
    fn normalize(mut self) -> Option<Self> {
        if self.queue.is_empty() {
            return None;
        }
        self.index = self.index.min(self.queue.len() - 1);
        if !self.position.is_finite() || self.position < 0.0 {
            self.position = 0.0;
        }
        if self.original_queue.as_ref().is_some_and(|original| {
            if original.len() != self.queue.len() {
                return true;
            }
            let mut a = original.iter().collect::<Vec<_>>();
            let mut b = self.queue.iter().collect::<Vec<_>>();
            a.sort();
            b.sort();
            a != b
        }) {
            self.original_queue = None;
        }
        Some(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Kind {
    Settings,
    Playback,
    Games,
}

impl Kind {
    fn filename(self) -> &'static str {
        match self {
            Self::Settings => "settings.json",
            Self::Playback => "playback.json",
            Self::Games => "games.json",
        }
    }
}

#[derive(Clone)]
enum Payload {
    Settings(Settings),
    Playback {
        queue: Arc<Vec<PathBuf>>,
        index: usize,
        position: f64,
        paused: bool,
        original_queue: Option<Arc<Vec<PathBuf>>>,
    },
    Bytes(Vec<u8>),
}
impl Payload {
    fn bytes(&self) -> Result<Cow<'_, [u8]>, String> {
        #[derive(Serialize)]
        struct PlaybackRef<'a> {
            queue: &'a [PathBuf],
            index: usize,
            position: f64,
            paused: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            original_queue: Option<&'a [PathBuf]>,
        }
        let bytes = match self {
            Self::Settings(settings) => {
                Cow::Owned(serde_json::to_vec(settings).map_err(|e| e.to_string())?)
            }
            Self::Playback {
                queue,
                index,
                position,
                paused,
                original_queue,
            } => Cow::Owned(
                serde_json::to_vec(&PlaybackRef {
                    queue: queue.as_slice(),
                    index: *index,
                    position: *position,
                    paused: *paused,
                    original_queue: original_queue.as_ref().map(|q| q.as_slice()),
                })
                .map_err(|e| e.to_string())?,
            ),
            Self::Bytes(bytes) => Cow::Borrowed(bytes.as_slice()),
        };
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err("State snapshot exceeds the size limit".into());
        }
        Ok(bytes)
    }
}
#[derive(Clone)]
struct Snapshot {
    sequence: u64,
    payload: Payload,
}

#[derive(Default)]
struct WriterState {
    sequence: u64,
    completed: u64,
    pending: BTreeMap<Kind, Snapshot>,
    failed: BTreeMap<Kind, (Snapshot, String)>,
    first: Option<Instant>,
    last: Option<Instant>,
    barriers: usize,
    closing: bool,
    stopped: bool,
    retired: Vec<Snapshot>,
}

struct Shared {
    state: Mutex<WriterState>,
    changed: Condvar,
}

pub struct Store {
    root: PathBuf,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| format!("Create Rust data folder: {e}"))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(WriterState::default()),
            changed: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let worker_root = root.clone();
        let worker = thread::Builder::new()
            .name("state-writer".into())
            .spawn(move || writer_loop(worker_root, worker_shared))
            .map_err(|e| format!("Start state writer: {e}"))?;
        Ok(Self {
            root,
            shared,
            worker: Some(worker),
        })
    }

    pub fn load_settings(&self) -> Settings {
        read_json::<Settings>(&self.root.join(Kind::Settings.filename()))
            .unwrap_or_default()
            .normalize()
    }

    pub fn load_playback(&self) -> Option<Playback> {
        read_json::<Playback>(&self.root.join(Kind::Playback.filename()))
            .and_then(Playback::normalize)
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        self.enqueue(
            Kind::Settings,
            Payload::Settings(settings.clone().normalize()),
        )
    }

    pub fn save_playback(&self, playback: &Playback) -> Result<(), String> {
        self.save_playback_shared_order(
            Arc::new(playback.queue.clone()),
            playback.index,
            playback.position,
            playback.paused,
            playback
                .original_queue
                .as_ref()
                .map(|queue| Arc::new(queue.clone())),
        )
    }
    /// Constant-size enqueue for the UI. Cache this immutable queue Arc until
    /// the queue changes; only the writer walks paths and serializes JSON.
    pub fn save_playback_shared(
        &self,
        queue: Arc<Vec<PathBuf>>,
        index: usize,
        position: f64,
        paused: bool,
    ) -> Result<(), String> {
        self.save_playback_shared_order(queue, index, position, paused, None)
    }
    /// Both immutable orders are retained by Arc; their path traversal and
    /// JSON serialization belong exclusively to the background state writer.
    pub fn save_playback_shared_order(
        &self,
        queue: Arc<Vec<PathBuf>>,
        index: usize,
        position: f64,
        paused: bool,
        original_queue: Option<Arc<Vec<PathBuf>>>,
    ) -> Result<(), String> {
        let index = index.min(queue.len().saturating_sub(1));
        let position = if position.is_finite() && position >= 0.0 {
            position
        } else {
            0.0
        };
        self.enqueue(
            Kind::Playback,
            Payload::Playback {
                queue,
                index,
                position,
                paused,
                original_queue,
            },
        )
    }
    pub fn load_games_snapshot(&self) -> Option<Vec<u8>> {
        let path = self.root.join(Kind::Games.filename());
        if fs::metadata(&path).ok()?.len() > MAX_STATE_BYTES {
            return None;
        }
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
        Some(bytes)
    }

    pub fn save_games_snapshot(&self, snapshot: &[u8]) -> Result<(), String> {
        if snapshot.len() as u64 > MAX_STATE_BYTES {
            return Err("Game snapshot is too large".into());
        }
        serde_json::from_slice::<serde_json::Value>(snapshot).map_err(|error| error.to_string())?;
        self.enqueue(Kind::Games, Payload::Bytes(snapshot.to_vec()))
    }

    fn enqueue(&self, kind: Kind, payload: Payload) -> Result<(), String> {
        let mut state = self.shared.state.lock().map_err(|e| e.to_string())?;
        if state.closing || state.stopped {
            return Err("State writer has stopped".into());
        }
        let now = Instant::now();
        if state.pending.is_empty() {
            state.first = Some(now);
        }
        state.last = Some(now);
        state.sequence += 1;
        let sequence = state.sequence;
        if let Some(old) = state.pending.insert(kind, Snapshot { sequence, payload }) {
            state.retired.push(old);
        }
        if let Some((old, _)) = state.failed.remove(&kind) {
            state.retired.push(old);
        }
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// A durability barrier for shutdown and explicit operations, never per frame.
    /// A previous failed snapshot is retried once by this explicit barrier.
    pub fn flush(&self) -> Result<(), String> {
        let mut state = self.shared.state.lock().map_err(|e| e.to_string())?;
        if !state.failed.is_empty() && !state.stopped {
            let failed = std::mem::take(&mut state.failed);
            let now = Instant::now();
            if state.pending.is_empty() {
                state.first = Some(now);
            }
            state.last = Some(now);
            for (kind, (mut snapshot, _)) in failed {
                if state.pending.contains_key(&kind) {
                    state.retired.push(snapshot);
                    continue;
                }
                state.sequence += 1;
                snapshot.sequence = state.sequence;
                state.pending.insert(kind, snapshot);
            }
        }
        let target = state.sequence;
        state.barriers += 1;
        self.shared.changed.notify_all();
        while state.completed < target && !state.stopped {
            state = self.shared.changed.wait(state).map_err(|e| e.to_string())?;
        }
        state.barriers -= 1;
        if state.completed < target {
            return Err("State writer stopped before flushing".into());
        }
        if let Some((_, (_, error))) = state.failed.iter().next() {
            return Err(error.clone());
        }
        Ok(())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(error) = self.flush() {
            eprintln!("State flush at shutdown: {error}");
        }
        if let Ok(mut state) = self.shared.state.lock() {
            state.closing = true;
        }
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn writer_loop(root: PathBuf, shared: Arc<Shared>) {
    loop {
        let mut state = match shared.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        // Releasing a superseded large queue is also worker work. Never drop
        // its final Arc while holding the mutex shared with UI enqueues.
        if !state.retired.is_empty() {
            let retired = std::mem::take(&mut state.retired);
            drop(state);
            drop(retired);
            continue;
        }
        while state.pending.is_empty() && state.retired.is_empty() && !state.closing {
            state = match shared.changed.wait(state) {
                Ok(state) => state,
                Err(_) => return,
            };
        }
        if !state.retired.is_empty() {
            drop(state);
            continue;
        }
        if state.pending.is_empty() && state.closing {
            state.stopped = true;
            shared.changed.notify_all();
            return;
        }
        if !state.closing && state.barriers == 0 {
            let now = Instant::now();
            let deadline = state.last.unwrap_or(now) + DEBOUNCE;
            let limit = state.first.unwrap_or(now) + MAX_WAIT;
            let delay = deadline.min(limit).saturating_duration_since(now);
            if !delay.is_zero() {
                match shared.changed.wait_timeout(state, delay) {
                    Ok(_) => continue,
                    Err(_) => return,
                }
            }
        }
        let pending = std::mem::take(&mut state.pending);
        state.first = None;
        state.last = None;
        drop(state);

        // No mutex is held during mkdir/write/fsync/rename.
        let mut completed = 0;
        let mut results = Vec::with_capacity(pending.len());
        for (kind, snapshot) in pending {
            completed = completed.max(snapshot.sequence);
            let result = snapshot
                .payload
                .bytes()
                .and_then(|bytes| {
                    atomic_write(&root.join(kind.filename()), &bytes).map_err(|e| e.to_string())
                })
                .map_err(|e| format!("Save {}: {e}", kind.filename()));
            results.push((kind, snapshot, result));
        }
        let mut state = match shared.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        for (kind, snapshot, result) in results {
            match result {
                Ok(()) => {
                    if let Some((old, _)) = state.failed.remove(&kind) {
                        state.retired.push(old);
                    }
                    state.retired.push(snapshot);
                }
                Err(error) => {
                    eprintln!("{error}");
                    // A newer queued value supersedes this failed one.
                    if !state.pending.contains_key(&kind) {
                        if let Some((old, _)) = state.failed.insert(kind, (snapshot, error)) {
                            state.retired.push(old);
                        }
                    } else {
                        state.retired.push(snapshot);
                    }
                }
            }
        }
        state.completed = state.completed.max(completed);
        shared.changed.notify_all();
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_STATE_BYTES || !metadata.is_file() {
        return None;
    }
    serde_json::from_reader(File::open(path).ok()?).ok()
}

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Persist directory-entry changes after rename/unlink/create. Linux falls
/// back to syncfs only when a filesystem does not implement directory fsync;
/// real I/O errors are propagated. Other Unix platforms use directory fsync
/// where supported and otherwise provide best effort (no portable syncfs).
pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let directory = File::open(path)?;
        if !directory.metadata()?.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "Expected a directory to sync",
            ));
        }
        sync_open_directory(&directory)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
#[cfg(unix)]
pub(crate) fn sync_open_directory(directory: &File) -> std::io::Result<()> {
    match directory.sync_all() {
        Err(error)
            if error
                .raw_os_error()
                .is_some_and(|code| code == libc::EINVAL || code == libc::ENOTSUP) =>
        {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd;
                loop {
                    if unsafe { libc::syncfs(directory.as_raw_fd()) } == 0 {
                        return Ok(());
                    }
                    let error = std::io::Error::last_os_error();
                    if error.kind() != std::io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                Ok(())
            }
        }
        result => result,
    }
}
/// Newly created directory links must also be durable, not only the file in
/// their deepest child. Used for private writes and verified transfer targets.
pub(crate) fn create_dir_all_synced(path: &Path) -> std::io::Result<()> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        let lookup = if current.as_os_str().is_empty() {
            Path::new(".")
        } else {
            current
        };
        match fs::metadata(lookup) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "Directory parent is a file",
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current.parent().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Directory has no existing parent",
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
    fs::create_dir_all(path)?;
    for directory in missing.into_iter().rev() {
        let parent = directory
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        sync_directory(parent)?;
        sync_directory(&directory)?;
    }
    Ok(())
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with(path, |file| file.write_all(bytes))
}

/// Stream a snapshot into the same durable, private atomic replacement.
/// A failed encoder leaves the previous file intact and removes the temporary.
pub(crate) fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Path has no parent")
    })?;
    create_dir_all_synced(parent)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut temporary = None;
    for _ in 0..32 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), id));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temporary, mut file) = temporary.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "No temporary file available",
        )
    })?;
    let result = (|| {
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        sync_directory(if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Write inside an already-open directory. Pinning its descriptor prevents a
/// renamed directory or a substituted symlink from redirecting private writes.
#[cfg(unix)]
pub(crate) fn atomic_write_at(
    directory: &File,
    name: &std::ffi::OsStr,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let name = CString::new(name.as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Filename contains a NUL")
    })?;
    if name.as_bytes().contains(&b'/') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Expected one filename",
        ));
    }
    let dirfd = directory.as_raw_fd();
    for _ in 0..32 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = CString::new(format!(".write-{}-{id}.tmp", std::process::id())).unwrap();
        let fd = unsafe {
            libc::openat(
                dirfd,
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(error);
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            if unsafe { libc::renameat(dirfd, temporary.as_ptr(), dirfd, name.as_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            sync_open_directory(directory)
        })();
        if result.is_err() {
            unsafe { libc::unlinkat(dirfd, temporary.as_ptr(), 0) };
        }
        return result;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "No temporary file available",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_shuffle_order_survives_restart_and_plain_saves_clear_it() {
        let root = temporary("physical-shuffle-order");
        let physical = Arc::new(vec![
            PathBuf::from("/music/a.flac"),
            PathBuf::from("/music/b.flac"),
            PathBuf::from("/music/a.flac"),
            PathBuf::from("/music/c.flac"),
        ]);
        let playing = Arc::new(vec![
            physical[3].clone(),
            physical[0].clone(),
            physical[1].clone(),
            physical[2].clone(),
        ]);
        {
            let store = Store::new(root.clone()).unwrap();
            store
                .save_playback_shared_order(playing.clone(), 1, 37.25, true, Some(physical.clone()))
                .unwrap();
            store.flush().unwrap();
        }
        let store = Store::new(root.clone()).unwrap();
        let loaded = store.load_playback().unwrap();
        assert_eq!(loaded.queue, *playing);
        assert_eq!(loaded.original_queue.as_ref(), Some(physical.as_ref()));
        assert_eq!(loaded.index, 1);
        assert_eq!(loaded.position, 37.25);
        assert!(loaded.paused);
        // The compatibility wrapper starts a new unshuffled snapshot; it may
        // not leave a stale physical queue from a prior coalesced payload.
        store
            .save_playback_shared(physical.clone(), 2, 0.0, false)
            .unwrap();
        store.flush().unwrap();
        let loaded = store.load_playback().unwrap();
        assert_eq!(loaded.queue, *physical);
        assert!(loaded.original_queue.is_none());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn playback_defaults_old_snapshots_and_rejects_foreign_physical_members() {
        let legacy: Playback =
            serde_json::from_str(r#"{"queue":["/music/a"],"index":0,"position":5,"paused":true}"#)
                .unwrap();
        assert!(legacy.normalize().unwrap().original_queue.is_none());
        let malformed = Playback {
            queue: vec!["/music/a".into(), "/music/b".into()],
            original_queue: Some(vec!["/music/a".into(), "/other/x".into()]),
            ..Default::default()
        }
        .normalize()
        .unwrap();
        assert!(malformed.original_queue.is_none());
        assert_eq!(malformed.queue.len(), 2);
    }

    fn temporary(name: &str) -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("betterpod-rust-{name}-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn sharing_name_stays_unset_until_chosen_and_bounds_unicode_without_splitting() {
        assert!(Settings::default().normalize().sharing_name.is_empty());
        let mut settings = Settings::default();
        settings.sharing_name = "é".repeat(40);
        assert_eq!(settings.normalize().sharing_name, "é".repeat(32));
        let mut settings = Settings::default();
        settings.sharing_name = " \n ".into();
        assert!(settings.normalize().sharing_name.is_empty());
    }

    #[test]
    fn burst_and_playback_are_durable_without_mutating_snapshots() {
        let root = temporary("snapshots");
        {
            let store = Store::new(root.clone()).unwrap();
            let mut settings = Settings::default();
            for step in 0..100 {
                settings.volume = step as f32 / 100.0;
                store.save_settings(&settings).unwrap();
            }
            settings.theme = "dark".into();
            settings.screensaver_dim = 0;
            store.save_settings(&settings).unwrap();
            settings.theme = "light".into();
            let mut playback = Playback {
                queue: vec![PathBuf::from("/music/album/song.flac")],
                position: 12.5,
                paused: true,
                ..Default::default()
            };
            store.save_playback(&playback).unwrap();
            playback.queue.clear();
            store.flush().unwrap();
            let loaded = store.load_settings();
            assert_eq!(loaded.theme, "dark");
            assert_eq!(loaded.screensaver_dim, 0);
            assert!((loaded.volume - 0.99).abs() < f32::EPSILON);
            let loaded = store.load_playback().unwrap();
            assert_eq!(loaded.queue.len(), 1);
            assert_eq!(loaded.position, 12.5);
            assert!(loaded.paused);
        }
        assert!(fs::read_dir(&root).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_write_is_retried_by_a_later_barrier() {
        let root = temporary("retry");
        let destination = root.join("settings.json");
        fs::create_dir(&destination).unwrap();
        {
            let store = Store::new(root.clone()).unwrap();
            let settings = Settings {
                theme: "dark".into(),
                ..Default::default()
            };
            store.save_settings(&settings).unwrap();
            assert!(store.flush().is_err());
            fs::remove_dir(&destination).unwrap();
            store.flush().unwrap();
            assert_eq!(store.load_settings().theme, "dark");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn drop_flushes_and_empty_queue_does_not_restore_old_playback() {
        let root = temporary("drop");
        {
            let store = Store::new(root.clone()).unwrap();
            store
                .save_settings(&Settings {
                    shuffle: true,
                    ..Default::default()
                })
                .unwrap();
            store.save_playback(&Playback::default()).unwrap();
        }
        {
            let store = Store::new(root.clone()).unwrap();
            assert!(store.load_settings().shuffle);
            assert!(store.load_playback().is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn game_snapshot_is_private_coalesced_and_durable() {
        let root = temporary("games");
        {
            let store = Store::new(root.clone()).unwrap();
            assert!(store.save_games_snapshot(b"bad json").is_err());
            store.save_games_snapshot(br#"{"score":1}"#).unwrap();
            store.save_games_snapshot(br#"{"score":2}"#).unwrap();
            store.flush().unwrap();
            assert_eq!(store.load_games_snapshot().unwrap(), br#"{"score":2}"#);
        }
        let store = Store::new(root.clone()).unwrap();
        assert_eq!(store.load_games_snapshot().unwrap(), br#"{"score":2}"#);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bad_state_is_ignored_and_numeric_values_are_bounded() {
        let root = temporary("invalid");
        fs::write(
            root.join("settings.json"),
            br#"{"volume":9,"theme":"bad","repeat":"bad","screensaver_dim":255,"effects":"light"}"#,
        )
        .unwrap();
        fs::write(root.join("playback.json"), b"partial json").unwrap();
        {
            let store = Store::new(root.clone()).unwrap();
            let settings = store.load_settings();
            assert_eq!(settings.volume, 1.0);
            assert_eq!(settings.theme, "light");
            assert_eq!(settings.repeat, "off");
            assert_eq!(settings.screensaver_dim, 100);
            assert_eq!(Settings::default().screensaver_dim, 0);
            assert!(
                serde_json::to_value(&settings)
                    .unwrap()
                    .get("effects")
                    .is_none(),
                "Legacy Light mode is ignored and is no longer saved"
            );
            assert!(store.load_playback().is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shared_large_queue_coalesces_and_remains_an_immutable_snapshot() {
        let root = temporary("shared-queue");
        let store = Store::new(root.clone()).unwrap();
        let mut queue = Arc::new(
            (0..25000)
                .map(|i| PathBuf::from(format!("/music/album-{}/track-{i:05}.flac", i / 12)))
                .collect::<Vec<_>>(),
        );
        for i in 0..100 {
            store
                .save_playback_shared(queue.clone(), 25000, i as f64, i % 2 == 1)
                .unwrap();
        }
        // Mutating the caller's next queue cannot alter the captured snapshot.
        Arc::make_mut(&mut queue).clear();
        store.flush().unwrap();
        let playback = store.load_playback().unwrap();
        assert_eq!(playback.queue.len(), 25000);
        assert_eq!(playback.index, 24999);
        assert_eq!(playback.position, 99.0);
        assert!(playback.paused);
        store
            .save_playback_shared(queue, 3, f64::NAN, false)
            .unwrap();
        store.flush().unwrap();
        assert!(store.load_playback().is_none());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shared_queue_failure_retries_after_caller_releases_the_arc() {
        let root = temporary("shared-retry");
        fs::create_dir(root.join("playback.json")).unwrap();
        let store = Store::new(root.clone()).unwrap();
        let queue = Arc::new(vec![PathBuf::from("/music/song.flac")]);
        store
            .save_playback_shared(queue.clone(), 99, -1.0, true)
            .unwrap();
        drop(queue);
        assert!(store.flush().is_err());
        fs::remove_dir(root.join("playback.json")).unwrap();
        store.flush().unwrap();
        let playback = store.load_playback().unwrap();
        assert_eq!(playback.index, 0);
        assert_eq!(playback.position, 0.0);
        assert!(playback.paused);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
