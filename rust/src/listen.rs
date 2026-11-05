//! Source Listen semantics: temporary current/next files, never library downloads.
//! One joined worker owns disk, HTTP, URL resolution and copy-only audio remuxes.
use crate::{
    downloads,
    online::{self, Context, OnlineItem, StreamSource},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, RwLock,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const FIRST: f64 = 20.0;
#[path = "listen_mp4.rs"]
mod mp4;
const NEXT: f64 = 180.0;
const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;
static SERIAL: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed,
    Ended,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub index: usize,
    pub part: usize,
    pub offset: f64,
    pub duration: f64,
    pub done: u64,
    pub total: Option<u64>,
    pub phase: Phase,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Ready {
    pub generation: u64,
    pub index: usize,
    pub part: usize,
    pub offset: f64,
    pub duration: f64,
    pub source: StreamSource,
}
enum Request {
    Start {
        generation: u64,
        items: Vec<OnlineItem>,
        index: usize,
        refresh: bool,
    },
    Advance {
        previous: u64,
        generation: u64,
    },
    Stop,
}
struct Shared {
    state: RwLock<Snapshot>,
    ready: Mutex<Option<Ready>>,
    active: Mutex<Option<Context>>,
    desired: AtomicU64,
}
pub struct ListenClient {
    tx: Option<mpsc::SyncSender<Request>>,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    generation: u64,
    worker: Option<JoinHandle<()>>,
    cached: RefCell<Snapshot>,
}
impl ListenClient {
    pub fn new(data: PathBuf) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(8);
        let shared = Arc::new(Shared {
            state: RwLock::new(Snapshot::default()),
            ready: Mutex::new(None),
            active: Mutex::new(None),
            desired: AtomicU64::new(0),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (published, stopped) = (shared.clone(), stop.clone());
        let worker = thread::Builder::new()
            .name("listen-cache".into())
            .spawn(move || worker(data, rx, published, stopped))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx: Some(tx),
            shared,
            stop,
            generation: 0,
            worker: Some(worker),
            cached: RefCell::new(Snapshot::default()),
        })
    }
    pub fn start(&mut self, items: Vec<OnlineItem>, index: usize) -> Result<(), String> {
        self.start_mode(items, index, false)
    }
    pub fn retry(&mut self, items: Vec<OnlineItem>, index: usize) -> Result<(), String> {
        self.start_mode(items, index, true)
    }
    fn start_mode(
        &mut self,
        items: Vec<OnlineItem>,
        index: usize,
        refresh: bool,
    ) -> Result<(), String> {
        if index >= items.len() || items.len() > 10000 {
            return Err("No Listen song selected".into());
        }
        for item in &items {
            online::validate_http(&item.url)?;
        }
        self.generation = self.generation.wrapping_add(1);
        self.shared
            .desired
            .store(self.generation, Ordering::Release);
        self.cancel_active();
        if let Ok(mut ready) = self.shared.ready.lock() {
            *ready = None;
        }
        let initial = Snapshot {
            generation: self.generation,
            index,
            duration: items[index].seconds.max(0.0),
            phase: Phase::Loading,
            ..Snapshot::default()
        };
        if let Ok(mut state) = self.shared.state.write() {
            *state = initial.clone();
        }
        *self.cached.borrow_mut() = initial;
        self.send(Request::Start {
            generation: self.generation,
            items,
            index,
            refresh,
        })
    }
    pub fn advance(&mut self) -> Result<(), String> {
        let previous = self.generation;
        self.generation = self.generation.wrapping_add(1);
        self.shared
            .desired
            .store(self.generation, Ordering::Release);
        self.cancel_active();
        self.send(Request::Advance {
            previous,
            generation: self.generation,
        })
    }
    pub fn stop(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.shared
            .desired
            .store(self.generation, Ordering::Release);
        self.cancel_active();
        let _ = self.send(Request::Stop);
        if let Ok(mut ready) = self.shared.ready.lock() {
            *ready = None;
        }
        if let Ok(mut state) = self.shared.state.write() {
            *state = Snapshot::default();
        }
        *self.cached.borrow_mut() = Snapshot::default();
    }
    pub fn snapshot(&self) -> Snapshot {
        if let Ok(state) = self.shared.state.try_read() {
            if state.generation == self.generation {
                *self.cached.borrow_mut() = state.clone();
            }
        }
        self.cached.borrow().clone()
    }
    pub fn take_ready(&self) -> Option<Ready> {
        self.shared
            .ready
            .try_lock()
            .ok()?
            .take()
            .filter(|ready| ready.generation == self.generation)
    }
    fn send(&self, request: Request) -> Result<(), String> {
        self.tx
            .as_ref()
            .ok_or("Listen worker closed")?
            .try_send(request)
            .map_err(|_| "Listen worker is busy".into())
    }
    fn cancel_active(&self) {
        if let Ok(active) = self.shared.active.lock() {
            if let Some(ctx) = active.as_ref() {
                ctx.cancel();
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.cancel_active();
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for ListenClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Plan {
    source: StreamSource,
    chunked: bool,
    duration: f64,
    fragment: Option<mp4::FragmentPlan>,
}
impl Plan {
    fn part(&self, index: usize) -> Option<(f64, f64)> {
        if let Some(fragment) = self.fragment.as_ref() {
            fragment
                .parts
                .get(index)
                .map(|part| (part.start, part.duration))
        } else {
            segment(index, self.duration)
        }
    }
}
#[derive(Clone)]
struct File {
    path: PathBuf,
    offset: f64,
    duration: f64,
    length: f64,
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
struct Session {
    generation: u64,
    items: Vec<OnlineItem>,
    index: usize,
    part: usize,
    root: PathBuf,
    plans: HashMap<usize, Plan>,
    files: HashMap<(usize, usize), File>,
    errors: HashMap<(usize, usize), String>,
    announced: Option<(u64, usize, usize)>,
    ended: bool,
}
fn segment(part: usize, duration: f64) -> Option<(f64, f64)> {
    let start = if part == 0 {
        0.0
    } else {
        FIRST + (part - 1) as f64 * NEXT
    };
    if !duration.is_finite() || duration <= start {
        return None;
    }
    Some((
        start,
        (if part == 0 { FIRST } else { NEXT }).min(duration - start),
    ))
}
impl Session {
    fn state(&self, phase: Phase) -> Snapshot {
        let offset = self
            .plans
            .get(&self.index)
            .filter(|p| p.chunked)
            .and_then(|p| p.part(self.part))
            .map_or(0.0, |p| p.0);
        Snapshot {
            generation: self.generation,
            index: self.index,
            part: self.part,
            offset,
            duration: self
                .plans
                .get(&self.index)
                .map_or(self.items[self.index].seconds, |p| p.duration),
            phase,
            ..Snapshot::default()
        }
    }
    fn target(&self) -> Option<(usize, usize)> {
        if self.ended {
            return None;
        }
        let current = (self.index, self.part);
        if !self.files.contains_key(&current) && !self.errors.contains_key(&current) {
            return Some(current);
        }
        if let Some(plan) = self.plans.get(&self.index).filter(|p| p.chunked) {
            let next = (self.index, self.part + 1);
            if plan.part(self.part + 1).is_some()
                && !self.files.contains_key(&next)
                && !self.errors.contains_key(&next)
            {
                return Some(next);
            }
        }
        let next = (self.index + 1, 0);
        if next.0 < self.items.len()
            && !self.files.contains_key(&next)
            && !self.errors.contains_key(&next)
        {
            return Some(next);
        }
        None
    }
    fn prune(&mut self) {
        let current = self.index;
        let part = self.part;
        self.files.retain(|&(index, p), file| {
            let keep = (index == current && (p == part || p == part + 1))
                || (index == current + 1 && p == 0);
            if !keep {
                let _ = fs::remove_file(&file.path);
            }
            keep
        });
        self.plans.retain(|&i, _| i == current || i == current + 1);
        self.errors.retain(|&(i, p), _| {
            (i == current && (p == part || p == part + 1)) || (i == current + 1 && p == 0)
        });
    }
}
fn publish(shared: &Shared, state: Snapshot) {
    if let Ok(mut target) = shared.state.write() {
        *target = state;
    }
}
fn worker(data: PathBuf, rx: mpsc::Receiver<Request>, shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    let mut session: Option<Session> = None;
    while !stop.load(Ordering::Acquire) {
        let command = if session.as_ref().is_some_and(|s| s.target().is_some()) {
            rx.try_recv().ok()
        } else {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(v) => Some(v),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(_) => break,
            }
        };
        if let Some(command) = command {
            match command {
                Request::Stop => {
                    if let Some(old) = session.take() {
                        let _ = fs::remove_dir_all(old.root);
                    }
                    publish(&shared, Snapshot::default());
                    continue;
                }
                Request::Start {
                    generation,
                    items,
                    index,
                    refresh,
                } => {
                    let reuse = session.as_ref().is_some_and(|s| {
                        s.items.len() == items.len()
                            && s.items.iter().zip(&items).all(|(a, b)| a.url == b.url)
                    });
                    if !reuse {
                        if let Some(old) = session.take() {
                            let _ = fs::remove_dir_all(old.root);
                        }
                        match downloads::private_dir(
                            &data,
                            &[
                                "tmp",
                                &format!(
                                    "listen-{}-{}",
                                    std::process::id(),
                                    SERIAL.fetch_add(1, Ordering::Relaxed)
                                ),
                            ],
                        ) {
                            Ok(root) => {
                                session = Some(Session {
                                    generation,
                                    items,
                                    index,
                                    part: 0,
                                    root,
                                    plans: HashMap::new(),
                                    files: HashMap::new(),
                                    errors: HashMap::new(),
                                    announced: None,
                                    ended: false,
                                })
                            }
                            Err(error) => {
                                publish(
                                    &shared,
                                    Snapshot {
                                        generation,
                                        index,
                                        phase: Phase::Failed,
                                        error: Some(error),
                                        ..Snapshot::default()
                                    },
                                );
                                continue;
                            }
                        }
                    } else if let Some(s) = session.as_mut() {
                        s.generation = generation;
                        s.items = items;
                        s.index = index;
                        s.part = 0;
                        s.announced = None;
                        s.ended = false;
                        if refresh {
                            s.files.retain(|&(i, _), file| {
                                if i == index {
                                    let _ = fs::remove_file(&file.path);
                                    false
                                } else {
                                    true
                                }
                            });
                            s.plans.remove(&index);
                            s.errors.retain(|&(i, _), _| i != index);
                        }
                        s.errors.remove(&(index, 0));
                        s.prune();
                    }
                }
                Request::Advance {
                    previous,
                    generation,
                } => {
                    let Some(s) = session.as_mut().filter(|s| s.generation == previous) else {
                        continue;
                    };
                    s.generation = generation;
                    let another_part = s
                        .plans
                        .get(&s.index)
                        .is_some_and(|p| p.chunked && p.part(s.part + 1).is_some());
                    if another_part {
                        s.part += 1;
                    } else if s.index + 1 < s.items.len() {
                        s.index += 1;
                        s.part = 0;
                    } else {
                        s.ended = true;
                        publish(&shared, s.state(Phase::Ended));
                        continue;
                    }
                    s.announced = None;
                    s.prune();
                }
            }
        }
        let Some(s) = session.as_mut() else {
            continue;
        };
        if s.generation != shared.desired.load(Ordering::Acquire) {
            continue;
        }
        if s.ended {
            continue;
        }
        let key = (s.index, s.part);
        if let Some(error) = s.errors.get(&key) {
            let mut state = s.state(Phase::Failed);
            state.error = Some(error.clone());
            publish(&shared, state);
        } else if let Some(file) = s.files.get(&key) {
            if s.announced != Some((s.generation, s.index, s.part)) {
                let ready = Ready {
                    generation: s.generation,
                    index: s.index,
                    part: s.part,
                    offset: file.offset,
                    duration: file.duration,
                    source: StreamSource {
                        video_url: file.path.to_string_lossy().into_owned(),
                        audio_url: None,
                        headers: Vec::new(),
                        duration: file.length,
                        title: s.items[s.index].title.clone(),
                        available_at: None,
                        chapters: 0,
                    },
                };
                if let Ok(mut target) = shared.ready.lock() {
                    *target = Some(ready);
                }
                s.announced = Some((s.generation, s.index, s.part));
                publish(&shared, s.state(Phase::Ready));
            }
        } else {
            publish(&shared, s.state(Phase::Loading));
        }
        let Some(target) = s.target() else {
            continue;
        };
        let context = Context::standalone();
        if let Ok(mut active) = shared.active.lock() {
            *active = Some(context.clone());
        }
        if stop.load(Ordering::Acquire) || s.generation != shared.desired.load(Ordering::Acquire) {
            context.cancel();
        }
        let result = fetch(&context, &data, s, target, &shared);
        if let Ok(mut active) = shared.active.lock() {
            *active = None;
        }
        if context.check().is_ok() {
            match result {
                Ok(file) => {
                    s.files.insert(target, file);
                }
                Err(error) => {
                    s.errors.insert(target, error);
                }
            }
        } else if let Ok(file) = result {
            let _ = fs::remove_file(file.path);
        }
    }
    if let Some(old) = session.take() {
        let _ = fs::remove_dir_all(old.root);
    }
}
fn fetch(
    context: &Context,
    data: &Path,
    session: &mut Session,
    key: (usize, usize),
    shared: &Shared,
) -> Result<File, String> {
    let (index, part) = key;
    let item = &session.items[index];
    if !session.plans.contains_key(&index) {
        let youtube = item.url.contains("youtube.com/") || item.url.contains("youtu.be/");
        let source = if youtube {
            downloads::resolve_listen_stream(context, data, item)?
        } else {
            StreamSource {
                video_url: item.url.clone(),
                audio_url: None,
                headers: Vec::new(),
                duration: item.seconds,
                title: item.title.clone(),
                available_at: None,
                chapters: 0,
            }
        };
        let fragment = if youtube {
            mp4::open(context, &source)?
        } else {
            None
        };
        let duration = fragment
            .as_ref()
            .map_or(source.duration.max(item.seconds), |p| p.duration);
        session.plans.insert(
            index,
            Plan {
                source,
                chunked: fragment.is_some(),
                duration,
                fragment,
            },
        );
    }
    let plan = &session.plans[&index];
    let path = session.root.join(format!(
        "{index:05}-{part:05}.{}",
        if plan.chunked { "mka" } else { "audio" }
    ));
    let temp = path.with_extension("part");
    let _temporary = Temporary(temp.clone());
    let active = key == (session.index, session.part);
    let (offset, length) = if plan.chunked {
        let (offset, length) = plan.part(part).ok_or("Listen part is out of range")?;
        if let Some(fragment) = plan.fragment.as_ref() {
            let mut state = session.state(Phase::Loading);
            mp4::fetch(
                context,
                data,
                &plan.source,
                fragment,
                part,
                &temp,
                |done, total| {
                    if active {
                        state.done = done;
                        state.total = total;
                        publish(shared, state.clone());
                    }
                },
            )?;
        }
        (offset, length)
    } else {
        let mut state = session.state(Phase::Loading);
        fetch_whole(
            context,
            &plan.source.video_url,
            &plan.source.headers,
            &temp,
            |done, total| {
                if active {
                    state.done = done;
                    state.total = total;
                    publish(shared, state.clone());
                }
            },
        )?;
        (0.0, plan.duration)
    };
    context.check()?;
    fs::rename(&temp, &path).map_err(|e| e.to_string())?;
    Ok(File {
        path,
        offset,
        duration: plan.duration,
        length,
    })
}
fn fetch_whole(
    context: &Context,
    url: &str,
    headers: &[(String, String)],
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    online::validate_http(url)?;
    context.check()?;
    let mut request = online::http_agent(false)
        .get(url)
        .header("User-Agent", online::USER_AGENT);
    for (name, value) in headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err("Invalid Listen header".into());
        }
        request = request.header(name, value);
    }
    let mut response = request.call().map_err(|e| e.to_string())?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if total.is_some_and(|n| n > MAX_FILE) {
        return Err("Listen file is too large".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dest)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut reader = response.body_mut().as_reader();
        let mut buffer = [0u8; 64 * 1024];
        let mut done = 0;
        progress(0, total);
        loop {
            context.check()?;
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > MAX_FILE {
                return Err("Listen file is too large".into());
            }
            file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            progress(done, total);
        }
        if done == 0 || total.is_some_and(|n| n != done) {
            return Err("Listen download was incomplete".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}
use std::os::unix::fs::OpenOptionsExt;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_segment_lengths_and_cache_bound() {
        assert_eq!(segment(0, 500.), Some((0., 20.)));
        assert_eq!(segment(1, 500.), Some((20., 180.)));
        assert_eq!(segment(3, 500.), Some((380., 120.)));
        assert_eq!(segment(4, 500.), None);
        assert_eq!(segment(0, 10.), Some((0., 10.)));
        assert_eq!(segment(0, 0.), None);
    }
    #[test]
    fn client_rejects_non_http_without_creating_files() {
        let root = std::env::temp_dir().join(format!(
            "betterpod-listen-invalid-{}",
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let mut client = ListenClient::new(root.clone()).unwrap();
        let mut item = OnlineItem::new(online::Provider::Archive);
        item.url = "file:///etc/passwd".into();
        assert!(client.start(vec![item], 0).is_err());
        client.shutdown();
        assert!(!root.exists());
    }
}

#[cfg(test)]
#[path = "listen_qa.rs"]
mod qa;
