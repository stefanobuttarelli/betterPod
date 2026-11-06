//! UI for background library services. No task or menu callback performs I/O.
#[path = "app_cover.rs"]
mod app_cover;
#[path = "app_listen.rs"]
mod listen_ui;
#[path = "app_metadata.rs"]
mod metadata_edit;
#[path = "app_share_visual.rs"]
mod share_visual;
#[path = "app_services_videos.rs"]
mod videos_ui;
#[path = "app_services_visual.rs"]
mod visual;
#[path = "app_weather_visual.rs"]
mod weather_visual;
use super::{Keyboard, KeyboardEffect, VisualAssets, FG, MUTED, WHITE};
use crate::input::Action;
use crate::render::Renderer;
use app_cover::{Editor as CoverEditor, Effect as CoverEffect};
use betterpod_rust::bluetooth::{Bluetooth, Request as BtRequest, State as BtState};
use betterpod_rust::downloads::DownloadRequest;
use betterpod_rust::metadata::TagEdit;
use betterpod_rust::online::{
    Event, JobHandle, OnlineItem, Output, Provider, Services, StreamSource, Task,
};
use betterpod_rust::playlists::{self, Playlist, PlaylistClient};
use betterpod_rust::podcasts::{Episode, Podcast};
use betterpod_rust::share::{self, ShareClient, ShareInfo};
use betterpod_rust::weather::{Place, Weather, WeatherCommand, WeatherEvent, WeatherSnapshot};
use share_visual::{ShareAction, ShareVisual};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use videos_ui::{VideoAction, VideoJob};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExtraPage {
    Playlists,
    AddToPlaylist,
    SearchSources,
    Online(Provider),
    Podcasts,
    Downloads,
    Tools,
    Bluetooth,
    AlbumInfo,
    Videos,
    SavedVideos,
    VideoHistory,
    Weather,
    Receive,
    Sharing,
}
#[derive(Clone)]
pub(super) enum Effect {
    VideoOpening {
        item: OnlineItem,
        queue: Vec<OnlineItem>,
        index: usize,
        start: f64,
    },
    VideoResolveError {
        origin: String,
        error: String,
    },
    Exit,
    Notice(String),
    Play {
        paths: Vec<PathBuf>,
        index: usize,
    },
    Stream {
        source: StreamSource,
        video: bool,
        origin: String,
        start: f64,
        queue: Vec<OnlineItem>,
        index: usize,
    },
    Rescan,
    DeleteAlbum(Vec<PathBuf>),
    MergeAlbum(Vec<PathBuf>),
    ChooseAlbumCover {
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
    },
    MergeApplied,
    MergeFailed(String),
    Queue {
        paths: Vec<PathBuf>,
        next: bool,
    },
    SharePaths(Vec<PathBuf>),
    SharingName(String),
    YouTubeEnabled(bool),
    PreviewOpening {
        item: OnlineItem,
        queue: Vec<OnlineItem>,
        index: usize,
    },
    Preview {
        source: StreamSource,
        origin: String,
        queue: Vec<OnlineItem>,
        index: usize,
    },
    TogglePreview,
    StopPreview,
    MediaArtwork {
        origin: String,
        path: PathBuf,
    },
}
#[derive(Clone)]
enum Choice {
    Video(VideoAction),
    Page(ExtraPage),
    NewPlaylist,
    MediaQueue(Vec<PathBuf>, bool),
    MediaAddPlaylist(Vec<PathBuf>),
    MediaShare(Vec<PathBuf>),
    CombinedSearch,
    CombinedClear,
    CombinedAutomatic,
    Playlist(Playlist),
    PlaylistSong(Arc<Playlist>, usize),
    AppendPlaylist(Playlist),
    DeletePlaylist(Playlist),
    RemovePlaylist(Arc<Playlist>, usize),
    Search(Provider),
    ClearSearch(Provider),
    ArchiveTrack(
        OnlineItem,
        String,
        serde_json::Value,
        Vec<OnlineItem>,
        usize,
    ),
    Preview(OnlineItem, Vec<OnlineItem>, usize),
    ConfirmDeletePlaylist(Playlist),
    ConfirmBtForget(String),
    ConfirmAction(Box<Choice>, String),
    YouTubeOff,
    ToggleYouTube,
    NextSearch(Provider, String, u32, bool),
    Online(OnlineItem, bool),
    ResumeVideo(OnlineItem, f64),
    Recent(OnlineItem),
    Download(DownloadRequest),
    Media(String, String, bool),
    Feed(Podcast),
    Subscribe(Podcast),
    Unsubscribe(Podcast),
    Episode(Episode, Podcast),
    DownloadEpisodes(Podcast, Vec<Episode>),
    TagField(Field),
    SaveTags,
    SaveTagBatch,
    UndoTags,
    FindInfo,
    EditSongs,
    EditSong(usize),
    ChooseEditedCover,
    MergeEdited,
    SeparateEdited,
    DeleteEditedAlbum,
    ResetEditedCover,
    SeparateBatch(Vec<PathBuf>),
    MetadataSearch(Provider),
    ApplyMetadata(OnlineItem),
    BtPower(bool),
    BtScan,
    BtDevice(String),
    BtConnect(String),
    BtDisconnect(String),
    BtForget(String),
    Cancel,
    Tools,
    RemoveTools,
    WeatherSearch,
    WeatherSettings,
    WeatherClear,
    WeatherRetry,
    WeatherPlace(usize),
    WeatherAdd(Place),
    WeatherRefresh(usize),
    WeatherRemove(usize),
    WeatherUnits,
    ShareReceive,
    ShareCancel,
    CoverSearch(Provider),
    UseCover(OnlineItem),
    Nothing,
    ConfirmTask(Task, String),
    RunTask(Task),
    Dismiss,
    EditSharingName,
    FinishShareName,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Year,
    Genre,
    Track,
    Disc,
}
#[derive(Clone)]
struct Item {
    options: Vec<Item>,
    cover: Option<PathBuf>,
    visual: super::RowVisual,
    title: String,
    detail: String,
    choice: Choice,
}
impl Item {
    fn new(title: impl Into<String>, choice: Choice) -> Self {
        Self {
            options: Vec::new(),
            cover: None,
            visual: super::RowVisual::default(),
            title: title.into(),
            detail: String::new(),
            choice,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PageRole {
    AlbumInfo,
    SongInfo(usize),
    SongsInfo,
    Combined,
    Normal,
    Search(Provider, bool),
    ArchiveItem,
    PodcastFeed,
    PodcastHome,
    WeatherHome,
    WeatherSettings,
    VideoHome,
    SavedVideos,
    RecentVideos,
    Sheet,
}
struct Page {
    mini: bool,
    role: PageRole,
    title: String,
    rows: Vec<Item>,
    selected: usize,
    scroll: f32,
    target: f32,
    visual_rows: Option<Arc<Vec<super::Row>>>,
    visual_status: String,
}
impl Page {
    fn new(title: impl Into<String>, rows: Vec<Item>) -> Self {
        Self {
            title: title.into(),
            rows,
            selected: 0,
            scroll: 0.0,
            target: 0.0,
            mini: false,
            role: PageRole::Normal,
            visual_rows: None,
            visual_status: String::new(),
        }
    }
}
#[derive(Clone)]
enum Purpose {
    Video(VideoJob),
    CoverContext {
        generation: u64,
    },
    Combined {
        generation: u64,
    },
    Search {
        provider: Provider,
        query: String,
        page: u32,
        metadata: bool,
        generation: u64,
    },
    Details {
        metadata: bool,
        generation: u64,
    },
    Feed {
        generation: u64,
    },
    Download,
    ArchiveStatus {
        generation: u64,
        item: String,
    },
    Downloads,
    Tags,
    Merge,
    Separate,
    CoverReset,
    Tools,
    Subscriptions,
    Remember,
    History,
    Cover,
    ReadTags {
        generation: u64,
    },
    WriteTags {
        generation: u64,
    },
    InfoPreview {
        source: String,
        generation: u64,
    },
    Preview(String),
    Stream {
        inline: bool,
        video: bool,
        origin: String,
        start: f64,
        queue: Vec<OnlineItem>,
        index: usize,
        generation: u64,
    },
}
#[derive(Clone, Copy)]
enum Typing {
    NewPlaylist,
    Combined,
    Search(Provider, bool),
    Tag(Field),
    Weather,
    Receive,
    SharingName,
}

enum PendingShare {
    Receive,
    Send {
        paths: Vec<PathBuf>,
        info: ShareInfo,
        cover: Option<PathBuf>,
    },
}
enum RetiredCustom {
    Share {
        title: String,
        view: ShareVisual,
    },
    Weather {
        title: String,
        place: Place,
        day: usize,
        fahrenheit: bool,
        loading: bool,
        error: String,
        view: weather_visual::ForecastVisual,
    },
}
struct CombinedState {
    cover_album: String,
    cover_artist: String,
    cover_paths: Arc<Vec<PathBuf>>,
    cover_context: Option<Arc<betterpod_rust::curation::CoverQuery>>,
    query: String,
    automatic_query: String,
    cover: bool,
    manual: bool,
    generation: u64,
    hits: Vec<OnlineItem>,
    pending: usize,
    errors: Vec<String>,
}
struct SearchState {
    provider: Provider,
    metadata: bool,
    query: String,
    items: Vec<OnlineItem>,
    total: usize,
    page: u32,
    error: String,
    busy: bool,
}

pub(super) struct Extras {
    videos: videos_ui::Videos,
    cover_jobs: HashMap<u64, JobHandle>,
    cover_events: VecDeque<Event>,
    pending_share: Option<PendingShare>,
    share_name_draft: String,
    retired_custom: Option<RetiredCustom>,
    library_snapshot: Arc<betterpod_rust::library::Library>,
    album_edit: Option<metadata_edit::AlbumEdit>,
    combined: Option<CombinedState>,
    share_view: Option<ShareVisual>,
    tools_state: betterpod_rust::downloads::ToolStatus,
    preview_origin: Option<String>,
    preview_depth: usize,
    preview_state: Option<betterpod_rust::video::VideoSnapshot>,
    listen: betterpod_rust::listen::ListenClient,
    listen_snapshot: betterpod_rust::listen::Snapshot,
    preview_queue: Vec<OnlineItem>,
    preview_decoder_floor: u64,
    preview_ready: bool,
    searches: Vec<SearchState>,
    subscriptions: Vec<Podcast>,
    active_feed: Option<(Podcast, Vec<Episode>)>,
    playlists: PlaylistClient,
    lists: Arc<Vec<Playlist>>,
    services: Services,
    jobs: HashMap<u64, (JobHandle, Purpose)>,
    pages: Vec<Page>,
    pending_navigation: Option<(super::Page, bool, f64)>,
    keyboard: Option<(Keyboard, Typing)>,
    closing_keyboard: Option<Keyboard>,
    visual_time: f64,
    viewport_height: f32,
    bluetooth: Bluetooth,
    bt_state: BtState,
    bt_selected: Option<String>,
    status: String,
    append_paths: Vec<PathBuf>,
    edit_paths: Vec<PathBuf>,
    edit: TagEdit,
    original: TagEdit,
    pending_metadata: Option<OnlineItem>,
    edit_title: String,
    effects: Vec<Effect>,
    generation: u64,
    weather: Weather,
    weather_state: WeatherSnapshot,
    weather_query: String,
    weather_results: Option<Vec<Place>>,
    weather_error: String,
    weather_loading: bool,
    pending_weather: Option<Place>,
    forecast_loading: bool,
    forecast_error: String,
    bt_error: String,
    bt_last_header: std::time::Instant,
    bt_last_devices: std::time::Instant,
    bt_scanning_until: Option<f64>,
    bt_scan_second: i64,
    forecast: Option<usize>,
    forecast_depth: usize,
    forecast_day: usize,
    weather_visual: weather_visual::ForecastVisual,
    share: ShareClient,
    cover_search: bool,
    cover_override: bool,
    youtube: bool,
    sharing_name: String,
    previews: HashMap<String, Option<PathBuf>>,
    active_media_art: Option<(String, String)>,
    cover_editor: Option<CoverEditor>,
    detail_item: Option<OnlineItem>,
    archive_status: HashMap<String, std::collections::BTreeMap<String, (usize, u64)>>,
    media_queue: Vec<OnlineItem>,
    media_generation: u64,
}
impl Extras {
    pub(super) fn new(music: PathBuf, data: PathBuf) -> Result<Self, String> {
        betterpod_rust::weather::warm_icons(&[168, 76, 256]);
        let mut extras = Self {
            videos: Default::default(),
            cover_jobs: HashMap::new(),
            cover_events: VecDeque::new(),
            pending_share: None,
            share_name_draft: String::new(),
            retired_custom: None,
            library_snapshot: Arc::new(betterpod_rust::library::Library::default()),
            album_edit: None,
            combined: None,
            share_view: None,
            tools_state: Default::default(),
            preview_origin: None,
            preview_depth: 0,
            preview_state: None,
            listen: betterpod_rust::listen::ListenClient::new(data.clone())?,
            listen_snapshot: Default::default(),
            preview_queue: Vec::new(),
            preview_decoder_floor: 0,
            preview_ready: false,
            searches: Vec::new(),
            subscriptions: Vec::new(),
            active_feed: None,
            playlists: PlaylistClient::new(music.clone(), data.clone()),
            lists: Arc::new(Vec::new()),
            services: Services::new_with_music(data.clone(), music)?,
            weather: Weather::new(data.clone())?,
            weather_state: WeatherSnapshot::default(),
            weather_query: String::new(),
            weather_results: None,
            weather_error: String::new(),
            weather_loading: false,
            pending_weather: None,
            forecast_loading: false,
            forecast_error: String::new(),
            bt_error: String::new(),
            bt_last_header: std::time::Instant::now(),
            bt_last_devices: std::time::Instant::now(),
            bt_scanning_until: None,
            bt_scan_second: -1,
            forecast: None,
            forecast_depth: 0,
            forecast_day: 0,
            weather_visual: weather_visual::ForecastVisual::new(),
            share: ShareClient::new(data),
            cover_search: false,
            cover_override: false,
            youtube: false,
            sharing_name: String::new(),
            previews: HashMap::new(),
            active_media_art: None,
            cover_editor: None,
            detail_item: None,
            archive_status: HashMap::new(),
            media_queue: Vec::new(),
            media_generation: 0,
            jobs: HashMap::new(),
            pages: Vec::new(),
            pending_navigation: None,
            keyboard: None,
            closing_keyboard: None,
            visual_time: 0.0,
            viewport_height: 440.0,
            bluetooth: Bluetooth::new(),
            bt_state: BtState::default(),
            bt_selected: None,
            status: String::new(),
            append_paths: Vec::new(),
            edit_paths: Vec::new(),
            edit: TagEdit::default(),
            original: TagEdit::default(),
            pending_metadata: None,
            edit_title: String::new(),
            effects: Vec::new(),
            generation: 0,
        };
        extras.playlist_command(playlists::Command::List);
        extras.task(Task::ToolsStatus, Purpose::Tools);
        extras.bluetooth.request(BtRequest::Header);
        extras.refresh_video_catalog();
        Ok(extras)
    }
    pub(in crate::app) fn start_missing_covers(
        &mut self,
        albums: Vec<betterpod_rust::curation::CoverQuery>,
    ) -> Result<JobHandle, String> {
        if self.cover_jobs.values().any(|job| !job.is_cancelled()) {
            return Err("A cover search is already running".into());
        }
        let handle = self.services.submit(Task::FindMissingCovers { albums })?;
        self.cover_jobs.insert(handle.id, handle.clone());
        Ok(handle)
    }
    pub(in crate::app) fn poll_cover_search(&mut self) -> Option<Event> {
        self.cover_events.pop_front()
    }
    pub(super) fn title(&self) -> &str {
        self.pages.last().map_or("Music", |p| p.title.as_str())
    }
    pub(super) fn resolve_media(
        &mut self,
        item: OnlineItem,
        queue: Vec<OnlineItem>,
        index: usize,
        video: bool,
        start: f64,
    ) {
        self.resolve_media_mode(item, queue, index, video, start, false);
    }
    fn resolve_media_mode(
        &mut self,
        item: OnlineItem,
        queue: Vec<OnlineItem>,
        index: usize,
        video: bool,
        start: f64,
        inline: bool,
    ) {
        if inline {
            self.start_inline_listen(item, queue, index);
            return;
        }
        if video {
            self.videos.current = Some(item.clone());
            if self.known_local_video(&item).is_some() {
                self.play_video_item(item, queue, index, start);
                return;
            }
        }
        self.media_generation = self.media_generation.wrapping_add(1);
        let generation = self.media_generation;
        for (handle, purpose) in self.jobs.values() {
            if matches!(purpose, Purpose::Stream { .. }) {
                handle.cancel();
            }
        }
        let origin = item.url.clone();
        if !inline && video {
            self.effects.push(Effect::VideoOpening {
                item: item.clone(),
                queue: queue.clone(),
                index,
                start,
            });
        }
        self.active_media_art = Some((origin.clone(), item.cover.clone()));
        self.request_preview(item.cover.clone());
        if origin.contains("youtube.com/") || origin.contains("youtu.be/") {
            self.task(
                Task::ResolveStream { item, video },
                Purpose::Stream {
                    inline,
                    video,
                    origin,
                    start,
                    queue,
                    index,
                    generation,
                },
            );
        } else {
            let source = StreamSource {
                video_url: item.url.clone(),
                audio_url: None,
                headers: Vec::new(),
                duration: item.seconds,
                title: item.title,
                available_at: None,
                chapters: 0,
            };
            self.effects.push(if inline {
                Effect::Preview {
                    source,
                    origin,
                    queue,
                    index,
                }
            } else {
                Effect::Stream {
                    source,
                    video,
                    origin,
                    start,
                    queue,
                    index,
                }
            });
            self.emit_media_art();
        }
    }
    fn request_preview(&mut self, url: String) {
        if url.is_empty()
            || self.previews.contains_key(&url)
            || self
                .jobs
                .values()
                .filter(|(_, p)| matches!(p, Purpose::Preview(_)))
                .count()
                >= 2
        {
            return;
        }
        if let Ok(handle) = self
            .services
            .submit(Task::CacheArtwork { url: url.clone() })
        {
            self.previews.insert(url.clone(), None);
            self.jobs.insert(handle.id, (handle, Purpose::Preview(url)));
        }
    }
    fn emit_media_art(&mut self) {
        if let Some((origin, url)) = &self.active_media_art {
            if let Some(Some(path)) = self.previews.get(url) {
                self.effects.push(Effect::MediaArtwork {
                    origin: origin.clone(),
                    path: path.clone(),
                });
            }
        }
    }
    pub(super) fn set_youtube(&mut self, enabled: bool) {
        self.youtube = enabled;
    }
    pub(super) fn curation_task(&mut self, task: Task) {
        if matches!(&task, Task::MergeAlbums { .. }) {
            self.push("Album Changes", vec![Item::new("Cancel", Choice::Cancel)]);
            self.task(task, Purpose::Merge);
        } else {
            self.pages.clear();
            self.push("Album Changes", vec![Item::new("Cancel", Choice::Cancel)]);
            self.task(task, Purpose::Tags);
        }
    }
    pub(super) fn open_cover_search(
        &mut self,
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
        provider: Provider,
    ) {
        self.edit_paths = paths;
        self.edit_title = album.clone();
        self.cover_search = true;
        self.cover_override = true;
        self.pages.clear();
        self.push("Choose Album Cover", Vec::new());
        self.search(provider, format!("{artist} {album}"), 1, true);
    }
    pub(super) fn set_sharing_name(&mut self, name: String) {
        self.sharing_name = name;
    }
    pub(super) fn playlists_snapshot(&self) -> Arc<Vec<Playlist>> {
        Arc::clone(&self.lists)
    }
    pub(super) fn open_playlist(&mut self, playlist: Playlist) {
        self.pages.clear();
        self.push(&playlist.name, self.playlist_rows(&playlist));
        let page = self.pages.last_mut().unwrap();
        page.mini = true;
        page.selected = 1;
    }
    pub(super) fn cancel_typing(&mut self) {
        if let Some((mut keyboard, typing)) = self.keyboard.take() {
            let query = keyboard.query.clone();
            if matches!(typing, Typing::SharingName) && self.pending_share.is_some() {
                self.share_name_draft = query.clone();
            }
            keyboard.hide(self.visual_time);
            self.closing_keyboard = Some(keyboard);
            match typing {
                Typing::Search(provider, metadata) => {
                    if let Some(state) = self
                        .searches
                        .iter_mut()
                        .find(|state| state.provider == provider && state.metadata == metadata)
                    {
                        state.query = query;
                    }
                    let rows = self.search_rows(provider, metadata);
                    if let Some(page) = self.pages.last_mut() {
                        page.rows = rows;
                    }
                }
                Typing::Combined => {
                    if let Some(state) = self.combined.as_mut() {
                        state.query = query;
                    }
                    self.refresh_combined();
                }
                Typing::Weather => {
                    self.weather_query = query;
                    let rows = self.weather_rows();
                    if let Some(page) = self.pages.last_mut() {
                        page.rows = rows;
                    }
                }
                Typing::Tag(_) => self.refresh_editor_page(),
                _ => {}
            }
        }
        self.refresh_share_name_gate();
        self.invalidate_visual_rows();
    }
    pub(in crate::app) fn set_viewport_height(&mut self, height: f32) {
        let height = height.clamp(1.0, 440.0);
        if (height - self.viewport_height).abs() < 0.01 {
            return;
        }
        self.viewport_height = height;
        if let Some(page) = self.pages.last_mut() {
            if page.role != PageRole::Sheet {
                Self::reveal(page, height);
            }
        }
    }
    pub(super) fn is_typing(&self) -> bool {
        self.keyboard.is_some()
    }
    pub(super) fn cancel_media(&mut self) {
        self.active_media_art = None;
        self.media_generation = self.media_generation.wrapping_add(1);
        for (handle, purpose) in self.jobs.values() {
            if matches!(purpose, Purpose::Stream { .. }) {
                handle.cancel();
            }
        }
    }
    pub(super) fn download(&mut self, request: DownloadRequest) {
        self.start_download(request);
    }
    fn start_download(&mut self, request: DownloadRequest) {
        self.stop_preview();
        if let DownloadRequest::YouTube {
            url, video: false, ..
        } = &request
        {
            let cover = self
                .detail_item
                .as_ref()
                .filter(|item| item.url == *url)
                .map(|item| item.cover.clone())
                .or_else(|| {
                    self.active_media_art
                        .as_ref()
                        .filter(|(origin, _)| origin == url)
                        .map(|(_, cover)| cover.clone())
                })
                .unwrap_or_default();
            if !cover.is_empty() {
                self.request_preview(cover.clone());
                let path = self.previews.get(&cover).and_then(Clone::clone);
                self.cover_editor = Some(CoverEditor::new(request, cover, path));
                return;
            }
        }
        self.task(Task::Download(request), Purpose::Download);
    }
    pub(super) fn remember_video(
        &mut self,
        url: String,
        title: String,
        position: f64,
        duration: f64,
    ) {
        self.remember_video_info(url, title, position, duration);
    }
    pub(super) fn send_share(
        &mut self,
        paths: Vec<PathBuf>,
        info: ShareInfo,
        cover: Option<PathBuf>,
    ) {
        if self.sharing_name.trim().is_empty() {
            self.open_share_name_gate(PendingShare::Send { paths, info, cover });
            return;
        }
        self.open(ExtraPage::Receive);
        self.share_view = Some(ShareVisual::send(
            info.clone(),
            cover.clone(),
            self.visual_time,
        ));
        self.replace("Share", Vec::new());
        if let Err(error) = self
            .share
            .submit(share::Command::Send { paths, info, cover })
        {
            self.effects.push(Effect::Notice(error));
        }
    }
    fn push(&mut self, title: impl Into<String>, rows: Vec<Item>) {
        if let Some(index) = self.pages.len().checked_sub(1) {
            if self.pages[index].role != PageRole::Sheet {
                if let Some(previous) = self.project_page(index, true) {
                    self.pending_navigation = Some((previous, true, self.visual_time));
                }
            }
        }
        self.pages.push(Page::new(title, rows));
    }
    fn replace(&mut self, title: impl Into<String>, rows: Vec<Item>) {
        let title = title.into();
        let mut page = Page::new(title, rows);
        if let Some(old) = self.pages.pop() {
            if old.title == page.title {
                page.selected = old.selected.min(page.rows.len().saturating_sub(1));
                page.scroll = old.scroll;
                page.target = old.target;
                page.mini = old.mini;
            }
        }
        self.pages.push(page);
    }
    fn task(&mut self, task: Task, purpose: Purpose) {
        let cover_job = matches!(purpose, Purpose::CoverContext { .. })
            || matches!(purpose, Purpose::Combined { .. })
                && self.combined.as_ref().is_some_and(|state| state.cover);
        match self.services.submit(task) {
            Ok(handle) => {
                self.jobs.insert(handle.id, (handle, purpose));
                if !cover_job {
                    self.status = "Working...".into();
                }
            }
            Err(error) => self.effects.push(Effect::Notice(error)),
        }
    }
    fn playlist_command(&mut self, command: playlists::Command) {
        if let Err(error) = self.playlists.submit(command) {
            self.effects.push(Effect::Notice(error));
        }
    }
    pub(super) fn open(&mut self, page: ExtraPage) {
        self.pending_share = None;
        self.videos.watch_depth = None;
        self.retain_current_visual();
        self.generation = self.generation.wrapping_add(1);
        self.pending_navigation = None;
        if self.share_view.take().is_some() {
            self.share.cancel();
        }
        self.pages.clear();
        self.cover_editor = None;
        self.forecast = None;
        self.keyboard = None;
        self.status.clear();
        self.push_page(page);
    }
    pub(super) fn add_to_playlist(&mut self, paths: Vec<PathBuf>) {
        self.append_paths = paths;
        self.open(ExtraPage::AddToPlaylist);
    }
    pub(super) fn edit_album(&mut self, paths: Vec<PathBuf>, title: String, edit: TagEdit) {
        self.cover_override = false;
        self.edit_paths = paths;
        self.edit_title = title;
        self.original = edit;
        self.pending_metadata = None;
        self.edit = TagEdit::default();
        self.album_edit = Some(metadata_edit::AlbumEdit::loading());
        self.open(ExtraPage::AlbumInfo);
        self.pages.last_mut().unwrap().role = PageRole::AlbumInfo;
        self.task(
            Task::ReadAlbumTags {
                paths: self.edit_paths.clone(),
            },
            Purpose::ReadTags {
                generation: self.generation,
            },
        );
    }
    fn push_page(&mut self, page: ExtraPage) {
        match page {
            ExtraPage::Playlists | ExtraPage::AddToPlaylist => {
                self.push(
                    if page == ExtraPage::Playlists {
                        "Playlists"
                    } else {
                        "Add to Playlist"
                    },
                    vec![Item::new("New Playlist…", Choice::NewPlaylist)],
                );
                self.playlist_command(playlists::Command::List);
                self.pages.last_mut().unwrap().mini = page == ExtraPage::Playlists;
            }
            ExtraPage::SearchSources => {
                self.push(
                    "Search Online",
                    [
                        Provider::Archive,
                        Provider::YouTube,
                        Provider::YouTubePlaylists,
                        Provider::Podcast,
                    ]
                    .into_iter()
                    .filter(|p| {
                        self.youtube || !matches!(p, Provider::YouTube | Provider::YouTubePlaylists)
                    })
                    .map(|p| Item::new(p.name(), Choice::Page(ExtraPage::Online(p))))
                    .collect(),
                );
            }
            ExtraPage::Online(provider) => {
                self.push(provider.name(), vec![visual::search_row(provider, "", "")]);
                self.pages.last_mut().unwrap().role = PageRole::Search(provider, false);
                self.keyboard = Some((
                    Keyboard::new(String::new()),
                    Typing::Search(provider, false),
                ));
            }
            ExtraPage::Podcasts => {
                let rows = self.podcast_home_rows();
                self.push("Podcasts", rows);
                self.pages.last_mut().unwrap().role = PageRole::PodcastHome;
                self.task(Task::Subscriptions, Purpose::Subscriptions);
            }
            ExtraPage::Downloads => {
                self.push("Downloaded", Vec::new());
                self.task(Task::Downloaded, Purpose::Downloads);
            }
            ExtraPage::Tools => {
                let rows = self.tools_rows();
                self.push("YouTube", rows);
                self.task(Task::ToolsStatus, Purpose::Tools);
            }
            ExtraPage::Bluetooth => {
                self.push("Bluetooth", Vec::new());
                self.bluetooth.request(BtRequest::Refresh);
                self.status = "Reading Bluetooth devices...".into();
            }
            ExtraPage::AlbumInfo => self.push("Album Info", self.tag_rows()),
            ExtraPage::Videos | ExtraPage::SavedVideos | ExtraPage::VideoHistory => {
                self.open_videos(page)
            }
            ExtraPage::Weather => {
                self.push("Weather", self.weather_rows());
                self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
                if self.weather_state.places.is_empty() {
                    self.keyboard =
                        Some((Keyboard::new(self.weather_query.clone()), Typing::Weather));
                } else {
                    self.pages.last_mut().unwrap().selected = 1;
                }
            }
            ExtraPage::Sharing => {
                let mut name = Item::new("Device Name", Choice::EditSharingName);
                name.visual.value = self.sharing_name.clone();
                name.detail = "Your friends see it when you share with them".into();
                name.visual.no_arrow = true;
                let mut how = visual::fact("How it works", "");
                how.detail = "[Y] on an album or a song → Share; Music → Receive".into();
                self.push("Sharing", vec![name, how]);
            }
            ExtraPage::Receive => {
                if self.sharing_name.trim().is_empty() {
                    self.open_share_name_gate(PendingShare::Receive);
                } else {
                    self.push("Receive", Vec::new());
                    self.share_view = Some(ShareVisual::receive(self.visual_time));
                }
            }
        }
    }
    fn tag_rows(&self) -> Vec<Item> {
        if self.album_edit.is_some() {
            return self.album_info_rows();
        }
        let value = |pending: &Option<String>, original: &Option<String>| {
            pending
                .as_ref()
                .or(original.as_ref())
                .cloned()
                .unwrap_or_default()
        };
        let mut rows = vec![
            Item::new(
                format!("Album: {}", value(&self.edit.album, &self.original.album)),
                Choice::TagField(Field::Album),
            ),
            Item::new(
                format!(
                    "Album Artist: {}",
                    value(&self.edit.album_artist, &self.original.album_artist)
                ),
                Choice::TagField(Field::AlbumArtist),
            ),
            Item::new(
                format!(
                    "Artist: {}",
                    value(&self.edit.artist, &self.original.artist)
                ),
                Choice::TagField(Field::Artist),
            ),
            Item::new(
                format!(
                    "Year: {}",
                    self.edit
                        .year
                        .or(self.original.year)
                        .map_or(String::new(), |y| y.to_string())
                ),
                Choice::TagField(Field::Year),
            ),
            Item::new(
                format!("Genre: {}", value(&self.edit.genre, &self.original.genre)),
                Choice::TagField(Field::Genre),
            ),
            Item::new(
                format!("Title: {}", value(&self.edit.title, &self.original.title)),
                Choice::TagField(Field::Title),
            ),
            Item::new(
                format!(
                    "Track: {}",
                    self.edit.track.map_or(String::new(), |n| n.to_string())
                ),
                Choice::TagField(Field::Track),
            ),
            Item::new(
                format!(
                    "Disc: {}",
                    self.edit.disc.map_or(String::new(), |n| n.to_string())
                ),
                Choice::TagField(Field::Disc),
            ),
            Item::new(
                "Search MusicBrainz",
                Choice::MetadataSearch(Provider::MusicBrainz),
            ),
            Item::new("Search iTunes", Choice::MetadataSearch(Provider::Itunes)),
            Item::new("Search Deezer", Choice::MetadataSearch(Provider::Deezer)),
            Item::new(
                "Album on Wikipedia",
                Choice::MetadataSearch(Provider::Wikipedia),
            ),
            Item::new(
                "Choose Cover from iTunes",
                Choice::CoverSearch(Provider::Itunes),
            ),
            Item::new(
                "Choose Cover from Deezer",
                Choice::CoverSearch(Provider::Deezer),
            ),
            Item::new("Save Changes", Choice::SaveTags),
        ];
        if self.edit_paths.len() != 1 {
            rows.retain(|row| {
                !matches!(
                    row.choice,
                    Choice::TagField(Field::Title | Field::Track | Field::Disc)
                )
            });
        }
        rows
    }

    fn show_playlists(&mut self) {
        let adding = self
            .pages
            .first()
            .is_some_and(|p| p.title == "Add to Playlist");
        let mut rows = vec![Item::new("New Playlist…", Choice::NewPlaylist)];
        for playlist in self.lists.iter() {
            if adding && playlist.read_only {
                continue;
            }
            let mut row = Item::new(
                &playlist.name,
                if adding {
                    Choice::AppendPlaylist(playlist.clone())
                } else {
                    Choice::Playlist(playlist.clone())
                },
            );
            row.detail = format!(
                "{} songs{}",
                playlist.tracks.len(),
                if playlist.read_only {
                    " - imported"
                } else {
                    ""
                }
            );
            rows.push(row);
        }
        self.replace(
            if adding {
                "Add to Playlist"
            } else {
                "Playlists"
            },
            rows,
        );
    }
    fn playlist_rows(&self, playlist: &Playlist) -> Vec<Item> {
        let playlist = Arc::new(playlist.clone());
        let mut rows = vec![visual::header(&playlist.name)];
        for (index, path) in playlist.tracks.iter().enumerate() {
            let track = self
                .library_snapshot
                .tracks
                .iter()
                .find(|track| track.path == *path);
            let mut row = Item::new(
                track.map(|track| track.title.clone()).unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                }),
                Choice::PlaylistSong(Arc::clone(&playlist), index),
            );
            if let Some(track) = track {
                row.detail = track.artist.clone();
                row.cover = track.cover.clone();
                row.visual.value = super::time_label(track.duration);
            }
            rows.push(row);
        }
        if rows.len() == 1 {
            rows.push(visual::fact("No songs", ""));
        }
        rows
    }
    pub(super) fn handle(&mut self, action: Action) {
        if action == Action::Back {
            self.retain_current_visual();
        }
        if let Some(view) = self.share_view.as_mut() {
            match view.handle(action, self.visual_time) {
                Some(ShareAction::Receive(code)) => {
                    if let Err(error) = self.share.submit(share::Command::Receive {
                        code,
                        name: self.sharing_name.clone(),
                    }) {
                        if let Some(view) = self.share_view.as_mut() {
                            view.event(&share::Event::Error(error), self.visual_time);
                        }
                    }
                }
                Some(ShareAction::Exit) => {
                    self.share.cancel();
                    self.share_view = None;
                    self.effects.push(Effect::Exit);
                }
                Some(ShareAction::Play(paths)) => {
                    self.effects.push(Effect::Play { paths, index: 0 })
                }
                None => {}
            }
            return;
        }

        if let Some(editor) = self.cover_editor.as_mut() {
            if let Some(effect) = editor.handle(action) {
                self.cover_editor = None;
                match effect {
                    CoverEffect::Cancel => {}
                    CoverEffect::Default(request) => {
                        self.task(Task::Download(request), Purpose::Download)
                    }
                    CoverEffect::Crop {
                        request,
                        path,
                        crop,
                    } => self.task(
                        Task::DownloadWithCover {
                            request,
                            cover: path,
                            crop,
                        },
                        Purpose::Download,
                    ),
                }
            }
            return;
        }
        if self.keyboard.is_some() {
            self.type_key(action);
            return;
        }
        if let Some(index) = self
            .forecast
            .filter(|_| self.pages.len() == self.forecast_depth)
        {
            match action {
                Action::Left | Action::Up => {
                    self.forecast_day = self.forecast_day.saturating_sub(1);
                    return;
                }
                Action::Right | Action::Down => {
                    let days = self
                        .weather_state
                        .places
                        .get(index)
                        .and_then(|p| p.forecast.as_ref())
                        .map_or(1, |f| f.days());
                    self.forecast_day = (self.forecast_day + 1).min(days.saturating_sub(1));
                    return;
                }
                Action::Select => {
                    self.weather.command(WeatherCommand::Refresh(index));
                    self.status = "Refreshing forecast...".into();
                    return;
                }
                Action::Options => return,
                Action::Back => self.forecast = None,
                _ => {}
            }
        }
        if action == Action::Back {
            if self.preview_origin.is_some() && self.pages.len() <= self.preview_depth {
                self.stop_preview();
            }
            if self.pages.len() > 1 {
                let index = self.pages.len() - 1;
                if self.pages[index].role != PageRole::Sheet {
                    if let Some(previous) = self.project_page(index, true) {
                        self.pending_navigation = Some((previous, false, self.visual_time));
                    }
                }
                self.pages.pop();
                self.generation = self.generation.wrapping_add(1);
                if let Some(edit) = self.album_edit.as_mut() {
                    edit.scope = self.pages.last().and_then(|page| {
                        if let PageRole::SongInfo(index) = page.role {
                            Some(index)
                        } else {
                            None
                        }
                    });
                }
                self.refresh_editor_page();
            } else {
                self.effects.push(Effect::Exit);
            }
            return;
        }
        if action == Action::Options {
            if let Some(page) = self.pages.last() {
                if let Some(row) = page.rows.get(page.selected) {
                    if !row.options.is_empty() {
                        let title = row.title.clone();
                        let options = row.options.clone();
                        self.push_sheet(title, options);
                        return;
                    }
                    match &row.choice {
                        Choice::CombinedSearch => {
                            self.push_sheet(
                                "",
                                vec![
                                    Item::new("Clear search", Choice::CombinedClear),
                                    Item::new(
                                        "Search automatically again",
                                        Choice::CombinedAutomatic,
                                    ),
                                ],
                            );
                            return;
                        }
                        Choice::WeatherSearch => {
                            self.push_sheet(
                                "",
                                vec![
                                    Item::new("Clear search", Choice::WeatherClear),
                                    Item::new("Weather Settings", Choice::WeatherSettings),
                                ],
                            );
                            return;
                        }
                        Choice::WeatherPlace(index) => {
                            if let Some(place) = self.weather_state.places.get(*index) {
                                self.push_sheet(
                                    "",
                                    vec![Item::new(
                                        format!("Remove {}", place.name),
                                        Choice::WeatherRemove(*index),
                                    )],
                                );
                                return;
                            }
                        }
                        Choice::BtDevice(address) => {
                            if let Some(device) =
                                self.bt_state.devices.iter().find(|d| d.address == *address)
                            {
                                let rows = vec![
                                    Item::new(
                                        if device.connected {
                                            "Disconnect"
                                        } else {
                                            "Connect"
                                        },
                                        if device.connected {
                                            Choice::BtDisconnect(address.clone())
                                        } else {
                                            Choice::BtConnect(address.clone())
                                        },
                                    ),
                                    Item::new(
                                        "Forget This Device",
                                        Choice::BtForget(address.clone()),
                                    ),
                                ];
                                self.push_sheet("", rows);
                                return;
                            }
                        }
                        Choice::Search(provider) => {
                            self.push_sheet(
                                "",
                                vec![Item::new("Clear search", Choice::ClearSearch(*provider))],
                            );
                            return;
                        }
                        Choice::Preview(entry, _, _) => {
                            if let Some(item) = self
                                .detail_item
                                .as_ref()
                                .filter(|item| item.provider == Provider::Archive)
                            {
                                if let Some(groups) = item.raw["groups"].as_object() {
                                    let mut selected = None;
                                    for (format, files) in groups {
                                        for file in
                                            files.as_array().map(Vec::as_slice).unwrap_or(&[])
                                        {
                                            if file["download_url"].as_str() == Some(&entry.url) {
                                                selected = Some((format.clone(), file.clone()));
                                            }
                                        }
                                    }
                                    if let Some((format, file)) = selected {
                                        self.push_sheet(
                                            "",
                                            vec![Item::new(
                                                "Download This Track",
                                                Choice::ArchiveTrack(
                                                    item.clone(),
                                                    format,
                                                    file,
                                                    Vec::new(),
                                                    0,
                                                ),
                                            )],
                                        );
                                        return;
                                    }
                                }
                            }
                        }
                        Choice::Feed(podcast) => {
                            let mut rows = Vec::new();
                            if !podcast.files.is_empty() {
                                rows.push(Item::new(
                                    "Delete Downloaded Episodes",
                                    Choice::ConfirmTask(
                                        Task::DeletePodcast {
                                            podcast: podcast.clone(),
                                        },
                                        "Delete Downloaded Episodes?".into(),
                                    ),
                                ));
                            }
                            rows.push(Item::new(
                                "Remove from Your Podcasts",
                                Choice::Unsubscribe(podcast.clone()),
                            ));
                            self.push_sheet("", rows);
                            return;
                        }
                        Choice::Episode(episode, podcast)
                            if podcast.files.contains_key(episode.key()) =>
                        {
                            self.push_sheet(
                                "Episode Options",
                                vec![Item::new(
                                    "Delete Download",
                                    Choice::ConfirmTask(
                                        Task::DeleteEpisodes {
                                            podcast: podcast.clone(),
                                            episodes: vec![episode.clone()],
                                        },
                                        "Delete this downloaded episode?".into(),
                                    ),
                                )],
                            );
                            return;
                        }
                        Choice::Playlist(playlist) => {
                            let mut rows = visual::media_options(playlist.tracks.clone());
                            if !playlist.read_only {
                                rows.push(Item::new(
                                    "Delete Playlist",
                                    Choice::DeletePlaylist(playlist.clone()),
                                ));
                            }
                            self.push_sheet("", rows);
                            return;
                        }
                        Choice::PlaylistSong(playlist, index) => {
                            let paths = playlist.tracks.get(*index).cloned().into_iter().collect();
                            let mut rows = visual::media_options(paths);
                            if !playlist.read_only {
                                rows.push(Item::new(
                                    "Remove from Playlist",
                                    Choice::RemovePlaylist(Arc::clone(playlist), *index),
                                ));
                            }
                            self.push_sheet("", rows);
                            return;
                        }
                        _ => {}
                    }
                }
            }
            if let Some((&id, _)) = self.jobs.iter().next() {
                if let Some((handle, _)) = self.jobs.get(&id) {
                    handle.cancel();
                    self.status = "Cancelling...".into();
                }
            }
            return;
        }
        match action {
            Action::Up => {
                if let Some(page) = self.pages.last_mut() {
                    page.selected = page.selected.saturating_sub(1);
                    while page.selected > 0 && page.rows[page.selected].visual.header {
                        page.selected -= 1;
                    }
                    if page
                        .rows
                        .get(page.selected)
                        .is_some_and(|r| r.visual.header)
                    {
                        page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1));
                    }
                    Self::reveal(page, self.viewport_height);
                }
            }
            Action::Down => {
                if let Some(page) = self.pages.last_mut() {
                    page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1));
                    while page.selected + 1 < page.rows.len()
                        && page.rows[page.selected].visual.header
                    {
                        page.selected += 1;
                    }
                    Self::reveal(page, self.viewport_height);
                }
            }
            Action::Select => {
                let choice = self
                    .pages
                    .last()
                    .and_then(|p| p.rows.get(p.selected))
                    .map(|r| r.choice.clone());
                if let Some(choice) = choice {
                    let sheet = self
                        .pages
                        .last()
                        .is_some_and(|page| page.role == PageRole::Sheet);
                    if sheet {
                        self.pages.pop();
                        if matches!(choice, Choice::Dismiss) {
                            return;
                        }
                    }
                    self.choose(choice);
                }
            }
            _ => {}
        }
    }
    fn reveal(page: &mut Page, viewport: f32) {
        let top = page
            .rows
            .iter()
            .take(page.selected)
            .map(visual::item_height)
            .sum::<f32>();
        let height = page
            .rows
            .get(page.selected)
            .map_or(50.0, visual::item_height);
        if top < page.target {
            page.target = top;
        }
        if top + height > page.target + viewport {
            page.target = top + height - viewport;
        }
        let total = page.rows.iter().map(visual::item_height).sum::<f32>();
        page.target = page.target.clamp(0.0, (total - viewport).max(0.0));
        page.scroll = page.target;
    }
    fn search(&mut self, provider: Provider, query: String, page: u32, metadata: bool) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        for (handle, purpose) in self.jobs.values() {
            if matches!(purpose, Purpose::Search { .. }) {
                handle.cancel();
            }
        }
        if let Some(state) = self
            .searches
            .iter_mut()
            .find(|s| s.provider == provider && s.metadata == metadata)
        {
            if page == 1 || state.query != query {
                state.items.clear();
                state.total = 0;
            }
            state.query = query.clone();
            state.page = page;
            state.error.clear();
            state.busy = true;
        } else {
            self.searches.push(SearchState {
                provider,
                metadata,
                query: query.clone(),
                items: Vec::new(),
                total: 0,
                page,
                error: String::new(),
                busy: true,
            });
        }
        let rows = self.search_rows(provider, metadata);
        if self.pages.last().is_some_and(|p| {
            p.role == PageRole::Search(provider, metadata)
                || provider == Provider::Podcast && p.role == PageRole::PodcastHome
        }) {
            self.replace(provider.name(), rows);
        } else {
            self.push(provider.name(), rows);
        }
        self.pages.last_mut().unwrap().role = PageRole::Search(provider, metadata);
        self.task(
            Task::Search {
                provider,
                query: query.clone(),
                page,
            },
            Purpose::Search {
                provider,
                query,
                page,
                metadata,
                generation,
            },
        );
    }
    fn choose(&mut self, choice: Choice) {
        match choice {
            Choice::Video(action) => self.video_action(action),
            Choice::Dismiss => {
                self.pages.pop();
            }
            Choice::ConfirmTask(task, title) => self.push_confirmation(
                title,
                "The other songs stay in your library",
                "Delete Downloaded Files",
                Choice::RunTask(task),
            ),
            Choice::RunTask(task) => {
                while self
                    .pages
                    .last()
                    .is_some_and(|page| page.role == PageRole::Sheet)
                {
                    self.pages.pop();
                }
                self.task(task, Purpose::Tags);
            }
            Choice::FinishShareName => {
                let query = self.share_name_draft.clone();
                self.submit_typing(Typing::SharingName, query);
            }
            Choice::EditSharingName => {
                self.keyboard = Some((
                    Keyboard::new(if self.pending_share.is_some() {
                        self.share_name_draft.clone()
                    } else {
                        self.sharing_name.clone()
                    }),
                    Typing::SharingName,
                ));
            }
            Choice::Page(page) => self.push_page(page),
            Choice::CombinedSearch => {
                if let Some(state) = &self.combined {
                    self.keyboard = Some((Keyboard::new(state.query.clone()), Typing::Combined));
                }
            }
            Choice::CombinedClear => {
                if let Some(state) = self.combined.as_mut() {
                    state.query.clear();
                }
                self.keyboard = Some((Keyboard::new(String::new()), Typing::Combined));
                self.refresh_combined();
            }
            Choice::CombinedAutomatic => self.run_combined(None),
            Choice::MediaQueue(paths, next) => {
                while self
                    .pages
                    .last()
                    .is_some_and(|page| page.role == PageRole::Sheet)
                {
                    self.pages.pop();
                }
                self.effects.push(Effect::Queue { paths, next });
            }
            Choice::MediaAddPlaylist(paths) => {
                while self
                    .pages
                    .last()
                    .is_some_and(|page| page.role == PageRole::Sheet)
                {
                    self.pages.pop();
                }
                self.append_paths = paths;
                self.push_page(ExtraPage::AddToPlaylist);
            }
            Choice::MediaShare(paths) => self.effects.push(Effect::SharePaths(paths)),
            Choice::NewPlaylist => {
                self.keyboard = Some((Keyboard::new(String::new()), Typing::NewPlaylist))
            }
            Choice::Playlist(playlist) => {
                self.push(&playlist.name, self.playlist_rows(&playlist));
                let page = self.pages.last_mut().unwrap();
                page.mini = true;
                page.selected = 1;
            }
            Choice::PlaylistSong(playlist, index) => self.effects.push(Effect::Play {
                paths: playlist.tracks.clone(),
                index,
            }),
            Choice::AppendPlaylist(playlist) => {
                self.playlist_command(playlists::Command::Append {
                    path: playlist.path,
                    tracks: self.append_paths.clone(),
                });
            }
            Choice::DeletePlaylist(playlist) => self.push_confirmation(
                format!("Delete “{}”?", playlist.name),
                "The songs stay in your library",
                "Delete Playlist",
                Choice::ConfirmDeletePlaylist(playlist),
            ),
            Choice::RemovePlaylist(playlist, index) => {
                self.playlist_command(playlists::Command::Remove {
                    path: playlist.path.clone(),
                    index,
                })
            }
            Choice::Search(provider) => {
                let metadata = self
                    .pages
                    .last()
                    .is_some_and(|p| p.role == PageRole::Search(provider, true));
                let query = self
                    .searches
                    .iter()
                    .find(|s| s.provider == provider && s.metadata == metadata)
                    .map_or(String::new(), |s| s.query.clone());
                self.keyboard = Some((Keyboard::new(query), Typing::Search(provider, metadata)));
            }
            Choice::ClearSearch(provider) => {
                self.searches.retain(|s| s.provider != provider);
                let rows = if provider == Provider::Podcast {
                    self.podcast_home_rows()
                } else {
                    vec![visual::search_row(provider, "", "")]
                };
                self.replace(provider.name(), rows);
                self.pages.last_mut().unwrap().role = PageRole::Search(provider, false);
                self.keyboard = Some((
                    Keyboard::new(String::new()),
                    Typing::Search(provider, false),
                ));
            }
            Choice::Preview(item, queue, index) => {
                if self.preview_origin.as_ref() == Some(&item.url) {
                    match listen_ui::row_tap(self.preview_ready, self.preview_state.as_ref()) {
                        listen_ui::RowTap::Retry => self.start_inline_listen(item, queue, index),
                        listen_ui::RowTap::Toggle => self.effects.push(Effect::TogglePreview),
                        listen_ui::RowTap::Ignore => {}
                    }
                } else {
                    self.start_inline_listen(item, queue, index);
                }
            }
            Choice::ArchiveTrack(item, format, file, _, _) => {
                self.stop_preview();
                let mut one = item;
                one.raw["groups"][&format] = serde_json::json!([file]);
                self.start_download(DownloadRequest::Archive { item: one, format });
            }
            Choice::ConfirmDeletePlaylist(playlist) => {
                while self
                    .pages
                    .last()
                    .is_some_and(|page| page.role == PageRole::Sheet)
                {
                    self.pages.pop();
                }
                self.playlist_command(playlists::Command::Delete {
                    path: playlist.path,
                });
            }
            Choice::ConfirmBtForget(address) => {
                self.bluetooth.request(BtRequest::Forget(address));
                self.status = "Removing pairing…".into();
                self.pages.pop();
            }
            Choice::ConfirmAction(choice, _) => self.choose(*choice),
            Choice::ToggleYouTube => {
                self.youtube = !self.youtube;
                self.effects.push(Effect::YouTubeEnabled(self.youtube));
                if self.youtube && !self.tools_state.ready {
                    self.task(Task::Tools, Purpose::Tools);
                }
                let rows = self.tools_rows();
                self.replace("YouTube", rows);
            }
            Choice::YouTubeOff => {
                self.youtube = false;
                self.effects.push(Effect::YouTubeEnabled(false));
            }
            Choice::NextSearch(provider, query, page, metadata) => {
                self.search(provider, query, page, metadata)
            }
            Choice::Online(item, metadata) => {
                if item.provider == Provider::Podcast {
                    match serde_json::from_value::<Podcast>(item.raw.clone()) {
                        Ok(podcast) => self.choose(Choice::Feed(podcast)),
                        Err(error) => self.effects.push(Effect::Notice(error.to_string())),
                    }
                } else {
                    if item.provider == Provider::Archive && !metadata {
                        self.push(
                            "Internet Archive",
                            vec![
                                visual::header(&item.title),
                                visual::fact("License", visual::license_label(&item.license)),
                                visual::fact("Loading…", ""),
                            ],
                        );
                        self.pages.last_mut().unwrap().role = PageRole::ArchiveItem;
                        self.pages.last_mut().unwrap().selected = 1;
                    }
                    self.task(
                        Task::Details(item),
                        Purpose::Details {
                            metadata,
                            generation: self.generation,
                        },
                    );
                }
            }
            Choice::ResumeVideo(item, start) => {
                self.play_video_item(item.clone(), vec![item], 0, start)
            }
            Choice::Recent(item) => {
                let start = item.raw["position"].as_f64().unwrap_or(0.);
                self.play_video_item(item.clone(), vec![item], 0, start);
            }
            Choice::Download(request) => {
                self.stop_preview();
                self.start_download(request);
            }
            Choice::Media(url, title, video) => {
                let mut item = OnlineItem::new(
                    if url.contains("youtube.com/") || url.contains("youtu.be/") {
                        Provider::YouTube
                    } else {
                        Provider::Archive
                    },
                );
                item.url = url.clone();
                item.title = title;
                item.kind = if video { "video" } else { "audio" }.into();
                let index = self.media_queue.iter().position(|i| i.url == url);
                let (mut item, mut queue, index) = if let Some(index) = index {
                    (
                        self.media_queue[index].clone(),
                        self.media_queue.clone(),
                        index,
                    )
                } else if (url.contains("list=") || url.contains("playlist"))
                    && !self.media_queue.is_empty()
                {
                    (self.media_queue[0].clone(), self.media_queue.clone(), 0)
                } else {
                    (item.clone(), vec![item], 0)
                };
                let auto_next = video
                    && self.detail_item.as_ref().is_some_and(|detail| {
                        detail.url == url
                            && (detail.kind == "playlist"
                                || detail.raw["entries"]
                                    .as_array()
                                    .is_some_and(|entries| !entries.is_empty()))
                    });
                item.raw["_video_auto_next"] = serde_json::json!(auto_next);
                for entry in &mut queue {
                    entry.raw["_video_auto_next"] = serde_json::json!(auto_next);
                }
                self.resolve_media(item, queue, index, video, 0.0);
            }
            Choice::Feed(podcast) => {
                self.push(
                    "Podcasts",
                    vec![
                        visual::header(&podcast.name),
                        visual::fact("Loading episodes…", ""),
                    ],
                );
                self.pages.last_mut().unwrap().role = PageRole::PodcastFeed;
                self.pages.last_mut().unwrap().selected = 1;
                self.task(
                    Task::Feed(podcast),
                    Purpose::Feed {
                        generation: self.generation,
                    },
                );
            }
            Choice::Subscribe(podcast) => self.task(Task::RememberPodcast(podcast), Purpose::Tags),
            Choice::Unsubscribe(podcast) => {
                while self.pages.last().is_some_and(|p| p.role == PageRole::Sheet) {
                    self.pages.pop();
                }
                if !podcast.files.is_empty() {
                    self.effects.push(Effect::Notice(
                        "Its episodes stay in Music › Podcasts".into(),
                    ));
                }
                self.task(Task::ForgetPodcast(podcast.feed), Purpose::Tags)
            }
            Choice::Episode(episode, podcast) => {
                if let Some(path) = podcast.files.get(episode.key()) {
                    self.effects.push(Effect::Play {
                        paths: vec![PathBuf::from(path)],
                        index: 0,
                    });
                    return;
                }
                if visual::episode_playable(&episode) {
                    self.start_download(DownloadRequest::Episodes {
                        podcast,
                        episodes: vec![episode],
                    });
                }
            }
            Choice::DownloadEpisodes(podcast, episodes) => self.task(
                Task::Download(DownloadRequest::Episodes { podcast, episodes }),
                Purpose::Download,
            ),
            Choice::TagField(field) => {
                let query = self.field_value(field);
                self.keyboard = Some((Keyboard::new(query), Typing::Tag(field)));
            }
            Choice::SaveTagBatch => {
                if let Some(edit) = self.album_edit.as_mut() {
                    let changes = edit
                        .changes()
                        .into_iter()
                        .filter(|(path, _)| visual::can_write_tags_ui(path))
                        .collect();
                    edit.saving = true;
                    self.task(
                        Task::WriteAlbumTags { edits: changes },
                        Purpose::WriteTags {
                            generation: self.generation,
                        },
                    );
                    self.refresh_editor_page();
                }
            }
            Choice::UndoTags => {
                if let Some(edit) = self.album_edit.as_mut() {
                    edit.undo();
                }
                self.pending_metadata = None;
                self.refresh_editor_page();
            }
            Choice::FindInfo => self.open_info_search(),
            Choice::EditSongs => {
                if let Some(edit) = self.album_edit.as_mut() {
                    edit.scope = None;
                }
                self.push("Songs", self.editor_songs_rows());
                self.pages.last_mut().unwrap().role = PageRole::SongsInfo;
            }
            Choice::EditSong(index) => {
                if let Some(edit) = self.album_edit.as_mut() {
                    edit.scope = Some(index);
                }
                self.push("Song Info", self.editor_song_rows(index));
                self.pages.last_mut().unwrap().role = PageRole::SongInfo(index);
            }
            Choice::ChooseEditedCover => self.effects.push(Effect::ChooseAlbumCover {
                paths: self.edit_paths.clone(),
                album: self.edit_title.clone(),
                artist: self.field_value(Field::AlbumArtist),
            }),
            Choice::MergeEdited => {
                let paths = self.editor_scope_paths();
                self.effects.push(Effect::MergeAlbum(paths));
            }
            Choice::SeparateEdited => {
                let paths = self
                    .editor_scope_paths()
                    .into_iter()
                    .filter(|path| {
                        self.library_snapshot
                            .tracks
                            .iter()
                            .any(|track| track.path == *path && track.added_album)
                    })
                    .collect::<Vec<_>>();
                if !paths.is_empty() {
                    self.push_confirmation(
                        format!("Send {} songs back to their own albums?", paths.len()),
                        "",
                        "Separate",
                        Choice::SeparateBatch(paths),
                    );
                }
            }
            Choice::SeparateBatch(paths) => self.task(Task::Unmerge { paths }, Purpose::Separate),
            Choice::ResetEditedCover => self.task(
                Task::ResetChosenCover {
                    paths: self.edit_paths.clone(),
                },
                Purpose::CoverReset,
            ),
            Choice::DeleteEditedAlbum => self
                .effects
                .push(Effect::DeleteAlbum(self.edit_paths.clone())),
            Choice::SaveTags => {
                if let Some(edit) = &self.album_edit {
                    let changes = edit.changes();
                    if changes.is_empty() {
                        return;
                    }
                    let writable = changes
                        .iter()
                        .filter(|(path, _)| visual::can_write_tags_ui(path))
                        .count();
                    if writable == 0 {
                        if let Some(edit) = self.album_edit.as_mut() {
                            edit.status = "Only MP3, FLAC, Ogg and M4A tags can be saved".into();
                        }
                        self.refresh_editor_page();
                        return;
                    }
                    let title = format!(
                        "Write into {writable} files? {}. It can't be undone.",
                        edit.summary()
                    );
                    self.push_confirmation(title, "", "Save Changes", Choice::SaveTagBatch);
                    return;
                }
                if let Some(mut item) = self.pending_metadata.clone() {
                    if let Some(value) = &self.edit.album {
                        item.title = value.clone();
                    }
                    if let Some(value) = &self.edit.album_artist {
                        item.artist = value.clone();
                    }
                    if let Some(value) = self.edit.year {
                        item.year = value.to_string();
                    }
                    self.task(
                        Task::ApplyAlbumInfo {
                            paths: self.edit_paths.clone(),
                            item,
                        },
                        Purpose::Tags,
                    );
                } else {
                    let changed = serde_json::to_value(&self.edit)
                        .ok()
                        .and_then(|v| v.as_object().map(|o| o.values().any(|v| !v.is_null())))
                        .unwrap_or(false);
                    if changed {
                        self.task(
                            Task::EditTags {
                                paths: self.edit_paths.clone(),
                                edit: self.edit.clone(),
                            },
                            Purpose::Tags,
                        );
                    } else {
                        self.effects.push(Effect::Notice("No changes".into()));
                    }
                }
            }
            Choice::MetadataSearch(provider) => {
                self.cover_search = false;
                self.keyboard = Some((
                    Keyboard::new(self.edit_title.clone()),
                    Typing::Search(provider, true),
                ))
            }
            Choice::ApplyMetadata(item) => {
                if self.album_edit.is_some() {
                    let source = item.provider.name().to_string();
                    self.task(
                        Task::PreviewAlbumInfo {
                            paths: self.edit_paths.clone(),
                            item,
                        },
                        Purpose::InfoPreview {
                            source,
                            generation: self.generation,
                        },
                    );
                } else {
                    self.pending_metadata = Some(item.clone());
                    if !item.title.is_empty() {
                        self.edit.album = Some(item.title);
                    }
                    if !item.artist.is_empty() {
                        self.edit.artist = Some(item.artist.clone());
                        self.edit.album_artist = Some(item.artist);
                    }
                    self.replace("Album Info", self.tag_rows());
                }
            }
            Choice::BtPower(on) => {
                self.bluetooth.request(BtRequest::Power(on));
                self.status = "Changing Bluetooth power...".into();
            }
            Choice::BtScan => {
                self.bluetooth.request(BtRequest::Scan);
                self.bt_scanning_until = Some(self.visual_time + 12.0);
                self.bt_scan_second = -1;
                self.replace("Bluetooth", self.bluetooth_rows());
            }
            Choice::BtDevice(address) => {
                let connected = self
                    .bt_state
                    .devices
                    .iter()
                    .any(|d| d.address == address && d.connected);
                self.bluetooth.request(if connected {
                    BtRequest::Disconnect(address)
                } else {
                    BtRequest::Connect(address)
                });
                self.status = if connected {
                    "Disconnecting…"
                } else {
                    "Connecting…"
                }
                .into();
            }
            Choice::BtConnect(address) => {
                self.bluetooth.request(BtRequest::Connect(address));
                self.status = "Connecting...".into();
            }
            Choice::BtDisconnect(address) => {
                self.bluetooth.request(BtRequest::Disconnect(address));
                self.status = "Disconnecting...".into();
            }
            Choice::BtForget(address) => self.push_confirmation(
                "Forget This Device",
                "You will need to pair it again",
                "Forget This Device",
                Choice::ConfirmBtForget(address),
            ),
            Choice::CoverSearch(provider) => {
                self.cover_search = true;
                self.keyboard = Some((
                    Keyboard::new(self.edit_title.clone()),
                    Typing::Search(provider, true),
                ));
            }
            Choice::UseCover(item) => {
                if self.cover_override && !item.cover.is_empty() {
                    self.task(
                        Task::SetCover {
                            paths: self.edit_paths.clone(),
                            cover_url: item.cover,
                        },
                        Purpose::Tags,
                    );
                    return;
                }
                if item.cover.is_empty() {
                    self.effects
                        .push(Effect::Notice("No cover available".into()));
                } else {
                    self.task(
                        Task::Download(DownloadRequest::File {
                            url: item.cover,
                            name: format!("{}.jpg", item.title),
                            category: "Covers".into(),
                            tags: None,
                        }),
                        Purpose::Cover,
                    );
                }
            }
            Choice::WeatherRetry => {
                self.weather_loading = true;
                self.weather_error.clear();
                self.weather
                    .command(WeatherCommand::Search(self.weather_query.clone()));
                self.replace("Weather", self.weather_rows());
                self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
            }
            Choice::WeatherSettings => {
                self.push("Weather Settings", self.weather_settings_rows());
                self.pages.last_mut().unwrap().role = PageRole::WeatherSettings;
            }
            Choice::WeatherClear => {
                self.weather_query.clear();
                self.weather_results = None;
                self.replace("Weather", self.weather_rows());
                self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
                self.keyboard = Some((Keyboard::new(String::new()), Typing::Weather));
            }
            Choice::WeatherSearch => {
                self.keyboard = Some((Keyboard::new(self.weather_query.clone()), Typing::Weather))
            }
            Choice::WeatherPlace(index) => {
                if let Some(place) = self.weather_state.places.get(index) {
                    let stale = place
                        .forecast
                        .as_ref()
                        .is_none_or(|forecast| forecast.is_stale());
                    self.forecast_loading = stale;
                    self.forecast_error.clear();
                    if stale {
                        self.weather.command(WeatherCommand::Refresh(index));
                    }
                    self.forecast = Some(index);
                    self.forecast_day = 0;
                    self.push("Weather", Vec::new());
                    self.forecast_depth = self.pages.len();
                }
            }
            Choice::WeatherAdd(place) => {
                self.pending_weather = Some(place.clone());
                self.weather_query.clear();
                self.weather_results = None;
                self.weather.command(WeatherCommand::Add(place));
                self.status = "Adding city...".into();
            }
            Choice::WeatherRefresh(index) => {
                self.forecast_loading = true;
                self.forecast_error.clear();
                self.weather.command(WeatherCommand::Refresh(index));
            }
            Choice::WeatherRemove(index) => {
                self.weather.command(WeatherCommand::Remove(index));
                self.forecast = None;
                self.pages.clear();
                self.push("Weather", self.weather_rows());
                self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
            }
            Choice::WeatherUnits => {
                self.weather
                    .command(WeatherCommand::Units(!self.weather_state.fahrenheit));
            }
            Choice::ShareReceive => {
                self.keyboard = Some((Keyboard::new(String::new()), Typing::Receive))
            }
            Choice::ShareCancel => {
                self.share.cancel();
                self.status = "Transfer cancelled".into();
            }
            Choice::Cancel => {
                if let Some((handle, _)) = self.jobs.values().next() {
                    handle.cancel();
                }
            }
            Choice::Tools => self.task(Task::Tools, Purpose::Tools),
            Choice::RemoveTools => self.push_confirmation(
                "Remove the YouTube Tools?",
                "Music and videos already downloaded stay",
                "Remove Tools",
                Choice::RunTask(Task::RemoveTools),
            ),
            Choice::Nothing => {}
        }
    }
    fn field_value(&self, field: Field) -> String {
        if let Some(edit) = &self.album_edit {
            let (value, same) = edit.common(field);
            return if same { value } else { String::new() };
        }
        match field {
            Field::Title => self
                .edit
                .title
                .as_ref()
                .or(self.original.title.as_ref())
                .cloned()
                .unwrap_or_default(),
            Field::Artist => self
                .edit
                .artist
                .as_ref()
                .or(self.original.artist.as_ref())
                .cloned()
                .unwrap_or_default(),
            Field::Album => self
                .edit
                .album
                .as_ref()
                .or(self.original.album.as_ref())
                .cloned()
                .unwrap_or_default(),
            Field::AlbumArtist => self
                .edit
                .album_artist
                .as_ref()
                .or(self.original.album_artist.as_ref())
                .cloned()
                .unwrap_or_default(),
            Field::Genre => self
                .edit
                .genre
                .as_ref()
                .or(self.original.genre.as_ref())
                .cloned()
                .unwrap_or_default(),
            Field::Year => self
                .edit
                .year
                .or(self.original.year)
                .map(|n| n.to_string())
                .unwrap_or_default(),
            Field::Track => self
                .edit
                .track
                .or(self.original.track)
                .map(|n| n.to_string())
                .unwrap_or_default(),
            Field::Disc => self
                .edit
                .disc
                .or(self.original.disc)
                .map(|n| n.to_string())
                .unwrap_or_default(),
        }
    }
    fn set_field(&mut self, field: Field, value: String) {
        if let Some(edit) = self.album_edit.as_mut() {
            if let Err(error) = edit.set(field, value) {
                edit.status = error;
            }
            return;
        }
        match field {
            Field::Title => self.edit.title = Some(value),
            Field::Artist => self.edit.artist = Some(value),
            Field::Album => self.edit.album = Some(value),
            Field::AlbumArtist => self.edit.album_artist = Some(value),
            Field::Genre => self.edit.genre = Some(value),
            Field::Year => self.edit.year = value.parse().ok(),
            Field::Track => self.edit.track = value.parse().ok(),
            Field::Disc => self.edit.disc = value.parse().ok(),
        }
    }
    fn type_key(&mut self, action: Action) {
        let Some((keyboard, _)) = self.keyboard.as_mut() else {
            return;
        };
        match keyboard.handle(action) {
            KeyboardEffect::Submit => {
                if self.pending_share.is_some()
                    && matches!(self.keyboard.as_ref(),Some((keyboard,Typing::SharingName))if keyboard.query.trim().is_empty())
                {
                    return;
                }
                let (mut keyboard, typing) = self.keyboard.take().unwrap();
                let query = keyboard.query.clone();
                keyboard.hide(self.visual_time);
                self.closing_keyboard = Some(keyboard);
                self.submit_typing(typing, query);
            }
            KeyboardEffect::Close => self.cancel_typing(),
            KeyboardEffect::Changed => {
                if let Some((keyboard, Typing::SharingName)) = &self.keyboard {
                    if self.pending_share.is_some() {
                        self.share_name_draft = keyboard.query.clone();
                        self.refresh_share_name_gate();
                    }
                }
                self.invalidate_visual_rows();
            }
            KeyboardEffect::None => {}
        }
    }
    fn submit_typing(&mut self, typing: Typing, query: String) {
        match typing {
            Typing::Combined => self.run_combined(Some(query)),
            Typing::NewPlaylist => {
                self.playlist_command(playlists::Command::Create { name: query })
            }
            Typing::Search(provider, metadata) => self.search(provider, query, 1, metadata),
            Typing::Weather => {
                self.weather_query = query.clone();
                self.weather_loading = true;
                self.weather_error.clear();
                self.weather_results = None;
                self.weather.command(WeatherCommand::Search(query));
                self.replace("Weather", self.weather_rows());
                self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
            }
            Typing::Receive => {
                if query.len() != 8 || !query.bytes().all(|c| c.is_ascii_digit()) {
                    self.effects
                        .push(Effect::Notice("Enter exactly 8 digits".into()));
                } else if let Err(error) = self.share.submit(share::Command::Receive {
                    code: query,
                    name: self.sharing_name.clone(),
                }) {
                    self.effects.push(Effect::Notice(error));
                } else {
                    self.status = "Waiting for the sender...".into();
                }
            }
            Typing::SharingName => {
                let name = query
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(32)
                    .collect::<String>()
                    .trim()
                    .to_string();
                if !name.is_empty() {
                    self.sharing_name = name.clone();
                    self.effects.push(Effect::SharingName(name));
                    if let Some(pending) = self.pending_share.take() {
                        match pending {
                            PendingShare::Receive => {
                                self.pages.clear();
                                self.push("Receive", Vec::new());
                                self.share_view = Some(ShareVisual::receive(self.visual_time));
                            }
                            PendingShare::Send {
                                paths,
                                mut info,
                                cover,
                            } => {
                                info.from = self.sharing_name.clone();
                                self.send_share(paths, info, cover);
                            }
                        }
                    } else {
                        self.replace(
                            "Sharing",
                            vec![Item::new("Device Name", Choice::EditSharingName)],
                        );
                    }
                }
            }
            Typing::Tag(field) => {
                self.set_field(field, query);
                self.refresh_editor_page();
            }
        }
    }

    fn details_rows(&self, item: &OnlineItem, metadata: bool) -> Vec<Item> {
        let mut rows = Vec::new();
        if metadata {
            rows.push(Item::new(
                "Use This Album Information",
                Choice::ApplyMetadata(item.clone()),
            ));
            if let Some(tracks) = item.raw["tracks"].as_array() {
                for track in tracks {
                    let title = track["title"].as_str().unwrap_or("");
                    rows.push(Item::new(title, Choice::Nothing));
                }
            }
            return rows;
        }
        match item.provider {
            Provider::Archive => return self.archive_rows(item),
            Provider::YouTube | Provider::YouTubePlaylists => {
                rows.push(Item::new(
                    "Listen",
                    Choice::Media(item.url.clone(), item.title.clone(), false),
                ));
                rows.push(Item::new(
                    "Watch Video",
                    Choice::Media(item.url.clone(), item.title.clone(), true),
                ));
                rows.push(Item::new(
                    "Download Audio",
                    Choice::Download(DownloadRequest::YouTube {
                        url: item.url.clone(),
                        video: false,
                        split_chapters: false,
                    }),
                ));
                if item.raw["chapters"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
                {
                    rows.push(Item::new(
                        "Download as Album",
                        Choice::Download(DownloadRequest::YouTube {
                            url: item.url.clone(),
                            video: false,
                            split_chapters: true,
                        }),
                    ));
                }
                rows.push(Item::new(
                    "Download Video",
                    Choice::Download(DownloadRequest::YouTube {
                        url: item.url.clone(),
                        video: true,
                        split_chapters: false,
                    }),
                ));
                if let Some(entries) = item.raw["entries"].as_array() {
                    for entry in entries {
                        let child =
                            betterpod_rust::online::youtube_item(entry.clone(), Provider::YouTube);
                        rows.push(Item::new(child.title.clone(), Choice::Online(child, false)));
                    }
                }
            }
            _ => {}
        }
        rows
    }
    fn bluetooth_rows(&self) -> Vec<Item> {
        if self.bt_error.contains("No such file") || self.bt_error.contains("not found") {
            let mut row = visual::fact("Bluetooth isn't available", "");
            row.detail = "bluetoothctl wasn't found on this system".into();
            return vec![row];
        }

        let mut power = Item::new("Bluetooth", Choice::BtPower(!self.bt_state.powered));
        power.visual.value = if self.bt_state.powered { "On" } else { "Off" }.into();
        power.visual.no_arrow = true;
        let mut rows = vec![power];
        if !self.bt_state.powered {
            return rows;
        }
        let paired = self
            .bt_state
            .devices
            .iter()
            .filter(|device| device.paired)
            .collect::<Vec<_>>();
        if paired.is_empty() {
            let mut row = visual::fact("No devices paired yet", "");
            row.detail = "Find New Devices below".into();
            rows.push(row);
        }
        for device in paired {
            let mut row = Item::new(&device.name, Choice::BtDevice(device.address.clone()));
            row.detail = if device.connected {
                "Connected"
            } else {
                "Not Connected"
            }
            .into();
            if let Some(battery) = device.battery {
                row.detail.push_str(&format!(" · Battery {battery}%"));
            }
            row.visual.icon =
                betterpod_rust::preview_icons::bluetooth_device_icon_name(device).into();
            rows.push(row);
        }
        let mut find = Item::new("Find New Devices", Choice::BtScan);
        find.detail = "Put the headphones in pairing mode first".into();
        if let Some(until) = self.bt_scanning_until {
            let left = (until - self.visual_time).ceil().max(0.0);
            find.title = "Searching…".into();
            find.detail = format!("{left:.0} s · new devices show below");
            find.visual.no_arrow = true;
            find.visual.progress = (1.0 - left / 12.0).max(0.001) as f32;
        }
        rows.push(find);
        for device in self.bt_state.devices.iter().filter(|device| !device.paired) {
            let mut row = Item::new(&device.name, Choice::BtDevice(device.address.clone()));
            row.detail = "New · press [A] to pair and connect".into();
            row.visual.icon =
                betterpod_rust::preview_icons::bluetooth_device_icon_name(device).into();
            rows.push(row);
        }
        rows
    }
    pub(super) fn tick(&mut self, _dt: f32) -> Vec<Effect> {
        self.poll_inline_listen();
        if self.bt_last_header.elapsed() >= std::time::Duration::from_secs(2) {
            self.bt_last_header = std::time::Instant::now();
            self.bluetooth.request(BtRequest::Header);
        }
        if self.bt_last_devices.elapsed() >= std::time::Duration::from_secs(10)
            && self.pages.last().is_some_and(|p| p.title == "Bluetooth")
            && self.bt_scanning_until.is_none()
        {
            self.bt_last_devices = std::time::Instant::now();
            self.bluetooth.request(BtRequest::Refresh);
        }

        if self.bt_scanning_until.is_some()
            && self
                .pages
                .last()
                .is_some_and(|page| page.title == "Bluetooth")
        {
            let second = self.visual_time as i64;
            if second != self.bt_scan_second {
                self.bt_scan_second = second;
                self.replace("Bluetooth", self.bluetooth_rows());
            }
        }
        for _ in 0..16 {
            let Some(event) = self.weather.poll() else {
                break;
            };
            match event {
                WeatherEvent::State(state) => {
                    self.weather_state = state;
                    self.status.clear();
                    self.forecast_loading = false;
                    if let Some(place) = self.pending_weather.take() {
                        if let Some(index) = self.weather_state.places.iter().position(|p| {
                            (p.lat - place.lat).abs() < 0.01 && (p.lon - place.lon).abs() < 0.01
                        }) {
                            self.replace("Weather", self.weather_rows());
                            self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
                            self.forecast = Some(index);
                            self.forecast_day = 0;
                            self.forecast_loading = true;
                            self.push("Weather", Vec::new());
                            self.forecast_depth = self.pages.len();
                        }
                    } else if self
                        .pages
                        .last()
                        .is_some_and(|p| p.role == PageRole::WeatherSettings)
                    {
                        self.replace("Weather Settings", self.weather_settings_rows());
                        self.pages.last_mut().unwrap().role = PageRole::WeatherSettings;
                    } else if self
                        .pages
                        .last()
                        .is_some_and(|p| p.role == PageRole::WeatherHome)
                    {
                        self.replace("Weather", self.weather_rows());
                        self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
                    }
                }
                WeatherEvent::SearchResults { query, places } => {
                    if query != self.weather_query {
                        continue;
                    }
                    self.weather_loading = false;
                    self.weather_error.clear();
                    self.weather_results = Some(places);
                    self.replace("Weather", self.weather_rows());
                    let page = self.pages.last_mut().unwrap();
                    page.role = PageRole::WeatherHome;
                    page.selected = if page.rows.len() > 1 { 1 } else { 0 };
                }
                WeatherEvent::Error { context, message } => {
                    if context.starts_with("forecast:") {
                        self.forecast_loading = false;
                        self.forecast_error = message;
                    } else if context == "search" {
                        self.weather_loading = false;
                        self.weather_error = message;
                        self.replace("Weather", self.weather_rows());
                        self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
                    } else {
                        self.effects
                            .push(Effect::Notice(format!("{context}: {message}")));
                    }
                }
            }
        }
        for _ in 0..16 {
            let Some(event) = self.share.poll() else {
                break;
            };
            if let Some(view) = self.share_view.as_mut() {
                view.event(&event, self.visual_time);
                if matches!(&event, share::Event::Received { .. }) {
                    self.effects.push(Effect::Rescan);
                }
                continue;
            }
            match event {
                share::Event::Preview { .. } => {}
                share::Event::Code(code) => {
                    self.status = format!("Share code: {code}");
                    self.replace(
                        format!("Share: {code}"),
                        vec![
                            Item::new("Waiting for receiver", Choice::Nothing),
                            Item::new("Cancel Transfer", Choice::ShareCancel),
                        ],
                    );
                }
                share::Event::Progress {
                    done,
                    total,
                    phase,
                    peer,
                } => {
                    self.status = format!(
                        "{phase} - {}% {}",
                        if total > 0 {
                            done.saturating_mul(100) / total
                        } else {
                            0
                        },
                        peer.unwrap_or_default()
                    );
                }
                share::Event::Received { paths, info: _ } => {
                    self.status.clear();
                    self.effects
                        .push(Effect::Notice(format!("Received {} songs", paths.len())));
                    self.effects.push(Effect::Rescan);
                }
                share::Event::Sent => {
                    self.status.clear();
                    self.effects
                        .push(Effect::Notice("Transfer complete".into()));
                }
                share::Event::Error(error) => {
                    self.status.clear();
                    self.effects.push(Effect::Notice(error));
                }
            }
        }
        while let Some(event) = self.playlists.poll() {
            match event {
                playlists::Event::Listed(lists) => {
                    self.lists = Arc::new(lists);
                    if self
                        .pages
                        .first()
                        .is_some_and(|p| p.title == "Playlists" || p.title == "Add to Playlist")
                    {
                        self.show_playlists();
                    }
                }
                playlists::Event::Created(playlist) => {
                    if self
                        .pages
                        .first()
                        .is_some_and(|p| p.title == "Add to Playlist")
                        && !self.append_paths.is_empty()
                    {
                        self.playlist_command(playlists::Command::Append {
                            path: playlist.path,
                            tracks: self.append_paths.clone(),
                        });
                    }
                    self.playlist_command(playlists::Command::List);
                    self.effects.push(Effect::Notice("Playlist created".into()));
                }
                playlists::Event::Saved => {
                    self.playlist_command(playlists::Command::List);
                    self.effects.push(Effect::Notice("Playlist saved".into()));
                }
                playlists::Event::Error(error) => self.effects.push(Effect::Notice(error)),
            }
        }
        while let Some(result) = self.bluetooth.poll() {
            match result {
                Ok(state) => {
                    if state.header_only {
                        self.bt_state.audio_connected = state.audio_connected;
                        continue;
                    }
                    self.bt_error.clear();
                    self.bt_state = state;
                    if self
                        .bt_scanning_until
                        .is_some_and(|until| self.visual_time >= until)
                    {
                        self.bt_scanning_until = None;
                    }
                    self.status.clear();
                    if self.pages.first().is_some_and(|p| p.title == "Bluetooth") {
                        self.pages.clear();
                        self.push("Bluetooth", self.bluetooth_rows());
                    }
                }
                Err(error) => {
                    self.bt_error = error.clone();
                    self.bt_scanning_until = None;
                    self.status.clear();
                    if self.pages.last().is_some_and(|p| p.title == "Bluetooth") {
                        self.replace("Bluetooth", self.bluetooth_rows());
                    }
                    if self.pages.last().is_some_and(|p| p.title == "Bluetooth") {
                        self.effects.push(Effect::Notice(error));
                    }
                }
            }
        }
        for _ in 0..16 {
            let Some(event) = self.services.poll() else {
                break;
            };
            let id = match &event {
                Event::Progress { id, .. } | Event::Finished { id, .. } => *id,
            };
            if self.cover_jobs.contains_key(&id) {
                if matches!(event, Event::Finished { .. }) {
                    self.cover_jobs.remove(&id);
                    self.cover_events.push_back(event);
                } else if let Some(index) = self
                    .cover_events
                    .iter()
                    .position(|old| matches!(old,Event::Progress{id:old,..}if *old==id))
                {
                    self.cover_events[index] = event;
                } else {
                    self.cover_events.push_back(event);
                }
                continue;
            }
            match event {
                Event::Progress {
                    status,
                    done,
                    total,
                    id,
                } => {
                    if self.jobs.get(&id).is_some_and(|(_, p)| {
                        matches!(p, Purpose::Preview(_) | Purpose::ArchiveStatus { .. })
                    }) {
                        continue;
                    }
                    self.status = if let Some(total) = total.filter(|n| *n > 0) {
                        format!(
                            "{status} - {}%",
                            (done.saturating_mul(100) / total).min(100)
                        )
                    } else {
                        status
                    };
                }
                Event::Finished { id, result } => {
                    let purpose = self.jobs.remove(&id).map(|(_, p)| p);
                    if let Some(Purpose::Video(job)) = purpose.as_ref() {
                        self.video_finished(job.clone(), result);
                        continue;
                    }
                    if let Some(Purpose::Preview(url)) = purpose.as_ref() {
                        if let Ok(Output::Files(files)) = &result {
                            if let Some(path) = files.first() {
                                self.previews.insert(url.clone(), Some(path.clone()));
                                self.invalidate_visual_rows();
                                self.emit_media_art();
                            }
                        }
                        if self.combined.as_ref().is_some_and(|state| state.cover) {
                            self.refresh_combined();
                        }
                        continue;
                    }
                    if !matches!(purpose, Some(Purpose::ArchiveStatus { .. })) {
                        self.status.clear();
                    }
                    match result {
                        Err(error) => {
                            if let Some(Purpose::Stream {
                                inline: false,
                                video: true,
                                origin,
                                generation,
                                ..
                            }) = &purpose
                            {
                                if *generation == self.media_generation {
                                    self.effects.push(Effect::VideoResolveError {
                                        origin: origin.clone(),
                                        error,
                                    });
                                }
                                continue;
                            }
                            if matches!(purpose, Some(Purpose::ArchiveStatus { .. })) {
                                continue;
                            }
                            if matches!(purpose, Some(Purpose::Download)) {
                                self.refresh_archive_status();
                            }
                            if matches!(purpose, Some(Purpose::Merge)) {
                                if self
                                    .pages
                                    .last()
                                    .is_some_and(|page| page.title == "Album Changes")
                                {
                                    self.pages.pop();
                                }
                                self.effects.push(Effect::MergeFailed(error));
                                continue;
                            }
                            if matches!(&purpose,Some(Purpose::ReadTags{generation}|Purpose::WriteTags{generation}|Purpose::InfoPreview{generation,..})if *generation!=self.generation)
                            {
                                continue;
                            }
                            if matches!(
                                &purpose,
                                Some(
                                    Purpose::ReadTags { .. }
                                        | Purpose::WriteTags { .. }
                                        | Purpose::InfoPreview { .. }
                                )
                            ) {
                                if let Some(edit) = self.album_edit.as_mut() {
                                    edit.loading = false;
                                    edit.saving = false;
                                    edit.status = error;
                                }
                                self.refresh_editor_page();
                                continue;
                            }
                            if let Some(
                                Purpose::Combined { generation }
                                | Purpose::CoverContext { generation },
                            ) = purpose.as_ref()
                            {
                                if let Some(state) = self
                                    .combined
                                    .as_mut()
                                    .filter(|state| state.generation == *generation)
                                {
                                    state.pending =
                                        if matches!(purpose, Some(Purpose::CoverContext { .. })) {
                                            0
                                        } else {
                                            state.pending.saturating_sub(1)
                                        };
                                    state.errors.push(error);
                                    self.refresh_combined();
                                }
                                continue;
                            }
                            if let Some(Purpose::Search {
                                provider,
                                metadata,
                                generation,
                                ..
                            }) = purpose
                            {
                                if generation == self.generation {
                                    if let Some(state) = self
                                        .searches
                                        .iter_mut()
                                        .find(|s| s.provider == provider && s.metadata == metadata)
                                    {
                                        state.busy = false;
                                        state.error = error;
                                    }
                                    let rows = self.search_rows(provider, metadata);
                                    self.replace(provider.name(), rows);
                                    self.pages.last_mut().unwrap().role =
                                        PageRole::Search(provider, metadata);
                                }
                            } else {
                                self.effects.push(Effect::Notice(error));
                            }
                        }
                        Ok(output) => match output {
                            Output::CoverQuery(query) => {
                                if let Some(Purpose::CoverContext { generation }) = purpose {
                                    if let Some(state) = self
                                        .combined
                                        .as_mut()
                                        .filter(|state| state.generation == generation)
                                    {
                                        state.automatic_query = query
                                            .album_query()
                                            .source_terms()
                                            .first()
                                            .cloned()
                                            .unwrap_or_else(|| query.album.clone());
                                        if !state.manual {
                                            state.query = state.automatic_query.clone();
                                        }
                                        state.cover_context = Some(query);
                                        self.start_combined_requests();
                                    }
                                }
                            }
                            Output::ArchiveStatus { have } => {
                                if let Some(Purpose::ArchiveStatus { generation, item }) = purpose {
                                    if generation == self.generation {
                                        self.archive_status.insert(item.clone(), have);
                                        if let Some(detail) = self
                                            .detail_item
                                            .as_ref()
                                            .filter(|detail| {
                                                detail.provider == Provider::Archive
                                                    && detail.id == item
                                            })
                                            .cloned()
                                        {
                                            if self.pages.last().is_some_and(|page| {
                                                page.role == PageRole::ArchiveItem
                                            }) {
                                                let rows = self.archive_rows(&detail);
                                                self.replace("Internet Archive", rows);
                                            }
                                        }
                                    }
                                }
                            }
                            Output::VideoCatalog(_) => {}
                            Output::AlbumTags(tags) => {
                                if matches!(purpose,Some(Purpose::ReadTags{generation})if generation==self.generation)
                                {
                                    if let Some(edit) = self.album_edit.as_mut() {
                                        edit.install(tags);
                                    }
                                    self.refresh_editor_page();
                                }
                            }
                            Output::AlbumInfoPreview {
                                edits,
                                matched,
                                total,
                            } => {
                                if let Some(Purpose::InfoPreview { source, generation }) = purpose {
                                    if generation == self.generation {
                                        if let Some(edit) = self.album_edit.as_mut() {
                                            edit.apply_preview(edits);
                                            edit.scope = None;
                                            edit.status = if matched == total {
                                                format!("From {source}: {matched} of {total} songs updated · check, then Save Changes")
                                            } else {
                                                format!("From {source}: only {matched} of {total} songs updated · check unmatched songs before saving")
                                            };
                                        }
                                        while self
                                            .pages
                                            .last()
                                            .is_some_and(|page| page.role != PageRole::AlbumInfo)
                                        {
                                            self.pages.pop();
                                        }
                                        self.refresh_editor_page();
                                    }
                                }
                            }

                            Output::CurationApplied { matched, total } => {
                                if matches!(purpose, Some(Purpose::CoverReset)) {
                                    self.effects.push(Effect::Rescan);
                                    self.effects
                                        .push(Effect::Notice("Using the album's own cover".into()));
                                    continue;
                                }
                                if matches!(purpose, Some(Purpose::Separate)) {
                                    self.effects.push(Effect::Rescan);
                                    self.effects.push(Effect::Exit);
                                    self.effects
                                        .push(Effect::Notice("Back in their own albums".into()));
                                    continue;
                                }
                                if matches!(purpose, Some(Purpose::Merge)) {
                                    if self
                                        .pages
                                        .last()
                                        .is_some_and(|page| page.title == "Album Changes")
                                    {
                                        self.pages.pop();
                                    }
                                    if self.album_edit.is_some() {
                                        self.task(
                                            Task::ReadAlbumTags {
                                                paths: self.edit_paths.clone(),
                                            },
                                            Purpose::ReadTags {
                                                generation: self.generation,
                                            },
                                        );
                                    }
                                    self.effects.push(Effect::MergeApplied);
                                }
                                self.effects.push(Effect::Notice(format!(
                                    "Saved {matched} of {total} album changes"
                                )));
                                self.effects.push(Effect::Rescan);
                            }
                            Output::MetadataApplied { matched, total } => {
                                self.effects.push(Effect::Notice(if matched==total {format!("Updated all {total} tracks")}else{format!("Updated track information for {matched} of {total}; album fields saved")}));
                                self.effects.push(Effect::Rescan);
                            }
                            Output::Search { items, total } => {
                                if let Some(Purpose::Combined { generation }) = purpose.as_ref() {
                                    if let Some(state) = self
                                        .combined
                                        .as_mut()
                                        .filter(|state| state.generation == *generation)
                                    {
                                        state.pending = state.pending.saturating_sub(1);
                                        for item in items {
                                            if !state.cover || !item.cover.is_empty() {
                                                if !state.hits.iter().any(|old| {
                                                    old.provider == item.provider
                                                        && old.id == item.id
                                                }) {
                                                    state.hits.push(item);
                                                }
                                            }
                                        }
                                        let cover = state.cover;
                                        state.hits.sort_by_key(|item| {
                                            let best =
                                                item.raw["_cover_best"].as_bool().unwrap_or(false);
                                            let exact =
                                                item.raw["_cover_exact"].as_bool().unwrap_or(false);
                                            (
                                                !best && cover,
                                                cover && best && !exact,
                                                match item.provider {
                                                    Provider::MusicBrainz => 0,
                                                    Provider::Itunes => 1,
                                                    Provider::Deezer => 2,
                                                    Provider::Wikipedia => 3,
                                                    _ => 4,
                                                },
                                            )
                                        });
                                        if state.pending == 0
                                            && state.cover
                                            && !state.hits.is_empty()
                                        {
                                            if let Some(page) = self.pages.last_mut() {
                                                if page.role == PageRole::Combined {
                                                    page.selected = 1;
                                                }
                                            }
                                        }
                                        self.refresh_combined();
                                    }
                                    continue;
                                }

                                if matches!(purpose, Some(Purpose::History)) {
                                    self.replace(
                                        "Recently Watched",
                                        items
                                            .into_iter()
                                            .map(|item| {
                                                Item::new(item.title.clone(), Choice::Recent(item))
                                            })
                                            .collect(),
                                    );
                                    continue;
                                }
                                if let Some(Purpose::Search {
                                    provider,
                                    query,
                                    page,
                                    metadata,
                                    generation,
                                }) = purpose
                                {
                                    if generation != self.generation {
                                        continue;
                                    }
                                    if matches!(
                                        provider,
                                        Provider::YouTube | Provider::YouTubePlaylists
                                    ) && !metadata
                                    {
                                        self.media_queue = items.clone();
                                    }
                                    if let Some(state) = self
                                        .searches
                                        .iter_mut()
                                        .find(|s| s.provider == provider && s.metadata == metadata)
                                    {
                                        state.busy = false;
                                        state.error.clear();
                                        state.total = total;
                                        for item in items {
                                            if !state
                                                .items
                                                .iter()
                                                .any(|old| old.id == item.id && old.url == item.url)
                                            {
                                                state.items.push(item);
                                            }
                                        }
                                    }
                                    let rows = self.search_rows(provider, metadata);
                                    self.replace(provider.name(), rows);
                                    self.pages.last_mut().unwrap().role =
                                        PageRole::Search(provider, metadata);
                                    if self.pages.last().unwrap().rows.len() > 1 {
                                        self.pages.last_mut().unwrap().selected = 1;
                                    }
                                }
                            }
                            Output::Item(item) => {
                                if let Some(Purpose::Details { generation, .. }) = &purpose {
                                    if *generation != self.generation {
                                        continue;
                                    }
                                }

                                self.detail_item = Some(item.clone());
                                if let Some(entries) = item.raw["entries"].as_array() {
                                    self.media_queue = entries
                                        .iter()
                                        .map(|entry| {
                                            betterpod_rust::online::youtube_item(
                                                entry.clone(),
                                                Provider::YouTube,
                                            )
                                        })
                                        .collect();
                                }

                                let metadata = matches!(
                                    purpose,
                                    Some(Purpose::Details { metadata: true, .. })
                                );
                                let title = if item.provider == Provider::Archive && !metadata {
                                    item.provider.name()
                                } else {
                                    &item.title
                                };
                                if metadata && self.album_edit.is_some() && !self.cover_search {
                                    self.choose(Choice::ApplyMetadata(item));
                                    continue;
                                }
                                let rows = self.details_rows(&item, metadata);
                                if item.provider == Provider::Archive
                                    && !metadata
                                    && self
                                        .pages
                                        .last()
                                        .is_some_and(|p| p.role == PageRole::ArchiveItem)
                                {
                                    self.replace(title, rows);
                                } else {
                                    self.push(title, rows);
                                }
                                if item.provider == Provider::Archive && !metadata {
                                    self.pages.last_mut().unwrap().role = PageRole::ArchiveItem;
                                    self.pages.last_mut().unwrap().selected =
                                        if self.pages.last().unwrap().rows.len() > 2 {
                                            2
                                        } else {
                                            1
                                        };
                                    self.refresh_archive_status();
                                }
                            }
                            Output::Feed { podcast, episodes } => {
                                if let Some(Purpose::Feed { generation }) = &purpose {
                                    if *generation != self.generation {
                                        continue;
                                    }
                                }

                                self.media_queue = episodes
                                    .iter()
                                    .map(|episode| {
                                        let mut item = OnlineItem::new(Provider::Podcast);
                                        item.url = episode.url.clone();
                                        item.title = episode.title.clone();
                                        item.seconds = episode.seconds;
                                        item.kind = "audio".into();
                                        item
                                    })
                                    .collect();

                                self.active_feed = Some((podcast.clone(), episodes.clone()));
                                let rows = self.podcast_feed_rows(&podcast, &episodes);
                                if self
                                    .pages
                                    .last()
                                    .is_some_and(|page| page.role == PageRole::PodcastFeed)
                                {
                                    self.replace("Podcasts", rows);
                                } else {
                                    self.push("Podcasts", rows);
                                }
                                let page = self.pages.last_mut().unwrap();
                                page.role = PageRole::PodcastFeed;
                                page.selected = 1;
                            }
                            Output::Subscriptions(podcasts) => {
                                self.subscriptions = podcasts;
                                if let Some((podcast, episodes)) = self.active_feed.as_mut() {
                                    if let Some(record) =
                                        self.subscriptions.iter().find(|p| p.feed == podcast.feed)
                                    {
                                        podcast.files = record.files.clone();
                                    } else {
                                        podcast.files.clear();
                                    }
                                    let _ = episodes;
                                }
                                if self
                                    .pages
                                    .last()
                                    .is_some_and(|p| p.role == PageRole::PodcastFeed)
                                {
                                    if let Some((podcast, episodes)) = &self.active_feed {
                                        let rows = self.podcast_feed_rows(podcast, episodes);
                                        self.replace("Podcasts", rows);
                                        let page = self.pages.last_mut().unwrap();
                                        page.role = PageRole::PodcastFeed;
                                        page.selected = 1;
                                    }
                                }
                                if self
                                    .pages
                                    .last()
                                    .is_some_and(|page| page.role == PageRole::PodcastHome)
                                {
                                    let rows = self.podcast_home_rows();
                                    self.replace("Podcasts", rows);
                                    self.pages.last_mut().unwrap().role = PageRole::PodcastHome;
                                }
                            }
                            Output::Files(files) => {
                                if matches!(purpose, Some(Purpose::Download)) {
                                    self.refresh_video_catalog();
                                }
                                if matches!(purpose, Some(Purpose::Download)) {
                                    self.refresh_archive_status();
                                }
                                if matches!(purpose, Some(Purpose::Cover)) {
                                    if let Some(path) = files.first() {
                                        self.edit.cover = Some(path.clone());
                                        self.effects.push(Effect::Notice(
                                            "Cover selected; choose Save Changes to apply".into(),
                                        ));
                                        self.push("Album Info", self.tag_rows());
                                    }
                                    continue;
                                }

                                if matches!(purpose, Some(Purpose::Downloads)) {
                                    let rows = files
                                        .into_iter()
                                        .map(|path| {
                                            let name = path
                                                .file_stem()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .into_owned();
                                            let video = path
                                                .extension()
                                                .and_then(|s| s.to_str())
                                                .is_some_and(|s| {
                                                    matches!(
                                                        s.to_ascii_lowercase().as_str(),
                                                        "mp4" | "mkv" | "webm" | "mov"
                                                    )
                                                });
                                            Item::new(
                                                name.clone(),
                                                if video {
                                                    Choice::Media(
                                                        path.to_string_lossy().into_owned(),
                                                        name,
                                                        true,
                                                    )
                                                } else {
                                                    Choice::Media(
                                                        path.to_string_lossy().into_owned(),
                                                        name,
                                                        false,
                                                    )
                                                },
                                            )
                                        })
                                        .collect();
                                    self.replace("Downloaded", rows);
                                } else {
                                    self.effects.push(Effect::Notice(format!(
                                        "Downloaded {} files",
                                        files.len()
                                    )));
                                    self.effects.push(Effect::Rescan);
                                    self.task(Task::Subscriptions, Purpose::Subscriptions);
                                }
                            }
                            Output::Saved => {
                                if matches!(purpose, Some(Purpose::Remember)) {
                                    self.refresh_video_catalog();
                                }
                                if matches!(&purpose,Some(Purpose::WriteTags{generation})if *generation!=self.generation)
                                {
                                    self.effects.push(Effect::Rescan);
                                    continue;
                                }
                                if matches!(purpose, Some(Purpose::WriteTags{generation})if generation==self.generation)
                                {
                                    if let Some(edit) = self.album_edit.as_mut() {
                                        for song in &mut edit.songs {
                                            song.original = song.current.clone();
                                        }
                                        edit.saving = false;
                                    }
                                    self.effects.push(Effect::Rescan);
                                    self.effects.push(Effect::Exit);
                                    continue;
                                }
                                if matches!(purpose, Some(Purpose::Remember)) {
                                    continue;
                                }
                                self.task(Task::Subscriptions, Purpose::Subscriptions);
                                self.effects.push(Effect::Notice("Changes saved".into()));
                                self.effects.push(Effect::Rescan);
                            }
                            Output::Stream(source) => {
                                if let Some(Purpose::Stream {
                                    inline,
                                    video,
                                    origin,
                                    start,
                                    queue,
                                    index,
                                    generation,
                                }) = purpose
                                {
                                    if generation != self.media_generation {
                                        continue;
                                    }
                                    self.effects.push(if inline {
                                        Effect::Preview {
                                            source,
                                            origin,
                                            queue,
                                            index,
                                        }
                                    } else {
                                        Effect::Stream {
                                            source,
                                            video,
                                            origin,
                                            start,
                                            queue,
                                            index,
                                        }
                                    });
                                    self.emit_media_art();
                                }
                            }
                            Output::ToolsStatus(state) => {
                                self.tools_state = state;
                                if self
                                    .pages
                                    .last()
                                    .is_some_and(|page| page.title == "YouTube")
                                {
                                    let rows = self.tools_rows();
                                    self.replace("YouTube", rows);
                                }
                            }
                            Output::ToolsRemoved => {
                                self.tools_state = Default::default();
                                self.youtube = false;
                                self.effects.push(Effect::YouTubeEnabled(false));
                                self.effects
                                    .push(Effect::Notice("YouTube tools removed".into()));
                            }
                            Output::ToolsReady => {
                                self.task(Task::ToolsStatus, Purpose::Tools);
                                self.effects
                                    .push(Effect::Notice("YouTube tools are ready".into()));
                            }
                        },
                    }
                }
            }
        }
        if let Some(editor) = self.cover_editor.as_mut() {
            editor.path = self.previews.get(&editor.url).and_then(Clone::clone);
        }
        let preview_urls = self
            .pages
            .last()
            .map(|page| {
                page.rows
                    .iter()
                    .skip(
                        if page.role == PageRole::Combined
                            && self.combined.as_ref().is_some_and(|s| s.cover)
                        {
                            0
                        } else {
                            (page.scroll / 52.0).max(0.0) as usize
                        },
                    )
                    .take(
                        if page.role == PageRole::Combined
                            && self.combined.as_ref().is_some_and(|s| s.cover)
                        {
                            128
                        } else {
                            8
                        },
                    )
                    .filter_map(|r| choice_art(&r.choice))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for url in preview_urls {
            self.request_preview(url);
        }
        if let Some(page) = self.pages.last_mut() {
            page.scroll = page.target;
        }
        std::mem::take(&mut self.effects)
    }
    pub(super) fn draw(
        &mut self,
        r: &mut Renderer,
        dark: bool,
        elapsed: f64,
        art: &mut crate::art::Artwork,
        assets: &VisualAssets,
    ) {
        self.visual_time = elapsed;
        if let Some(view) = self.share_view.as_mut() {
            view.draw(r, assets, art, elapsed);
            return;
        }
        if let Some(editor) = self.cover_editor.as_ref() {
            editor.draw(r, art, dark);
            return;
        }
        if let Some(index) = self
            .forecast
            .filter(|_| self.pages.len() == self.forecast_depth)
        {
            if let Some(place) = self.weather_state.places.get(index) {
                self.weather_visual.draw(
                    r,
                    place,
                    self.forecast_day,
                    self.weather_state.fahrenheit,
                    self.forecast_loading,
                    &self.forecast_error,
                    elapsed,
                );
                return;
            }
        }

        let fg = if dark { WHITE } else { FG };
        let muted = if dark { [0.65, 0.68, 0.73, 1.0] } else { MUTED };
        let Some(page) = self.pages.last() else {
            return;
        };
        r.set_clip(0.0, 40.0, 640.0, 365.0);
        let first = (page.scroll / 52.0).floor().max(0.0) as usize;
        for (i, row) in page.rows.iter().enumerate().skip(first).take(9) {
            let y = 42.0 + i as f32 * 52.0 - page.scroll;
            let selected = i == page.selected;
            if selected {
                r.gradient(
                    0.0,
                    y,
                    640.0,
                    52.0,
                    [0.3, 0.62, 0.97, 1.0],
                    [0.06, 0.36, 0.77, 1.0],
                );
            }
            let image = choice_art(&row.choice)
                .and_then(|url| self.previews.get(url))
                .and_then(Option::as_deref);
            let x = if let Some(path) = image {
                let texture = art.get(Some(path));
                r.image(texture, 10.0, y + 5.0, 42.0, 42.0, 1.0);
                64.0
            } else {
                16.0
            };
            r.text(
                &row.title,
                x,
                y + if row.detail.is_empty() { 12.0 } else { 3.0 },
                22,
                if selected { WHITE } else { fg },
                620.0 - x,
            );
            if !row.detail.is_empty() {
                r.text(
                    &row.detail,
                    x,
                    y + 29.0,
                    16,
                    if selected { WHITE } else { muted },
                    620.0 - x,
                );
            }
        }
        if page.rows.is_empty() {
            r.centered(
                if self.status.is_empty() {
                    "No items"
                } else {
                    &self.status
                },
                15.0,
                207.0,
                610.0,
                22,
                muted,
            );
        }
        r.reset_clip();
        if !self.status.is_empty() {
            r.text(&self.status, 15.0, 417.0, 17, muted, 610.0);
        }
        r.text(
            "A: Select    B: Back    Y: Options / Cancel",
            15.0,
            455.0,
            16,
            muted,
            610.0,
        );
        if let Some((keyboard, _)) = self.keyboard.as_mut() {
            keyboard.draw(r, assets, dark, elapsed);
        }
        if let Some(keyboard) = self.closing_keyboard.as_mut() {
            keyboard.draw(r, assets, dark, elapsed);
        }
        if self
            .closing_keyboard
            .as_ref()
            .is_some_and(|k| k.finished(elapsed))
        {
            self.closing_keyboard = None;
        }
    }
    pub(super) fn shutdown(&mut self) {
        self.listen.shutdown();
        for job in self.cover_jobs.values() {
            job.cancel();
        }
        // Final video history is a tiny local write. Cancel network jobs first,
        // but allow this queued bookmark to finish before closing its worker.
        for (handle, purpose) in self.jobs.values() {
            if !matches!(purpose, Purpose::Remember) {
                handle.cancel();
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        while self
            .jobs
            .values()
            .any(|(_, p)| matches!(p, Purpose::Remember))
            && std::time::Instant::now() < deadline
        {
            while let Some(event) = self.services.poll() {
                if let Event::Finished { id, .. } = event {
                    self.jobs.remove(&id);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.services.shutdown();
        self.playlists.shutdown();
        self.share.shutdown();
        self.weather.shutdown();
    }
}

fn choice_art(choice: &Choice) -> Option<&str> {
    let url = match choice {
        Choice::Video(VideoAction::Play(_, _, _, cover)) => cover,
        Choice::Video(VideoAction::Watch(i))
        | Choice::Online(i, _)
        | Choice::UseCover(i)
        | Choice::Recent(i)
        | Choice::ApplyMetadata(i) => &i.cover,
        Choice::Feed(p) | Choice::Subscribe(p) | Choice::Unsubscribe(p) => {
            if p.art600.is_empty() {
                &p.art
            } else {
                &p.art600
            }
        }
        Choice::Episode(_, p) => {
            if p.art600.is_empty() {
                &p.art
            } else {
                &p.art600
            }
        }
        _ => return None,
    };
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}
