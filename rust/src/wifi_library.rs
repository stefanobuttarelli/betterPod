//! Explicit, short-lived LAN library editing. No listener exists until Start.
//! The pairing code authenticates SPAKE2; subsequent frames are encrypted.
use crate::library::Library;
use rand::{rngs::OsRng, Rng, RngCore};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    io::ErrorKind,
    net::{IpAddr, Shutdown, TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[path = "wifi_library_files.rs"]
mod files;
#[path = "wifi_library_wire.rs"]
mod wire;

pub const DISCOVERY_PORT: u16 = 45841;
pub const DISCOVERY_QUERY: &[u8] = b"betterpod-library-discover-v1";
const PAIRING_LIFETIME: Duration = Duration::from_secs(10 * 60);
const SESSION_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
const IDLE_LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub struct Config {
    pub music: PathBuf,
    pub data: PathBuf,
    pub name: String,
}

#[derive(Clone, Debug)]
pub enum Event {
    Ready {
        code: String,
        port: u16,
        id: String,
        addresses: Vec<String>,
    },
    Connected {
        name: String,
    },
    Disconnected {
        reason: String,
    },
    Changed {
        paths: Vec<PathBuf>,
    },
    Stopped,
    Error(String),
}

struct Snapshot {
    library: Arc<Library>,
    protected: Vec<PathBuf>,
    revision: u64,
}
struct Shared {
    snapshot: Mutex<Snapshot>,
    socket: Mutex<Option<TcpStream>>,
    cancelled: AtomicBool,
}

pub struct Controller {
    config: Config,
    shared: Arc<Shared>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Controller {
    pub fn new(config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            config,
            shared: Arc::new(Shared {
                snapshot: Mutex::new(Snapshot {
                    library: Arc::new(Library::default()),
                    protected: Vec::new(),
                    revision: 1,
                }),
                socket: Mutex::new(None),
                cancelled: AtomicBool::new(true),
            }),
            tx,
            rx,
            worker: None,
        }
    }

    /// No filesystem, random generation or network work runs on the UI thread.
    pub fn start(&mut self, library: Arc<Library>, protected: Vec<PathBuf>) -> Result<(), String> {
        if let Some(worker) = self.worker.as_ref() {
            if !worker.is_finished() {
                return Err(
                    "The previous Wi-Fi library session is still running or stopping.".into(),
                );
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        while self.rx.try_recv().is_ok() {}
        self.update_snapshot(library, protected);
        self.shared.cancelled.store(false, Ordering::Release);
        let config = self.config.clone();
        let shared = self.shared.clone();
        let tx = self.tx.clone();
        self.worker = Some(
            thread::Builder::new()
                .name("wifi-library".into())
                .spawn(move || {
                    if let Err(error) = run(&config, &shared, &tx) {
                        if !shared.cancelled.load(Ordering::Acquire) {
                            let _ = tx.send(Event::Error(error));
                        }
                    }
                    shared.cancelled.store(true, Ordering::Release);
                    if let Ok(mut socket) = shared.socket.lock() {
                        *socket = None;
                    }
                    let _ = tx.send(Event::Stopped);
                })
                .map_err(|e| {
                    self.shared.cancelled.store(true, Ordering::Release);
                    e.to_string()
                })?,
        );
        Ok(())
    }

    /// This only swaps an Arc and a short decoder-path list. A worker reads tags.
    pub fn update_snapshot(&self, library: Arc<Library>, protected: Vec<PathBuf>) {
        if let Ok(mut current) = self.shared.snapshot.lock() {
            if !Arc::ptr_eq(&current.library, &library) {
                current.revision = current.revision.wrapping_add(1);
            }
            current.library = library;
            current.protected = protected;
        }
    }

    pub fn poll(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// Revoke the connection immediately. The worker checks cancellation before
    /// publishing each staged file, including when a batch is interrupted.
    pub fn stop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        if let Ok(socket) = self.shared.socket.lock() {
            if let Some(socket) = socket.as_ref() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }

    pub fn shutdown(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn local_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}

fn run(config: &Config, shared: &Arc<Shared>, tx: &mpsc::Sender<Event>) -> Result<(), String> {
    let tcp = TcpListener::bind(("0.0.0.0", 0))
        .map_err(|e| format!("Cannot open the Wi-Fi library connection: {e}"))?;
    // Discovery can be occupied by another local preview; a displayed address
    // still allows the paired Mac to connect directly to this TCP listener.
    let udp = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).ok();
    tcp.set_nonblocking(true).map_err(|e| e.to_string())?;
    if let Some(udp) = &udp {
        udp.set_read_timeout(Some(Duration::from_millis(40)))
            .map_err(|e| e.to_string())?;
    }
    let mut random = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|e| e.to_string())?;
    let id = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let code = format!("{:08}", OsRng.gen_range(0..100_000_000u32));
    let name = files::safe_name(&config.name);
    let port = tcp.local_addr().map_err(|e| e.to_string())?.port();
    let discovery = serde_json::to_vec(&json!({"v":1,"id":id,"name":name,"port":port}))
        .map_err(|e| e.to_string())?;
    let roots = files::Roots::new(config)?;
    tx.send(Event::Ready {
        code: code.clone(),
        port,
        id: id.clone(),
        addresses: local_addresses(),
    })
    .map_err(|e| e.to_string())?;
    let until = Instant::now() + PAIRING_LIFETIME;
    let mut attempts = 0;
    let mut buffer = [0; 256];
    while !shared.cancelled.load(Ordering::Acquire) {
        if Instant::now() >= until {
            return Err("The Wi-Fi library code expired. Start a new session.".into());
        }
        if let Some(udp) = &udp {
            match udp.recv_from(&mut buffer) {
                Ok((length, peer))
                    if local_peer(peer.ip()) && &buffer[..length] == DISCOVERY_QUERY =>
                {
                    let _ = udp.send_to(&discovery, peer);
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.to_string()),
            }
        } else {
            thread::sleep(Duration::from_millis(40));
        }
        match tcp.accept() {
            Ok((stream, peer)) if local_peer(peer.ip()) => {
                if let Ok(mut active) = shared.socket.lock() {
                    *active = stream.try_clone().ok();
                }
                match wire::accept(stream, &code, &id, &name, shared) {
                    Ok((mut channel, client_name)) => {
                        let _ = tx.send(Event::Connected { name: client_name });
                        let result = serve(config, &roots, shared, tx, &mut channel);
                        if let Ok(mut active) = shared.socket.lock() {
                            *active = None;
                        }
                        if shared.cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let _ = tx.send(Event::Disconnected {
                            reason: result
                                .err()
                                .unwrap_or_else(|| "The Mac disconnected.".into()),
                        });
                    }
                    Err(_) => {
                        attempts += 1;
                        if let Ok(mut active) = shared.socket.lock() {
                            *active = None;
                        }
                        if attempts >= 5 {
                            return Err("Too many unsuccessful pairing attempts. Start a new Wi-Fi library session.".into());
                        }
                    }
                }
            }
            Ok((stream, _)) => {
                let _ = stream.shutdown(Shutdown::Both);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

fn local_addresses() -> Vec<String> {
    let mut addresses = Vec::new();
    #[cfg(unix)]
    unsafe {
        let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut first) == 0 {
            let mut current = first;
            while !current.is_null() {
                let item = &*current;
                if !item.ifa_addr.is_null()
                    && (*item.ifa_addr).sa_family as i32 == libc::AF_INET
                    && item.ifa_flags & libc::IFF_UP as u32 != 0
                {
                    let value = &*(item.ifa_addr as *const libc::sockaddr_in);
                    let ip = std::net::Ipv4Addr::from(value.sin_addr.s_addr.to_ne_bytes());
                    if !ip.is_loopback() && (ip.is_private() || ip.is_link_local()) {
                        addresses.push(ip.to_string());
                    }
                }
                current = item.ifa_next;
            }
            libc::freeifaddrs(first);
        }
    }
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        addresses.push("127.0.0.1".into());
    }
    addresses
}

fn serve(
    config: &Config,
    roots: &files::Roots,
    shared: &Arc<Shared>,
    tx: &mpsc::Sender<Event>,
    channel: &mut wire::Channel,
) -> Result<(), String> {
    let started = Instant::now();
    let mut changed_artwork = HashSet::new();
    let mut changed_tags = HashSet::new();
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if started.elapsed() > SESSION_LIFETIME {
            return Err("The Wi-Fi library session expired. Connect again.".into());
        }
        let request = match channel.receive(shared, IDLE_LIFETIME) {
            Ok(request) => request,
            Err(error) if error == "Connection closed" => return Ok(()),
            Err(error) => return Err(error),
        };
        let result = handle(
            config,
            roots,
            shared,
            tx,
            &request,
            &mut changed_tags,
            &mut changed_artwork,
        );
        let response = result.unwrap_or_else(|error| json!({"error":error}));
        channel.send(&response, shared)?;
        if request.get("op").and_then(Value::as_str) == Some("disconnect") {
            return Ok(());
        }
    }
}

fn handle(
    config: &Config,
    roots: &files::Roots,
    shared: &Arc<Shared>,
    tx: &mpsc::Sender<Event>,
    request: &Value,
    changed_tags: &mut HashSet<PathBuf>,
    changed_artwork: &mut HashSet<PathBuf>,
) -> Result<Value, String> {
    let (library, revision, protected) = {
        let state = shared.snapshot.lock().map_err(|e| e.to_string())?;
        (
            state.library.clone(),
            state.revision.to_string(),
            state.protected.clone(),
        )
    };
    let protected = protected
        .into_iter()
        .flat_map(|path| {
            let canonical = std::fs::canonicalize(&path).ok();
            std::iter::once(path).chain(canonical)
        })
        .collect::<HashSet<_>>();
    match request.get("op").and_then(Value::as_str).unwrap_or("") {
        "list" => {
            let display = (!changed_tags.is_empty())
                .then(|| crate::curation::RemoteAlbumDisplay::load(&config.data));
            let offset = request.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let limit = request
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(256)
                .min(256) as usize;
            if limit == 0 || offset > library.tracks.len() {
                return Err("Invalid library page".into());
            }
            let tracks = library
                .tracks
                .iter()
                .skip(offset)
                .take(limit)
                .map(|track| {
                    files::record_with_display(
                        track,
                        roots,
                        changed_tags.contains(&track.path),
                        &protected,
                        display.as_ref(),
                    )
                })
                .collect::<Vec<_>>();
            Ok(
                json!({"tracks":tracks,"total":library.tracks.len(),"revision":revision,"capabilities":["album-join-v1"]}),
            )
        }
        "read" => {
            let display = (!changed_tags.is_empty())
                .then(|| crate::curation::RemoteAlbumDisplay::load(&config.data));
            let paths: Vec<String> =
                serde_json::from_value(request.get("paths").cloned().unwrap_or(Value::Null))
                    .map_err(|_| "Invalid selected paths")?;
            if paths.len() > 256 {
                return Err("Too many selected tracks".into());
            }
            let mut tracks = Vec::new();
            let mut skipped = Vec::new();
            for path in paths {
                if let Some(track) = library
                    .tracks
                    .iter()
                    .find(|track| track.path == PathBuf::from(&path))
                {
                    tracks.push(files::record_with_display(
                        track,
                        roots,
                        changed_tags.contains(&track.path),
                        &protected,
                        display.as_ref(),
                    ));
                } else {
                    skipped.push(format!("{path}: not in this library"));
                }
            }
            Ok(
                json!({"tracks":tracks,"skipped":skipped,"revision":revision,"capabilities":["album-join-v1"]}),
            )
        }
        "cover" => {
            let path = request
                .get("path")
                .and_then(Value::as_str)
                .ok_or("Missing track path")?;
            let track = library
                .tracks
                .iter()
                .find(|t| t.path == PathBuf::from(path))
                .ok_or("This track is not in the library")?;
            files::cover(track, roots, changed_artwork.contains(&track.path))
        }
        "save" => {
            let changes: Vec<files::Change> =
                serde_json::from_value(request.get("tracks").cloned().unwrap_or(Value::Null))
                    .map_err(|e| format!("Invalid tag changes: {e}"))?;
            if changes.is_empty() || changes.len() > 50 {
                return Err("Save between 1 and 50 tracks per request".into());
            }
            let mut saved = Vec::new();
            let mut joined = Vec::new();
            let mut failures = Vec::new();
            let mut warnings = Vec::new();
            let mut seen = HashSet::new();
            for change in changes {
                let path = change.path.clone();
                let result = if !seen.insert(path.clone()) {
                    Err("This file appears more than once in the batch".into())
                } else {
                    files::save(config, roots, shared, &library, &change)
                };
                match result {
                    Ok(warning) => {
                        saved.push(path.clone());
                        if change.join_album && warning.is_none() {
                            joined.push(path.clone());
                        }
                        if change.changes_grouping() {
                            changed_tags.insert(path.clone());
                        }
                        if change.cover.is_some() || change.remove_cover {
                            changed_artwork.insert(path.clone());
                        }
                        if let Some(warning) = warning {
                            warnings.push(warning);
                        }
                    }
                    Err(error) => failures.push(json!({"path":path,"error":error})),
                }
                if shared.cancelled.load(Ordering::Acquire) {
                    break;
                }
            }
            if !saved.is_empty() {
                if let Ok(mut state) = shared.snapshot.lock() {
                    state.revision = state.revision.wrapping_add(1);
                }
                let _ = tx.send(Event::Changed {
                    paths: saved.clone(),
                });
            }
            let revision = shared
                .snapshot
                .lock()
                .map_err(|e| e.to_string())?
                .revision
                .to_string();
            Ok(
                json!({"saved":saved,"joined":joined,"failures":failures,"warnings":warnings,"revision":revision}),
            )
        }
        "disconnect" => Ok(json!({"ok":true})),
        _ => Err("Unknown library request".into()),
    }
}

#[cfg(test)]
#[path = "wifi_library_tests.rs"]
mod tests;
