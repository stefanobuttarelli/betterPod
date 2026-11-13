//! Isolated filesystem maintenance. A single worker does traversal, hashing,
//! copying and verification; the UI only submits commands and polls events.
use crate::storage::{atomic_write, create_dir_all_synced, sync_directory};
mod relocation;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub type JobId = u64;
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
const OWNER: &str = ".betterpod-rust.json";
pub(crate) const DATA_ITEMS: &[&str] = &[
    "library.json",
    "update-check.json",
    "settings.json",
    "playback.json",
    "weather.json",
    "podcasts.json",
    "video-history.json",
    "games.json",
    "recent-searches.json",
    "album-merges.json",
    "share-name.txt",
    "artwork",
    "artwork-scaled",
    "chosen_covers",
    "chosen-covers",
    "curation.json",
    "playlists",
    "games",
    "puzzles",
    "downloads",
    "videos",
    "podcasts",
    "tools",
    "incoming",
    "transfers",
];
#[derive(Clone, Debug)]
pub struct Config {
    pub bootstrap: PathBuf,
    pub music: PathBuf,
    pub data: PathBuf,
    pub protected: Vec<PathBuf>,
}
#[derive(Clone, Copy, Debug)]
pub enum ResetScope {
    /// Reset preferences, playback and game saves; keep the music index/caches.
    Preferences,
    /// Match Clear App Data: reset app state/caches, keep downloaded music/tools.
    AppData,
    /// Explicitly remove all owned private data, including downloads and tools.
    EverythingIncludingTools,
}
#[derive(Clone, Debug)]
pub enum Command {
    Browse(PathBuf),
    PendingTransfers,
    ResumeTransfer {
        manifest: PathBuf,
        confirmed: bool,
    },
    PrepareTransferredClients {
        manifest: PathBuf,
        quiescent: bool,
    },
    FinishTransferredMusic {
        manifest: PathBuf,
        clients_ready: bool,
    },
    SelectMusic(PathBuf),
    TransferMusic {
        destination: PathBuf,
        remove_source: bool,
        confirmed: bool,
    },
    MoveDatabase {
        destination: PathBuf,
        quiescent: bool,
    },
    ReplaceDatabase {
        destination: PathBuf,
        quiescent: bool,
        confirmed: bool,
    },
    UseDatabase {
        directory: PathBuf,
        quiescent: bool,
    },
    Reset {
        scope: ResetScope,
        confirmed: bool,
        quiescent: bool,
    },
    ClearArtwork {
        quiescent: bool,
    },
    DeleteAlbum {
        paths: Vec<PathBuf>,
        confirmed: bool,
    },
}
#[derive(Clone, Debug)]
pub struct FolderEntry {
    pub path: PathBuf,
    pub name: String,
    pub writable: bool,
    pub is_current_music: bool,
    pub is_current_data: bool,
}
#[derive(Clone, Debug)]
pub struct PendingTransfer {
    pub manifest: PathBuf,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub remove_source: bool,
    pub phase: String,
}
#[derive(Clone, Debug)]
pub enum Event {
    TransferPlan {
        id: JobId,
        manifest: PathBuf,
        source: PathBuf,
        destination: PathBuf,
        remove_source: bool,
        files: Vec<(PathBuf, u64)>,
    },
    PendingTransfers {
        id: JobId,
        transfers: Vec<PendingTransfer>,
    },
    Folders {
        id: JobId,
        entries: Vec<FolderEntry>,
    },
    Progress {
        id: JobId,
        phase: &'static str,
        done: u64,
        total: u64,
        path: PathBuf,
    },
    MusicSelected {
        id: JobId,
        previous: PathBuf,
        current: PathBuf,
    },
    DatabaseSelected {
        id: JobId,
        previous: PathBuf,
        current: PathBuf,
    },
    ResetCompleted {
        id: JobId,
        scope: ResetScope,
    },
    Completed {
        id: JobId,
    },
    Error {
        id: JobId,
        message: String,
    },
}
impl Event {
    pub fn id(&self) -> JobId {
        match self {
            Self::TransferPlan { id, .. } => *id,
            Self::PendingTransfers { id, .. }
            | Self::Folders { id, .. }
            | Self::Progress { id, .. }
            | Self::MusicSelected { id, .. }
            | Self::DatabaseSelected { id, .. }
            | Self::ResetCompleted { id, .. }
            | Self::Completed { id }
            | Self::Error { id, .. } => *id,
        }
    }
}
struct Request {
    id: JobId,
    command: Command,
    cancel: Arc<AtomicBool>,
    ack: Arc<AtomicBool>,
}
pub struct Maintenance {
    tx: Option<mpsc::SyncSender<Request>>,
    rx: mpsc::Receiver<Event>,
    jobs: HashMap<JobId, (Arc<AtomicBool>, Arc<AtomicBool>)>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Maintenance {
    pub fn new(config: Config) -> Result<Self, String> {
        let (tx, requests) = mpsc::sync_channel::<Request>(8);
        let (events, rx) = mpsc::sync_channel(128);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::Builder::new()
            .name("maintenance".into())
            .spawn(move || {
                background_io_priority();
                let mut config = match initialise(config) {
                    Ok(c) => c,
                    Err(message) => {
                        let _ = events.send(Event::Error { id: 0, message });
                        return;
                    }
                };
                while !worker_stop.load(Ordering::Acquire) {
                    let request = match requests.recv_timeout(Duration::from_millis(100)) {
                        Ok(r) => r,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return,
                    };
                    let context = Context {
                        id: request.id,
                        events: &events,
                        cancel: &request.cancel,
                        ack: &request.ack,
                        stop: &worker_stop,
                    };
                    let result = execute(&mut config, request.command, &context);
                    let event = match result {
                        Ok(()) => Event::Completed { id: request.id },
                        Err(message) => Event::Error {
                            id: request.id,
                            message,
                        },
                    };
                    if events.send(event).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx: Some(tx),
            rx,
            jobs: HashMap::new(),
            stop,
            worker: Some(worker),
        })
    }
    pub fn shutdown(&mut self) {
        let _ = self.shutdown_collect();
    }
    pub fn shutdown_collect(&mut self) -> Vec<Event> {
        let mut collected = Vec::new();
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        for (cancel, _) in self.jobs.values() {
            cancel.store(true, Ordering::Release);
        }
        if let Some(worker) = self.worker.take() {
            while !worker.is_finished() {
                if let Ok(event) = self.rx.recv_timeout(Duration::from_millis(20)) {
                    if !matches!(event, Event::Progress { .. }) {
                        collected.push(event);
                    }
                }
            }
            let _ = worker.join();
        }
        while let Ok(event) = self.rx.try_recv() {
            if !matches!(event, Event::Progress { .. }) {
                collected.push(event);
            }
        }
        self.jobs.clear();
        collected
    }
    pub fn command(&mut self, command: Command) -> Option<JobId> {
        let id = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let ack = Arc::new(AtomicBool::new(false));
        self.tx
            .as_ref()?
            .try_send(Request {
                id,
                command,
                cancel: cancel.clone(),
                ack: ack.clone(),
            })
            .ok()?;
        self.jobs.insert(id, (cancel, ack));
        Some(id)
    }
    pub fn poll(&mut self) -> Option<Event> {
        let event = self.rx.try_recv().ok()?;
        if matches!(event, Event::Completed { .. } | Event::Error { .. }) {
            self.jobs.remove(&event.id());
        }
        Some(event)
    }
    /// Call only after clients have switched roots and old writes have drained.
    pub fn acknowledge_switch(&self, id: JobId) {
        if let Some((_, ack)) = self.jobs.get(&id) {
            ack.store(true, Ordering::Release);
        }
    }
    pub fn cancel(&self, id: JobId) {
        if let Some((cancel, _)) = self.jobs.get(&id) {
            cancel.store(true, Ordering::Release);
        }
    }
}
impl Drop for Maintenance {
    fn drop(&mut self) {
        self.shutdown();
    }
}
struct Context<'a> {
    id: JobId,
    events: &'a mpsc::SyncSender<Event>,
    cancel: &'a AtomicBool,
    ack: &'a AtomicBool,
    stop: &'a AtomicBool,
}
impl Context<'_> {
    fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
            Err("Cancelled; verified copies and resume manifest were kept".into())
        } else {
            Ok(())
        }
    }
    fn wait_for_ack(&self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !self.ack.load(Ordering::Acquire) {
            self.check()?;
            if std::time::Instant::now() >= deadline {
                return Err("New directory is verified; originals retained because clients did not acknowledge the switch".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
    fn progress(&self, phase: &'static str, done: u64, total: u64, path: &Path) {
        let _ = self.events.try_send(Event::Progress {
            id: self.id,
            phase,
            done,
            total,
            path: path.into(),
        });
    }
    fn event(&self, event: Event) -> Result<(), String> {
        self.events
            .send(event)
            .map_err(|_| "Maintenance UI closed".into())
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Owner {
    application: String,
    id: String,
    ready: bool,
}
fn initial_marker() -> Owner {
    Owner {
        application: "betterpod-rust".into(),
        id: format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ),
        ready: true,
    }
}
fn read_owner(path: &Path) -> Result<Owner, String> {
    let p = path.join(OWNER);
    let metadata = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err("Invalid Rust database marker".into());
    }
    let owner: Owner = serde_json::from_slice(&fs::read(p).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if owner.application != "betterpod-rust" {
        return Err("That directory belongs to another application".into());
    }
    Ok(owner)
}
fn write_owner(path: &Path, owner: &Owner) -> Result<(), String> {
    atomic_write(
        &path.join(OWNER),
        &serde_json::to_vec(owner).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn background_io_priority() {
    #[cfg(target_os = "linux")]
    unsafe {
        // Linux nice/ioprio apply to this worker thread, leaving audio/main
        // priorities alone. Failure (e.g. a restricted sandbox) is harmless.
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
        libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13);
    }
}
fn canonical_dir(path: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !path.is_dir() {
        return Err("Expected a directory".into());
    }
    Ok(path)
}
fn protected(path: &Path, config: &Config) -> bool {
    config
        .protected
        .iter()
        .any(|p| path == p || path.starts_with(p))
}
fn prospective(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("Private data requires an absolute, normalized path".into());
    }
    let mut ancestor = path;
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or("Invalid data directory")?
                .to_os_string(),
        );
        ancestor = ancestor.parent().ok_or("Invalid data directory")?;
    }
    let mut result = canonical_dir(ancestor)?;
    for part in missing.into_iter().rev() {
        result.push(part);
    }
    Ok(result)
}

/// Startup guard: call before Store or any client can write. Does not create
/// directories or mark a folder; it validates both existing and future paths.
pub fn validate_private_data(
    bootstrap: &Path,
    data: &Path,
    protected_paths: &[PathBuf],
) -> Result<(), String> {
    let bootstrap = prospective(bootstrap)?;
    let data = prospective(data)?;
    for target in [&bootstrap, &data] {
        if target.parent().is_none() {
            return Err("Filesystem root cannot be a Rust database".into());
        }
        for path in protected_paths {
            let path = prospective(path)?;
            if target == &path || target.starts_with(&path) || path.starts_with(target) {
                return Err(
                    "Private Rust data overlaps the original application's protected directory"
                        .into(),
                );
            }
        }
        if target.join(".miyoopod_library.json").exists()
            || target.join("ipodjs_settings.json").exists()
        {
            return Err("Refusing the original betterPod database".into());
        }
        if target.join(OWNER).exists() && !read_owner(target)?.ready {
            return Err("Rust database is incomplete".into());
        }
    }
    if data != bootstrap
        && data.exists()
        && !data.join(OWNER).exists()
        && fs::read_dir(&data)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("Selected data folder is not marked as a Rust database".into());
    }
    Ok(())
}
fn initialise(mut config: Config) -> Result<Config, String> {
    validate_private_data(&config.bootstrap, &config.data, &config.protected)?;
    config.bootstrap = prepare_dir(&config.bootstrap)?;
    config.music = prospective(&config.music)?;
    config.data = prepare_dir(&config.data)?;
    config.protected = config
        .protected
        .iter()
        .map(|p| prospective(p))
        .collect::<Result<Vec<_>, _>>()?;
    if protected(&config.data, &config) || protected(&config.bootstrap, &config) {
        return Err("The original application's database is protected".into());
    }
    for path in [&config.bootstrap, &config.data] {
        if path.join(".miyoopod_library.json").exists()
            || path.join("ipodjs_settings.json").exists()
        {
            return Err(
                "Refusing to mark the original betterPod database as a Rust database".into(),
            );
        }
        if path.join(OWNER).exists() {
            if !read_owner(path)?.ready {
                return Err("Rust database copy is incomplete; resume it before using it".into());
            }
        } else {
            write_owner(path, &initial_marker())?;
        }
    }
    Ok(config)
}
/// Resolve only this app's own bootstrap pointers before constructing Store
/// or clients. Missing/unmounted folders fall back to the supplied defaults.
/// The music folder chosen in Move Music when it is not there at start (its
/// card was taken out): this run uses the default folder instead.
pub fn missing_music_dir(bootstrap: &Path) -> Option<PathBuf> {
    let path = bootstrap.join("musicdir.txt");
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() >= 4096 {
        return None;
    }
    let chosen = PathBuf::from(fs::read_to_string(path).ok()?.trim());
    (chosen.is_absolute() && !chosen.is_dir()).then_some(chosen)
}
pub fn resolve_paths(bootstrap: &Path, music: &Path, data: &Path) -> (PathBuf, PathBuf) {
    fn read(base: &Path, name: &str, fallback: &Path) -> PathBuf {
        let path = base.join(name);
        let result = fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() < 4096)
            .and_then(|_| fs::read_to_string(&path).ok())
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| p.is_absolute() && p.is_dir());
        result.unwrap_or_else(|| fallback.into())
    }
    (read(bootstrap, "musicdir.txt", music), {
        let selected = read(bootstrap, "datadir.txt", data);
        if selected == data || read_owner(&selected).is_ok_and(|owner| owner.ready) {
            selected
        } else {
            data.to_path_buf()
        }
    })
}
fn prepare_dir(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Choose an absolute directory path".into());
    }
    create_dir_all_synced(path).map_err(|e| format!("{}: {e}", path.display()))?;
    canonical_dir(path)
}
fn pointer(config: &Config, name: &str, path: &Path) -> Result<(), String> {
    if protected(&config.bootstrap, config) {
        return Err("The bootstrap directory is protected".into());
    }
    read_owner(&config.bootstrap)?;
    let value = path.to_str().ok_or("Directory path is not UTF-8")?;
    if value.contains(['\n', '\r']) {
        return Err("Directory name cannot contain a line break".into());
    }
    atomic_write(
        &config.bootstrap.join(name),
        format!("{value}\n").as_bytes(),
    )
    .map_err(|e| e.to_string())
}
fn execute(config: &mut Config, command: Command, ctx: &Context<'_>) -> Result<(), String> {
    ctx.check()?;
    match command {
        Command::PendingTransfers => {
            let mut transfers = Vec::new();
            let base = config.data.join("transfers");
            if base.is_dir() {
                for entry in fs::read_dir(&base).map_err(|e| e.to_string())? {
                    ctx.check()?;
                    let path = entry.map_err(|e| e.to_string())?.path();
                    if let Ok(t) = load_transfer(&path, config) {
                        if t.phase != "done" {
                            transfers.push(PendingTransfer {
                                manifest: path,
                                source: t.source,
                                destination: t.destination,
                                remove_source: t.remove_source,
                                phase: t.phase,
                            });
                        }
                    }
                }
            }
            ctx.event(Event::PendingTransfers {
                id: ctx.id,
                transfers,
            })
        }
        Command::ResumeTransfer {
            manifest,
            confirmed,
        } => {
            let transfer = load_transfer(&manifest, config)?;
            if transfer.remove_source && !confirmed {
                return Err("Resuming source cleanup requires confirmation".into());
            }
            perform_transfer(config, &manifest, transfer, ctx)
        }
        Command::PrepareTransferredClients {
            manifest,
            quiescent,
        } => {
            if !quiescent {
                return Err(
                    "Stop and flush all clients before preparing the transferred library".into(),
                );
            }
            prepare_transferred_clients(config, &manifest, ctx)
        }
        Command::FinishTransferredMusic {
            manifest,
            clients_ready,
        } => {
            if !clients_ready {
                return Err(
                    "Reopen clients on the verified destination before completing cleanup".into(),
                );
            }
            let transfer = load_transfer(&manifest, config)?;
            if transfer.clients_prepared != Some(true) {
                return Err("Prepare transferred clients before completing cleanup".into());
            }
            if !matches!(transfer.phase.as_str(), "awaiting-app" | "delete" | "done")
                || config.music != transfer.destination
            {
                return Err("Cleanup requires the already selected verified destination".into());
            }
            perform_transfer_ready(config, &manifest, transfer, ctx, true)
        }
        Command::Browse(path) => {
            let path = canonical_dir(&path)?;
            let mut entries = Vec::new();
            for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                ctx.check()?;
                let entry = entry.map_err(|e| e.to_string())?;
                let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    continue;
                }
                let canonical = canonical_dir(&entry.path())?;
                entries.push(FolderEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    writable: !meta.permissions().readonly(),
                    is_current_music: canonical == config.music,
                    is_current_data: canonical == config.data,
                    path: canonical,
                });
                if entries.len() > 4096 {
                    return Err("Too many folders in this directory".into());
                }
            }
            entries.sort_by_key(|e| e.name.to_lowercase());
            ctx.event(Event::Folders {
                id: ctx.id,
                entries,
            })
        }
        Command::SelectMusic(path) => {
            let path = canonical_dir(&path)?;
            if protected(&path, config) {
                return Err("That directory belongs to the original application".into());
            }
            if path == config.data || path == config.bootstrap {
                return Err("Choose a music folder distinct from the Rust database root".into());
            }
            let rewritten =
                relocation::plan(&config.data, &config.music, &config.music, true, ctx)?;
            relocation::apply(&rewritten, ctx)?;
            if let Err(error) = pointer(config, "musicdir.txt", &path) {
                relocation::rollback(&rewritten)?;
                return Err(error);
            }
            let previous = std::mem::replace(&mut config.music, path.clone());
            ctx.event(Event::MusicSelected {
                id: ctx.id,
                previous,
                current: path,
            })
        }
        Command::TransferMusic {
            destination,
            remove_source,
            confirmed,
        } => {
            if remove_source && !confirmed {
                return Err("Removing source music requires explicit confirmation".into());
            }
            transfer_music(config, &destination, remove_source, ctx)
        }
        Command::MoveDatabase {
            destination,
            quiescent,
        } => {
            if !quiescent {
                return Err(
                    "Flush the Store and stop background clients before moving the database".into(),
                );
            }
            move_database(config, &destination, ctx)
        }
        Command::ReplaceDatabase {
            destination,
            quiescent,
            confirmed,
        } => {
            if !quiescent || !confirmed {
                return Err(
                    "Replacing a database requires confirmation and stopped clients".into(),
                );
            }
            replace_database(config, &destination, ctx)
        }
        Command::UseDatabase {
            directory,
            quiescent,
        } => {
            if !quiescent {
                return Err(
                    "Flush the Store and stop background clients before switching databases".into(),
                );
            }
            let next = canonical_dir(&directory)?;
            if protected(&next, config) {
                return Err("The original application's database is protected".into());
            }
            if !read_owner(&next)?.ready {
                return Err("That database copy is incomplete".into());
            }
            pointer(config, "datadir.txt", &next)?;
            let previous = std::mem::replace(&mut config.data, next.clone());
            ctx.event(Event::DatabaseSelected {
                id: ctx.id,
                previous,
                current: next,
            })
        }
        Command::Reset {
            scope,
            confirmed,
            quiescent,
        } => {
            if !confirmed || !quiescent {
                return Err(
                    "Reset requires confirmation, flushed Store and stopped clients".into(),
                );
            }
            read_owner(&config.data)?;
            let names: Vec<&str> = match scope {
                ResetScope::Preferences => vec![
                    "settings.json",
                    "playback.json",
                    "games.json",
                    "weather.json",
                    "recent-searches.json",
                    "games",
                    "puzzles",
                ],
                ResetScope::AppData => vec![
                    "settings.json",
                    "playback.json",
                    "games.json",
                    "weather.json",
                    "library.json",
                    "curation.json",
                    "chosen-covers",
                    "chosen_covers",
                    "artwork",
                    "artwork-scaled",
                    "playlists",
                    "games",
                    "puzzles",
                    "recent-searches.json",
                    "podcasts.json",
                    "album-merges.json",
                    "video-history.json",
                ],
                ResetScope::EverythingIncludingTools => DATA_ITEMS.to_vec(),
            };
            // Preflight the complete allowlist before removing anything.
            for name in &names {
                let path = config.data.join(name);
                match fs::symlink_metadata(&path) {
                    Ok(m) if m.file_type().is_symlink() => {
                        return Err("Refusing to reset a symlink".into())
                    }
                    Ok(m) if m.is_dir() => {
                        let _ = inventory(&path, &[], ctx)?;
                    }
                    Ok(m) if m.is_file() => {}
                    Ok(_) => return Err("Refusing to reset a special file".into()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            for (i, name) in names.iter().enumerate() {
                ctx.check()?;
                let path = config.data.join(name);
                match fs::symlink_metadata(&path) {
                    Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
                        fs::remove_dir_all(&path).map_err(|e| e.to_string())?
                    }
                    Ok(m) if m.is_file() && !m.file_type().is_symlink() => {
                        fs::remove_file(&path).map_err(|e| e.to_string())?
                    }
                    Ok(_) => return Err("Reset target changed; reset stopped".into()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
                ctx.progress("reset", i as u64 + 1, names.len() as u64, &path);
            }
            sync_directory(&config.data).map_err(|e| e.to_string())?;
            ctx.event(Event::ResetCompleted { id: ctx.id, scope })
        }
        Command::ClearArtwork { quiescent } => {
            if !quiescent {
                return Err("Stop artwork workers before clearing their cache".into());
            }
            read_owner(&config.data)?;
            for folder in ["artwork", crate::art::SCALED_DIR] {
                let path = config.data.join(folder);
                if path.exists() {
                    let _ = inventory(&path, &[], ctx)?;
                    fs::remove_dir_all(path).map_err(|e| e.to_string())?;
                    sync_directory(&config.data).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        Command::DeleteAlbum { paths, confirmed } => {
            if !confirmed {
                return Err("Deleting album files requires explicit confirmation".into());
            }
            let mut checked = Vec::new();
            let mut unique = HashSet::new();
            for path in paths {
                if !unique.insert(path.clone()) {
                    continue;
                }
                let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err("Album deletion accepts only regular files, never symlinks".into());
                }
                let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
                if !path.starts_with(&config.music) || protected(&path, config) {
                    return Err("Album file is outside the selected music directory".into());
                }
                let fingerprint = fingerprint(&path, &meta, ctx)?;
                checked.push((path, fingerprint));
            }
            let total = checked.len() as u64;
            for (i, (path, before)) in checked.into_iter().enumerate() {
                ctx.check()?;
                let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                if fingerprint(&path, &meta, ctx)? != before {
                    return Err(format!("{} changed; deletion stopped", path.display()));
                }
                fs::remove_file(&path).map_err(|e| e.to_string())?;
                sync_directory(path.parent().ok_or("Missing source directory")?)
                    .map_err(|e| e.to_string())?;
                ctx.progress("delete", i as u64 + 1, total, &path);
            }
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Fingerprint {
    size: u64,
    modified_seconds: u64,
    modified_nanos: u32,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Transfer {
    application: String,
    source: PathBuf,
    destination: PathBuf,
    remove_source: bool,
    phase: String,
    files: BTreeMap<PathBuf, Fingerprint>,
    copied: HashSet<PathBuf>,
    deleted: HashSet<PathBuf>,
    #[serde(default)]
    rewritten: BTreeMap<PathBuf, String>,
    /// None marks legacy journals, which startup must never use to clear
    /// playback. False is a durable verified switch awaiting client repair.
    #[serde(default)]
    clients_prepared: Option<bool>,
}
fn hash_file(path: &Path, ctx: &Context<'_>) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        ctx.check()?;
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn fingerprint(
    path: &Path,
    metadata: &fs::Metadata,
    ctx: &Context<'_>,
) -> Result<Fingerprint, String> {
    let time = metadata
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(Fingerprint {
        size: metadata.len(),
        modified_seconds: time.as_secs(),
        modified_nanos: time.subsec_nanos(),
        sha256: hash_file(path, ctx)?,
    })
}
fn inventory(
    root: &Path,
    exclude: &[PathBuf],
    ctx: &Context<'_>,
) -> Result<BTreeMap<PathBuf, Fingerprint>, String> {
    fn walk(
        base: &Path,
        path: &Path,
        exclude: &[PathBuf],
        out: &mut BTreeMap<PathBuf, Fingerprint>,
        ctx: &Context<'_>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            ctx.check()?;
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if exclude.iter().any(|p| path == *p || path.starts_with(p)) {
                continue;
            }
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if meta.file_type().is_symlink() {
                return Err(format!("Symlink in source tree: {}", path.display()));
            }
            if meta.is_dir() {
                walk(base, &path, exclude, out, ctx)?;
            } else if meta.is_file() {
                let relative = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_path_buf();
                validate_relative(&relative)?;
                let fingerprint = fingerprint(&path, &meta, ctx)?;
                out.insert(relative, fingerprint);
            } else {
                return Err(format!("Unsupported special file: {}", path.display()));
            }
        }
        Ok(())
    }
    let root = canonical_dir(root)?;
    let mut out = BTreeMap::new();
    walk(&root, &root, exclude, &mut out, ctx)?;
    Ok(out)
}
fn validate_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Unsafe relative transfer path".into());
    }
    Ok(())
}
fn reject_overlap(source: &Path, destination: &Path) -> Result<(), String> {
    if source == destination || source.starts_with(destination) || destination.starts_with(source) {
        Err("Source and destination must not overlap".into())
    } else {
        Ok(())
    }
}
#[cfg(unix)]
fn identity(path: &Path) -> Result<(u64, u64), String> {
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    Ok((m.dev(), m.ino()))
}
#[cfg(not(unix))]
fn identity(path: &Path) -> Result<(u64, u64), String> {
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    Ok((m.len(), 0))
}
fn ensure_identity(path: &Path, expected: (u64, u64)) -> Result<(), String> {
    if identity(path)? == expected {
        Ok(())
    } else {
        Err(format!(
            "{} changed or its card was removed; retry after restoring it",
            path.display()
        ))
    }
}
fn check_target_names(root: &Path, files: &BTreeMap<PathBuf, Fingerprint>) -> Result<(), String> {
    let mut names = HashSet::new();
    let insensitive = case_insensitive(root);
    let mut components = HashMap::<String, PathBuf>::new();
    for (relative, info) in files {
        let folded = relative.to_string_lossy().to_lowercase();
        if insensitive && !names.insert(folded) {
            return Err("Source names collide on a case-insensitive card".into());
        }
        if insensitive {
            let mut prefix = PathBuf::new();
            for component in relative.components() {
                prefix.push(component.as_os_str());
                let folded = prefix.to_string_lossy().to_lowercase();
                if let Some(prior) = components.insert(folded, prefix.clone()) {
                    if prior != prefix {
                        return Err("Source folder names collide on a case-insensitive card".into());
                    }
                }
            }
        }
        if info.size >= 4 * 1024 * 1024 * 1024 && is_fat32(root) {
            return Err(format!("{} is too large for FAT32", relative.display()));
        }
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn is_fat32(root: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(root.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    unsafe {
        libc::statfs(path.as_ptr(), stat.as_mut_ptr()) == 0 && stat.assume_init().f_type == 0x4d44
    }
}
#[cfg(not(target_os = "linux"))]
fn is_fat32(_root: &Path) -> bool {
    false
}
fn case_insensitive(root: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        if let Ok(p) = std::ffi::CString::new(root.as_os_str().as_bytes()) {
            let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
            if unsafe { libc::statfs(p.as_ptr(), stat.as_mut_ptr()) } == 0 {
                return matches!(
                    unsafe { stat.assume_init() }.f_type as u64,
                    0x4d44 | 0x2011bab0 | 0x5346544e
                );
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = root;
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
fn destination_file(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    validate_relative(relative)?;
    let path = root.join(relative);
    let parent = path.parent().ok_or("Missing target parent")?;
    // Walk before creating: create_dir_all could otherwise follow a malicious
    // intermediate symlink and create directories outside the selected card.
    let mut current = root.to_path_buf();
    for component in parent
        .strip_prefix(root)
        .map_err(|e| e.to_string())?
        .components()
    {
        let Component::Normal(name) = component else {
            return Err("Unsafe target parent".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return Err("Target parent is a symlink or special file".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| e.to_string())?;
                sync_directory(current.parent().ok_or("Missing destination parent")?)
                    .map_err(|e| e.to_string())?;
                sync_directory(&current).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let actual = canonical_dir(parent)?;
    if !actual.starts_with(root) {
        return Err("Target parent escapes the selected directory".into());
    }
    let path = actual.join(path.file_name().ok_or("Missing target filename")?);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink() || !m.is_file()) {
        return Err("Target is a symlink or special file".into());
    }
    Ok(path)
}
fn copy_file(
    source: &Path,
    target: &Path,
    expected: &Fingerprint,
    replace_owned: bool,
    ctx: &Context<'_>,
) -> Result<(), String> {
    if target.exists() {
        let meta = fs::symlink_metadata(target).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Refusing to overwrite a symlink or directory".into());
        }
        if meta.len() == expected.size && hash_file(target, ctx)? == expected.sha256 {
            return Ok(());
        }
        if !replace_owned {
            return Err(format!(
                "Different file already exists: {}",
                target.display()
            ));
        }
    }
    let source_meta = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if !source_meta.is_file() || source_meta.file_type().is_symlink() {
        return Err("Source is no longer a regular file".into());
    }
    let temp = target.with_file_name(format!(
        ".{}.{}-{}.kpart",
        target.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        ctx.id
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut output = match options.open(&temp) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(&temp).map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("Unsafe transfer temporary file".into());
            }
            fs::remove_file(&temp).map_err(|e| e.to_string())?;
            options.open(&temp).map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut input_options = OpenOptions::new();
    input_options.read(true);
    #[cfg(unix)]
    input_options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut input = input_options.open(source).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut written = 0;
    let mut last_progress = std::time::Instant::now();
    ctx.progress("copy-file", 0, expected.size, source);
    loop {
        ctx.check()?;
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        hash.update(&buffer[..n]);
        written += n as u64;
        if last_progress.elapsed() >= Duration::from_millis(100) {
            ctx.progress("copy-file", written, expected.size, source);
            last_progress = std::time::Instant::now();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if written != expected.size || format!("{:x}", hash.finalize()) != expected.sha256 {
        return Err("Source changed while copying; original was kept".into());
    }
    if let Ok(modified) = source_meta.modified() {
        let _ = output.set_times(fs::FileTimes::new().set_modified(modified));
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    if hash_file(&temp, ctx)? != expected.sha256 {
        return Err("Copy read-back verification failed; original was kept".into());
    }
    // rename is atomic. A pre-existing differing target is allowed only for
    // this app's marked database-copy destination, never ordinary music.
    if !replace_owned && target.exists() {
        return Err("Target appeared during transfer; original was kept".into());
    }
    fs::rename(&temp, target).map_err(|e| e.to_string())?;
    sync_directory(target.parent().ok_or("Missing destination parent")?)
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn save_transfer(path: &Path, transfer: &Transfer) -> Result<(), String> {
    atomic_write(
        path,
        &serde_json::to_vec(transfer).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn music_excludes(config: &Config, source: &Path) -> Vec<PathBuf> {
    [config.data.clone(), config.bootstrap.clone()]
        .into_iter()
        .filter(|path| path != source && path.starts_with(source))
        .collect()
}
fn transfer_music(
    config: &mut Config,
    destination: &Path,
    remove_source: bool,
    ctx: &Context<'_>,
) -> Result<(), String> {
    let source = config.music.clone();
    if source == config.data || source == config.bootstrap {
        return Err("Music source cannot be the Rust database root".into());
    }
    let destination = prospective(destination)?;
    if protected(&destination, config) || protected(&source, config) {
        return Err("Protected application files cannot be transferred".into());
    }
    reject_overlap(&source, &destination)?;
    let destination = prepare_dir(&destination)?;
    let excludes = music_excludes(config, &source);
    ctx.progress("scan", 0, 0, &source);
    let files = inventory(&source, &excludes, ctx)?;
    check_target_names(&destination, &files)?;
    let key = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{remove_source}",
                source.display(),
                destination.display()
            )
            .as_bytes()
        )
    );
    let manifest = config
        .data
        .join("transfers")
        .join(format!("{}.json", &key[..24]));
    let transfer = if manifest.exists() {
        let prior = load_transfer(&manifest, config)?;
        if prior.source != source
            || prior.destination != destination
            || prior.remove_source != remove_source
            || prior.files != files
        {
            return Err("Source changed since the interrupted transfer; use Resume Transfer or a new destination".into());
        }
        prior
    } else {
        Transfer {
            application: "betterpod-rust".into(),
            source,
            destination,
            remove_source,
            phase: "copy".into(),
            files,
            copied: HashSet::new(),
            deleted: HashSet::new(),
            rewritten: BTreeMap::new(),
            clients_prepared: Some(false),
        }
    };
    save_transfer(&manifest, &transfer)?;
    perform_transfer(config, &manifest, transfer, ctx)
}
fn load_transfer(path: &Path, config: &Config) -> Result<Transfer, String> {
    load_transfer_mode(path, config, true)
}
fn read_transfer_record(path: &Path, config: &Config) -> Result<Transfer, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 * 1024
    {
        return Err("Unsafe transfer manifest".into());
    }
    let directory = config.data.join("transfers");
    let metadata = fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Unsafe transfer journal directory".into());
    }
    let base = canonical_dir(&directory)?;
    let actual = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if actual.parent() != Some(base.as_path()) {
        return Err("Transfer manifest is outside Rust data".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut bytes = Vec::new();
    options
        .open(actual)
        .map_err(|e| e.to_string())?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Transfer manifest grew beyond its limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn load_transfer_mode(
    path: &Path,
    config: &Config,
    require_source: bool,
) -> Result<Transfer, String> {
    let t = read_transfer_record(path, config)?;
    if t.application != "betterpod-rust"
        || t.files.len() > 250_000
        || !matches!(
            t.phase.as_str(),
            "copy" | "verify" | "awaiting-app" | "delete" | "done"
        )
    {
        return Err("Invalid transfer manifest".into());
    }
    let source = if require_source {
        canonical_dir(&t.source)?
    } else {
        match fs::symlink_metadata(&t.source) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err("Unsafe historical transfer source".into())
            }
            Ok(_) => canonical_dir(&t.source)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => prospective(&t.source)?,
            Err(error) => return Err(error.to_string()),
        }
    };
    if source != t.source
        || canonical_dir(&t.destination)? != t.destination
        || protected(&t.source, config)
        || protected(&t.destination, config)
    {
        return Err("Unsafe transfer roots".into());
    }
    reject_overlap(&t.source, &t.destination)?;
    for (path, info) in &t.files {
        validate_relative(path)?;
        if info.sha256.len() != 64 || !info.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid checksum".into());
        }
    }
    if t.rewritten.iter().any(|(path, hash)| {
        !t.files.contains_key(path)
            || !is_playlist(path)
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
    }) {
        return Err("Invalid relocated playlist checksum".into());
    }
    Ok(t)
}
fn prepare_transferred_clients(
    config: &Config,
    manifest: &Path,
    ctx: &Context<'_>,
) -> Result<(), String> {
    let mut transfer = load_transfer_mode(manifest, config, false)?;
    if !matches!(transfer.phase.as_str(), "awaiting-app" | "delete" | "done") {
        return Err("The transfer has not reached its verified switch barrier".into());
    }
    let selected = resolve_paths(&config.bootstrap, &config.music, &config.data).0;
    if selected != transfer.destination {
        return Err("The verified transfer destination is no longer selected".into());
    }
    if transfer.clients_prepared == Some(true) {
        return Ok(());
    }
    // This barrier never deletes originals. Cleanup still requires the strict
    // source loader and full target hashes after new clients acknowledge it.
    for (relative, expected) in &transfer.files {
        ctx.check()?;
        let target = destination_file(&transfer.destination, relative)?;
        let metadata = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Transferred file is no longer a regular file".into());
        }
        if let Some(hash) = transfer.rewritten.get(relative) {
            if hash_file(&target, ctx)? != *hash {
                return Err("Transferred playlist changed before reopening clients".into());
            }
        } else if metadata.len() != expected.size {
            return Err("Transferred file changed before reopening clients".into());
        }
    }
    let rewritten = relocation::plan(
        &config.data,
        &transfer.source,
        &transfer.destination,
        true,
        ctx,
    )?;
    relocation::apply(&rewritten, ctx)?;
    transfer.clients_prepared = Some(true);
    save_transfer(manifest, &transfer)
}
/// Repair the verified switch barrier before Store/App opens any snapshots.
/// Only newly journalled, selected and explicitly unprepared transfers qualify.
/// Legacy journals and already-prepared clients never clear live playback.
pub fn recover_transferred_clients(config: &Config) -> Result<usize, String> {
    let directory = config.data.join("transfers");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Unsafe transfer recovery directory".into());
    }
    if !config.data.join(OWNER).exists() {
        return Ok(0);
    }
    if !read_owner(&config.data)?.ready {
        return Err("Transfer recovery requires a ready owned Rust database".into());
    }
    if protected(&config.data, config) {
        return Err("Transfer recovery cannot touch protected data".into());
    }
    let (events, _rx) = mpsc::sync_channel(1);
    let cancel = AtomicBool::new(false);
    let ack = AtomicBool::new(true);
    let stop = AtomicBool::new(false);
    let ctx = Context {
        id: 0,
        events: &events,
        cancel: &cancel,
        ack: &ack,
        stop: &stop,
    };
    let mut repaired = 0;
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if index >= 1024 {
            return Err("Too many transfer journals for bounded startup recovery".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if !kind.is_file()
            || kind.is_symlink()
            || path.extension().and_then(|s| s.to_str()) != Some("json")
        {
            continue;
        }
        let record = match read_transfer_record(&path, config) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if record.application != "betterpod-rust"
            || record.clients_prepared != Some(false)
            || record.destination != config.music
            || !matches!(record.phase.as_str(), "awaiting-app" | "delete" | "done")
        {
            continue;
        }
        prepare_transferred_clients(config, &path, &ctx)?;
        repaired += 1;
    }
    Ok(repaired)
}
fn perform_transfer(
    config: &mut Config,
    manifest: &Path,
    transfer: Transfer,
    ctx: &Context<'_>,
) -> Result<(), String> {
    perform_transfer_ready(config, manifest, transfer, ctx, false)
}
fn perform_transfer_ready(
    config: &mut Config,
    manifest: &Path,
    mut transfer: Transfer,
    ctx: &Context<'_>,
    clients_ready: bool,
) -> Result<(), String> {
    let source = transfer.source.clone();
    let destination = transfer.destination.clone();
    let source_id = identity(&source)?;
    let destination_id = identity(&destination)?;
    let excludes = music_excludes(config, &source);
    let cleanup = matches!(transfer.phase.as_str(), "awaiting-app" | "delete" | "done");
    let current = inventory(&source, &excludes, ctx)?;
    if cleanup {
        // Missing originals are expected after an interrupted deletion, but
        // new or changed originals must never be removed by an old manifest.
        if current
            .iter()
            .any(|(p, f)| transfer.files.get(p) != Some(f))
        {
            return Err("Source changed; resume cleanup stopped".into());
        }
    } else if current != transfer.files {
        return Err("Source changed since transfer began".into());
    }
    check_target_names(&destination, &transfer.files)?;
    let total = transfer.files.values().try_fold(0u64, |sum, f| {
        sum.checked_add(f.size).ok_or("Transfer size overflow")
    })?;
    let mut done = 0;
    ctx.event(Event::TransferPlan {
        id: ctx.id,
        manifest: manifest.into(),
        source: source.clone(),
        destination: destination.clone(),
        remove_source: transfer.remove_source,
        files: transfer
            .files
            .iter()
            .map(|(path, info)| (path.clone(), info.size))
            .collect(),
    })?;
    if !cleanup {
        for (relative, expected) in &transfer.files {
            ctx.check()?;
            ensure_identity(&source, source_id)?;
            ensure_identity(&destination, destination_id)?;
            let target = destination_file(&destination, relative)?;
            if is_playlist(relative) {
                let hash = copy_playlist(
                    &source.join(relative),
                    &target,
                    expected,
                    &source,
                    &destination,
                    ctx,
                )?;
                transfer.rewritten.insert(relative.clone(), hash);
            } else {
                copy_file(&source.join(relative), &target, expected, false, ctx)?;
            }
            done += expected.size;
            transfer.copied.insert(relative.clone());
            save_transfer(manifest, &transfer)?;
            ctx.progress("copy", done, total, relative);
        }
        transfer.phase = "verify".into();
        save_transfer(manifest, &transfer)?;
    }
    done = 0;
    for (relative, expected) in &transfer.files {
        ctx.check()?;
        ensure_identity(&destination, destination_id)?;
        let target = destination_file(&destination, relative)?;
        let wanted = transfer.rewritten.get(relative).unwrap_or(&expected.sha256);
        if hash_file(&target, ctx)? != *wanted {
            return Err(format!(
                "Destination verification failed for {}; originals retained",
                relative.display()
            ));
        }
        done += expected.size;
        ctx.progress("verify", done, total, relative);
    }
    if !cleanup && inventory(&source, &excludes, ctx)? != transfer.files {
        return Err("Source changed before final verification; originals retained".into());
    }
    if !clients_ready {
        transfer.phase = "awaiting-app".into();
        if !cleanup {
            transfer.clients_prepared = Some(false);
        }
        save_transfer(manifest, &transfer)?;
        let rewritten = relocation::plan(&config.data, &source, &destination, true, ctx)?;
        relocation::apply(&rewritten, ctx)?;
        if let Err(error) = pointer(config, "musicdir.txt", &destination) {
            relocation::rollback(&rewritten)?;
            return Err(error);
        }
        let previous = std::mem::replace(&mut config.music, destination.clone());
        ctx.event(Event::MusicSelected {
            id: ctx.id,
            previous,
            current: destination.clone(),
        })?;
    }
    if transfer.remove_source {
        if !clients_ready {
            ctx.wait_for_ack()?;
        }
        transfer.phase = "delete".into();
        save_transfer(manifest, &transfer)?;
        for (i, (relative, expected)) in transfer.files.iter().enumerate() {
            ctx.check()?;
            ensure_identity(&source, source_id)?;
            ensure_identity(&destination, destination_id)?;
            if hash_file(&destination_file(&destination, relative)?, ctx)?
                != *transfer.rewritten.get(relative).unwrap_or(&expected.sha256)
            {
                return Err("Verified copy changed; cleanup stopped".into());
            }
            let original = source.join(relative);
            match fs::symlink_metadata(&original) {
                Ok(m) => {
                    if !m.is_file()
                        || m.file_type().is_symlink()
                        || fingerprint(&original, &m, ctx)? != *expected
                    {
                        return Err("Original changed; cleanup stopped".into());
                    }
                    fs::remove_file(&original).map_err(|e| e.to_string())?;
                    sync_directory(original.parent().ok_or("Missing source parent")?)
                        .map_err(|e| e.to_string())?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
            transfer.deleted.insert(relative.clone());
            save_transfer(manifest, &transfer)?;
            ctx.progress(
                "delete",
                i as u64 + 1,
                transfer.files.len() as u64,
                relative,
            );
        }
    }
    transfer.phase = "done".into();
    save_transfer(manifest, &transfer)?;
    Ok(())
}

fn is_playlist(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
}
fn copy_playlist(
    source_file: &Path,
    target: &Path,
    expected: &Fingerprint,
    source_root: &Path,
    destination_root: &Path,
    ctx: &Context<'_>,
) -> Result<String, String> {
    if expected.size > 16 * 1024 * 1024 {
        return Err("A playlist exceeds 16 MB; original retained".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(source_file).map_err(|e| e.to_string())?;
    let mut before = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut before)
        .map_err(|e| e.to_string())?;
    if before.len() as u64 != expected.size
        || format!("{:x}", Sha256::digest(&before)) != expected.sha256
    {
        return Err("Playlist changed during transfer; original retained".into());
    }
    let after = relocation::playlist_rewrite(&before, source_root, destination_root)?;
    let wanted = format!("{:x}", Sha256::digest(&after));
    if target.exists() {
        let metadata = fs::symlink_metadata(target).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Playlist destination is not a regular file".into());
        }
        let actual = hash_file(target, ctx)?;
        if actual == wanted {
            return Ok(wanted);
        }
        if actual != expected.sha256 {
            return Err("A different playlist already exists at the destination".into());
        }
    }
    ctx.check()?;
    atomic_write(target, &after).map_err(|e| e.to_string())?;
    if hash_file(target, ctx)? != wanted {
        return Err("Relocated playlist failed verification".into());
    }
    Ok(wanted)
}

fn database_inventory(
    source: &Path,
    ctx: &Context<'_>,
) -> Result<BTreeMap<PathBuf, Fingerprint>, String> {
    let mut out = BTreeMap::new();
    for name in DATA_ITEMS {
        let path = source.join(name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            return Err(format!("Symlink in Rust database: {name}"));
        }
        if meta.is_dir() {
            for (relative, info) in inventory(&path, &[], ctx)? {
                out.insert(PathBuf::from(name).join(relative), info);
            }
        } else if meta.is_file() {
            out.insert(name.into(), fingerprint(&path, &meta, ctx)?);
        } else {
            return Err("Special file in Rust database".into());
        }
    }
    Ok(out)
}
fn move_database(config: &mut Config, destination: &Path, ctx: &Context<'_>) -> Result<(), String> {
    let source = config.data.clone();
    let destination = prospective(destination)?;
    reject_overlap(&source, &destination)?;
    if destination == config.music || config.music.starts_with(&destination) {
        return Err("Database destination cannot contain the selected music root".into());
    }
    if protected(&destination, config) {
        return Err("Original app database is protected".into());
    }
    let destination = prepare_dir(&destination)?;
    let source_owner = read_owner(&source)?;
    let destination_owner = if destination.join(OWNER).exists() {
        let owner = read_owner(&destination)?;
        if owner.id != source_owner.id {
            return Err(
                "Destination contains a different Rust database; use Use Existing instead".into(),
            );
        }
        owner
    } else {
        if fs::read_dir(&destination)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
        {
            return Err(
                "Database destination must be empty or an interrupted copy of this database".into(),
            );
        }
        let owner = Owner {
            ready: false,
            ..source_owner.clone()
        };
        write_owner(&destination, &owner)?;
        owner
    };
    let source_id = identity(&source)?;
    let destination_id = identity(&destination)?;
    let files = database_inventory(&source, ctx)?;
    check_target_names(&destination, &files)?;
    let total = files.len() as u64;
    for (i, (relative, expected)) in files.iter().enumerate() {
        ctx.check()?;
        ensure_identity(&source, source_id)?;
        ensure_identity(&destination, destination_id)?;
        let target = destination_file(&destination, relative)?;
        copy_file(&source.join(relative), &target, expected, true, ctx)?;
        ctx.progress("database-copy", i as u64 + 1, total, relative);
    }
    for (relative, expected) in &files {
        ctx.check()?;
        ensure_identity(&source, source_id)?;
        ensure_identity(&destination, destination_id)?;
        if hash_file(&destination_file(&destination, relative)?, ctx)? != expected.sha256 {
            return Err("Database verification failed; source was kept".into());
        }
    }
    if database_inventory(&source, ctx)? != files {
        return Err("Database changed during copy; flush and stop clients before retrying".into());
    }
    let rewritten = relocation::plan(&destination, &source, &destination, false, ctx)?;
    relocation::apply(&rewritten, ctx)?;
    let verified = database_inventory(&destination, ctx)?;
    if verified.len() != files.len() || verified.keys().any(|p| !files.contains_key(p)) {
        return Err("Destination has stale database files; original database retained".into());
    }
    ensure_identity(&source, source_id)?;
    ensure_identity(&destination, destination_id)?;
    write_owner(
        &destination,
        &Owner {
            ready: true,
            ..destination_owner
        },
    )?;
    let relocated_music = config
        .music
        .strip_prefix(&source)
        .ok()
        .map(|relative| destination.join(relative));
    if let Some(music) = relocated_music.as_ref() {
        if !music.is_dir() {
            return Err(
                "The selected music folder was not part of the copied database; original retained"
                    .into(),
            );
        }
        pointer(config, "musicdir.txt", music)?;
    }
    if let Err(error) = pointer(config, "datadir.txt", &destination) {
        if relocated_music.is_some() {
            pointer(config, "musicdir.txt", &config.music)?;
        }
        return Err(error);
    }
    if let Some(music) = relocated_music {
        config.music = music;
    }
    let previous = std::mem::replace(&mut config.data, destination.clone());
    ctx.event(Event::DatabaseSelected {
        id: ctx.id,
        previous,
        current: destination.clone(),
    })?;
    ctx.wait_for_ack()?;
    for (relative, expected) in &files {
        ctx.check()?;
        ensure_identity(&source, source_id)?;
        ensure_identity(&destination, destination_id)?;
        let original = source.join(relative);
        let metadata = fs::symlink_metadata(&original).map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || fingerprint(&original, &metadata, ctx)? != *expected
            || hash_file(&destination_file(&destination, relative)?, ctx)?
                != verified[relative].sha256
        {
            return Err("Database changed; verified copy selected, old files retained".into());
        }
        fs::remove_file(&original).map_err(|e| e.to_string())?;
        sync_directory(original.parent().ok_or("Missing database parent")?)
            .map_err(|e| e.to_string())?;
    }
    // Keep unrelated files and bootstrap pointers. Empty folders are harmless;
    // never recursively delete the old root or another app's files.
    Ok(())
}

fn replace_database(
    config: &mut Config,
    destination: &Path,
    ctx: &Context<'_>,
) -> Result<(), String> {
    let destination = canonical_dir(destination)?;
    let source = config.data.clone();
    reject_overlap(&source, &destination)?;
    if config.bootstrap != source {
        reject_overlap(&config.bootstrap, &destination)?;
    }
    if protected(&destination, config)
        || destination == config.music
        || config.music.starts_with(&destination)
    {
        return Err("Database replacement overlaps protected data or music".into());
    }
    let previous = read_owner(&destination)?;
    if !previous.ready {
        return Err("Cannot replace an incomplete database".into());
    }
    // Preflight both owned inventories before moving either directory. Keep
    // the previous database intact next to the destination for recovery.
    database_inventory(&source, ctx)?;
    database_inventory(&destination, ctx)?;
    ctx.check()?;
    let backup = destination.with_file_name(format!(
        ".betterpod-rust-backup-{}-{}",
        ctx.id,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    if backup.exists() {
        return Err("Database backup destination already exists".into());
    }
    fs::rename(&destination, &backup).map_err(|e| e.to_string())?;
    // Backup still contains absolute paths for its original location. Keep
    // it unavailable to Use Existing until restored to that location.
    let prepared = (|| {
        sync_directory(destination.parent().ok_or("Missing destination parent")?)
            .map_err(|e| e.to_string())?;
        write_owner(
            &backup,
            &Owner {
                ready: false,
                ..previous.clone()
            },
        )
    })();
    if let Err(error) = prepared {
        let restored = (|| {
            if !read_owner(&backup)
                .is_ok_and(|owner| owner.ready == previous.ready && owner.id == previous.id)
            {
                write_owner(&backup, &previous)?;
            }
            fs::rename(&backup, &destination).map_err(|e| e.to_string())?;
            sync_directory(destination.parent().ok_or("Missing destination parent")?)
                .map_err(|e| e.to_string())
        })();
        return Err(match restored {
            Ok(()) => error,
            Err(recovery) => format!(
                "{error}; previous database retained at {}: {recovery}",
                backup.display()
            ),
        });
    }
    match move_database(config, &destination, ctx) {
        Ok(()) => Ok(()),
        Err(error) if config.data != destination => {
            if destination.exists() {
                let owner = read_owner(&destination)?;
                if owner.id != read_owner(&source)?.id {
                    return Err(format!(
                        "{error}; previous database remains at {}",
                        backup.display()
                    ));
                }
                let failed = backup.with_extension("incomplete");
                if failed.exists() {
                    return Err(format!(
                        "{error}; previous database remains at {}",
                        backup.display()
                    ));
                }
                fs::rename(&destination, &failed).map_err(|e| e.to_string())?;
            }
            write_owner(&backup, &previous)?;
            fs::rename(&backup, &destination)
                .map_err(|e| format!("{error}; restoring previous database failed: {e}"))?;
            sync_directory(destination.parent().ok_or("Missing destination parent")?)
                .map_err(|e| e.to_string())?;
            Err(error)
        }
        Err(error) => Err(error), // verified replacement selected; retain backup and source remnants
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_chosen_music_card_that_is_out_is_reported() {
        let root = std::env::temp_dir().join(format!("bp-musicdir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("music")).unwrap();
        assert_eq!(missing_music_dir(&root), None);
        fs::write(
            root.join("musicdir.txt"),
            root.join("music").to_string_lossy().as_bytes(),
        )
        .unwrap();
        assert_eq!(missing_music_dir(&root), None);
        let card = root.join("card-taken-out/music");
        fs::write(root.join("musicdir.txt"), format!("{}\n", card.display())).unwrap();
        assert_eq!(missing_music_dir(&root), Some(card));
        fs::write(root.join("musicdir.txt"), "relative/path").unwrap();
        assert_eq!(missing_music_dir(&root), None);
        fs::remove_dir_all(root).unwrap();
    }
    struct Fixture {
        root: PathBuf,
        config: Config,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rust-maint-{}-{}",
                std::process::id(),
                NEXT_JOB.fetch_add(1, Ordering::Relaxed)
            ));
            let music = root.join("music");
            let data = root.join("rust");
            let stable = root.join("stable");
            for p in [&music, &data, &stable] {
                fs::create_dir_all(p).unwrap();
            }
            fs::write(stable.join("settings.json"), b"original").unwrap();
            fs::create_dir_all(music.join("Album")).unwrap();
            fs::write(music.join("Album/song.flac"), b"synthetic audio fixture").unwrap();
            let config = initialise(Config {
                bootstrap: data.clone(),
                data,
                music,
                protected: vec![stable],
            })
            .unwrap();
            Self { root, config }
        }
        fn context(&self, f: impl FnOnce(&Context<'_>)) {
            let (tx, _rx) = mpsc::sync_channel(256);
            let cancel = AtomicBool::new(false);
            let stop = AtomicBool::new(false);
            let ack = AtomicBool::new(true);
            f(&Context {
                id: 99,
                events: &tx,
                cancel: &cancel,
                stop: &stop,
                ack: &ack,
            });
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn music_copy_preserves_originals_and_uses_private_pointer() {
        let f = Fixture::new();
        let target = f.root.join("copy");
        let config = f.config.clone();
        f.context(|ctx| {
            let mut c = config;
            transfer_music(&mut c, &target, false, ctx).unwrap();
            assert_eq!(c.music, fs::canonicalize(&target).unwrap());
        });
        assert_eq!(
            fs::read(target.join("Album/song.flac")).unwrap(),
            b"synthetic audio fixture"
        );
        assert!(f.config.music.join("Album/song.flac").exists());
        assert_eq!(
            resolve_paths(&f.config.bootstrap, &f.config.music, &f.config.data).0,
            target
        );
        // A different pre-existing destination is never overwritten/deleted.
        let conflict = f.root.join("conflict");
        fs::create_dir_all(conflict.join("Album")).unwrap();
        fs::write(conflict.join("Album/song.flac"), b"different").unwrap();
        let mut conflict_config = f.config.clone();
        f.context(|ctx| {
            assert!(transfer_music(&mut conflict_config, &conflict, true, ctx).is_err())
        });
        assert!(f.root.join("music/Album/song.flac").exists());
    }
    #[test]
    fn music_move_waits_for_app_ack_before_any_delete() {
        let f = Fixture::new();
        let original = f.config.music.join("Album/song.flac");
        let destination = f.root.join("moved");
        let mut m = Maintenance::new(f.config.clone()).unwrap();
        let id = m
            .command(Command::TransferMusic {
                destination: destination.clone(),
                remove_source: true,
                confirmed: true,
            })
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut switched = false;
        while std::time::Instant::now() < deadline {
            if let Some(e) = m.poll() {
                match e {
                    Event::MusicSelected { .. } => {
                        switched = true;
                        break;
                    }
                    Event::Error { message, .. } => panic!("{message}"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(switched);
        assert!(original.exists());
        assert!(destination.join("Album/song.flac").exists());
        m.acknowledge_switch(id);
        loop {
            if let Some(e) = m.poll() {
                match e {
                    Event::Completed { .. } => break,
                    Event::Error { message, .. } => panic!("{message}"),
                    _ => {}
                }
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!original.exists());
    }
    #[test]
    fn interrupted_cleanup_resumes_after_pointer_switched_and_unlink_before_manifest() {
        let f = Fixture::new();
        let config = f.config.clone();
        let target = f.root.join("resume");
        fs::create_dir(&target).unwrap();
        f.context(|ctx| {
            let mut c = config;
            let files = inventory(&c.music, &[c.data.clone()], ctx).unwrap();
            let source = c.music.clone();
            for (p, e) in &files {
                copy_file(
                    &source.join(p),
                    &destination_file(&target, p).unwrap(),
                    e,
                    false,
                    ctx,
                )
                .unwrap();
            }
            let manifest = c.data.join("transfers/interrupted.json");
            let transfer = Transfer {
                application: "betterpod-rust".into(),
                source: source.clone(),
                destination: target.clone(),
                remove_source: true,
                phase: "delete".into(),
                files,
                copied: HashSet::new(),
                deleted: HashSet::new(),
                rewritten: BTreeMap::new(),
                clients_prepared: Some(false),
            };
            save_transfer(&manifest, &transfer).unwrap();
            fs::remove_file(source.join("Album/song.flac")).unwrap();
            c.music = target.clone();
            let restored = load_transfer(&manifest, &c).unwrap();
            perform_transfer(&mut c, &manifest, restored, ctx).unwrap();
            assert_eq!(load_transfer(&manifest, &c).unwrap().phase, "done");
        });
    }
    #[test]
    fn database_move_preserves_curation_and_unrelated_files() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let target = f.root.join("db");
        fs::write(config.data.join("curation.json"), b"{\"chosen\":true}").unwrap();
        fs::create_dir(config.data.join("chosen-covers")).unwrap();
        fs::write(config.data.join("chosen-covers/one.jpg"), b"cover").unwrap();
        fs::write(config.data.join("unrelated.txt"), b"keep").unwrap();
        f.context(|ctx| move_database(&mut config, &target, ctx).unwrap());
        assert!(target.join("curation.json").exists());
        assert!(target.join("chosen-covers/one.jpg").exists());
        assert!(!f.config.data.join("curation.json").exists());
        assert!(f.config.data.join("unrelated.txt").exists());
        assert!(f.config.bootstrap.join("datadir.txt").exists());
    }
    #[test]
    fn database_move_relocates_private_media_playlists_and_chosen_covers() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let source = config.data.clone();
        let target = f.root.join("db");
        fs::create_dir_all(source.join("downloads")).unwrap();
        fs::create_dir_all(source.join("chosen-covers")).unwrap();
        fs::create_dir_all(source.join("playlists")).unwrap();
        let media = source.join("downloads/private.flac");
        let cover = source.join("chosen-covers/chosen.png");
        fs::write(&media, b"audio").unwrap();
        fs::write(&cover, b"image").unwrap();
        let music = config.music.join("Album/song.flac");
        fs::write(source.join("playback.json"),serde_json::to_vec(&serde_json::json!({"queue":[media,music],"index":0,"position":12.25,"paused":true})).unwrap()).unwrap();
        fs::write(source.join("curation.json"),serde_json::to_vec(&serde_json::json!({"tracks":{media.to_str().unwrap():{"album":"A","cover":cover}}})).unwrap()).unwrap();
        fs::write(
            source.join("playlists/Test.m3u"),
            format!("#EXTM3U\n{}\n{}\n", media.display(), music.display()),
        )
        .unwrap();
        f.context(|ctx| move_database(&mut config, &target, ctx).unwrap());
        let playback: crate::storage::Playback =
            serde_json::from_slice(&fs::read(target.join("playback.json")).unwrap()).unwrap();
        assert_eq!(
            playback.queue,
            vec![target.join("downloads/private.flac"), music.clone()]
        );
        assert_eq!(playback.position, 12.25);
        let edits: serde_json::Value =
            serde_json::from_slice(&fs::read(target.join("curation.json")).unwrap()).unwrap();
        assert_eq!(
            edits["tracks"][target.join("downloads/private.flac").to_str().unwrap()]["cover"],
            target.join("chosen-covers/chosen.png").to_str().unwrap()
        );
        assert_eq!(
            fs::read_to_string(target.join("playlists/Test.m3u")).unwrap(),
            format!(
                "#EXTM3U\n{}\n{}\n",
                target.join("downloads/private.flac").display(),
                music.display()
            )
        );
        assert_eq!(fs::read(music).unwrap(), b"synthetic audio fixture");
    }
    #[test]
    fn music_copy_relocates_private_playlists_and_clears_old_playback() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let source = config.music.clone();
        let target = f.root.join("music-copy");
        fs::create_dir_all(config.data.join("playlists")).unwrap();
        fs::write(
            config.data.join("playlists/Test.m3u"),
            format!("{}\n", source.join("Album/song.flac").display()),
        )
        .unwrap();
        fs::write(
            config.data.join("playback.json"),
            format!(
                "{{\"queue\":[\"{}\"],\"index\":0,\"position\":50,\"paused\":false}}",
                source.join("Album/song.flac").display()
            ),
        )
        .unwrap();
        f.context(|ctx| transfer_music(&mut config, &target, false, ctx).unwrap());
        assert_eq!(
            fs::read_to_string(config.data.join("playlists/Test.m3u")).unwrap(),
            format!("{}\n", target.join("Album/song.flac").display())
        );
        let playback: crate::storage::Playback =
            serde_json::from_slice(&fs::read(config.data.join("playback.json")).unwrap()).unwrap();
        assert!(playback.queue.is_empty());
        assert!(source.join("Album/song.flac").is_file());
    }
    #[test]
    fn moved_imported_playlist_is_rewritten_and_its_verified_hash_survives_resume() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let source = config.music.clone();
        let target = f.root.join("music-move");
        let original = format!(
            "#EXTM3U\r\n{}\r\n",
            source.join("Album/song.flac").display()
        );
        fs::write(source.join("list.m3u8"), original).unwrap();
        f.context(|ctx| {
            transfer_music(&mut config, &target, true, ctx).unwrap();
            assert_eq!(
                fs::read_to_string(target.join("list.m3u8")).unwrap(),
                format!(
                    "#EXTM3U\r\n{}\r\n",
                    target.join("Album/song.flac").display()
                )
            );
            assert!(!source.join("list.m3u8").exists());
            assert!(!source.join("Album/song.flac").exists());
            let manifest = fs::read_dir(config.data.join("transfers"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let transfer = load_transfer(&manifest, &config).unwrap();
            assert!(transfer.rewritten.contains_key(Path::new("list.m3u8")));
            perform_transfer(&mut config, &manifest, transfer.clone(), ctx).unwrap();
            let mut bad = transfer;
            bad.rewritten
                .insert("Album/song.flac".into(), "0".repeat(64));
            save_transfer(&manifest, &bad).unwrap();
            assert!(load_transfer(&manifest, &config).is_err());
        });
    }
    #[test]
    fn replace_database_requires_ready_ownership_and_retains_previous_database_backup() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let target = f.root.join("existing-db");
        fs::create_dir(&target).unwrap();
        write_owner(&target, &initial_marker()).unwrap();
        fs::write(target.join("settings.json"), br#"{"previous":true}"#).unwrap();
        fs::write(target.join("unrelated.txt"), b"keep previous").unwrap();
        fs::write(config.data.join("settings.json"), br#"{"current":true}"#).unwrap();
        f.context(|ctx| {
            assert!(execute(
                &mut config,
                Command::ReplaceDatabase {
                    destination: target.clone(),
                    quiescent: true,
                    confirmed: false
                },
                ctx
            )
            .is_err());
            replace_database(&mut config, &target, ctx).unwrap();
        });
        assert_eq!(
            fs::read(target.join("settings.json")).unwrap(),
            br#"{"current":true}"#
        );
        let backup = fs::read_dir(&f.root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".betterpod-rust-backup-")
            })
            .unwrap();
        assert_eq!(
            fs::read(backup.join("unrelated.txt")).unwrap(),
            b"keep previous"
        );
        assert!(!read_owner(&backup).unwrap().ready);
        let unknown = f.root.join("unknown");
        fs::create_dir(&unknown).unwrap();
        fs::write(unknown.join("settings.json"), b"foreign").unwrap();
        f.context(|ctx| assert!(replace_database(&mut config, &unknown, ctx).is_err()));
        assert_eq!(fs::read(unknown.join("settings.json")).unwrap(), b"foreign");
    }
    #[test]
    fn startup_transfer_recovery_repairs_late_store_flush_before_first_load() {
        let fixture = Fixture::new();
        let mut config = fixture.config.clone();
        let source = config.music.clone();
        let store = crate::storage::Store::new(config.data.clone()).unwrap();
        fixture.context(|ctx| {
            transfer_music(
                &mut config,
                &fixture.root.join("recovery-target"),
                false,
                ctx,
            )
            .unwrap()
        });
        let destination = config.music.clone();
        let manifest = fs::read_dir(config.data.join("transfers"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            load_transfer(&manifest, &config).unwrap().clients_prepared,
            Some(false)
        );
        // This is the last flush of the OLD Store, after the durable pointer
        // switch but before the process could run PrepareTransferredClients.
        let old = source.join("Album/song.flac");
        store
            .save_playback_shared_order(
                std::sync::Arc::new(vec![old.clone()]),
                0,
                41.25,
                false,
                Some(std::sync::Arc::new(vec![old])),
            )
            .unwrap();
        store.flush().unwrap();
        drop(store);
        assert_eq!(recover_transferred_clients(&config).unwrap(), 1);
        let reopened = crate::storage::Store::new(config.data.clone()).unwrap();
        assert!(reopened.load_playback().is_none());
        drop(reopened);
        assert_eq!(
            load_transfer(&manifest, &config).unwrap().clients_prepared,
            Some(true)
        );
        assert!(source.join("Album/song.flac").is_file());
        assert!(destination.join("Album/song.flac").is_file());
        // New clients can now persist real progress; neither a repeated startup
        // nor an explicit duplicate Prepare may clear their live snapshots.
        let new=serde_json::to_vec(&serde_json::json!({"queue":[destination.join("Album/song.flac")],"index":0,"position":73.5,"paused":false})).unwrap();
        atomic_write(&config.data.join("playback.json"), &new).unwrap();
        assert_eq!(recover_transferred_clients(&config).unwrap(), 0);
        fixture.context(|ctx| {
            execute(
                &mut config,
                Command::PrepareTransferredClients {
                    manifest: manifest.clone(),
                    quiescent: true,
                },
                ctx,
            )
            .unwrap()
        });
        assert_eq!(fs::read(config.data.join("playback.json")).unwrap(), new);
        assert!(source.join("Album/song.flac").is_file());
    }
    #[test]
    fn startup_transfer_recovery_never_clears_legacy_or_unselected_journals() {
        let fixture = Fixture::new();
        let mut config = fixture.config.clone();
        let source = config.music.clone();
        fixture.context(|ctx| {
            transfer_music(&mut config, &fixture.root.join("legacy-target"), false, ctx).unwrap()
        });
        let manifest = fs::read_dir(config.data.join("transfers"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        record.as_object_mut().unwrap().remove("clients_prepared");
        atomic_write(&manifest, &serde_json::to_vec(&record).unwrap()).unwrap();
        let new =
            br#"{"queue":["/current/new/song.flac"],"index":0,"position":22.5,"paused":false}"#;
        atomic_write(&config.data.join("playback.json"), new).unwrap();
        assert_eq!(recover_transferred_clients(&config).unwrap(), 0);
        assert_eq!(fs::read(config.data.join("playback.json")).unwrap(), new);
        assert_eq!(
            load_transfer(&manifest, &config).unwrap().clients_prepared,
            None
        );
        fixture.context(|ctx| {
            assert!(execute(
                &mut config,
                Command::FinishTransferredMusic {
                    manifest: manifest.clone(),
                    clients_ready: true
                },
                ctx
            )
            .is_err())
        });
        let mut record = load_transfer(&manifest, &config).unwrap();
        record.clients_prepared = Some(false);
        save_transfer(&manifest, &record).unwrap();
        let mut other = config.clone();
        other.music = source.clone();
        assert_eq!(recover_transferred_clients(&other).unwrap(), 0);
        assert_eq!(fs::read(config.data.join("playback.json")).unwrap(), new);
        assert!(source.join("Album/song.flac").is_file());
    }
    #[test]
    fn startup_transfer_recovery_accepts_missing_historical_source_but_cleanup_does_not() {
        let fixture = Fixture::new();
        let mut config = fixture.config.clone();
        let source = config.music.clone();
        fixture.context(|ctx| {
            transfer_music(
                &mut config,
                &fixture.root.join("missing-source-target"),
                false,
                ctx,
            )
            .unwrap()
        });
        let manifest = fs::read_dir(config.data.join("transfers"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        // Simulate removal of the old card after its verified copy. Recovery
        // repairs private metadata only and never opens/deletes that source.
        fs::remove_dir_all(&source).unwrap();
        assert!(load_transfer(&manifest, &config).is_err());
        assert_eq!(recover_transferred_clients(&config).unwrap(), 1);
        assert_eq!(
            load_transfer_mode(&manifest, &config, false)
                .unwrap()
                .clients_prepared,
            Some(true)
        );
        fixture.context(|ctx| {
            execute(
                &mut config,
                Command::PrepareTransferredClients {
                    manifest: manifest.clone(),
                    quiescent: true,
                },
                ctx,
            )
            .unwrap();
            assert!(execute(
                &mut config,
                Command::FinishTransferredMusic {
                    manifest: manifest.clone(),
                    clients_ready: true
                },
                ctx
            )
            .is_err());
        });
        assert!(!source.exists());
        assert_eq!(
            fs::read(config.music.join("Album/song.flac")).unwrap(),
            b"synthetic audio fixture"
        );
    }
    #[test]
    fn startup_transfer_finish_requires_durable_preparation() {
        let fixture = Fixture::new();
        let mut config = fixture.config.clone();
        let source = config.music.clone();
        fixture.context(|ctx| {
            transfer_music(
                &mut config,
                &fixture.root.join("prepared-target"),
                false,
                ctx,
            )
            .unwrap()
        });
        let manifest = fs::read_dir(config.data.join("transfers"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fixture.context(|ctx| {
            assert!(execute(
                &mut config,
                Command::FinishTransferredMusic {
                    manifest: manifest.clone(),
                    clients_ready: true
                },
                ctx
            )
            .unwrap_err()
            .contains("Prepare"))
        });
        assert!(source.join("Album/song.flac").is_file());
        assert_eq!(recover_transferred_clients(&config).unwrap(), 1);
        fixture.context(|ctx| {
            execute(
                &mut config,
                Command::FinishTransferredMusic {
                    manifest,
                    clients_ready: true,
                },
                ctx,
            )
            .unwrap()
        });
        assert!(source.join("Album/song.flac").is_file());
    }
    #[test]
    fn prepared_cleanup_never_rewrites_new_live_client_state() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let source = config.music.clone();
        let destination = f.root.join("transferred");
        f.context(|ctx|{
            transfer_music(&mut config,&destination,false,ctx).unwrap();
            let manifest=fs::read_dir(config.data.join("transfers")).unwrap().next().unwrap().unwrap().path();let mut transfer=load_transfer(&manifest,&config).unwrap();transfer.remove_source=true;transfer.phase="awaiting-app".into();save_transfer(&manifest,&transfer).unwrap();
            // The old Store flushes its snapshot after the first worker stops.
            fs::write(config.data.join("playback.json"),serde_json::to_vec(&serde_json::json!({"queue":[source.join("Album/song.flac")],"index":0,"position":12,"paused":true})).unwrap()).unwrap();
            assert!(execute(&mut config,Command::PrepareTransferredClients{manifest:manifest.clone(),quiescent:false},ctx).is_err());
            execute(&mut config,Command::PrepareTransferredClients{manifest:manifest.clone(),quiescent:true},ctx).unwrap();
            let cleared:crate::storage::Playback=serde_json::from_slice(&fs::read(config.data.join("playback.json")).unwrap()).unwrap();assert!(cleared.queue.is_empty());assert!(source.join("Album/song.flac").exists());
            // A newly opened App has already started playing on the new card.
            let new_state=serde_json::to_vec(&serde_json::json!({"queue":[destination.join("Album/song.flac")],"index":0,"position":33.5,"paused":false})).unwrap();fs::write(config.data.join("playback.json"),&new_state).unwrap();
            assert!(execute(&mut config,Command::FinishTransferredMusic{manifest:manifest.clone(),clients_ready:false},ctx).is_err());
            execute(&mut config,Command::FinishTransferredMusic{manifest,clients_ready:true},ctx).unwrap();assert_eq!(fs::read(config.data.join("playback.json")).unwrap(),new_state);assert!(!source.join("Album/song.flac").exists());assert!(destination.join("Album/song.flac").exists());
        });
    }
    #[test]
    fn moving_database_relocates_selected_music_inside_private_downloads() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let destination = f.root.join("database");
        let original = config.data.join("downloads");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("song.flac"), b"private music").unwrap();
        config.music = original;
        f.context(|ctx| move_database(&mut config, &destination, ctx).unwrap());
        let selected = resolve_paths(&config.bootstrap, &f.config.music, &f.config.data);
        assert_eq!(
            selected,
            (destination.join("downloads"), destination.clone())
        );
        assert_eq!(
            fs::read(config.music.join("song.flac")).unwrap(),
            b"private music"
        );
    }
    #[test]
    fn failed_database_replacement_restores_previous_ready_database() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let target = f.root.join("existing");
        fs::create_dir(&target).unwrap();
        write_owner(&target, &initial_marker()).unwrap();
        fs::write(target.join("settings.json"), b"previous bytes").unwrap();
        fs::write(config.data.join("curation.json"), b"invalid json").unwrap();
        f.context(|ctx| assert!(replace_database(&mut config, &target, ctx).is_err()));
        assert_eq!(config.data, f.config.data);
        assert!(read_owner(&target).unwrap().ready);
        assert_eq!(
            fs::read(target.join("settings.json")).unwrap(),
            b"previous bytes"
        );
        assert_eq!(
            fs::read(config.data.join("curation.json")).unwrap(),
            b"invalid json"
        );
    }
    #[test]
    fn safety_rejects_unconfirmed_nonquiescent_original_and_incomplete_db() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        f.context(|ctx| {
            assert!(execute(
                &mut config,
                Command::TransferMusic {
                    destination: f.root.join("copy"),
                    remove_source: true,
                    confirmed: false
                },
                ctx
            )
            .is_err());
            assert!(execute(
                &mut config,
                Command::MoveDatabase {
                    destination: f.root.join("copy"),
                    quiescent: false
                },
                ctx
            )
            .is_err());
            assert!(move_database(&mut config, &f.root.join("stable"), ctx).is_err());
            let incomplete = f.root.join("incomplete");
            fs::create_dir(&incomplete).unwrap();
            write_owner(
                &incomplete,
                &Owner {
                    ready: false,
                    ..initial_marker()
                },
            )
            .unwrap();
            assert!(execute(
                &mut config,
                Command::UseDatabase {
                    directory: incomplete,
                    quiescent: true
                },
                ctx
            )
            .is_err());
        });
        assert_eq!(
            fs::read(f.root.join("stable/settings.json")).unwrap(),
            b"original"
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_parent_cannot_create_or_write_outside_target() {
        let f = Fixture::new();
        let target = f.root.join("target");
        let outside = f.root.join("outside");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, target.join("alias")).unwrap();
        assert!(destination_file(&target, Path::new("alias/new/song.flac")).is_err());
        assert!(!outside.join("new").exists());
        std::os::unix::fs::symlink(outside, f.config.music.join("unsafe")).unwrap();
        f.context(|ctx| assert!(inventory(&f.config.music, &[], ctx).is_err()));
    }
    #[test]
    fn reset_only_owned_allowlist_and_cache_keeps_chosen_covers() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        for dir in ["artwork", "chosen-covers", "downloads", "tools"] {
            fs::create_dir(config.data.join(dir)).unwrap();
            fs::write(config.data.join(dir).join("one"), b"keep").unwrap();
        }
        fs::write(config.data.join("settings.json"), b"{}").unwrap();
        fs::write(config.data.join("unrelated"), b"keep").unwrap();
        f.context(|ctx| {
            execute(&mut config, Command::ClearArtwork { quiescent: true }, ctx).unwrap();
            assert!(!config.data.join("artwork").exists());
            assert!(config.data.join("chosen-covers/one").exists());
            execute(
                &mut config,
                Command::Reset {
                    scope: ResetScope::AppData,
                    confirmed: true,
                    quiescent: true,
                },
                ctx,
            )
            .unwrap();
        });
        assert!(!config.data.join("settings.json").exists());
        assert!(config.data.join("downloads/one").exists());
        assert!(config.data.join("tools/one").exists());
        assert!(config.data.join("unrelated").exists());
        assert!(config.music.join("Album/song.flac").exists());
        assert!(config.data.join(OWNER).exists());
    }
    #[test]
    fn startup_guard_and_missing_music_keep_safe_folder_selection_available() {
        let f = Fixture::new();
        let missing = f.root.join("not-created/music");
        let mut config = f.config.clone();
        config.music = missing.clone();
        let mut config = initialise(config).unwrap();
        assert!(!missing.exists());
        f.context(|ctx| {
            execute(&mut config, Command::Browse(f.root.clone()), ctx).unwrap();
            execute(
                &mut config,
                Command::SelectMusic(f.config.music.clone()),
                ctx,
            )
            .unwrap();
        });
        assert_eq!(config.music, f.config.music);
        assert!(validate_private_data(
            &f.config.bootstrap,
            &f.root.join("stable"),
            &f.config.protected
        )
        .is_err());
        assert!(validate_private_data(&f.root, &f.root, &f.config.protected).is_err());
        let foreign = f.root.join("foreign");
        fs::create_dir(&foreign).unwrap();
        write_owner(
            &foreign,
            &Owner {
                application: "other".into(),
                ..initial_marker()
            },
        )
        .unwrap();
        assert!(validate_private_data(&f.config.bootstrap, &foreign, &f.config.protected).is_err());
        fs::write(
            f.config.bootstrap.join("datadir.txt"),
            format!("{}\n", f.root.join("stable").display()),
        )
        .unwrap();
        assert_eq!(
            resolve_paths(&f.config.bootstrap, &f.config.music, &f.config.data).1,
            f.config.data
        );
    }
    #[test]
    fn private_downloads_can_be_a_source_without_excluding_all_music() {
        let f = Fixture::new();
        let mut config = f.config.clone();
        let downloads = config.data.join("downloads");
        fs::create_dir(&downloads).unwrap();
        fs::write(downloads.join("song.flac"), b"synthetic private download").unwrap();
        config.music = downloads.clone();
        let destination = f.root.join("copy-downloads");
        f.context(|ctx| transfer_music(&mut config, &destination, false, ctx).unwrap());
        assert_eq!(
            fs::read(destination.join("song.flac")).unwrap(),
            b"synthetic private download"
        );
        assert!(downloads.join("song.flac").exists());
        config.music = config.data.clone();
        f.context(|ctx| {
            assert!(transfer_music(&mut config, &f.root.join("unsafe-copy"), true, ctx).is_err())
        });
        assert!(!f.root.join("unsafe-copy").exists());
    }
}
