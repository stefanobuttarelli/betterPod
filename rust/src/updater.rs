//! Independent update channel. No production betterPod/Go manifest is accepted.
//! The local experiment has no feed by default. All network/ZIP/filesystem work
//! runs on one worker; archives are verified before the install tree changes.
use crate::storage::{atomic_write, create_dir_all_synced, sync_directory};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
const APP: &str = "betterpod-rust";
const LAUNCHER: &str = "betterPod Rust.sh";
const BINARY: &str = "betterpod-rust/betterpod-rust";
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_EXPANDED: u64 = 768 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Config {
    pub installation: PathBuf,
    pub current_version: String,
    pub feed: Option<String>,
    pub beta_feed: Option<String>,
    pub allow_beta: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub application: String,
    pub version: String,
    pub arch: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
}
#[derive(Clone, Debug)]
pub enum Command {
    /// Channel changes are atomic and are never dropped behind queued jobs.
    SetBeta(bool),
    Check,
    CheckIfDue {
        state: PathBuf,
    },
    Apply {
        manifest: Manifest,
        quiescent: bool,
    },
    /// Explicit manual import, useful for offline installs and synthetic QA.
    ApplyArchive {
        manifest: Manifest,
        archive: PathBuf,
        quiescent: bool,
    },
}
#[derive(Clone, Debug)]
pub enum Status {
    LocalBuild,
    UpToDate,
    Available(Manifest),
}
#[derive(Clone, Debug)]
pub enum Event {
    Status(Status),
    Progress {
        phase: &'static str,
        done: u64,
        total: u64,
    },
    Applied {
        version: String,
    },
    Error(String),
}
pub struct Updater {
    tx: Option<mpsc::SyncSender<Command>>,
    rx: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    allow_beta: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Updater {
    pub fn new(config: Config) -> Result<Self, String> {
        let (tx, requests) = mpsc::sync_channel::<Command>(2);
        let (events, rx) = mpsc::sync_channel(32);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let allow_beta = Arc::new(AtomicBool::new(config.allow_beta));
        let worker_beta = allow_beta.clone();
        let worker = std::thread::Builder::new()
            .name("rust-updater".into())
            .spawn(move || {
                let mut auto_started = false;
                while !worker_stop.load(Ordering::Acquire) {
                    let command = match requests.recv_timeout(Duration::from_millis(100)) {
                        Ok(c) => c,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return,
                    };
                    let ctx = Context {
                        stop: &worker_stop,
                        events: &events,
                    };
                    let mut config = config.clone();
                    config.allow_beta = worker_beta.load(Ordering::Acquire);
                    let result = match command {
                        Command::SetBeta(value) => {
                            worker_beta.store(value, Ordering::Release);
                            Ok(())
                        }
                        Command::Check => check(&config, &ctx),
                        Command::CheckIfDue { state } => {
                            let startup = !auto_started;
                            auto_started = true;
                            check_if_due(&config, &state, startup, &ctx)
                        }
                        Command::Apply {
                            manifest,
                            quiescent,
                        } => {
                            if !quiescent {
                                Err(
                                    "Stop playback and quiesce clients before applying an update"
                                        .into(),
                                )
                            } else {
                                download_apply(&config, manifest, &ctx)
                            }
                        }
                        Command::ApplyArchive {
                            manifest,
                            archive,
                            quiescent,
                        } => {
                            if !quiescent {
                                Err(
                                    "Stop playback and quiesce clients before applying an update"
                                        .into(),
                                )
                            } else {
                                apply_archive(&config, &manifest, &archive, &ctx, None).and_then(
                                    |()| {
                                        ctx.send(Event::Applied {
                                            version: manifest.version,
                                        })
                                    },
                                )
                            }
                        }
                    };
                    if let Err(message) = result {
                        if ctx.send(Event::Error(message)).is_err() {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx: Some(tx),
            rx,
            stop,
            allow_beta,
            worker: Some(worker),
        })
    }
    /// Join also waits for a cancelled commit to finish its rollback.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            while !worker.is_finished() {
                let _ = self.rx.recv_timeout(Duration::from_millis(20));
            }
            let _ = worker.join();
        }
        while self.rx.try_recv().is_ok() {}
    }
    pub fn command(&self, command: Command) -> bool {
        if let Command::SetBeta(value) = command {
            if self.stop.load(Ordering::Acquire) {
                return false;
            }
            self.allow_beta.store(value, Ordering::Release);
            return true;
        }
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.try_send(command).is_ok())
    }
    pub fn poll(&mut self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}
impl Drop for Updater {
    fn drop(&mut self) {
        self.shutdown();
    }
}
struct Context<'a> {
    stop: &'a AtomicBool,
    events: &'a mpsc::SyncSender<Event>,
}
impl Context<'_> {
    fn check(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            Err("Update cancelled; installed files are preserved".into())
        } else {
            Ok(())
        }
    }
    fn send(&self, event: Event) -> Result<(), String> {
        self.events
            .send(event)
            .map_err(|_| "Updater UI closed".into())
    }
    fn progress(&self, phase: &'static str, done: u64, total: u64) {
        let _ = self.events.try_send(Event::Progress { phase, done, total });
    }
}
fn validate_manifest(m: &Manifest) -> Result<(), String> {
    if m.application != APP {
        return Err("This update belongs to another application; stable betterPod packages are never accepted".into());
    }
    if m.arch != "aarch64-linux" {
        return Err("The update is not for the AArch64 Linux Rust app".into());
    }
    if m.version.is_empty()
        || m.version.len() > 80
        || !m
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
    {
        return Err("Invalid update version".into());
    }
    version(&m.version)?;
    if m.size == 0
        || m.size > MAX_ARCHIVE
        || m.sha256.len() != 64
        || !m.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid update size or SHA-256".into());
    }
    if m.notes.len() > 64 * 1024 {
        return Err("Update notes exceed the limit".into());
    }
    validate_url(&m.url)
}
fn validate_url(url: &str) -> Result<(), String> {
    let secure = url.starts_with("https://");
    let loopback = ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
        .iter()
        .any(|p| url.starts_with(p));
    if url.len() > 4096 || url.contains(['\n', '\r']) || (!secure && !loopback) {
        return Err("Update URLs must use HTTPS (local test servers may use loopback HTTP)".into());
    }
    Ok(())
}
fn version(text: &str) -> Result<semver::Version, String> {
    let text = text.trim_start_matches('v');
    let split = text.find(['-', '+']).unwrap_or(text.len());
    let (core, tail) = text.split_at(split);
    let count = core.split('.').count();
    let normalized = match count {
        1 => format!("{core}.0.0{tail}"),
        2 => format!("{core}.0{tail}"),
        _ => text.to_owned(),
    };
    semver::Version::parse(&normalized).map_err(|e| format!("Invalid update version: {e}"))
}
fn newer(candidate: &str, current: &str) -> Result<bool, String> {
    Ok(version(candidate)?.cmp_precedence(&version(current)?) == std::cmp::Ordering::Greater)
}
fn fetch_manifest(feed: &str, ctx: &Context<'_>) -> Result<Manifest, String> {
    validate_url(feed)?;
    ctx.check()?;
    let response = ureq::get(feed)
        .config()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    let bytes = read_limited(response.into_body().into_reader(), 128 * 1024, ctx)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid Rust update manifest: {e}"))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}
fn feeds(config: &Config) -> Vec<&str> {
    let mut urls = Vec::with_capacity(2);
    if let Some(url) = config.feed.as_deref().filter(|s| !s.is_empty()) {
        urls.push(url);
    }
    if config.allow_beta {
        if let Some(url) = config.beta_feed.as_deref().filter(|s| !s.is_empty()) {
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }
    urls
}
fn channel_key(config: &Config) -> Option<String> {
    if feeds(config).is_empty() {
        return None;
    }
    // Include disabled beta URL too: changing either configured channel must
    // invalidate an old offer, and a beta offer must never leak into stable.
    let bytes = serde_json::to_vec(&(
        config.feed.as_deref(),
        config.beta_feed.as_deref(),
        config.allow_beta,
    ))
    .ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}
fn fetch_status(config: &Config, ctx: &Context<'_>) -> Result<Status, String> {
    let urls = feeds(config);
    if urls.is_empty() {
        return Ok(Status::LocalBuild);
    }
    let current = version(&config.current_version)?;
    let mut chosen: Option<(semver::Version, Manifest)> = None;
    let mut valid = false;
    let mut errors = Vec::new();
    for url in urls {
        ctx.check()?;
        match fetch_manifest(url, ctx) {
            Ok(manifest) => {
                valid = true;
                let candidate = version(&manifest.version)?;
                if !config.allow_beta && !candidate.pre.is_empty() {
                    continue;
                }
                if candidate.cmp_precedence(&current).is_gt()
                    && chosen
                        .as_ref()
                        .is_none_or(|(prior, _)| candidate.cmp_precedence(prior).is_gt())
                {
                    chosen = Some((candidate, manifest));
                }
            }
            Err(error) => {
                ctx.check()?;
                errors.push(error);
            }
        }
    }
    if let Some((_, manifest)) = chosen {
        Ok(Status::Available(manifest))
    } else if valid {
        Ok(Status::UpToDate)
    } else {
        Err(errors.join("; "))
    }
}
fn check(config: &Config, ctx: &Context<'_>) -> Result<(), String> {
    ctx.send(Event::Status(fetch_status(config, ctx)?))
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct CheckState {
    feed: String,
    checked: u64,
    retry_after: u64,
    available: Option<Manifest>,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn check_if_due(
    config: &Config,
    state_path: &Path,
    startup: bool,
    ctx: &Context<'_>,
) -> Result<(), String> {
    let Some(key) = channel_key(config) else {
        return ctx.send(Event::Status(Status::LocalBuild));
    };
    if state_path.file_name().and_then(|s| s.to_str()) != Some("update-check.json") {
        return Err(
            "Automatic update cache must be named update-check.json in private Rust data".into(),
        );
    }
    let parent = state_path
        .parent()
        .ok_or("Missing update state directory")?;
    let marker = parent.join(".betterpod-rust.json");
    ensure_regular_or_absent(&marker)?;
    if fs::metadata(&marker)
        .map_err(|_| "Automatic update cache requires an owned Rust database")?
        .len()
        > 4096
    {
        return Err("Invalid database marker".into());
    }
    let owner: serde_json::Value =
        serde_json::from_slice(&fs::read(marker).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if owner.get("application").and_then(|v| v.as_str()) != Some(APP)
        || owner.get("ready").and_then(|v| v.as_bool()) != Some(true)
    {
        return Err("Automatic update cache cannot use another application's data".into());
    }
    ensure_regular_or_absent(state_path)?;
    let mut cached = fs::metadata(state_path)
        .ok()
        .filter(|m| m.len() < 128 * 1024)
        .and_then(|_| fs::read(state_path).ok())
        .and_then(|b| serde_json::from_slice::<CheckState>(&b).ok())
        .filter(|s| s.feed == key)
        .unwrap_or_else(|| CheckState {
            feed: key,
            ..CheckState::default()
        });
    cached.available = cached.available.filter(|m| {
        validate_manifest(m).is_ok()
            && newer(&m.version, &config.current_version).unwrap_or(false)
            && (config.allow_beta || version(&m.version).is_ok_and(|v| v.pre.is_empty()))
    });
    if let Some(manifest) = &cached.available {
        ctx.send(Event::Status(Status::Available(manifest.clone())))?;
    }
    let time = now();
    let retrying = cached.retry_after > time;
    let fresh = cached.checked > 0 && cached.checked <= time && time - cached.checked < 86400;
    if retrying || fresh && !(startup && cached.available.is_some()) {
        if fresh && cached.available.is_none() {
            ctx.send(Event::Status(Status::UpToDate))?;
        }
        return Ok(());
    }
    match fetch_status(config, ctx) {
        Ok(status) => {
            let prior = cached.available.as_ref().map(|m| m.version.clone());
            cached.checked = time;
            cached.retry_after = 0;
            cached.available = if let Status::Available(m) = &status {
                Some(m.clone())
            } else {
                None
            };
            atomic_write(
                state_path,
                &serde_json::to_vec(&cached).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if !matches!(&status,Status::Available(m)if prior.as_ref()==Some(&m.version)) {
                ctx.send(Event::Status(status))?;
            }
            Ok(())
        }
        Err(error) => {
            cached.retry_after = time.saturating_add(3600);
            atomic_write(
                state_path,
                &serde_json::to_vec(&cached).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            Err(error)
        }
    }
}
fn read_limited(mut reader: impl Read, limit: u64, ctx: &Context<'_>) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        ctx.check()?;
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if output.len() as u64 + n as u64 > limit {
            return Err("Download exceeds its size limit".into());
        }
        output.extend_from_slice(&buffer[..n]);
    }
    Ok(output)
}
fn download_apply(config: &Config, manifest: Manifest, ctx: &Context<'_>) -> Result<(), String> {
    validate_manifest(&manifest)?;
    let root = install_root(config)?;
    let work = root.join(APP).join(".rust-download");
    ensure_regular_or_absent(&work)?;
    let response = ureq::get(&manifest.url)
        .config()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    let mut reader = response.into_body().into_reader();
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut output = options.open(&work).map_err(|e| e.to_string())?;
    let mut size = 0;
    let mut buffer = [0u8; 65536];
    loop {
        ctx.check()?;
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > manifest.size {
            return Err("Downloaded archive is larger than its manifest".into());
        }
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        ctx.progress("download", size, manifest.size);
        std::thread::sleep(Duration::from_millis(1));
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    let result = apply_archive(config, &manifest, &work, ctx, None);
    if result.is_ok() {
        let _ = fs::remove_file(&work);
    }
    result?;
    ctx.send(Event::Applied {
        version: manifest.version,
    })
}
fn ensure_regular_or_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(format!("Unsafe update target: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn install_root(config: &Config) -> Result<PathBuf, String> {
    let root = fs::canonicalize(&config.installation).map_err(|e| e.to_string())?;
    let app = root.join(APP);
    let metadata = fs::symlink_metadata(&app).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Rust application directory is missing or is a symlink".into());
    }
    let marker = app.join(".rust-app.json");
    ensure_regular_or_absent(&marker)?;
    if fs::metadata(&marker)
        .map_err(|_| "This directory is not marked as the independent Rust installation")?
        .len()
        > 4096
    {
        return Err("Invalid Rust installation marker".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(marker).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if value.get("application").and_then(|v| v.as_str()) != Some(APP) {
        return Err("Installation marker belongs to another application".into());
    }
    Ok(root)
}
fn safe_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        Err("Unsafe archive path".into())
    } else {
        Ok(())
    }
}
fn allowed(path: &Path) -> bool {
    let Some(s) = path.to_str() else {
        return false;
    };
    if s.contains('\\') || s.contains('\0') {
        return false;
    }
    if s == LAUNCHER || s == BINARY {
        return true;
    }
    if ["betterpod-rust/assets/", "betterpod-rust/libs/"]
        .iter()
        .any(|p| s.starts_with(p))
    {
        return true;
    }
    let Some(name) = s.strip_prefix("betterpod-rust/") else {
        return false;
    };
    !name.contains('/')
        && (name == "README.md"
            || name == "THIRD_PARTY_LICENSES.md"
            || name == "port.json"
            || name == ".rust-app.json"
            || name.starts_with("LICENSE"))
}
fn safe_parent(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Unsafe update directory root".into());
    }
    safe_relative(path)?;
    let mut current = root.to_path_buf();
    let parent = path.parent().unwrap_or(Path::new(""));
    for component in parent.components() {
        let Component::Normal(name) = component else {
            return Err("Unsafe update parent".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return Err("Update parent is a symlink or special file".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| e.to_string())?;
                sync_directory(current.parent().ok_or("Missing update parent")?)
                    .map_err(|e| e.to_string())?;
                sync_directory(&current).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let target = current.join(path.file_name().ok_or("Missing update filename")?);
    ensure_regular_or_absent(&target)?;
    Ok(target)
}
fn sha(path: &Path, ctx: &Context<'_>) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut f = options.open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        ctx.check()?;
        let n = f.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn elf_aarch64(path: &Path) -> Result<(), String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut h = [0u8; 64];
    f.read_exact(&mut h)
        .map_err(|_| "Rust executable is not a valid ELF file")?;
    if &h[..4] != b"\x7fELF"
        || h[4] != 2
        || h[5] != 1
        || u16::from_le_bytes([h[18], h[19]]) != 183
        || !matches!(u16::from_le_bytes([h[16], h[17]]), 2 | 3)
    {
        return Err("Executable architecture is not 64-bit AArch64 Linux ELF".into());
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    existed: bool,
    started: bool,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    application: String,
    complete: bool,
    entries: Vec<Entry>,
}
fn persist_rename(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|e| e.to_string())?;
    let destination = target
        .parent()
        .ok_or("Missing rename destination directory")?;
    sync_directory(destination).map_err(|e| e.to_string())?;
    let origin = source.parent().ok_or("Missing rename source directory")?;
    if origin != destination {
        sync_directory(origin).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn persist_unlink(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| e.to_string())?;
    sync_directory(path.parent().ok_or("Missing unlink parent")?).map_err(|e| e.to_string())
}
fn remove_work(work: &Path) -> Result<(), String> {
    fs::remove_dir_all(work).map_err(|e| e.to_string())?;
    sync_directory(work.parent().ok_or("Missing staging parent")?).map_err(|e| e.to_string())
}
fn write_journal(work: &Path, journal: &Journal) -> Result<(), String> {
    atomic_write(
        &work.join("journal.json"),
        &serde_json::to_vec(journal).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
/// On a process/power failure, the next explicit apply first restores the old
/// complete installation. Journal and backups remain if restoration fails.
fn recover(root: &Path, work: &Path) -> Result<(), String> {
    let path = work.join("journal.json");
    if !path.exists() {
        return Ok(());
    }
    ensure_regular_or_absent(&path)?;
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
        return Err("Update recovery journal exceeds limit".into());
    }
    let journal: Journal = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if journal.application != APP {
        return Err("Refusing a foreign update journal".into());
    }
    for entry in &journal.entries {
        safe_relative(&entry.path)?;
        if !allowed(&entry.path) {
            return Err("Unsafe recovery path".into());
        }
    }
    if !journal.complete {
        for entry in journal.entries.iter().rev().filter(|e| e.started) {
            let target = safe_parent(root, &entry.path)?;
            let backup = safe_parent(&work.join("backup"), &entry.path)?;
            if entry.existed {
                if backup.exists() {
                    ensure_regular_or_absent(&backup)?;
                    // Atomic replacement also avoids a missing-binary window
                    // if power fails while rollback is restoring an executable.
                    persist_rename(&backup, &target)?;
                }
            } else if target.exists() {
                persist_unlink(&target)?;
            }
        }
    }
    remove_work(work)
}
/// Startup recovery before creating Store/SDL clients. Returns true only
/// when an incomplete transaction was rolled back; the caller should restart
/// or exit so an already-loaded new executable cannot run with restored assets.
/// A completed journal only needs cleanup and returns false.
pub fn recover_installation_if_needed(config: &Config) -> Result<bool, String> {
    let work = config.installation.join(APP).join(".rust-update");
    let path = work.join("journal.json");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    let root = install_root(config)?;
    let metadata = fs::symlink_metadata(&work).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Unsafe update recovery directory".into());
    }
    ensure_regular_or_absent(&path)?;
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
        return Err("Update journal exceeds its limit".into());
    }
    let journal: Journal = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let rollback = !journal.complete;
    recover(&root, &work)?;
    Ok(rollback)
}
fn apply_archive(
    config: &Config,
    manifest: &Manifest,
    archive: &Path,
    ctx: &Context<'_>,
    fail_after: Option<usize>,
) -> Result<(), String> {
    validate_manifest(manifest)?;
    if !newer(&manifest.version, &config.current_version)? {
        return Err("Rust update must be newer than the installed version".into());
    }
    ctx.check()?;
    let root = install_root(config)?;
    let metadata = fs::symlink_metadata(archive).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != manifest.size {
        return Err("Archive size does not match the Rust manifest".into());
    }
    if sha(archive, ctx)? != manifest.sha256.to_ascii_lowercase() {
        return Err("Archive SHA-256 verification failed".into());
    }
    let work = root.join(APP).join(".rust-update");
    if let Ok(m) = fs::symlink_metadata(&work) {
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err("Unsafe update staging directory".into());
        }
        recover(&root, &work)?;
        if work.exists() {
            remove_work(&work)?;
        }
    }
    create_dir_all_synced(&work).map_err(|e| e.to_string())?;
    let payload = work.join("payload");
    create_dir_all_synced(&payload).map_err(|e| e.to_string())?;
    create_dir_all_synced(&work.join("backup")).map_err(|e| e.to_string())?;
    let staged = (|| {
        let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if zip.len() > 10_000 {
            return Err("Update contains too many files".into());
        }
        let mut names = HashSet::new();
        let mut files = Vec::new();
        let mut expanded = 0u64;
        let zip_count = zip.len();
        for i in 0..zip_count {
            ctx.check()?;
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().trim_end_matches('/').to_owned();
            let path = PathBuf::from(&name);
            safe_relative(&path)?;
            if entry.is_dir() {
                if name != APP
                    && name != "betterpod-rust/conf"
                    && name != "betterpod-rust/assets"
                    && name != "betterpod-rust/libs"
                    && !name.starts_with("betterpod-rust/assets/")
                    && !name.starts_with("betterpod-rust/libs/")
                {
                    return Err("Archive contains a non-whitelisted directory".into());
                }
                continue;
            }
            if !allowed(&path) || !names.insert(name.to_lowercase()) {
                return Err(format!(
                    "Archive contains an unsafe or duplicate entry: {name}"
                ));
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
            {
                return Err("Archive symlinks/special files are forbidden".into());
            }
            expanded = expanded
                .checked_add(entry.size())
                .ok_or("Expanded size overflow")?;
            if expanded > MAX_EXPANDED {
                return Err("Expanded update exceeds limit".into());
            }
            let expected = entry.size();
            let target = safe_parent(&payload, &path)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|e| e.to_string())?;
            let mut count = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                ctx.check()?;
                let n = entry.read(&mut buffer).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                if count > expected {
                    return Err("Expanded file exceeds declared size".into());
                }
                output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(1));
            }
            if count != expected {
                return Err("Truncated ZIP entry".into());
            }
            output.sync_all().map_err(|e| e.to_string())?;
            #[cfg(unix)]
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if name == BINARY || name == LAUNCHER {
                    0o755
                } else {
                    0o644
                }),
            )
            .map_err(|e| e.to_string())?;
            // chmod follows the initial data sync; persist the final mode too.
            output.sync_all().map_err(|e| e.to_string())?;
            files.push(path);
            ctx.progress("verify-package", i as u64 + 1, zip_count as u64);
        }
        if !names.contains(&BINARY.to_lowercase()) || !names.contains(&LAUNCHER.to_lowercase()) {
            return Err("Rust package must contain its binary and separate launcher".into());
        }
        elf_aarch64(&payload.join(BINARY))?;
        let marker = payload.join("betterpod-rust/.rust-app.json");
        if marker.exists() {
            if fs::metadata(&marker).map_err(|e| e.to_string())?.len() > 4096 {
                return Err("Invalid package installation marker".into());
            }
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(marker).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if value.get("application").and_then(|v| v.as_str()) != Some(APP) {
                return Err("Package installation marker belongs to another application".into());
            }
        }
        // Validate every actual destination before touching installed files.
        for path in &files {
            let target = safe_parent(&root, path)?;
            if path.starts_with("betterpod-rust/libs")
                && target.exists()
                && sha(&target, ctx)? != sha(&payload.join(path), ctx)?
            {
                return Err("This prototype cannot replace installed codec libraries; a reviewed separate installation is required".into());
            }
        }
        files.sort_by_key(|p| (p == Path::new(BINARY), p.clone()));
        Ok(files)
    })();
    let files = match staged {
        Ok(files) => files,
        Err(e) => {
            let _ = remove_work(&work);
            return Err(e);
        }
    };
    let mut journal = Journal {
        application: APP.into(),
        complete: false,
        entries: files
            .into_iter()
            .map(|path| Entry {
                existed: root.join(&path).exists(),
                path,
                started: false,
            })
            .collect(),
    };
    write_journal(&work, &journal)?;
    let result: Result<(), String> = (|| {
        for i in 0..journal.entries.len() {
            ctx.check()?;
            journal.entries[i].started = true;
            write_journal(&work, &journal)?;
            let entry = &journal.entries[i];
            let target = safe_parent(&root, &entry.path)?;
            if entry.existed {
                let backup = safe_parent(&work.join("backup"), &entry.path)?;
                persist_rename(&target, &backup)?;
            }
            persist_rename(&payload.join(&entry.path), &target)?;
            ctx.progress("install", i as u64 + 1, journal.entries.len() as u64);
            if fail_after == Some(i + 1) {
                return Err("Injected interrupted update".into());
            }
        }
        journal.complete = true;
        write_journal(&work, &journal)?;
        Ok(())
    })();
    if let Err(error) = result {
        return match recover(&root, &work) {
            Ok(()) => Err(format!("{error}; previous installation restored")),
            Err(recovery) => Err(format!("{error}; rollback needs recovery: {recovery}")),
        };
    }
    remove_work(&work).map_err(|e| format!("Update installed; staging cleanup failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_ID: AtomicU64 = AtomicU64::new(1);
    struct Fixture {
        root: PathBuf,
        config: Config,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rust-update-{}-{}",
                std::process::id(),
                TEST_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let ports = root.join("ports");
            let app = ports.join(APP);
            fs::create_dir_all(app.join("conf")).unwrap();
            fs::create_dir(ports.join("betterpod")).unwrap();
            fs::write(
                app.join(".rust-app.json"),
                b"{\"application\":\"betterpod-rust\"}",
            )
            .unwrap();
            fs::write(app.join("betterpod-rust"), b"old Rust binary").unwrap();
            fs::write(ports.join(LAUNCHER), b"old launcher").unwrap();
            fs::write(app.join("conf/settings.json"), b"private settings").unwrap();
            fs::write(ports.join("betterpod/betterpod"), b"stable unchanged").unwrap();
            Self {
                config: Config {
                    installation: ports,
                    current_version: "0.1.0".into(),
                    feed: None,
                    beta_feed: None,
                    allow_beta: false,
                },
                root,
            }
        }
        fn ctx(&self, f: impl FnOnce(&Context<'_>)) {
            let (tx, _rx) = mpsc::sync_channel(128);
            let stop = AtomicBool::new(false);
            f(&Context {
                stop: &stop,
                events: &tx,
            });
        }
        fn archive(&self, extra: Option<(&str, &[u8])>) -> (PathBuf, Manifest) {
            let path = self.root.join("new.zip");
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            let elf = elf();
            for (name, data) in [
                (LAUNCHER, b"#!/bin/sh\n# new Rust launcher\n".as_slice()),
                (BINARY, elf.as_slice()),
                ("betterpod-rust/assets/new.txt", b"new asset".as_slice()),
            ] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
            if let Some((name, data)) = extra {
                zip.start_file(name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
            let bytes = fs::read(&path).unwrap();
            let m = Manifest {
                application: APP.into(),
                version: "0.2.0".into(),
                arch: "aarch64-linux".into(),
                url: "https://example.invalid/rust.zip".into(),
                size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                notes: "offline fixture".into(),
            };
            (path, m)
        }
        fn preserved(&self) {
            assert_eq!(
                fs::read(self.config.installation.join("betterpod/betterpod")).unwrap(),
                b"stable unchanged"
            );
            assert_eq!(
                fs::read(
                    self.config
                        .installation
                        .join("betterpod-rust/conf/settings.json")
                )
                .unwrap(),
                b"private settings"
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn elf() -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        data[16..18].copy_from_slice(&3u16.to_le_bytes());
        data[18..20].copy_from_slice(&183u16.to_le_bytes());
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[52..54].copy_from_slice(&64u16.to_le_bytes());
        data
    }
    fn serve_feeds(
        responses: Vec<(&'static str, u16, Vec<u8>)>,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut paths = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            while paths.len() < requests {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "missing fixture feed request"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut req = [0; 4096];
                let n = stream.read(&mut req).unwrap();
                let path = String::from_utf8_lossy(&req[..n])
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                let (status, body) = responses
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, status, body)| (*status, body.as_slice()))
                    .unwrap_or((404, b"missing"));
                write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
                paths.push(path);
            }
            paths
        });
        (format!("http://{address}"), server)
    }
    #[test]
    fn two_feed_channel_selects_newest_and_preserves_valid_stable_on_beta_error() {
        let f = Fixture::new();
        let (_, base) = f.archive(None);
        for (stable, beta, beta_status, expected) in [
            ("1.2", "1.3-beta.2", 200, "1.3-beta.2"),
            ("1.3", "1.3-beta.9", 200, "1.3"),
            ("1.2", "1.4-beta.1", 500, "1.2"),
        ] {
            let mut sm = base.clone();
            sm.version = stable.into();
            let mut bm = base.clone();
            bm.version = beta.into();
            let (url, server) = serve_feeds(
                vec![
                    ("/stable", 200, serde_json::to_vec(&sm).unwrap()),
                    ("/beta", beta_status, serde_json::to_vec(&bm).unwrap()),
                ],
                2,
            );
            let mut config = f.config.clone();
            config.feed = Some(format!("{url}/stable"));
            config.beta_feed = Some(format!("{url}/beta"));
            config.allow_beta = true;
            f.ctx(|ctx|assert!(matches!(fetch_status(&config,ctx).unwrap(),Status::Available(m) if m.version==expected)));
            assert_eq!(server.join().unwrap(), vec!["/stable", "/beta"]);
        }
        f.preserved();
    }
    #[test]
    fn stable_channel_ignores_prereleases_and_does_not_fetch_beta() {
        let f = Fixture::new();
        let (_, mut manifest) = f.archive(None);
        manifest.version = "1.3-beta.7".into();
        let (url, server) = serve_feeds(
            vec![("/stable", 200, serde_json::to_vec(&manifest).unwrap())],
            1,
        );
        let mut config = f.config.clone();
        config.feed = Some(format!("{url}/stable"));
        config.beta_feed = Some(format!("{url}/beta"));
        f.ctx(|ctx| {
            assert!(matches!(
                fetch_status(&config, ctx).unwrap(),
                Status::UpToDate
            ))
        });
        assert_eq!(server.join().unwrap(), vec!["/stable"]);
    }
    #[test]
    fn beta_cache_never_survives_channel_or_feed_change() {
        let f = Fixture::new();
        let (_, mut stable) = f.archive(None);
        stable.version = "1.2".into();
        let mut beta = stable.clone();
        beta.version = "1.3-beta.2".into();
        let (url, server) = serve_feeds(
            vec![
                ("/stable", 200, serde_json::to_vec(&stable).unwrap()),
                ("/beta", 200, serde_json::to_vec(&beta).unwrap()),
            ],
            3,
        );
        let mut config = f.config.clone();
        config.feed = Some(format!("{url}/stable"));
        config.beta_feed = Some(format!("{url}/beta"));
        config.allow_beta = true;
        let cache = config.installation.join(APP).join("conf/update-check.json");
        fs::write(
            cache.parent().unwrap().join(".betterpod-rust.json"),
            b"{\"application\":\"betterpod-rust\",\"ready\":true}",
        )
        .unwrap();
        f.ctx(|ctx| check_if_due(&config, &cache, false, ctx).unwrap());
        let initial: CheckState = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        assert_eq!(initial.available.unwrap().version, "1.3-beta.2");
        config.allow_beta = false;
        f.ctx(|ctx| check_if_due(&config, &cache, false, ctx).unwrap());
        let stable_state: CheckState = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        assert_eq!(stable_state.available.unwrap().version, "1.2");
        assert_ne!(initial.feed, stable_state.feed);
        let old = channel_key(&config);
        config.beta_feed = Some(format!("{url}/changed"));
        assert_ne!(old, channel_key(&config));
        assert_eq!(server.join().unwrap(), vec!["/stable", "/beta", "/stable"]);
        f.preserved();
    }
    #[test]
    fn beta_toggle_is_not_lost_when_request_queue_is_full() {
        let (tx, _requests) = mpsc::sync_channel(2);
        let (_events, rx) = mpsc::sync_channel(2);
        tx.try_send(Command::Check).unwrap();
        tx.try_send(Command::Check).unwrap();
        let mut updater = Updater {
            tx: Some(tx),
            rx,
            stop: Arc::new(AtomicBool::new(false)),
            allow_beta: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        assert!(!updater.command(Command::Check));
        assert!(updater.command(Command::SetBeta(true)));
        assert!(updater.allow_beta.load(Ordering::Acquire));
        updater.shutdown();
        assert!(!updater.command(Command::SetBeta(false)));
    }
    #[test]
    fn no_feed_is_real_local_build_status_without_network() {
        let f = Fixture::new();
        let (tx, rx) = mpsc::sync_channel(8);
        let stop = AtomicBool::new(false);
        check(
            &f.config,
            &Context {
                stop: &stop,
                events: &tx,
            },
        )
        .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::Status(Status::LocalBuild)
        ));
        f.preserved();
    }
    #[test]
    fn package_installs_rust_binary_last_and_preserves_stable_and_data() {
        let f = Fixture::new();
        let (path, m) = f.archive(None);
        f.ctx(|ctx| apply_archive(&f.config, &m, &path, ctx, None).unwrap());
        assert_eq!(fs::read(f.config.installation.join(BINARY)).unwrap(), elf());
        assert_eq!(
            fs::read(f.config.installation.join("betterpod-rust/assets/new.txt")).unwrap(),
            b"new asset"
        );
        assert!(!f
            .config
            .installation
            .join(APP)
            .join(".rust-update")
            .exists());
        f.preserved();
    }
    #[test]
    fn injected_partial_install_restores_entire_previous_app() {
        let f = Fixture::new();
        let (path, m) = f.archive(None);
        f.ctx(|ctx| {
            let error = apply_archive(&f.config, &m, &path, ctx, Some(2)).unwrap_err();
            assert!(error.contains("previous installation restored"), "{error}");
        });
        assert_eq!(
            fs::read(f.config.installation.join(BINARY)).unwrap(),
            b"old Rust binary"
        );
        assert_eq!(
            fs::read(f.config.installation.join(LAUNCHER)).unwrap(),
            b"old launcher"
        );
        assert!(!f
            .config
            .installation
            .join("betterpod-rust/assets/new.txt")
            .exists());
        f.preserved();
    }
    #[test]
    fn stable_manifest_wrong_checksum_and_architecture_are_rejected() {
        let f = Fixture::new();
        let (path, m) = f.archive(None);
        f.ctx(|ctx| {
            let mut wrong = m.clone();
            wrong.application = "betterpod".into();
            assert!(apply_archive(&f.config, &wrong, &path, ctx, None).is_err());
            wrong = m.clone();
            wrong.sha256 = "0".repeat(64);
            assert!(apply_archive(&f.config, &wrong, &path, ctx, None).is_err());
            wrong = m.clone();
            wrong.arch = "x86_64-linux".into();
            assert!(apply_archive(&f.config, &wrong, &path, ctx, None).is_err());
        });
        assert_eq!(
            fs::read(f.config.installation.join(BINARY)).unwrap(),
            b"old Rust binary"
        );
        f.preserved();
    }
    #[test]
    fn traversal_conf_and_duplicate_case_entries_rejected() {
        for name in [
            "../escape",
            "betterpod-rust/conf/settings.json",
            "betterpod/betterpod",
            "betterpod-rust/ASSETS/NEW.TXT",
            "betterpod-rust/assets/../escape",
        ] {
            let f = Fixture::new();
            let (path, m) = f.archive(Some((name, b"malicious")));
            f.ctx(|ctx| {
                assert!(
                    apply_archive(&f.config, &m, &path, ctx, None).is_err(),
                    "{name}"
                )
            });
            assert_eq!(
                fs::read(f.config.installation.join(BINARY)).unwrap(),
                b"old Rust binary"
            );
            f.preserved();
        }
    }
    #[cfg(unix)]
    #[test]
    fn installed_symlink_parent_cannot_escape() {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, f.config.installation.join(APP).join("assets"))
            .unwrap();
        let (path, m) = f.archive(None);
        f.ctx(|ctx| assert!(apply_archive(&f.config, &m, &path, ctx, None).is_err()));
        assert!(!outside.join("new.txt").exists());
        f.preserved();
    }
    #[test]
    fn power_loss_journal_restores_old_file_before_next_apply() {
        let f = Fixture::new();
        let work = f.config.installation.join(APP).join(".rust-update");
        fs::create_dir_all(work.join("backup")).unwrap();
        fs::rename(
            f.config.installation.join(LAUNCHER),
            work.join("backup").join(LAUNCHER),
        )
        .unwrap();
        fs::write(f.config.installation.join(LAUNCHER), b"partial new").unwrap();
        write_journal(
            &work,
            &Journal {
                application: APP.into(),
                complete: false,
                entries: vec![Entry {
                    path: LAUNCHER.into(),
                    existed: true,
                    started: true,
                }],
            },
        )
        .unwrap();
        assert!(recover_installation_if_needed(&f.config).unwrap());
        assert!(!recover_installation_if_needed(&f.config).unwrap());
        assert_eq!(
            fs::read(f.config.installation.join(LAUNCHER)).unwrap(),
            b"old launcher"
        );
        f.preserved();
    }
    #[test]
    fn check_reads_only_independent_manifest_from_loopback_fixture() {
        use std::net::TcpListener;
        let f = Fixture::new();
        let (_, m) = f.archive(None);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = serde_json::to_vec(&m).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = [0; 4096];
            let _ = stream.read(&mut req);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });
        let mut config = f.config.clone();
        config.feed = Some(format!("http://{address}/manifest.json"));
        let (tx, rx) = mpsc::sync_channel(8);
        let stop = AtomicBool::new(false);
        check(
            &config,
            &Context {
                stop: &stop,
                events: &tx,
            },
        )
        .unwrap();
        assert!(
            matches!(rx.try_recv().unwrap(),Event::Status(Status::Available(Manifest{application,..}))if application==APP)
        );
        server.join().unwrap();
        f.preserved();
    }
    #[test]
    fn semantic_versions_order_beta_stable_and_patch_without_downgrades() {
        for (candidate, current, expected) in [
            ("1.1", "1.1-beta.8", true),
            ("1.1-beta.10", "1.1-beta.9", true),
            ("1.1-beta.2", "1.1", false),
            ("1.0.9", "1.1", false),
            ("v1.1.0+new", "1.1+old", false),
            ("1.1.1", "1.1", true),
            ("1.1", "1.1.0", false),
        ] {
            assert_eq!(
                newer(candidate, current).unwrap(),
                expected,
                "{candidate} vs {current}"
            );
        }
        assert!(version("1.1-beta.01").is_err());
    }
    #[test]
    fn automatic_check_cache_is_daily_private_and_retries_failures_hourly() {
        use std::net::TcpListener;
        let f = Fixture::new();
        let cache = f
            .config
            .installation
            .join(APP)
            .join("conf/update-check.json");
        fs::write(
            cache.parent().unwrap().join(".betterpod-rust.json"),
            b"{\"application\":\"betterpod-rust\",\"ready\":true}",
        )
        .unwrap();
        let (_, m) = f.archive(None);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = serde_json::to_vec(&m).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = [0; 4096];
            let _ = stream.read(&mut req);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });
        let mut config = f.config.clone();
        config.feed = Some(format!("http://{address}/manifest.json"));
        f.ctx(|ctx| check_if_due(&config, &cache, true, ctx).unwrap());
        server.join().unwrap();
        let first: CheckState = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        assert!(first.checked > 0);
        assert!(first.available.is_some());
        // The listener is gone. A fresh cache must avoid a second request.
        f.ctx(|ctx| check_if_due(&config, &cache, false, ctx).unwrap());
        let second: CheckState = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        assert_eq!(first.checked, second.checked);
        // Failed startup refresh keeps the cached offer and schedules one hour.
        f.ctx(|ctx| assert!(check_if_due(&config, &cache, true, ctx).is_err()));
        let failed: CheckState = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        assert!(failed.retry_after >= now() + 3599);
        assert!(failed.available.is_some());
        f.ctx(|ctx| check_if_due(&config, &cache, true, ctx).unwrap());
        let invalid = f.root.join("update-check.json");
        f.ctx(|ctx| assert!(check_if_due(&config, &invalid, true, ctx).is_err()));
        f.preserved();
    }
    #[test]
    fn codec_replacement_is_blocked_and_startup_does_not_touch_foreign_installations() {
        let f = Fixture::new();
        let lib = f.config.installation.join(APP).join("libs/libcodec.so");
        fs::create_dir(lib.parent().unwrap()).unwrap();
        fs::write(&lib, b"existing codec").unwrap();
        let (path, m) = f.archive(Some((
            "betterpod-rust/libs/libcodec.so",
            b"incompatible codec",
        )));
        f.ctx(|ctx| {
            assert!(apply_archive(&f.config, &m, &path, ctx, None)
                .unwrap_err()
                .contains("cannot replace installed codec"))
        });
        assert_eq!(fs::read(&lib).unwrap(), b"existing codec");
        assert!(!recover_installation_if_needed(&f.config).unwrap());
        let work = f.config.installation.join(APP).join(".rust-update");
        fs::create_dir(&work).unwrap();
        fs::write(
            work.join("journal.json"),
            b"{\"application\":\"betterpod\",\"complete\":false,\"entries\":[]}",
        )
        .unwrap();
        assert!(recover_installation_if_needed(&f.config).is_err());
        assert!(work.join("journal.json").exists());
        f.preserved();
    }
}
