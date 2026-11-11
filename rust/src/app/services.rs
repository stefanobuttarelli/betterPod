//! UI for background library services. No task or menu callback performs I/O.
mod downloads_ui;
mod external_foreground;
mod foreground_ui;
use external_foreground::{ExternalKind, ExternalPending};
mod listen_ui;
mod metadata_edit;
mod share_visual;
mod track_list_ui;
mod videos_ui;
mod visual;
#[cfg(test)]
mod youtube_visual_qa;
pub(super) use downloads_ui::draw_job_button;
use downloads_ui::DownloadState;
mod lists_ui;
mod weather_visual;
use super::{Keyboard, KeyboardEffect, VisualAssets, FG, MUTED, WHITE};
use crate::input::Action;
use crate::render::Renderer;
use betterpod_rust::bluetooth::{Bluetooth, Request as BtRequest, State as BtState};
use betterpod_rust::downloads::DownloadRequest;
use betterpod_rust::metadata::TagEdit;
use betterpod_rust::online::{
    Event, JobHandle, OnlineItem, Output, Provider, Services, StreamSource, Task,
};
use betterpod_rust::playlists::{self, Playlist, PlaylistClient};
use betterpod_rust::podcasts::{Episode, Podcast};
use betterpod_rust::recent_searches::{RecentSearches, SearchScreen};
use betterpod_rust::share::{self, ShareClient, ShareInfo};
use betterpod_rust::weather::{Place, Weather, WeatherCommand, WeatherEvent, WeatherSnapshot};
use share_visual::{ShareAction, ShareVisual};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use videos_ui::{VideoAction, VideoJob};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtraPage {
    Playlists,
    AddToPlaylist,
    // Built only by tests.
    #[cfg_attr(not(test), allow(dead_code))]
    SearchSources,
    Online(Provider),
    Podcasts,
    ActiveDownloads,
    Tools,
    Bluetooth,
    AlbumInfo,
    Videos,
    SavedVideos,
    VideoHistory,
    Weather,
    Receive,
}
#[derive(Clone)]
pub(super) enum Effect {
    /// Recent Searches changed; the app's state writer saves them.
    SaveRecentSearches(RecentSearches),
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
    /// Run a search again from the list under the search field.
    RecentSearch(Provider, String),
    RemoveRecentSearch(SearchScreen, String),
    ClearRecentSearches(SearchScreen),
    Video(VideoAction),
    Page(ExtraPage),
    NewPlaylist,
    MediaQueue(Vec<PathBuf>, bool),
    MediaAddPlaylist(Vec<PathBuf>),
    MediaShare(Vec<PathBuf>),
    CombinedSearch,
    CombinedClear,
    CombinedAutomatic,
    CombinedRetry,
    Playlist(Playlist),
    PlaylistSong(Arc<Playlist>, usize),
    AppendPlaylist(Playlist),
    DeletePlaylist(Playlist),
    RemovePlaylist(Arc<Playlist>, usize),
    Search(Provider),
    ClearSearch(Provider),
    YouTubeSearchSettings(Provider),
    YouTubeSearchMode {
        provider: Provider,
        owner: u64,
        query: String,
    },
    YouTubePlaylistMedia(OnlineItem, bool),
    ArchiveTrack(Arc<OnlineItem>, String, serde_json::Value),
    Preview(OnlineItem, Arc<Vec<OnlineItem>>, usize),
    ConfirmDeletePlaylist(Playlist),
    ConfirmBtForget(String),
    ConfirmAction(Box<Choice>),
    ToggleYouTube,
    NextSearch(Provider, String, u32, bool),
    Online(OnlineItem, bool),
    Download(DownloadRequest),
    ShowDownload(u64),
    CancelDownload(u64),
    RemoveDownload(u64),
    ClearDownloads,
    ConfirmClearDownloads,
    Media(String, String, bool),
    Feed(Podcast),
    Unsubscribe(Podcast),
    Episode(Episode, Podcast),
    DownloadEpisodes(Podcast, Vec<Episode>),
    TagField(Field),
    SaveTags,
    SaveTagBatch,
    UndoTags,
    FindInfo,
    /// Find the release a full-album video holds, to split it into songs.
    FindTrackList(OnlineItem),
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
    WeatherRemove(usize),
    WeatherUnits,
    ShareCancel,
    CoverSearch(Provider),
    UseCover(OnlineItem),
    YouTubeCover(OnlineItem),
    YouTubeCoverSource(OnlineItem, Provider),
    YouTubeOriginalCover(String),
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
    gear: Option<Choice>,
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
            gear: None,
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
    ActiveDownloads,
    Download(u64),
    YouTubeItem,
    DownloadedFiles,
    Tools,
    AlbumInfo,
    SongInfo(usize),
    SongsInfo,
    Combined,
    Normal,
    Playlists(bool),
    PlaylistSongs,
    Search(Provider, bool),
    YouTubeSearchSettings(u64),
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
static NEXT_PAGE_KEY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
struct Page {
    key: u64,
    resource: Option<String>,
    mini: bool,
    role: PageRole,
    playlist_path: Option<PathBuf>,
    title: String,
    rows: Vec<Item>,
    selected: usize,
    gear_on: bool,
    scroll: f32,
    target: f32,
    visual_rows: Option<Arc<Vec<super::Row>>>,
    visual_status: String,
    /// The keyboard state the visual rows were built with, so opening,
    /// editing or closing it rebuilds the row being typed into.
    visual_typing: Option<(Typing, String)>,
}
impl Page {
    fn new(title: impl Into<String>, rows: Vec<Item>) -> Self {
        Self {
            key: NEXT_PAGE_KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            resource: None,
            title: title.into(),
            rows,
            selected: 0,
            gear_on: false,
            scroll: 0.0,
            target: 0.0,
            mini: false,
            role: PageRole::Normal,
            playlist_path: None,
            visual_rows: None,
            visual_status: String::new(),
            visual_typing: None,
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
        // Read only by tests.
        #[cfg_attr(not(test), allow(dead_code))]
        query: String,
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
    // Built only by tests, as a background job that owns no page.
    #[cfg_attr(not(test), allow(dead_code))]
    Downloads,
    Tags,
    Merge,
    Separate,
    CoverReset,
    Tools,
    Subscriptions,
    ToolAction,
    Remember,
    Cover,
    YouTubeCover {
        origin: String,
        owner: u64,
    },
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
#[derive(Clone, Copy, PartialEq)]
enum Typing {
    NewPlaylist,
    Combined,
    Search(Provider, bool),
    Tag(Field),
    Weather,
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
    /// The full-album video a chosen release will split into songs.
    split_video: Option<OnlineItem>,
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
    generation: u64,
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
    recent_searches: RecentSearches,
    /// Set by the app while music is copied to another card.
    music_moving: bool,
    active_feed: Option<(Podcast, Vec<Episode>)>,
    playlists: PlaylistClient,
    lists: Arc<Vec<Playlist>>,
    services: Services,
    jobs: HashMap<u64, (JobHandle, Purpose)>,
    job_pages: HashMap<u64, u64>,
    foreground: Option<foreground_ui::Foreground>,
    external_pending: Option<ExternalPending>,
    service_write_result: Option<(u64, bool)>,
    download_states: std::collections::BTreeMap<u64, DownloadState>,
    status_job: Option<u64>,
    status_page: Option<u64>,
    pages: Vec<Page>,
    pending_navigation: Option<(super::Page, bool, f64)>,
    keyboard: Option<(Keyboard, Typing)>,
    closing_keyboard: Option<Keyboard>,
    visual_time: f64,
    viewport_height: f32,
    bluetooth: Bluetooth,
    bt_state: BtState,
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
    youtube_cover_target: Option<(String, u64)>,
    youtube_covers: HashMap<String, PathBuf>,
    cover_override: bool,
    youtube: bool,
    sharing_name: String,
    previews: HashMap<String, Option<PathBuf>>,
    active_media_art: Option<(String, String)>,
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
            recent_searches: RecentSearches::default(),
            music_moving: false,
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
            youtube_cover_target: None,
            youtube_covers: HashMap::new(),
            cover_override: false,
            youtube: false,
            sharing_name: String::new(),
            previews: HashMap::new(),
            active_media_art: None,
            detail_item: None,
            archive_status: HashMap::new(),
            media_queue: Vec::new(),
            media_generation: 0,
            jobs: HashMap::new(),
            job_pages: HashMap::new(),
            foreground: None,
            external_pending: None,
            service_write_result: None,
            download_states: std::collections::BTreeMap::new(),
            status_job: None,
            status_page: None,
            pages: Vec::new(),
            pending_navigation: None,
            keyboard: None,
            closing_keyboard: None,
            visual_time: 0.0,
            viewport_height: 440.0,
            bluetooth: Bluetooth::new(),
            bt_state: BtState::default(),
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
            self.push("Album Changes", vec![Item::new("Cancel", Choice::Cancel)]);
            self.task(task, Purpose::Tags);
        }
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
        page.role = PageRole::PlaylistSongs;
        page.playlist_path = Some(playlist.path);
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
            if let Some(cover) = self.youtube_covers.get(url).cloned() {
                self.task(
                    Task::DownloadWithCover {
                        request,
                        cover,
                        crop: betterpod_rust::downloads::SquareCrop::Center,
                    },
                    Purpose::Download,
                );
                return;
            }
            // A split album takes the release's cover, else the video's.
            let release_cover = match &request {
                DownloadRequest::YouTube {
                    track_list: Some(list),
                    ..
                } if !list.cover.is_empty() => Some(list.cover.clone()),
                _ => None,
            };
            if let Some(url) = release_cover {
                self.task(
                    Task::DownloadWithAutomaticCover { request, url },
                    Purpose::Download,
                );
                return;
            }
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
                if let Some(path) = self.previews.get(&cover).and_then(Clone::clone) {
                    self.task(
                        Task::DownloadWithCover {
                            request,
                            cover: path,
                            crop: betterpod_rust::downloads::SquareCrop::Center,
                        },
                        Purpose::Download,
                    );
                    return;
                }
                self.task(
                    Task::DownloadWithAutomaticCover {
                        request,
                        url: cover,
                    },
                    Purpose::Download,
                );
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
            if let Some(view) = self.share_view.as_mut() {
                view.event(&share::Event::Error(error), self.visual_time);
            }
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
        self.status.clear();
        self.status_job = None;
        self.status_page = None;
        self.pages.push(Page::new(title, rows));
    }
    fn replace(&mut self, title: impl Into<String>, rows: Vec<Item>) {
        let title = title.into();
        let mut page = Page::new(title, rows);
        if let Some(old) = self.pages.pop() {
            if old.title == page.title {
                page.key = old.key;
                page.resource = old.resource;
                page.role = old.role;
                page.playlist_path = old.playlist_path;
                page.selected = old.selected.min(page.rows.len().saturating_sub(1));
                page.gear_on = old.gear_on
                    && page
                        .rows
                        .get(page.selected)
                        .is_some_and(|row| row.gear.is_some());
                page.scroll = old.scroll;
                page.target = old.target;
                page.mini = old.mini;
            }
        }
        self.pages.push(page);
    }
    fn task(&mut self, task: Task, purpose: Purpose) {
        let download = if matches!(purpose, Purpose::Download) {
            DownloadState::from_task(&task)
        } else {
            None
        };
        if let Some(state) = &download {
            if let Some((&id, _)) = self
                .download_states
                .iter()
                .find(|(_, old)| old.active && !old.dismissed && old.key == state.key)
            {
                self.show_download(id);
                return;
            }
            if self.music_moving {
                self.effects
                    .push(Effect::Notice("Wait for the music to be moved".into()));
                return;
            }
        }
        let foreground = Self::foreground_purpose(&purpose);
        let pending_purpose = foreground.then(|| purpose.clone());
        let hidden = foreground
            || matches!(
                task,
                Task::ToolsStatus
                    | Task::Subscriptions
                    | Task::VideoCatalog
                    | Task::RememberVideoInfo { .. }
                    | Task::RememberVideo { .. }
                    | Task::ArchiveStatus { .. }
                    | Task::CacheArtwork { .. }
            )
            || matches!(
                purpose,
                Purpose::Combined { .. } | Purpose::CoverContext { .. } | Purpose::Stream { .. }
            );
        let page = self.pages.last().map(|page| page.key);
        let prepared = if foreground {
            self.prepare_foreground_purpose(&purpose)
        } else {
            None
        };
        match self.services.submit(task) {
            Ok(handle) => {
                let id = handle.id;
                self.jobs.insert(id, (handle, purpose));
                if let Some(page) = page {
                    self.job_pages.insert(id, page);
                    if let Some(purpose) = pending_purpose {
                        self.claim_foreground(id, page, purpose);
                    }
                }
                if let Some(mut state) = download {
                    if state.title == "YouTube download" {
                        if let Some(item) = self.detail_item.as_ref() {
                            state.use_youtube_info(item);
                        }
                    }
                    self.download_states.insert(id, state);
                    self.refresh_download_pages();
                    self.show_download(id);
                } else if !hidden {
                    self.status = "Working...".into();
                    self.status_job = Some(id);
                    self.status_page = page;
                }
            }
            Err(error) => {
                self.clear_prepared_foreground_row(prepared);
                self.foreground_submission_failed(&purpose, &error);
                self.effects.push(Effect::Notice(error));
            }
        }
    }
    fn playlist_command(&mut self, command: playlists::Command) {
        if let Err(error) = self.playlists.submit(command) {
            self.effects.push(Effect::Notice(error));
        }
    }
    pub(super) fn open(&mut self, page: ExtraPage) {
        self.cancel_browse_jobs();
        self.youtube_cover_target = None;
        self.pending_share = None;
        self.videos.watch_depth = None;
        self.retain_current_visual();
        self.generation = self.generation.wrapping_add(1);
        self.pending_navigation = None;
        if self.share_view.take().is_some() {
            self.share.cancel();
        }
        self.pages.clear();
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
                let target = self.pages.last_mut().unwrap();
                target.mini = page == ExtraPage::Playlists;
                target.role = PageRole::Playlists(page == ExtraPage::AddToPlaylist);
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
                    .map(|p| {
                        Item::new(
                            match p {
                                Provider::YouTube => "YouTube Videos",
                                Provider::YouTubePlaylists => "YouTube Albums",
                                _ => p.name(),
                            },
                            Choice::Page(ExtraPage::Online(p)),
                        )
                    })
                    .collect(),
                );
            }
            ExtraPage::Online(provider) => {
                let query = if matches!(provider, Provider::YouTube | Provider::YouTubePlaylists) {
                    let query = self
                        .searches
                        .iter()
                        .filter(|state| {
                            !state.metadata
                                && matches!(
                                    state.provider,
                                    Provider::YouTube | Provider::YouTubePlaylists
                                )
                        })
                        .max_by_key(|state| state.generation)
                        .map_or_else(String::new, |state| state.query.clone());
                    self.seed_youtube_query(provider, &query);
                    query
                } else {
                    String::new()
                };
                let rows = self.search_rows(provider, false);
                self.push(provider.name(), rows);
                self.pages.last_mut().unwrap().role = PageRole::Search(provider, false);
                self.keyboard = Some((Keyboard::new(query), Typing::Search(provider, false)));
            }
            ExtraPage::Podcasts => {
                let rows = self.podcast_home_rows();
                self.push("Podcasts", rows);
                self.pages.last_mut().unwrap().role = PageRole::PodcastHome;
                self.task(Task::Subscriptions, Purpose::Subscriptions);
            }
            ExtraPage::ActiveDownloads => self.show_downloads(None),
            ExtraPage::Tools => {
                let rows = self.tools_rows();
                self.push("YouTube", rows);
                self.pages.last_mut().unwrap().role = PageRole::Tools;
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
            Item::new("Search Discogs", Choice::MetadataSearch(Provider::Discogs)),
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
            .last()
            .is_some_and(|p| p.role == PageRole::Playlists(true));
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
        self.pages.last_mut().unwrap().role = PageRole::Playlists(adding);
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
            row.visual.thumbnail = true;
            if let Some(track) = track {
                row.detail = track.artist.clone();
                row.cover = track.cover.clone();
                row.visual.value = super::time_label(track.duration);
            }
            rows.push(row);
        }
        if rows.len() == 1 {
            let mut empty = visual::fact("No songs yet", "");
            empty.detail = "Press [Y] on a song, then Add to Playlist…".into();
            rows.push(empty);
        }
        rows
    }
    pub(super) fn handle(&mut self, action: Action) {
        if self.handle_foreground(action) || self.handle_external_foreground(action) {
            return;
        }
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

        if self.handle_download_view(action) {
            return;
        }
        if self.keyboard.is_some() {
            self.type_key(action);
            return;
        }
        if self.search_gear_key(action) {
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
                    self.forecast_loading = true;
                    self.start_external_weather(
                        WeatherCommand::Refresh(index),
                        ExternalKind::Forecast { index, open: false },
                    );
                    return;
                }
                Action::Options => return,
                Action::Back => self.forecast = None,
                _ => {}
            }
        }
        if action == Action::Back {
            if matches!(
                self.pages.last().map(|page| page.role),
                Some(PageRole::YouTubeSearchSettings(_))
            ) {
                let index = self.pages.len() - 1;
                if let Some(previous) = self.project_page(index, true) {
                    self.pending_navigation = Some((previous, false, self.visual_time));
                }
                self.pages.pop();
                return;
            }
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
                self.cancel_browse_jobs();
                self.status.clear();
                self.status_job = None;
                self.status_page = None;
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
            if self.show_download_history_options() {
                return;
            }
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
                                                    Arc::new(item.clone()),
                                                    format,
                                                    file,
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
                            rows.push(Item::new(
                                "Delete Playlist",
                                Choice::DeletePlaylist(playlist.clone()),
                            ));
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
            let current = self.pages.last().map(|page| page.key);
            if let Some((&id, _)) = self.job_pages.iter().find(|(id, page)| {
                Some(**page) == current && !self.download_states.contains_key(id)
            }) {
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
    fn search_gear_key(&mut self, action: Action) -> bool {
        let Some(page) = self.pages.last_mut() else {
            return false;
        };
        let gear = page
            .rows
            .get(page.selected)
            .and_then(|row| row.gear.clone());
        let Some(gear) = gear else {
            page.gear_on = false;
            return false;
        };
        match action {
            Action::Right => {
                page.gear_on = true;
                page.visual_rows = None;
                true
            }
            Action::Left if page.gear_on => {
                page.gear_on = false;
                page.visual_rows = None;
                true
            }
            Action::Up | Action::Down => {
                page.gear_on = false;
                page.visual_rows = None;
                false
            }
            Action::Select if page.gear_on => {
                self.choose(gear);
                true
            }
            _ => false,
        }
    }
    pub(in crate::app) fn set_music_moving(&mut self, moving: bool) {
        self.music_moving = moving;
    }
    pub(in crate::app) fn set_recent_searches(&mut self, searches: RecentSearches) {
        self.recent_searches = searches;
    }
    fn remember_search(&mut self, provider: Provider, term: &str) {
        let Some(screen) = recent_screen(provider) else {
            return;
        };
        if self.recent_searches.record(screen, term, unix_now()) {
            self.effects
                .push(Effect::SaveRecentSearches(self.recent_searches.clone()));
        }
    }
    /// After a removal: save, and redraw the search page that lists them.
    fn recent_searches_changed(&mut self) {
        self.effects
            .push(Effect::SaveRecentSearches(self.recent_searches.clone()));
        let rows = match self.pages.last().map(|page| page.role) {
            Some(PageRole::Search(provider, false)) => self.search_rows(provider, false),
            Some(PageRole::PodcastHome) => self.podcast_home_rows(),
            _ => return,
        };
        if let Some(page) = self.pages.last_mut() {
            page.rows = rows;
            page.selected = page.selected.min(page.rows.len().saturating_sub(1));
            page.visual_rows = None;
        }
        self.invalidate_visual_rows();
    }
    fn search(&mut self, provider: Provider, query: String, page: u32, metadata: bool) {
        self.search_on_page(provider, query, page, metadata, None);
    }
    fn search_on_page(
        &mut self,
        provider: Provider,
        query: String,
        page: u32,
        metadata: bool,
        parent: Option<u64>,
    ) {
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
            state.generation = generation;
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
                generation,
            });
        }
        let rows = self.search_rows(provider, metadata);
        if let Some(owner) = parent {
            self.update_search_page(owner, provider, metadata, false);
        } else if self.pages.last().is_some_and(|p| {
            p.role == PageRole::Search(provider, metadata)
                || provider == Provider::Podcast && p.role == PageRole::PodcastHome
        }) {
            self.replace(provider.name(), rows);
        } else {
            self.push(provider.name(), rows);
        }
        if parent.is_none() {
            self.pages.last_mut().unwrap().role = PageRole::Search(provider, metadata);
        }
        let task = Task::Search {
            provider,
            query: query.clone(),
            page,
        };
        let purpose = Purpose::Search {
            provider,
            query,
            metadata,
            generation,
        };
        if let Some(owner) = parent {
            let prepared = self.prepare_foreground_row();
            match self.services.submit(task) {
                Ok(handle) => {
                    let id = handle.id;
                    self.claim_foreground(id, owner, purpose.clone());
                    self.jobs.insert(id, (handle, purpose));
                    self.job_pages.insert(id, owner);
                }
                Err(error) => {
                    self.clear_prepared_foreground_row(prepared);
                    self.effects.push(Effect::Notice(error));
                }
            }
        } else {
            self.task(task, purpose);
        }
    }
    fn youtube_search_mode_available(&self, provider: Provider, metadata: bool) -> bool {
        !metadata
            && matches!(provider, Provider::YouTube | Provider::YouTubePlaylists)
            && self.videos.watch_depth.is_none()
    }
    fn seed_youtube_query(&mut self, provider: Provider, query: &str) {
        if let Some(state) = self
            .searches
            .iter_mut()
            .find(|state| state.provider == provider && !state.metadata)
        {
            if state.query != query {
                state.items.clear();
                state.total = 0;
                state.error.clear();
            }
            state.query = query.into();
            state.busy = false;
            state.generation = self.generation;
        } else {
            self.searches.push(SearchState {
                provider,
                metadata: false,
                query: query.into(),
                items: Vec::new(),
                total: 0,
                page: 1,
                error: String::new(),
                busy: false,
                generation: self.generation,
            });
        }
    }
    fn youtube_search_settings(&mut self, provider: Provider) {
        if !self.youtube_search_mode_available(provider, false) {
            return;
        }
        let Some(page) = self
            .pages
            .last()
            .filter(|page| page.role == PageRole::Search(provider, false))
        else {
            return;
        };
        let owner = page.key;
        let query = self
            .searches
            .iter()
            .find(|state| state.provider == provider && !state.metadata)
            .map_or_else(String::new, |state| state.query.clone());
        let rows = Self::youtube_search_settings_rows(provider, owner, &query);
        self.push("Search Settings", rows);
        let page = self.pages.last_mut().unwrap();
        page.role = PageRole::YouTubeSearchSettings(owner);
        page.selected = usize::from(provider == Provider::YouTubePlaylists);
    }
    fn youtube_search_settings_rows(provider: Provider, owner: u64, query: &str) -> Vec<Item> {
        [
            (
                Provider::YouTube,
                "Videos",
                "Songs, live sets, albums in one video",
            ),
            (
                Provider::YouTubePlaylists,
                "Playlists & Albums",
                "Each video a song of the album",
            ),
        ]
        .into_iter()
        .map(|(mode, label, description)| {
            let mut row = Item::new(
                label,
                Choice::YouTubeSearchMode {
                    provider: mode,
                    owner,
                    query: query.into(),
                },
            );
            row.detail = description.into();
            row.visual.checked = mode == provider;
            row.visual.no_arrow = true;
            row
        })
        .collect()
    }
    fn set_youtube_search_mode(&mut self, provider: Provider, owner: u64, query: String) {
        if !self.youtube_search_mode_available(provider, false)
            || !self
                .pages
                .last()
                .is_some_and(|page| page.role == PageRole::YouTubeSearchSettings(owner))
            || !self.pages.iter().any(|page| {
                page.key == owner
                    && matches!(
                        page.role,
                        PageRole::Search(Provider::YouTube | Provider::YouTubePlaylists, false)
                    )
            })
        {
            return;
        }
        let same_mode = self
            .pages
            .iter()
            .any(|page| page.key == owner && page.role == PageRole::Search(provider, false));
        if same_mode
            && self.searches.iter().any(|state| {
                state.provider == provider
                    && !state.metadata
                    && !state.items.is_empty()
                    && state.error.is_empty()
            })
        {
            return;
        }
        self.cancel_browse_jobs();
        self.media_queue.clear();
        self.seed_youtube_query(provider, &query);
        let rows = Self::youtube_search_settings_rows(provider, owner, &query);
        let settings = self.pages.last_mut().unwrap();
        settings.rows = rows;
        settings.visual_rows = None;
        if query.trim().is_empty() {
            self.generation = self.generation.wrapping_add(1);
            self.update_search_page(owner, provider, false, false);
        } else {
            self.search_on_page(provider, query.clone(), 1, false, Some(owner));
        }
    }
    fn owns_search_result(
        &self,
        provider: Provider,
        metadata: bool,
        generation: u64,
        owner: Option<u64>,
    ) -> bool {
        generation == self.generation && self.pages.last().is_some_and(|page| {
            Some(page.key) == owner && page.role == PageRole::Search(provider, metadata)
                || matches!(page.role, PageRole::YouTubeSearchSettings(parent) if Some(parent) == owner)
                    && self.pages.iter().any(|parent| Some(parent.key) == owner && parent.role == PageRole::Search(provider, metadata))
        })
    }
    fn update_search_page(
        &mut self,
        owner: u64,
        provider: Provider,
        metadata: bool,
        select_result: bool,
    ) {
        let rows = self.search_rows(provider, metadata);
        let Some(page) = self.pages.iter_mut().find(|page| page.key == owner) else {
            return;
        };
        page.title = provider.name().into();
        page.role = PageRole::Search(provider, metadata);
        page.rows = rows;
        page.visual_rows = None;
        page.selected = page.selected.min(page.rows.len().saturating_sub(1));
        if select_result {
            if let Some(index) = page.rows.iter().position(|row| {
                matches!(
                    row.choice,
                    Choice::Online(_, _)
                        | Choice::Video(VideoAction::Watch(_))
                        | Choice::UseCover(_)
                )
            }) {
                page.selected = index;
                page.gear_on = false;
            }
        }
        Self::reveal(page, self.viewport_height);
    }
    fn choose(&mut self, choice: Choice) {
        match choice {
            Choice::Video(action) => self.video_action(action),
            Choice::ShowDownload(id) => self.show_download(id),
            Choice::CancelDownload(id) => {
                if let Some((handle, Purpose::Download)) = self.jobs.get(&id) {
                    handle.cancel();
                    if let Some(state) = self.download_states.get_mut(&id) {
                        state.request_cancel();
                    }
                    self.refresh_download_pages();
                }
            }
            Choice::Dismiss => {
                self.pages.pop();
            }
            Choice::RemoveDownload(id) => self.remove_download_history(&[id]),
            Choice::ClearDownloads => {
                self.push_confirmation(
                    if self.active_download_count() > 0 {
                        "Clear list and cancel all? Files stay"
                    } else {
                        "Clear list? Saved files stay"
                    },
                    "",
                    "Clear List",
                    Choice::ConfirmClearDownloads,
                );
            }
            Choice::ConfirmClearDownloads => {
                let ids: Vec<_> = self.download_states.keys().copied().collect();
                self.remove_download_history(&ids);
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
                let purpose = if matches!(task, Task::RemoveTools | Task::Tools) {
                    Purpose::ToolAction
                } else {
                    Purpose::Tags
                };
                self.task(task, purpose);
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
                self.keyboard.as_mut().unwrap().0.search = false;
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
            Choice::CombinedRetry => {
                let query = self.combined.as_ref().map(|state| state.query.clone());
                self.run_combined(query);
            }
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
                page.role = PageRole::PlaylistSongs;
                page.playlist_path = Some(playlist.path.clone());
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
            Choice::YouTubeSearchSettings(provider) => self.youtube_search_settings(provider),
            Choice::YouTubeSearchMode {
                provider,
                owner,
                query,
            } => self.set_youtube_search_mode(provider, owner, query),
            Choice::YouTubePlaylistMedia(item, video) => {
                let mut queue = Self::youtube_playlist_entries(&item);
                for entry in &mut queue {
                    entry.raw["_video_auto_next"] = serde_json::json!(video);
                }
                if let Some(first) = queue.first().cloned() {
                    self.resolve_media(first, queue, 0, video, 0.);
                }
            }
            Choice::RecentSearch(provider, term) => {
                self.remember_search(provider, &term);
                self.search(provider, term, 1, false);
            }
            Choice::RemoveRecentSearch(screen, term) => {
                if self.recent_searches.remove(screen, &term) {
                    self.recent_searches_changed();
                }
            }
            Choice::ClearRecentSearches(screen) => {
                if self.recent_searches.clear(screen) {
                    self.recent_searches_changed();
                }
            }
            Choice::ClearSearch(provider) => {
                self.generation = self.generation.wrapping_add(1);
                self.cancel_browse_jobs();
                self.searches.retain(|s| s.provider != provider);
                let rows = if provider == Provider::Podcast {
                    self.podcast_home_rows()
                } else {
                    self.search_rows(provider, false)
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
                        listen_ui::RowTap::Retry => {
                            self.start_inline_listen(item, queue.as_ref().clone(), index)
                        }
                        listen_ui::RowTap::Toggle => self.effects.push(Effect::TogglePreview),
                        listen_ui::RowTap::Ignore => {}
                    }
                } else {
                    self.start_inline_listen(item, queue.as_ref().clone(), index);
                }
            }
            Choice::ArchiveTrack(item, format, file) => {
                self.stop_preview();
                let mut one = item.as_ref().clone();
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
                self.start_external_bluetooth(BtRequest::Forget(address));
            }
            Choice::ConfirmAction(choice) => self.choose(*choice),
            Choice::ToggleYouTube => {
                self.youtube = !self.youtube;
                self.effects.push(Effect::YouTubeEnabled(self.youtube));
                if self.youtube && !self.tools_state.ready {
                    self.task(Task::Tools, Purpose::ToolAction);
                }
                let rows = self.tools_rows();
                self.replace("YouTube", rows);
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
                    self.task(
                        Task::Details(item.clone()),
                        Purpose::Details {
                            metadata,
                            generation: self.generation,
                        },
                    );
                }
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
                    && (item.raw["_video_auto_next"].as_bool() == Some(true)
                        || self.detail_item.as_ref().is_some_and(|detail| {
                            detail.url == url
                                && (detail.kind == "playlist"
                                    || detail.raw["entries"]
                                        .as_array()
                                        .is_some_and(|entries| !entries.is_empty()))
                        }));
                item.raw["_video_auto_next"] = serde_json::json!(auto_next);
                for entry in &mut queue {
                    entry.raw["_video_auto_next"] = serde_json::json!(auto_next);
                }
                self.resolve_media(item, queue, index, video, 0.0);
            }
            Choice::Feed(podcast) => {
                self.active_feed = None;
                self.task(
                    Task::Feed(podcast),
                    Purpose::Feed {
                        generation: self.generation,
                    },
                );
            }
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
            Choice::FindTrackList(video) => self.open_track_list_search(video),
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
                self.start_external_bluetooth(BtRequest::Power(on));
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
                self.start_external_bluetooth(if connected {
                    BtRequest::Disconnect(address)
                } else {
                    BtRequest::Connect(address)
                });
            }
            Choice::BtConnect(address) => {
                self.start_external_bluetooth(BtRequest::Connect(address));
            }
            Choice::BtDisconnect(address) => {
                self.start_external_bluetooth(BtRequest::Disconnect(address));
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
            Choice::YouTubeCover(item) => {
                let mut rows = [Provider::Itunes, Provider::Deezer, Provider::YouTube]
                    .into_iter()
                    .map(|provider| {
                        Item::new(
                            format!("Search {}", provider.name()),
                            Choice::YouTubeCoverSource(item.clone(), provider),
                        )
                    })
                    .collect::<Vec<_>>();
                rows.push(Item::new(
                    "Use Original Thumbnail",
                    Choice::YouTubeOriginalCover(item.url),
                ));
                self.push_sheet("Cover", rows);
            }
            Choice::YouTubeCoverSource(item, provider) => {
                if let Some(page) = self.pages.last() {
                    self.youtube_cover_target = Some((item.url, page.key));
                    self.cover_search = true;
                    self.cover_override = false;
                    self.search(provider, format!("{} {}", item.artist, item.title), 1, true);
                }
            }
            Choice::YouTubeOriginalCover(origin) => {
                self.youtube_covers.remove(&origin);
                self.refresh_youtube_cover_page(&origin);
            }
            Choice::UseCover(item) => {
                if let Some((origin, owner)) =
                    self.youtube_cover_target.clone().filter(|(_, owner)| {
                        self.pages
                            .iter()
                            .any(|p| p.key == *owner && p.role == PageRole::YouTubeItem)
                    })
                {
                    if item.cover.is_empty() {
                        self.effects
                            .push(Effect::Notice("No cover available".into()));
                    } else {
                        self.task(
                            Task::CacheArtwork { url: item.cover },
                            Purpose::YouTubeCover { origin, owner },
                        );
                    }
                    return;
                }
                if self.cover_override && !item.cover.is_empty() {
                    self.task(
                        if matches!(
                            item.provider,
                            Provider::YouTube | Provider::YouTubePlaylists
                        ) {
                            Task::SetYouTubeCover {
                                paths: self.edit_paths.clone(),
                                url: item.cover,
                            }
                        } else {
                            Task::SetCover {
                                paths: self.edit_paths.clone(),
                                cover_url: item.cover,
                            }
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
                self.start_external_weather(
                    WeatherCommand::Search(self.weather_query.clone()),
                    ExternalKind::WeatherSearch,
                );
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
                        self.start_external_weather(
                            WeatherCommand::Refresh(index),
                            ExternalKind::Forecast { index, open: true },
                        );
                    } else {
                        self.forecast = Some(index);
                        self.forecast_day = 0;
                        self.push("Weather", Vec::new());
                        self.forecast_depth = self.pages.len();
                    }
                }
            }
            Choice::WeatherAdd(place) => {
                self.start_external_weather(
                    WeatherCommand::Add(place.clone()),
                    ExternalKind::WeatherAdd(place),
                );
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
            Choice::ShareCancel => {
                self.share.cancel();
                self.status = "Transfer cancelled".into();
            }
            Choice::Cancel => {
                if let Some((handle, _)) = self.jobs.values().next() {
                    handle.cancel();
                }
            }
            Choice::Tools => self.task(Task::Tools, Purpose::ToolAction),
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
                if matches!(self.keyboard.as_ref(), Some((keyboard, Typing::NewPlaylist)) if keyboard.query.trim().is_empty())
                {
                    self.keyboard.as_mut().unwrap().0.show(self.visual_time);
                    self.effects
                        .push(Effect::Notice("Enter a playlist name".into()));
                    return;
                }
                if self.pending_share.is_some()
                    && matches!(self.keyboard.as_ref(),Some((keyboard,Typing::SharingName))if keyboard.query.trim().is_empty())
                {
                    self.keyboard.as_mut().unwrap().0.show(self.visual_time);
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
            Typing::Search(provider, metadata) => {
                if !metadata {
                    self.remember_search(provider, &query);
                }
                self.search(provider, query, 1, metadata)
            }
            Typing::Weather => {
                self.weather_query = query.clone();
                self.weather_loading = true;
                self.weather_error.clear();
                self.weather_results = None;
                self.start_external_weather(
                    WeatherCommand::Search(query),
                    ExternalKind::WeatherSearch,
                );
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
                return self.youtube_details_rows(item)
            }
            _ => {}
        }
        rows
    }
    // Go's ytItemView: poster, Listen, Watch, Cover, download actions, tracks.
    fn youtube_details_rows(&self, item: &OnlineItem) -> Vec<Item> {
        let playlist = item.kind == "playlist"
            || item.raw["entries"]
                .as_array()
                .is_some_and(|entries| !entries.is_empty());
        let entries = Arc::new(if playlist {
            Self::youtube_playlist_entries(item)
        } else {
            vec![item.clone()]
        });
        let cover = self.previews.get(&item.cover).and_then(Clone::clone);
        let mut poster = visual::fact(&item.title, "");
        poster.visual.thumbnail = true;
        poster.cover = cover.clone();
        let mut subtitle = vec![item.artist.clone()];
        if playlist {
            subtitle.push(format!("{} videos", entries.len()));
        } else if item.seconds > 0.0 {
            subtitle.push(super::time_label(item.seconds));
        }
        poster.detail = subtitle
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let mut rows = vec![poster];
        if let Some(first) = entries.first() {
            let mut listen = Item::new(
                "Listen",
                Choice::Preview(first.clone(), Arc::clone(&entries), 0),
            );
            listen.detail = if entries.len() > 1 {
                format!("{} songs, before you download", entries.len())
            } else {
                "Before you download".into()
            };
            listen.visual.no_arrow = true;
            rows.push(listen);
            let mut watch = if playlist {
                Item::new(
                    "Watch the Videos",
                    Choice::YouTubePlaylistMedia(item.clone(), true),
                )
            } else {
                Item::new(
                    "Watch Video",
                    Choice::Media(item.url.clone(), item.title.clone(), true),
                )
            };
            watch.detail = if playlist {
                "In the player, one after another"
            } else {
                "In the player"
            }
            .into();
            if !playlist {
                watch.options.push(Item::new(
                    "Download",
                    Choice::Video(VideoAction::Download(item.clone())),
                ));
            }
            rows.push(watch);
        }
        let cover_pending = !self.previews.contains_key(&item.cover)
            || self
                .jobs
                .values()
                .any(|(_, purpose)| matches!(purpose, Purpose::Preview(url) if url == &item.cover));
        {
            let mut artwork = Item::new("Cover", Choice::YouTubeCover(item.clone()));
            artwork.visual.thumbnail = true;
            artwork.cover = self.youtube_covers.get(&item.url).cloned().or(cover);
            artwork.detail = if artwork.cover.is_some() {
                if self.youtube_covers.contains_key(&item.url) {
                    "Selected cover · [A] Change"
                } else {
                    "Automatic · [A] Change"
                }
            } else if !item.cover.is_empty() && cover_pending {
                "Loading the picture…"
            } else {
                "Picture unavailable"
            }
            .into();
            rows.push(artwork);
        }
        let identity = format!(
            "{} — {}",
            item.artist,
            item.title.trim_start_matches("Album - ")
        );
        if playlist {
            let mut download = Item::new(
                format!(
                    "Download as Album · {} {}",
                    entries.len(),
                    if entries.len() == 1 { "song" } else { "songs" }
                ),
                Choice::Download(DownloadRequest::YouTube {
                    url: item.url.clone(),
                    video: false,
                    split_chapters: false,
                    track_list: None,
                }),
            );
            download.detail = identity;
            rows.push(download);
            for (index, child) in entries.iter().enumerate() {
                let mut row = Item::new(
                    &child.title,
                    Choice::Preview(child.clone(), Arc::clone(&entries), index),
                );
                row.visual.no_arrow = true;
                if child.seconds > 0.0 {
                    row.visual.value = super::time_label(child.seconds);
                }
                row.options.push(Item::new(
                    "Watch Video",
                    Choice::Media(child.url.clone(), child.title.clone(), true),
                ));
                rows.push(row);
            }
            return rows;
        }
        let chapters = item.raw["chapters"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if chapters.len() >= 2 {
            let mut download = Item::new(
                format!("Download as Album · {} tracks", chapters.len()),
                Choice::Download(DownloadRequest::YouTube {
                    url: item.url.clone(),
                    video: false,
                    split_chapters: true,
                    track_list: None,
                }),
            );
            download.detail = format!("A track per chapter · {identity}");
            rows.push(download);
        }
        // A whole album in one video: cut it at the pauses between songs,
        // using a track list from Find Info Online.
        if item.seconds == 0.0 || item.seconds >= 8.0 * 60.0 {
            let mut split = Item::new(
                "Download as Album · Find Track List",
                Choice::FindTrackList(item.clone()),
            );
            split.detail = "Split at the pauses between songs".into();
            rows.push(split);
        }
        let mut one = Item::new(
            "Download as One Song",
            Choice::Download(DownloadRequest::YouTube {
                url: item.url.clone(),
                video: false,
                split_chapters: false,
                track_list: None,
            }),
        );
        one.detail = if item.seconds > 0.0 {
            format!("{identity} · {}", super::time_label(item.seconds))
        } else {
            identity
        };
        rows.push(one);
        if chapters.len() >= 2 {
            for chapter in chapters {
                let mut row = visual::fact(chapter["title"].as_str().unwrap_or(""), "");
                let seconds = betterpod_rust::online::number(chapter, "end_time")
                    - betterpod_rust::online::number(chapter, "start_time");
                if seconds > 0.0 {
                    row.visual.value = super::time_label(seconds);
                }
                rows.push(row);
            }
        }
        rows
    }
    fn youtube_playlist_entries(item: &OnlineItem) -> Vec<OnlineItem> {
        item.raw["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| {
                !entry["id"].as_str().unwrap_or("").is_empty()
                    && !matches!(
                        entry["availability"].as_str().unwrap_or(""),
                        "private" | "needs_auth" | "subscriber_only" | "premium_only"
                    )
                    && !matches!(
                        entry["title"].as_str().unwrap_or(""),
                        "[Deleted video]" | "[Private video]"
                    )
            })
            .map(|entry| betterpod_rust::online::youtube_item(entry.clone(), Provider::YouTube))
            .filter(|entry| !entry.url.is_empty() && entry.kind != "playlist")
            .collect()
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
            let Some(reply) = self.weather.poll_owned() else {
                break;
            };
            self.external_weather_reply(reply);
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
                        done.saturating_mul(100).checked_div(total).unwrap_or(0),
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
                    self.apply_playlist_listing(lists);
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
                // the Go app's messages
                playlists::Event::Appended { name, added } => {
                    self.playlist_command(playlists::Command::List);
                    self.effects.push(Effect::Notice(match added {
                        0 => format!("Already in {name}"),
                        1 => format!("Added to {name}"),
                        n => format!("{n} songs added to {name}"),
                    }));
                }
                playlists::Event::Error(error) => self.effects.push(Effect::Notice(error)),
            }
        }
        while let Some(reply) = self.bluetooth.poll_owned() {
            self.external_bluetooth_reply(reply);
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
                    self.apply_service_progress(id, done, total, status);
                }
                Event::Finished { id, result } => {
                    let purpose = self.jobs.remove(&id).map(|(_, p)| p);
                    let owner = self.job_pages.remove(&id);
                    if let Err(error) = &result {
                        self.foreground_error(id, error);
                    }
                    if purpose.as_ref().is_some_and(|purpose| {
                        !self.complete_foreground_job(id, owner, purpose, result.is_err())
                    }) {
                        continue;
                    }
                    let owns_page =
                        owner.is_some() && owner == self.pages.last().map(|page| page.key);
                    if matches!(purpose, Some(Purpose::Download)) {
                        self.download_finished(id, &result);
                    }
                    if self.status_job == Some(id) {
                        self.status.clear();
                        self.status_job = None;
                        self.status_page = None;
                    }
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
                        if self
                            .pages
                            .last()
                            .is_some_and(|page| page.role == PageRole::YouTubeItem)
                        {
                            if let Some(item) = self
                                .detail_item
                                .as_ref()
                                .filter(|item| item.cover == *url)
                                .cloned()
                            {
                                let rows = self.youtube_details_rows(&item);
                                if let Some(page) = self.pages.last_mut() {
                                    page.rows = rows;
                                    page.selected =
                                        page.selected.min(page.rows.len().saturating_sub(1));
                                    page.visual_rows = None;
                                }
                            }
                        }
                        if self.combined.as_ref().is_some_and(|state| state.cover) {
                            self.refresh_combined();
                        }
                        continue;
                    }
                    match result {
                        Err(error) => {
                            if matches!(
                                purpose,
                                Some(Purpose::Details { .. } | Purpose::Feed { .. })
                            ) {
                                continue;
                            }
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
                                if self.owns_search_result(provider, metadata, generation, owner) {
                                    if let Some(state) = self
                                        .searches
                                        .iter_mut()
                                        .find(|s| s.provider == provider && s.metadata == metadata)
                                    {
                                        state.busy = false;
                                        state.error = error;
                                    }
                                    self.update_search_page(
                                        owner.unwrap(),
                                        provider,
                                        metadata,
                                        false,
                                    );
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
                                            if (!state.cover || !item.cover.is_empty())
                                                && !state.hits.iter().any(|old| {
                                                    old.provider == item.provider
                                                        && old.id == item.id
                                                })
                                            {
                                                state.hits.push(item);
                                            }
                                        }
                                        let cover = state.cover;
                                        let query = state.query.clone();
                                        state.hits.sort_by_key(|item| {
                                            let best =
                                                item.raw["_cover_best"].as_bool().unwrap_or(false);
                                            let exact =
                                                item.raw["_cover_exact"].as_bool().unwrap_or(false);
                                            (
                                                !best && cover,
                                                cover && best && !exact,
                                                // Information results naming every searched
                                                // word come first, whatever their source.
                                                std::cmp::Reverse(if cover {
                                                    0
                                                } else {
                                                    visual::matched_words(&query, item)
                                                }),
                                                visual::combined_providers(cover)
                                                    .iter()
                                                    .position(|p| *p == item.provider)
                                                    .unwrap_or(usize::MAX),
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

                                if let Some(Purpose::Search {
                                    provider,
                                    metadata,
                                    generation,
                                    ..
                                }) = purpose
                                {
                                    if !self
                                        .owns_search_result(provider, metadata, generation, owner)
                                    {
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
                                    self.update_search_page(
                                        owner.unwrap(),
                                        provider,
                                        metadata,
                                        true,
                                    );
                                }
                            }
                            Output::Item(item) => {
                                if let Some(Purpose::Details { generation, .. }) = &purpose {
                                    if *generation != self.generation || !owns_page {
                                        continue;
                                    }
                                }

                                self.detail_item = Some(item.clone());
                                if let Some(entries) = item.raw["entries"].as_array() {
                                    self.media_queue = entries
                                        .iter()
                                        .map(|entry| {
                                            let mut child = betterpod_rust::online::youtube_item(
                                                entry.clone(),
                                                Provider::YouTube,
                                            );
                                            child.raw["_video_auto_next"] = serde_json::json!(true);
                                            child
                                        })
                                        .collect();
                                }

                                let metadata = matches!(
                                    purpose,
                                    Some(Purpose::Details { metadata: true, .. })
                                );
                                let youtube = !metadata
                                    && matches!(
                                        item.provider,
                                        Provider::YouTube | Provider::YouTubePlaylists
                                    );
                                if youtube {
                                    self.request_preview(item.cover.clone());
                                }
                                let title = if youtube {
                                    "YouTube"
                                } else if item.provider == Provider::Archive && !metadata {
                                    item.provider.name()
                                } else {
                                    &item.title
                                };
                                if let Some(video) = self.track_list_video().filter(|_| metadata) {
                                    let rows = Self::track_list_rows(&video, &item);
                                    self.push(
                                        if item.title.is_empty() {
                                            "Track List"
                                        } else {
                                            &item.title
                                        },
                                        rows,
                                    );
                                    continue;
                                }
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
                                    self.pages.last_mut().unwrap().resource = Some(item.id.clone());
                                    self.pages.last_mut().unwrap().selected =
                                        if self.pages.last().unwrap().rows.len() > 2 {
                                            2
                                        } else {
                                            1
                                        };
                                    self.refresh_archive_status();
                                }
                                if youtube {
                                    let page = self.pages.last_mut().unwrap();
                                    page.role = PageRole::YouTubeItem;
                                    page.resource = Some(item.url.clone());
                                    page.selected = 1.min(page.rows.len().saturating_sub(1));
                                }
                            }
                            Output::Feed { podcast, episodes } => {
                                if let Some(Purpose::Feed { generation, .. }) = &purpose {
                                    if *generation != self.generation || !owns_page {
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
                                if let Some(Purpose::YouTubeCover { origin, owner }) =
                                    purpose.as_ref()
                                {
                                    if let Some(index) =
                                        self.pages.iter().position(|p| p.key == *owner)
                                    {
                                        if let Some(path) = files.first() {
                                            self.youtube_covers
                                                .insert(origin.clone(), path.clone());
                                            self.pages.truncate(index + 1);
                                            self.youtube_cover_target = None;
                                            self.refresh_youtube_cover_page(origin);
                                            self.effects
                                                .push(Effect::Notice("Cover chosen".into()));
                                        }
                                    }
                                    continue;
                                }
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
                                    if !owns_page
                                        || !self
                                            .pages
                                            .last()
                                            .is_some_and(|p| p.role == PageRole::DownloadedFiles)
                                    {
                                        continue;
                                    }
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
                                    self.pages.last_mut().unwrap().role = PageRole::DownloadedFiles;
                                } else {
                                    if !matches!(purpose, Some(Purpose::Download)) {
                                        self.effects.push(Effect::Notice(format!(
                                            "Downloaded {} files",
                                            files.len()
                                        )));
                                    }
                                    if !matches!(purpose, Some(Purpose::Download)) {
                                        self.effects.push(Effect::Rescan);
                                    }
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
                                    .is_some_and(|page| page.role == PageRole::Tools)
                                {
                                    let rows = self.tools_rows();
                                    self.replace("YouTube", rows);
                                    self.pages.last_mut().unwrap().role = PageRole::Tools;
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
        if self
            .pages
            .last()
            .is_some_and(|page| page.role == PageRole::YouTubeItem)
        {
            if let Some(item) = self.detail_item.as_ref() {
                self.request_preview(item.cover.clone());
            }
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
            view.draw(r, assets, art, elapsed, dark);
            return;
        }
        if self.draw_download_view(r, assets, dark, elapsed) {
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
        | Choice::ApplyMetadata(i) => &i.cover,
        Choice::Feed(p) | Choice::Unsubscribe(p) => {
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

#[cfg(test)]
mod followup_qa;

/// The Recent Searches list a search screen keeps (Go's "ia", "youtube" and
/// "podcasts"); metadata and cover searches keep none.
fn recent_screen(provider: Provider) -> Option<SearchScreen> {
    match provider {
        Provider::Archive => Some(SearchScreen::Archive),
        Provider::YouTube | Provider::YouTubePlaylists => Some(SearchScreen::YouTube),
        Provider::Podcast => Some(SearchScreen::Podcasts),
        _ => None,
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}
