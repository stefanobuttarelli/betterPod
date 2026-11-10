//! Cancellable, bounded background services. No request, decode or download runs on the UI.
use crate::{
    curation::{self, CoverQuery},
    downloads::{self, DownloadRequest},
    metadata::{self, TagEdit},
    podcasts::{self, Episode, Podcast},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Provider {
    Archive,
    YouTube,
    YouTubePlaylists,
    Podcast,
    MusicBrainz,
    Itunes,
    Deezer,
    Wikipedia,
}
impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Archive => "Internet Archive",
            Self::YouTube | Self::YouTubePlaylists => "YouTube",
            Self::Podcast => "Podcasts",
            Self::MusicBrainz => "MusicBrainz",
            Self::Itunes => "iTunes",
            Self::Deezer => "Deezer",
            Self::Wikipedia => "Wikipedia",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnlineItem {
    pub provider: Provider,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub url: String,
    pub cover: String,
    pub license: String,
    pub kind: String,
    pub count: usize,
    pub seconds: f64,
    #[serde(default)]
    pub raw: Value,
}
impl OnlineItem {
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            id: String::new(),
            title: String::new(),
            artist: String::new(),
            year: String::new(),
            url: String::new(),
            cover: String::new(),
            license: String::new(),
            kind: String::new(),
            count: 0,
            seconds: 0.0,
            raw: Value::Null,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Task {
    VideoCatalog,
    DeleteSavedVideo {
        path: PathBuf,
    },
    ForgetVideo {
        id: String,
    },
    RememberVideoInfo {
        item: OnlineItem,
        position: f64,
    },
    InstallFfmpeg,
    PrepareCoverQuery {
        paths: Arc<Vec<PathBuf>>,
        album: String,
        artist: String,
        library: Arc<crate::library::Library>,
    },
    SearchCovers {
        provider: Provider,
        album: Arc<CoverQuery>,
        query: Option<String>,
    },
    ArchiveStatus {
        item: OnlineItem,
    },
    DownloadWithCover {
        request: DownloadRequest,
        cover: PathBuf,
        crop: downloads::SquareCrop,
    },
    DownloadWithAutomaticCover {
        request: DownloadRequest,
        url: String,
    },
    CacheArtwork {
        url: String,
    },
    Unmerge {
        paths: Vec<PathBuf>,
    },
    DeleteEpisodes {
        podcast: Podcast,
        episodes: Vec<Episode>,
    },
    DeletePodcast {
        podcast: Podcast,
    },
    SetCover {
        paths: Vec<PathBuf>,
        cover_url: String,
    },
    SetCoverFile {
        paths: Vec<PathBuf>,
        cover: PathBuf,
    },
    SetYouTubeCover {
        paths: Vec<PathBuf>,
        url: String,
    },
    ResetChosenCover {
        paths: Vec<PathBuf>,
    },
    MergeAlbums {
        paths: Vec<PathBuf>,
        target_paths: Vec<PathBuf>,
        album: String,
        artist: String,
    },
    FindMissingCovers {
        albums: Vec<CoverQuery>,
    },
    ApplyAlbumInfo {
        paths: Vec<PathBuf>,
        item: OnlineItem,
    },
    ReadAlbumTags {
        paths: Vec<PathBuf>,
    },
    WriteAlbumTags {
        edits: Vec<(PathBuf, TagEdit)>,
    },
    PreviewAlbumInfo {
        paths: Vec<PathBuf>,
        item: OnlineItem,
    },
    VideoHistory,
    RememberVideo {
        url: String,
        title: String,
        position: f64,
    },
    Downloaded,
    Subscriptions,
    RememberPodcast(Podcast),
    ForgetPodcast(String),
    ResolveStream {
        item: OnlineItem,
        video: bool,
    },
    Search {
        provider: Provider,
        query: String,
        page: u32,
    },
    Details(OnlineItem),
    Feed(Podcast),
    Download(DownloadRequest),
    EditTags {
        paths: Vec<PathBuf>,
        edit: TagEdit,
    },
    Tools,
    RemoveTools,
    ToolsStatus,
}
#[derive(Clone, Debug)]
pub enum Output {
    VideoCatalog(crate::video_catalog::Snapshot),
    CoverQuery(Arc<CoverQuery>),
    ArchiveStatus {
        have: std::collections::BTreeMap<String, (usize, u64)>,
    },
    AlbumTags(Vec<(PathBuf, TagEdit)>),
    AlbumInfoPreview {
        edits: Vec<(PathBuf, TagEdit)>,
        matched: usize,
        total: usize,
    },
    CurationApplied {
        matched: usize,
        total: usize,
    },
    MetadataApplied {
        matched: usize,
        total: usize,
    },
    Subscriptions(Vec<Podcast>),
    Stream(StreamSource),
    Search {
        items: Vec<OnlineItem>,
        total: usize,
    },
    Item(OnlineItem),
    Feed {
        podcast: Podcast,
        episodes: Vec<Episode>,
    },
    Files(Vec<PathBuf>),
    Saved,
    ToolsReady,
    ToolsRemoved,
    ToolsStatus(downloads::ToolStatus),
}
#[derive(Clone, Debug)]
pub struct StreamSource {
    pub video_url: String,
    pub audio_url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub duration: f64,
    pub title: String,
    pub available_at: Option<f64>,
    pub chapters: usize,
}
#[derive(Clone, Debug)]
pub enum Event {
    Progress {
        id: u64,
        done: u64,
        total: Option<u64>,
        status: String,
    },
    Finished {
        id: u64,
        result: Result<Output, String>,
    },
}
#[derive(Clone)]
pub struct JobHandle {
    pub id: u64,
    cancelled: Arc<AtomicBool>,
}
impl JobHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
struct Request {
    task: Task,
    context: Context,
}
pub struct Services {
    tx: Option<mpsc::SyncSender<Request>>,
    download_tx: Option<mpsc::SyncSender<Request>>,
    events: mpsc::SyncSender<Event>,
    rx: Option<mpsc::Receiver<Event>>,
    workers: Vec<thread::JoinHandle<()>>,
    jobs: HashMap<u64, JobHandle>,
    next: u64,
    data: PathBuf,
}
#[derive(Clone)]
pub struct Context {
    pub id: u64,
    cancelled: Arc<AtomicBool>,
    events: mpsc::SyncSender<Event>,
    progress_scope: Option<ProgressScope>,
}
#[derive(Clone)]
struct ProgressScope {
    base: u64,
    total: Option<u64>,
    label: String,
    file_total: bool,
}
impl Context {
    /// Interrupt a standalone worker's bounded request or external tool.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    #[cfg(test)]
    pub(crate) fn cancel_for_test(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Cancelled".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn progress_segment(&self, base: u64, total: Option<u64>, label: String) -> Self {
        let mut child = self.clone();
        child.progress_scope = Some(ProgressScope {
            base,
            total,
            label,
            file_total: false,
        });
        child
    }
    pub(crate) fn progress_file(&self, label: String) -> Self {
        let mut child = self.progress_segment(0, None, label);
        child.progress_scope.as_mut().unwrap().file_total = true;
        child
    }
    fn progress_event(&self, done: u64, total: Option<u64>, status: String) -> Event {
        let (done, total, status) = if let Some(scope) = &self.progress_scope {
            (
                scope.base.saturating_add(done),
                scope.total.or(total.filter(|_| scope.file_total)),
                format!("{} · {status}", scope.label),
            )
        } else {
            (done, total, status)
        };
        Event::Progress {
            id: self.id,
            done,
            total,
            status,
        }
    }
    pub fn progress(&self, done: u64, total: Option<u64>, status: impl Into<String>) {
        let _ = self
            .events
            .try_send(self.progress_event(done, total, status.into()));
    }
    /// A final cumulative report must precede Finished. Only service workers
    /// call this; closing the receiver during shutdown releases the send.
    pub fn progress_important(
        &self,
        done: u64,
        total: Option<u64>,
        status: impl Into<String>,
    ) -> bool {
        self.events
            .send(self.progress_event(done, total, status.into()))
            .is_ok()
    }
    pub fn sleep(&self, duration: Duration) -> Result<(), String> {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.check()?;
            thread::sleep(
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            );
        }
        self.check()
    }
    pub fn get_bytes(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        self.check()?;
        validate_http(url)?;
        let agent = http_agent(true);
        let mut response = agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().as_reader();
        let mut out = Vec::new();
        let mut buffer = [0u8; 32768];
        loop {
            self.check()?;
            use std::io::Read;
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            if out.len().saturating_add(n) > limit {
                return Err("Response is too large".into());
            }
            out.extend_from_slice(&buffer[..n]);
        }
        Ok(out)
    }
    pub fn get_json(&self, url: &str) -> Result<Value, String> {
        service_json(&self.get_bytes(url, 8 * 1024 * 1024)?)
    }
    pub fn standalone() -> Self {
        let (tx, _) = mpsc::sync_channel(1);
        Self {
            id: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            events: tx,
            progress_scope: None,
        }
    }
}
fn service_json(bytes: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid service response: {error}"))?;
    // Every endpoint consumed by this client returns an object. In particular,
    // never let an error array/string reach the mutable metadata projections:
    // serde_json's string indexing would panic and abort the release app.
    if !value.is_object() {
        return Err("Invalid service response: expected an object. Please try again".into());
    }
    Ok(value)
}
pub const USER_AGENT: &str = "betterPod-Rust/0.1 (https://github.com/stefanobuttarelli/betterPod)";
pub fn http_agent(short: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(if short {
            Some(Duration::from_secs(30))
        } else {
            None
        })
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_per_read(Some(Duration::from_secs(10)))
        .build()
        .into()
}
pub fn validate_http(url: &str) -> Result<(), String> {
    if (url.starts_with("https://") || url.starts_with("http://")) && !url.contains(['\r', '\n']) {
        Ok(())
    } else {
        Err("Expected an HTTP or HTTPS URL".into())
    }
}
pub fn base(name: &str, default: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| default.into())
        .trim_end_matches('/')
        .into()
}
pub fn text(value: &Value, key: &str) -> String {
    string(&value[key])
}
pub fn string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Array(values) => values
            .iter()
            .map(string)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}
pub fn number(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .or_else(|| value[key].as_str().and_then(|s| s.parse().ok()))
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}
pub fn encode(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}
fn background_download_priority() {
    #[cfg(target_os = "linux")]
    unsafe {
        // Thread-local on Linux; interactive service and audio workers retain
        // their existing priority. Download helper processes inherit this.
        if libc::getpriority(libc::PRIO_PROCESS, 0) < 10 {
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
        }
        libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13);
    }
}
impl Services {
    pub fn new(data: PathBuf) -> Result<Self, String> {
        Self::new_inner(data, None)
    }
    pub fn new_with_music(data: PathBuf, music: PathBuf) -> Result<Self, String> {
        let music = std::fs::canonicalize(music).map_err(|e| e.to_string())?;
        if !music.is_dir() {
            return Err("Music root is not a directory".into());
        }
        Self::new_inner(data, Some(music))
    }
    fn new_inner(data: PathBuf, music: Option<PathBuf>) -> Result<Self, String> {
        std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
        let (tx, jobs) = mpsc::sync_channel::<Request>(32);
        let (download_tx, download_jobs) = mpsc::sync_channel::<Request>(8);
        let (events, rx) = mpsc::sync_channel(64);
        let jobs = Arc::new(Mutex::new(jobs));
        let download_jobs = Arc::new(Mutex::new(download_jobs));
        let mut workers = Vec::with_capacity(3);
        for n in 0..3 {
            // Long transfers have one bounded lane. Browsing retains both
            // interactive workers while an album is downloading.
            let jobs = if n == 2 {
                download_jobs.clone()
            } else {
                jobs.clone()
            };
            let data = data.clone();
            let music = music.clone();
            let worker = thread::Builder::new()
                .name(format!("online-{n}"))
                .spawn(move || {
                    if n == 2 {
                        background_download_priority();
                    }
                    loop {
                        let request = match jobs.lock() {
                            Ok(r) => match r.recv() {
                                Ok(job) => job,
                                Err(_) => break,
                            },
                            Err(_) => break,
                        };
                        let id = request.context.id;
                        if n == 2 && request.context.check().is_ok() {
                            // Queue state must not stay stale when progress is busy.
                            if !request
                                .context
                                .progress_important(0, None, "Starting download")
                            {
                                break;
                            }
                        }
                        let result =
                            execute(&data, music.as_deref(), &request.context, request.task);
                        if request
                            .context
                            .events
                            .send(Event::Finished { id, result })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            workers.push(worker);
        }
        Ok(Self {
            tx: Some(tx),
            download_tx: Some(download_tx),
            events,
            rx: Some(rx),
            workers,
            jobs: HashMap::new(),
            next: 0,
            data,
        })
    }
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data
    }
    pub fn submit(&mut self, task: Task) -> Result<JobHandle, String> {
        self.next += 1;
        let handle = JobHandle {
            id: self.next,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let context = Context {
            id: handle.id,
            cancelled: handle.cancelled.clone(),
            events: self.events.clone(),
            progress_scope: None,
        };
        let sender = if matches!(
            task,
            Task::Download(_)
                | Task::DownloadWithCover { .. }
                | Task::DownloadWithAutomaticCover { .. }
        ) {
            &self.download_tx
        } else {
            &self.tx
        };
        sender
            .as_ref()
            .ok_or("Services stopped")?
            .try_send(Request { task, context })
            .map_err(|_| "Service queue is full".to_string())?;
        self.jobs.insert(handle.id, handle.clone());
        Ok(handle)
    }
    pub fn poll(&mut self) -> Option<Event> {
        let event = self.rx.as_ref()?.try_recv().ok()?;
        if let Event::Finished { id, .. } = &event {
            self.jobs.remove(id);
        }
        Some(event)
    }
    pub fn cancel_all(&self) {
        for job in self.jobs.values() {
            job.cancel();
        }
    }
    /// Lifecycle barrier: after this returns no worker can modify the old root.
    /// Disconnect events before joining so an unpolled full queue cannot deadlock.
    pub fn shutdown(&mut self) {
        self.cancel_all();
        self.tx.take();
        self.download_tx.take();
        self.rx.take();
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                eprintln!("Online worker stopped unexpectedly");
            }
        }
        self.jobs.clear();
    }
}
impl Drop for Services {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn validated_tag_paths(
    data: &std::path::Path,
    music: Option<&std::path::Path>,
    paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    if let Some(music) = music {
        roots.push(std::fs::canonicalize(music).map_err(|e| e.to_string())?);
    }
    if let Ok(downloads) = std::fs::canonicalize(data.join("downloads")) {
        roots.push(downloads);
    }
    if paths.len() > 10000 {
        return Err("Too many selected songs".into());
    }
    let mut result = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Select regular music files".into());
        }
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        if !roots.iter().any(|root| path.starts_with(root)) {
            return Err("Selected song is outside the music library".into());
        }
        result.push(path);
    }
    Ok(result)
}
#[cfg(test)]
mod album_path_tests {
    use super::*;
    #[test]
    fn editor_paths_are_regular_files_contained_by_music_or_private_downloads() {
        let root =
            std::env::temp_dir().join(format!("betterpod-tags-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let music = root.join("music");
        let data = root.join("data");
        let downloads = data.join("downloads");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::create_dir_all(&downloads).unwrap();
        let song = music.join("song.mp3");
        let private = downloads.join("episode.m4a");
        let outside = root.join("outside.mp3");
        for path in [&song, &private, &outside] {
            std::fs::write(path, b"fixture").unwrap();
        }
        assert_eq!(
            validated_tag_paths(&data, Some(&music), &[song.clone(), private.clone()])
                .unwrap()
                .len(),
            2
        );
        assert!(validated_tag_paths(&data, Some(&music), std::slice::from_ref(&outside)).is_err());
        assert!(validated_tag_paths(&data, Some(&music), std::slice::from_ref(&music)).is_err());
        #[cfg(unix)]
        {
            let link = music.join("escape.mp3");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(validated_tag_paths(&data, Some(&music), &[link]).is_err());
        }
        assert!(validated_tag_paths(&data, None, &[song]).is_err());
        assert!(validated_tag_paths(&data, None, &[private]).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
fn execute(
    data: &std::path::Path,
    music: Option<&std::path::Path>,
    context: &Context,
    task: Task,
) -> Result<Output, String> {
    context.check()?;
    match task {
        Task::PrepareCoverQuery {
            paths,
            album,
            artist,
            library,
        } => Ok(Output::CoverQuery(Arc::new(CoverQuery::from_paths(
            paths.as_ref().clone(),
            album,
            artist,
            &library,
        )))),
        Task::SearchCovers {
            provider,
            album,
            query,
        } => {
            let items = curation::search_cover_items(
                context,
                provider,
                &album.album_query(),
                query.as_deref(),
            )?;
            let total = items.len();
            Ok(Output::Search { items, total })
        }
        Task::ArchiveStatus { item } => Ok(Output::ArchiveStatus {
            have: downloads::archive_status(context, data, &item)?,
        }),
        Task::DownloadWithCover {
            request,
            cover,
            crop,
        } => Ok(Output::Files(downloads::run_with_cover(
            context, data, request, &cover, crop,
        )?)),
        Task::DownloadWithAutomaticCover { request, url } => {
            context.progress(0, None, "Getting cover artwork");
            let cover = cache_artwork(data, context, &url)?;
            context.check()?;
            Ok(Output::Files(downloads::run_with_cover(
                context,
                data,
                request,
                &cover,
                downloads::SquareCrop::Center,
            )?))
        }
        Task::CacheArtwork { url } => Ok(Output::Files(vec![cache_artwork(data, context, &url)?])),
        Task::Unmerge { paths } => {
            curation::unmerge(data, music, &paths, context)?;
            Ok(Output::CurationApplied {
                matched: paths.len(),
                total: paths.len(),
            })
        }
        Task::DeleteEpisodes { podcast, episodes } => {
            podcasts::delete_episodes(data, &podcast, &episodes, false, context)?;
            Ok(Output::Saved)
        }
        Task::DeletePodcast { podcast } => {
            podcasts::delete_episodes(data, &podcast, &[], true, context)?;
            Ok(Output::Saved)
        }
        Task::SetCover { paths, cover_url } => {
            curation::set_cover_url(data, music, &paths, &cover_url, context)?;
            Ok(Output::CurationApplied {
                matched: paths.len(),
                total: paths.len(),
            })
        }
        Task::SetCoverFile { paths, cover } => {
            curation::set_cover_file(data, music, &paths, &cover, context)?;
            Ok(Output::CurationApplied {
                matched: paths.len(),
                total: paths.len(),
            })
        }
        Task::SetYouTubeCover { paths, url } => {
            let cover = cache_artwork(data, context, &url)?;
            let cover = downloads::prepare_square_cover(
                context,
                data,
                &cover,
                downloads::SquareCrop::Center,
            )?;
            curation::set_cover_file(data, music, &paths, &cover, context)?;
            Ok(Output::CurationApplied {
                matched: paths.len(),
                total: paths.len(),
            })
        }
        Task::ResetChosenCover { paths } => {
            curation::reset_chosen_cover(data, music, &paths, context)?;
            Ok(Output::CurationApplied {
                matched: paths.len(),
                total: paths.len(),
            })
        }
        Task::MergeAlbums {
            paths,
            target_paths,
            album,
            artist,
        } => {
            curation::merge(data, music, &paths, &target_paths, &album, &artist, context)?;
            Ok(Output::CurationApplied {
                matched: paths.len(),
                total: paths.len(),
            })
        }
        Task::FindMissingCovers { albums } => {
            let total = albums.len();
            let matched = curation::find_covers(data, music, &albums, context)?;
            Ok(Output::CurationApplied { matched, total })
        }
        Task::ApplyAlbumInfo { paths, item } => {
            let matched = metadata::apply_album_info(&paths, &item, context)?;
            Ok(Output::MetadataApplied {
                matched,
                total: paths.len(),
            })
        }
        Task::ReadAlbumTags { paths } => {
            let paths = validated_tag_paths(data, music, &paths)?;
            Ok(Output::AlbumTags(metadata::read_album_tags(
                &paths, context,
            )?))
        }
        Task::WriteAlbumTags { edits } => {
            let paths: Vec<_> = edits.iter().map(|(path, _)| path.clone()).collect();
            let paths = validated_tag_paths(data, music, &paths)?;
            let edits: Vec<_> = paths
                .into_iter()
                .zip(edits.into_iter().map(|(_, edit)| edit))
                .collect();
            metadata::write_album_tags(&edits, context)?;
            Ok(Output::Saved)
        }
        Task::PreviewAlbumInfo { paths, item } => {
            let paths = validated_tag_paths(data, music, &paths)?;
            let plan = metadata::preview_album_info(&paths, &item, context)?;
            Ok(Output::AlbumInfoPreview {
                edits: plan.edits,
                matched: plan.matched,
                total: plan.total,
            })
        }
        Task::VideoCatalog => Ok(Output::VideoCatalog(crate::video_catalog::catalog(
            context, data,
        )?)),
        Task::DeleteSavedVideo { path } => {
            crate::video_catalog::delete_saved(context, data, &path)?;
            Ok(Output::Saved)
        }
        Task::ForgetVideo { id } => {
            crate::video_catalog::forget(data, &id)?;
            Ok(Output::Saved)
        }
        Task::RememberVideoInfo { item, position } => {
            crate::video_catalog::remember(data, &item, position)?;
            Ok(Output::Saved)
        }
        Task::InstallFfmpeg => {
            crate::video_catalog::install_ffmpeg(context, data)?;
            Ok(Output::Saved)
        }
        Task::VideoHistory => {
            let items = crate::video_catalog::catalog(context, data)?.recent;
            let total = items.len();
            Ok(Output::Search { items, total })
        }
        Task::RememberVideo {
            url,
            title,
            position,
        } => {
            let mut item = OnlineItem::new(Provider::YouTube);
            item.url = url;
            item.title = title;
            crate::video_catalog::remember(data, &item, position)?;
            Ok(Output::Saved)
        }
        Task::Downloaded => Ok(Output::Files(downloads::downloaded(data))),
        Task::Subscriptions => Ok(Output::Subscriptions(podcasts::saved(data))),
        Task::RememberPodcast(podcast) => {
            podcasts::remember(data, podcast)?;
            Ok(Output::Saved)
        }
        Task::ForgetPodcast(feed) => {
            podcasts::forget(data, &feed)?;
            Ok(Output::Saved)
        }
        Task::ResolveStream { item, video } => Ok(Output::Stream(downloads::resolve_stream(
            context, data, &item, video,
        )?)),
        Task::Search {
            provider,
            query,
            page,
        } => {
            let (items, total) = search(context, data, provider, &query, page.max(1))?;
            Ok(Output::Search { items, total })
        }
        Task::Details(item) => Ok(Output::Item(details(context, data, item)?)),
        Task::Feed(podcast) => {
            let (podcast, episodes) = podcasts::fetch(context, podcast)?;
            Ok(Output::Feed { podcast, episodes })
        }
        Task::Download(request) => Ok(Output::Files(downloads::run(context, data, request)?)),
        Task::EditTags { paths, edit } => {
            for (index, path) in paths.iter().enumerate() {
                context.check()?;
                context.progress(
                    index as u64,
                    Some(paths.len() as u64),
                    format!("Saving {}", path.display()),
                );
                metadata::write_tags_cancellable(path, &edit, context)?;
            }
            Ok(Output::Saved)
        }
        Task::ToolsStatus => Ok(Output::ToolsStatus(downloads::tool_status(context, data)?)),
        Task::RemoveTools => {
            downloads::remove_tools(context, data)?;
            Ok(Output::ToolsRemoved)
        }
        Task::Tools => {
            downloads::install_tools(context, data)?;
            Ok(Output::ToolsReady)
        }
    }
}

pub fn search(
    context: &Context,
    data: &std::path::Path,
    provider: Provider,
    query: &str,
    page: u32,
) -> Result<(Vec<OnlineItem>, usize), String> {
    context.progress(0, None, format!("Searching {}", provider.name()));
    match provider {
        Provider::Archive => archive_search(context, query, page),
        Provider::YouTube | Provider::YouTubePlaylists => {
            const PAGE: usize = 25;
            const MAX_PAGES: u32 = 8;
            if page > MAX_PAGES {
                return Ok((Vec::new(), 0));
            }
            let page = page.max(1);
            let end = page as usize * PAGE + 1;
            let target = if provider == Provider::YouTubePlaylists {
                format!(
                    "https://www.youtube.com/results?search_query={}&sp=EgIQAw%3D%3D",
                    encode(query)
                )
            } else {
                // ytsearch is explicitly videos-only. Playlist searches must
                // retain YouTube's playlist filter in the results URL.
                format!("ytsearch{end}:{query}")
            };
            let value = downloads::yt_json(
                context,
                data,
                &[
                    "--flat-playlist",
                    "--playlist-end",
                    &end.to_string(),
                    &target,
                ],
            )?;
            let entries = value["entries"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            context.check()?;
            let offset = (page as usize - 1) * PAGE;
            let valid_item = |entry: &Value| {
                if !youtube_available_entry(entry) {
                    return None;
                }
                let item = youtube_item(entry.clone(), provider);
                (!item.id.is_empty()
                    && (provider != Provider::YouTubePlaylists || item.kind == "playlist"))
                    .then_some(item)
            };
            let earlier = entries.iter().take(offset).filter_map(valid_item).count();
            let items = entries
                .iter()
                .skip(offset)
                .take(PAGE)
                .filter_map(valid_item)
                .collect::<Vec<_>>();
            // A lookahead result means one more page may exist. Do not leave
            // a permanent More row after a short final result page.
            let total = if items.is_empty() {
                0
            } else {
                earlier
                    + items.len()
                    + usize::from(entries.len() > offset + PAGE && page < MAX_PAGES)
            };
            Ok((items, total))
        }
        Provider::Podcast => {
            let podcasts = podcasts::search(context, query)?;
            let total = podcasts.len();
            Ok((
                podcasts
                    .into_iter()
                    .map(|p| {
                        let mut item = OnlineItem::new(provider);
                        item.id = p.feed.clone();
                        item.title = p.name.clone();
                        item.artist = p.author.clone();
                        item.url = p.feed.clone();
                        item.cover = p.art.clone();
                        item.count = p.count;
                        item.raw = serde_json::to_value(p).unwrap_or_default();
                        item
                    })
                    .collect(),
                total,
            ))
        }
        _ => metadata::search(context, provider, query),
    }
}
pub fn details(
    context: &Context,
    data: &std::path::Path,
    item: OnlineItem,
) -> Result<OnlineItem, String> {
    context.progress(0, None, format!("Opening {}", item.title));
    match item.provider {
        Provider::Archive => archive_details(context, &item.id),
        Provider::YouTube | Provider::YouTubePlaylists => {
            if item.kind == "playlist" || youtube_playlist_url(&item.url) {
                let mut raw = downloads::yt_json(
                    context,
                    data,
                    &["--flat-playlist", "--playlist-end", "501", &item.url],
                )?;
                context.check()?;
                let entries = youtube_playlist_entries(&raw)?;
                raw["playlist_count"] = json!(entries.len());
                raw["entries"] = json!(entries);
                return Ok(youtube_item(raw, item.provider));
            }
            let raw = downloads::youtube_video_info(context, data, &item.url)?;
            Ok(youtube_item(raw, item.provider))
        }
        Provider::Podcast => Ok(item),
        _ => metadata::details(context, item),
    }
}
pub fn youtube_item(raw: Value, provider: Provider) -> OnlineItem {
    let mut item = OnlineItem::new(provider);
    item.id = text(&raw, "id");
    item.title = text(&raw, "title");
    item.artist = text(&raw, "artist");
    if item.artist.is_empty() {
        item.artist = text(&raw, "channel");
    }
    if item.artist.is_empty() {
        item.artist = text(&raw, "uploader");
    }
    item.artist = item.artist.trim_end_matches(" - Topic").into();
    item.url = text(&raw, "webpage_url");
    if item.url.is_empty() {
        item.url = text(&raw, "url");
    }
    item.kind = if text(&raw, "_type") == "playlist"
        || matches!(
            text(&raw, "ie_key").as_str(),
            "YoutubeTab" | "YoutubePlaylist"
        )
        || youtube_playlist_url(&item.url)
    {
        "playlist"
    } else {
        "video"
    }
    .into();
    if !item.url.starts_with("http") {
        item.url = if item.kind == "playlist" {
            format!("https://www.youtube.com/playlist?list={}", item.id)
        } else {
            format!("https://www.youtube.com/watch?v={}", item.id)
        };
    }
    item.cover = text(&raw, "thumbnail");
    if item.cover.is_empty() {
        if let Some(thumb) = raw["thumbnails"].as_array().and_then(|a| a.last()) {
            item.cover = text(thumb, "url");
        }
    }
    item.count = ["playlist_count", "entry_count", "n_entries"]
        .into_iter()
        .map(|key| number(&raw, key) as usize)
        .find(|count| *count > 0)
        .unwrap_or_else(|| raw["entries"].as_array().map_or(0, Vec::len));
    item.seconds = number(&raw, "duration");
    item.year = text(&raw, "release_year");
    if item.year.is_empty() {
        item.year = text(&raw, "upload_date").chars().take(4).collect();
    }
    item.raw = youtube_ui_projection(raw);
    item
}

pub(crate) fn youtube_ui_projection(mut raw: Value) -> Value {
    if let Some(object) = raw.as_object_mut() {
        if object
            .get("thumbnail")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            if let Some(thumbnail) = object
                .get("thumbnails")
                .and_then(Value::as_array)
                .and_then(|v| v.last())
                .and_then(|v| v.get("url"))
                .cloned()
            {
                object.insert("thumbnail".into(), thumbnail);
            }
        }
        object.retain(|key, _| {
            key.starts_with('_')
                || matches!(
                    key.as_str(),
                    "id" | "title"
                        | "artist"
                        | "album"
                        | "channel"
                        | "uploader"
                        | "webpage_url"
                        | "url"
                        | "thumbnail"
                        | "duration"
                        | "width"
                        | "height"
                        | "fps"
                        | "release_year"
                        | "upload_date"
                        | "playlist_count"
                        | "entry_count"
                        | "n_entries"
                        | "availability"
                        | "ie_key"
                        | "chapters"
                        | "entries"
                        | "position"
                        | "at"
                        | "local_path"
                )
        });
        if let Some(entries) = object.get_mut("entries").and_then(Value::as_array_mut) {
            for entry in entries {
                *entry = youtube_ui_projection(std::mem::take(entry));
            }
        }
    }
    raw
}

pub(crate) fn youtube_playlist_url(url: &str) -> bool {
    url.contains("list=") && !url.contains("v=") && !url.contains("youtu.be/")
}

fn youtube_available_entry(entry: &Value) -> bool {
    !entry.is_null()
        && !text(entry, "id").is_empty()
        && !matches!(
            text(entry, "availability").as_str(),
            "private" | "needs_auth" | "subscriber_only" | "premium_only"
        )
        && !matches!(
            text(entry, "title").as_str(),
            "[Deleted video]" | "[Private video]"
        )
}

/// Keep an album's original valid video order while refusing unbounded or nested
/// playlists. Opening one does not resolve every video's streaming formats.
pub(crate) fn youtube_playlist_entries(raw: &Value) -> Result<Vec<Value>, String> {
    let entries = raw["entries"]
        .as_array()
        .ok_or("The playlist has no track list")?;
    if entries.len() > 500
        || ["playlist_count", "entry_count", "n_entries"]
            .into_iter()
            .any(|key| number(raw, key) > 500.0)
    {
        return Err("This playlist has more than 500 videos. Choose a smaller playlist".into());
    }
    let entries = entries
        .iter()
        .filter(|entry| youtube_available_entry(entry))
        .filter(|entry| youtube_item((*entry).clone(), Provider::YouTube).kind != "playlist")
        .cloned()
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Err("This playlist has no available videos".into());
    }
    Ok(entries)
}

#[cfg(test)]
mod automatic_cover_tests;
#[cfg(test)]
mod youtube_tests;
fn archive_search(
    context: &Context,
    query: &str,
    page: u32,
) -> Result<(Vec<OnlineItem>, usize), String> {
    let query = format!("({query}) AND mediatype:(audio OR etree)");
    let url=format!("{}/advancedsearch.php?q={}&fl[]=identifier&fl[]=title&fl[]=creator&fl[]=year&fl[]=date&fl[]=licenseurl&sort[]=downloads%20desc&rows=50&page={page}&output=json",base("BETTERPOD_RUST_IA_BASE","https://archive.org"),encode(&query));
    let json = context.get_json(&url)?;
    let rows = json["response"]["docs"]
        .as_array()
        .ok_or("Archive search has no results array")?;
    let items = rows
        .iter()
        .filter_map(|row| {
            let mut item = OnlineItem::new(Provider::Archive);
            item.id = text(row, "identifier");
            if item.id.is_empty() {
                return None;
            }
            item.title = text(row, "title");
            if item.title.is_empty() {
                item.title = item.id.clone();
            }
            item.artist = text(row, "creator");
            item.year = text(row, "year");
            if item.year.is_empty() {
                item.year = text(row, "date").chars().take(4).collect();
            }
            item.license = text(row, "licenseurl");
            item.url = format!(
                "{}/details/{}",
                base("BETTERPOD_RUST_IA_BASE", "https://archive.org"),
                encode(&item.id)
            );
            item.cover = format!(
                "{}/services/img/{}",
                base("BETTERPOD_RUST_IA_BASE", "https://archive.org"),
                encode(&item.id)
            );
            item.raw = row.clone();
            Some(item)
        })
        .collect();
    Ok((
        items,
        json["response"]["numFound"].as_u64().unwrap_or(0) as usize,
    ))
}
fn archive_details(context: &Context, id: &str) -> Result<OnlineItem, String> {
    if id.is_empty() || id.contains(['/', '\\']) {
        return Err("Invalid Archive identifier".into());
    }
    let server = base("BETTERPOD_RUST_IA_BASE", "https://archive.org");
    let raw = context.get_json(&format!("{server}/metadata/{}", encode(id)))?;
    let mut item = OnlineItem::new(Provider::Archive);
    item.id = id.into();
    let metadata = &raw["metadata"];
    item.title = text(metadata, "title");
    item.artist = text(metadata, "creator");
    item.year = text(metadata, "date").chars().take(4).collect();
    item.license = text(metadata, "licenseurl");
    item.url = format!("{server}/details/{}", encode(id));
    item.cover = format!("{server}/services/img/{}", encode(id));
    let mut groups: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut chosen: HashMap<(String, String), (u8, Value)> = HashMap::new();
    for file in raw["files"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let name = text(file, "name");
        let Some((format, priority)) = archive_format(&text(file, "format")) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let key = (
            format.to_string(),
            std::path::Path::new(&name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase(),
        );
        let old = chosen.get(&key).map(|(p, _)| *p);
        if old.is_none_or(|p| priority < p) {
            let mut file = file.clone();
            file["download_url"] = json!(format!(
                "{server}/download/{}/{}",
                encode(id),
                name.split('/').map(encode).collect::<Vec<_>>().join("/")
            ));
            chosen.insert(key, (priority, file));
        }
    }
    for ((format, _), (_, file)) in chosen {
        groups
            .entry(format)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(file);
    }
    for files in groups.values_mut() {
        let files = files.as_array_mut().unwrap();
        files.sort_by_key(|f| {
            (
                text(f, "track")
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .parse::<u32>()
                    .unwrap_or(u32::MAX),
                text(f, "name"),
            )
        });
        let total = files.len();
        for (index, file) in files.iter_mut().enumerate() {
            file["_betterpod_track"] = json!(index + 1);
            file["_betterpod_total"] = json!(total);
        }
    }
    item.count = groups
        .values()
        .filter_map(Value::as_array)
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    // The source response duplicates every file and may contain very large
    // descriptions. Keep only the normalized download groups in UI state.
    item.raw = json!({"groups": groups});
    Ok(item)
}
fn archive_format(format: &str) -> Option<(&'static str, u8)> {
    let format = format.to_lowercase();
    if format.contains("flac") {
        Some(("FLAC", 0))
    } else if format.contains("alac") || format.contains("apple lossless") {
        Some(("ALAC", 0))
    } else if format.contains("wav") {
        Some(("WAV", 0))
    } else if format.contains("aiff") {
        Some(("AIFF", 0))
    } else if format.contains("mp3") {
        Some((
            "MP3",
            if format.contains("vbr") {
                0
            } else if format.contains("320") {
                1
            } else if format.contains("256") {
                2
            } else if format == "mp3" {
                3
            } else {
                4
            },
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn youtube_process_reports_stderr_bytes_and_keeps_playlist_file_totals() {
        let (tx, rx) = mpsc::sync_channel(8);
        let context = Context {
            id: 91,
            cancelled: Arc::new(AtomicBool::new(false)),
            events: tx,
            progress_scope: None,
        };
        let track = context.progress_file("Track 2 of 4".into());
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "printf 'BP_PROGRESS|524288.0|NA|1048576.0\\n' >&2"]);
        downloads::run_command(&track, &mut command, Duration::from_secs(3)).unwrap();
        match rx.recv_timeout(Duration::from_secs(1)).unwrap() {
            Event::Progress {
                id,
                done,
                total,
                status,
            } => {
                assert_eq!(id, 91);
                assert_eq!(done, 524288);
                assert_eq!(total, Some(1048576));
                assert!(status.starts_with("Track 2 of 4"));
            }
            _ => panic!("Expected progress from the child process stderr"),
        }
    }
    #[test]
    fn album_progress_segments_keep_one_cumulative_total() {
        let (tx, rx) = mpsc::sync_channel(4);
        let context = Context {
            id: 7,
            cancelled: Arc::new(AtomicBool::new(false)),
            events: tx,
            progress_scope: None,
        };
        let track = context.progress_segment(100, Some(300), "Track 2 of 2".into());
        track.progress(50, Some(200), "Downloading song.flac");
        let Event::Progress {
            id,
            done,
            total,
            status,
        } = rx.recv().unwrap()
        else {
            panic!("Expected progress");
        };
        assert_eq!((id, done, total), (7, 150, Some(300)));
        assert!(status.contains("Track 2 of 2"));
    }
    #[test]
    fn queued_transfers_do_not_occupy_interactive_workers() {
        use std::io::{Read, Write};
        let data =
            std::env::temp_dir().join(format!("betterpod-download-lane-{}", std::process::id()));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let release = Arc::new(AtomicBool::new(false));
        let released = release.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut streams = Vec::new();
            while !released.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut bytes = [0; 4096];
                        let _ = stream.read(&mut bytes).unwrap();
                        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n").unwrap();
                        streams.push(stream);
                        let _ = ready_tx.send(());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            for mut stream in streams {
                let _ = stream.write_all(b"abcdefgh");
            }
        });
        let mut services = Services::new(data.clone()).unwrap();
        for n in 0..2 {
            services
                .submit(Task::Download(DownloadRequest::File {
                    url: format!("{url}/{n}"),
                    name: format!("{n}.bin"),
                    category: "Fixture".into(),
                    tags: None,
                }))
                .unwrap();
        }
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let fast = services.submit(Task::Downloaded).unwrap();
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut observed = false;
        while Instant::now() < deadline {
            if matches!(services.poll(), Some(Event::Finished { id, result: Ok(_) }) if id == fast.id)
            {
                observed = true;
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        release.store(true, Ordering::Release);
        server.join().unwrap();
        services.shutdown();
        assert!(
            observed,
            "A file transfer prevented an interactive request from running"
        );
        std::fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn shutdown_joins_workers_even_when_the_event_queue_is_full() {
        let data =
            std::env::temp_dir().join(format!("betterpod-services-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let mut services = Services::new(data.clone()).unwrap();
        for _ in 0..64 {
            services
                .events
                .try_send(Event::Progress {
                    id: 0,
                    done: 0,
                    total: None,
                    status: String::new(),
                })
                .unwrap();
        }
        services.submit(Task::Downloaded).unwrap();
        services
            .submit(Task::RememberVideo {
                url: "https://example.invalid/never-contacted".into(),
                title: "Local history only".into(),
                position: 2.0,
            })
            .unwrap();
        services.shutdown();
        assert!(services.submit(Task::Downloaded).is_err());
        std::fs::remove_dir_all(&data).unwrap();
        // A joined writer cannot recreate its former private root after a move.
        thread::sleep(Duration::from_millis(20));
        assert!(!data.exists());
        services.shutdown();
    }
    #[test]
    fn artwork_cache_preserves_aspect_and_works_after_the_local_server_closes() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let data = std::env::temp_dir().join(format!("betterpod-art-cache-{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        let mut body = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(128, 64)
            .write_to(&mut body, image::ImageFormat::Png)
            .unwrap();
        let body = body.into_inner();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/wide-cover", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
            stream.write_all(&body).unwrap();
        });
        let context = Context::standalone();
        let first = cache_artwork(&data, &context, &url).unwrap();
        server.join().unwrap();
        let image = image::open(&first).unwrap();
        assert_eq!(image.width(), image.height() * 2);
        assert_eq!(cache_artwork(&data, &context, &url).unwrap(), first);
        assert!(cache_artwork(&data, &context, "file:///private").is_err());
        std::fs::remove_dir_all(data).unwrap();
    }
}

fn cache_artwork(data: &std::path::Path, context: &Context, url: &str) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    validate_http(url)?;
    let directory = downloads::private_dir(data, &["artwork", "online"])?;
    let path = directory.join(format!("{:x}.jpg", Sha256::digest(url.as_bytes())));
    let decode = |bytes: &[u8]| -> Result<image::DynamicImage, String> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Online artwork exceeds 4 MB".into());
        }
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().map_err(|e| e.to_string())
    };
    if std::fs::symlink_metadata(&path)
        .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink() && m.len() <= 4 * 1024 * 1024)
    {
        if let Ok(bytes) = std::fs::read(&path) {
            if decode(&bytes).is_ok() {
                return Ok(path);
            }
        }
    }
    let bytes = context.get_bytes(url, 4 * 1024 * 1024)?;
    let image = decode(&bytes)?.thumbnail(512, 512);
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    context.check()?;
    crate::storage::atomic_write(&path, &output.into_inner()).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod response_shape_tests {
    use super::*;
    #[test]
    fn service_error_shapes_return_errors_before_mutable_metadata_indexing() {
        for bytes in [
            b"[]".as_slice(),
            b"42",
            b"null",
            b"\"temporarily unavailable\"",
            b"<html>Error</html>",
        ] {
            assert!(service_json(bytes).is_err());
        }
        assert!(service_json(br#"{"metadata":{},"files":[]}"#)
            .unwrap()
            .is_object());
    }
}
