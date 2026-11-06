//! Rust application state and navigation. Drawing performs no card I/O.
#[path = "app_curation.rs"]
mod app_curation;
#[path = "app_files.rs"]
mod app_files;
#[path = "app_settings.rs"]
mod app_settings;
use app_files::{Effect as FilesEffect, Files, Mode as FilesMode};
#[cfg(test)]
#[path = "app_qa.rs"]
mod qa;
use app_settings::SettingValue;
#[path = "app_parity_model.rs"]
mod app_parity_model;
#[path = "app_queue_order.rs"]
mod app_queue_order;
#[path = "app_search.rs"]
mod app_search;
#[path = "app_visual_menu.rs"]
mod app_visual_menu;
#[path = "app_visual_player.rs"]
mod app_visual_player;
#[path = "app_visual_video.rs"]
mod app_visual_video;
use app_search::SearchResults;
use app_visual_menu::MenuVisual;
pub(crate) use app_visual_menu::RowVisual;
use app_visual_player::PlayerVisual;
use app_visual_video::VideoVisual;
#[path = "app_feedback.rs"]
mod app_feedback;
use app_feedback::Feedback;
#[path = "app_compositor.rs"]
mod app_compositor;
#[path = "app_keyboard.rs"]
mod app_keyboard;
#[path = "app_remote_qa.rs"]
mod app_remote_qa;
#[path = "app_stream_control.rs"]
mod app_stream_control;
use app_keyboard::{Keyboard, KeyboardEffect};
use betterpod_rust::visual_assets::VisualAssets;
#[path = "app_quiz.rs"]
mod app_quiz;
use app_curation::{Curation, CurationEffect, Mode as CurationMode};
#[path = "app_services.rs"]
mod app_services;
use crate::app_maintenance::{
    Controller as MaintenanceController, Effect as MaintenanceEffect, Mode as MaintenanceMode,
    Request as MaintenanceRequest,
};
use crate::art::Artwork;
use crate::audio::{AudioCommand, AudioEngine, AudioSnapshot};
use crate::games::{GameCommand, GameEffect, GameKind, Games};
use crate::input::Action;
use crate::library::{Library, Track};
use crate::platform::{IdleGuard, Platform, SystemVolume};
use crate::render::Texture;
use crate::render::{Color, Renderer};
use crate::storage::{Playback, Settings, Store};
use app_quiz::{Effect as QuizEffect, Quiz};
use app_services::{Effect as ExtraEffect, ExtraPage, Extras};
use betterpod_rust::online::StreamSource;
use betterpod_rust::puzzles::{PuzzleGame, PUZZLES};
use betterpod_rust::saver::{self, Saver, SaverAction};
use betterpod_rust::video::{VideoCommand, VideoPlayer};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BG: Color = [1.0, 1.0, 1.0, 1.0];
const FG: Color = [0.0, 0.0, 0.0, 1.0];
const MUTED: Color = [100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0, 1.0];
const WHITE: Color = [1.0; 4];
const BLUE: Color = [0.12, 0.48, 0.88, 1.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Screen {
    Home,
    Music,
    Albums,
    Artists,
    Artist(usize),
    Album(usize),
    Songs,
    Queue,
    Settings,
    Apps,
    About,
    Search,
    SearchResults(u8),
    NowPlaying,
    CoverFlow,
    Visualizer,
    Game,
    Extra,
    Puzzles,
    Puzzle,
    Video,
    GameClocks,
    GameHistory,
    Quiz,
    Curation,
    Maintenance,
    Files,
    Library,
    Screensaver,
    SearchHub,
    YouTubeSettings,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewKind {
    Split,
    Full,
    CoverFlow,
}
impl Screen {
    fn default_kind(self) -> ViewKind {
        match self {
            Self::Home
            | Self::Music
            | Self::Settings
            | Self::Apps
            | Self::SearchHub
            | Self::Puzzles => ViewKind::Split,
            Self::CoverFlow => ViewKind::CoverFlow,
            _ => ViewKind::Full,
        }
    }
}
#[derive(Clone)]
pub(crate) enum Intent {
    Open(Screen),
    Song(usize),
    QueueSong(usize),
    PlayAlbum(usize),
    ShuffleSongs,
    Genius,
    Quiz,
    Shuffle,
    Repeat,
    Theme,
    Volume,
    Rescan,
    SearchKeyboard,
    Game(GameKind),
    AddNext(usize),
    AddLater(usize),
    Stop,
    CloseOptions,
    Extra(ExtraPage),
    AddToPlaylist(usize),
    AlbumInfo(usize),
    Puzzle(usize),
    DownloadMedia(bool),
    VideoRestart,
    DownloadVideoChapters,
    VideoDownloadMenu,
    ShareTrack(usize),
    ShareAlbum(usize),
    SongInfo(usize),
    Effects,
    Apple,
    YouTube,
    Podcasts,
    SaverTime,
    SaverDim,
    StartSaver,
    SaverStyle,
    SaverEffect,
    SaverColors,
    GameClock(String),
    Curation(CurationMode),
    Maintenance(MaintenanceMode),
    AddAlbumPlaylist(usize),
    AddQueuePlaylist,
    QueueAlbum(usize, bool),
    RemoveQueue(usize),
    Playlist(betterpod_rust::playlists::Playlist),
    Unmerge(usize),
    MergeSong(usize),
    SeparateSong(usize),
    ClearUpcoming,
    AskClearUpcoming,
    AskClearNowPlaying,
    MoveQueueNext(usize),
    EndQueueMusic,
    ClearNowPlaying,
    Setting(SettingValue),
    Files(FilesMode),
    Noop,
    Quit,
    QuitColumn,
    SoftwareUpdate,
    SharingSettings,
    SharingName,
    YouTubeSettings,
    ToggleBeta,
    QuizNewGame,
    StopQuiz,
    GameCommand(GameCommand),
    PuzzleNew,
    PuzzleUndo,
    QuitPuzzle,
}
#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) cover: Option<PathBuf>,
    pub(crate) intent: Intent,
    pub(crate) arrow: bool,
    pub(crate) visual: RowVisual,
}
impl Row {
    pub(crate) fn new(label: impl Into<String>, intent: Intent) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            cover: None,
            intent,
            arrow: true,
            visual: RowVisual::default(),
        }
    }
}
#[derive(Clone)]
pub(crate) struct Page {
    pub(crate) screen: Screen,
    pub(crate) kind: ViewKind,
    pub(crate) mini: bool,
    pub(crate) title: String,
    pub(crate) rows: Arc<Vec<Row>>,
    pub(crate) selected: usize,
    pub(crate) scroll: f32,
    pub(crate) target_scroll: f32,
    pub(crate) enter: f32,
}
impl Page {
    fn split(&self) -> bool {
        self.kind == ViewKind::Split
    }
    pub(crate) fn reveal_selection(&mut self) {
        self.reveal_selection_in(440.0);
    }
    pub(crate) fn reveal_selection_in(&mut self, available: f32) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.scroll = 0.0;
            self.target_scroll = 0.0;
            return;
        }
        self.selected = self.selected.min(self.rows.len() - 1);
        if self.rows[self.selected].visual.header {
            self.selected = (self.selected + 1).min(self.rows.len() - 1);
        }
        let top = self
            .rows
            .iter()
            .take(self.selected)
            .map(app_visual_menu::row_height)
            .sum::<f32>();
        let height = app_visual_menu::row_height(&self.rows[self.selected]);
        // Every list owns440px below its header; designated views reserve116
        // for their mini-player at draw time, where exact ensureVisible is applied.
        let available = available.clamp(1.0, 440.0);
        if top < self.target_scroll {
            self.target_scroll = top;
        } else if top + height > self.target_scroll + available {
            self.target_scroll = top + height - available;
        }
        let total = self
            .rows
            .iter()
            .map(app_visual_menu::row_height)
            .sum::<f32>();
        self.target_scroll = self.target_scroll.clamp(0.0, (total - available).max(0.0));
        self.scroll = self.target_scroll;
    }
}
#[derive(Clone)]
struct SavedPlayback {
    queue: Arc<Vec<PathBuf>>,
    index: usize,
    position: f64,
    paused: bool,
}
struct ScannedLibrary {
    library: Library,
    by_path: HashMap<PathBuf, usize>,
    songs: Arc<Vec<Row>>,
    albums: Arc<Vec<Row>>,
    artists: Arc<Vec<Row>>,
    album_tracks: Vec<Arc<Vec<Row>>>,
    artist_albums: Vec<Arc<Vec<Row>>>,
}
impl ScannedLibrary {
    fn new(library: Library) -> Self {
        let songs = Arc::new(
            library
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let mut row = Row::new(&t.title, Intent::Song(i));
                    row.detail = format!("{} • {}", t.artist, t.album);
                    row.cover = t.cover.clone();
                    row.visual.thumbnail = true;
                    row.arrow = false;
                    row
                })
                .collect::<Vec<_>>(),
        );
        let albums = Arc::new(
            library
                .albums
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let mut row = Row::new(&a.name, Intent::Open(Screen::Album(i)));
                    row.detail = a.artist.clone();
                    row.cover = a.cover.clone();
                    row.visual.thumbnail = true;
                    row
                })
                .collect::<Vec<_>>(),
        );
        let artists = Arc::new(
            library
                .artists
                .iter()
                .enumerate()
                .map(|(i, a)| Row::new(a, Intent::Open(Screen::Artist(i))))
                .collect(),
        );
        let album_tracks = library
            .albums
            .iter()
            .map(|a| {
                let mut header = Row::new(
                    if a.artist.is_empty() {
                        a.name.clone()
                    } else {
                        format!("{} · {}", a.name, a.artist)
                    },
                    Intent::Noop,
                );
                header.visual.header = true;
                header.arrow = false;
                let mut rows = vec![header];
                for &i in &a.track_indices {
                    let mut row = Row::new(&library.tracks[i].title, Intent::Song(i));
                    row.arrow = false;
                    rows.push(row);
                }
                Arc::new(rows)
            })
            .collect();
        let mut by_artist: HashMap<&str, Vec<Row>> = HashMap::new();
        for (i, a) in library.albums.iter().enumerate() {
            by_artist
                .entry(&a.artist)
                .or_default()
                .push(albums[i].clone());
        }
        let artist_albums = library
            .artists
            .iter()
            .map(|a| {
                let mut header = Row::new(a, Intent::Noop);
                header.visual.header = true;
                header.arrow = false;
                let mut rows = vec![header];
                rows.extend(by_artist.remove(a.as_str()).unwrap_or_default());
                Arc::new(rows)
            })
            .collect();
        let by_path = library
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.path.clone(), i))
            .collect();
        Self {
            library,
            by_path,
            songs,
            albums,
            artists,
            album_tracks,
            artist_albums,
        }
    }
}
#[derive(Clone, Copy)]
enum KeyboardContext {
    Library,
    Curation,
    Sharing,
}
struct Toast {
    text: String,
    until: Instant,
}

pub struct App {
    pub running: bool,
    pub user_quit: bool,
    pub screen_off: bool,
    pub phase: &'static str,
    pub audio_snapshot: AudioSnapshot,
    pub maintenance_request: Option<MaintenanceRequest>,
    maintenance: Option<MaintenanceController>,
    audio: AudioEngine,
    volume: SystemVolume,
    used_system_volume: bool,
    platform: Platform,
    idle: IdleGuard,
    idle_stopped: bool,
    idle_play_pending: Option<Instant>,
    last_idle_error: Option<String>,
    art: Artwork,
    store: Store,
    extras: Extras,
    settings: Settings,
    library: Arc<Library>,
    by_path: HashMap<PathBuf, usize>,
    pages: Vec<Page>,
    selection_layout: Option<(Arc<Vec<Row>>, usize, Screen, u32)>,
    queue: Arc<Vec<PathBuf>>,
    queue_index: usize,
    queue_original: Option<Arc<Vec<PathBuf>>>,
    queued_marks: HashMap<PathBuf, String>,
    detached_next: Option<usize>,
    now_track: Option<Track>,
    restored_position: f64,
    restored: bool,
    restore: Option<Playback>,
    end_seen: u64,
    pending_audio: VecDeque<AudioCommand>,
    music_dir: PathBuf,
    data_dir: PathBuf,
    import: Option<PathBuf>,
    scan: Option<mpsc::Receiver<Result<ScannedLibrary, String>>>,
    views_songs: Arc<Vec<Row>>,
    views_albums: Arc<Vec<Row>>,
    views_artists: Arc<Vec<Row>>,
    views_album_tracks: Vec<Arc<Vec<Row>>>,
    views_artist_albums: Vec<Arc<Vec<Row>>>,
    search_rows: Arc<Vec<Row>>,
    search_views: [Arc<Vec<Row>>; 4],
    library_generation: u64,
    search_result: Option<mpsc::Receiver<(u64, String, SearchResults)>>,
    search_worker: Option<thread::JoinHandle<()>>,
    search_pending: Option<String>,
    scanning: bool,
    scan_worker: Option<thread::JoinHandle<()>>,
    library_error: Option<String>,
    cover_selected: usize,
    cover_position: f32,
    cover_card: Option<usize>,
    cover_flip: f32,
    meter: f32,
    preview_rows: Vec<Row>,
    preview_focus: bool,
    preview_selected: usize,
    np_mode: u8,
    np_option: usize,
    np_seek: Option<(f64, Instant)>,
    cover_from_np: bool,
    search_query: String,
    keyboard: Option<Keyboard>,
    keyboard_context: KeyboardContext,
    options: Option<Page>,
    game: Option<Games>,
    quiz: Option<Quiz>,
    retired_quiz: Option<Quiz>,
    retired_game: Option<Games>,
    curation: Curation,
    files: Files,
    library_info_revision: u64,
    quiz_restore: Option<SavedPlayback>,
    puzzle: Option<PuzzleGame>,
    game_saves: HashMap<String, Vec<u8>>,
    video: Option<VideoPlayer>,
    video_open: bool,
    video_visual: VideoVisual,
    video_texture: Option<Texture>,
    video_overlay: Instant,
    video_title: String,
    video_cover: Option<PathBuf>,
    video_origin: String,
    video_audio_only: bool,
    inline_preview: bool,
    pending_stream_paused: bool,
    wired_headphones: bool,
    last_bt_audio: Option<bool>,
    video_queue: Vec<betterpod_rust::online::OnlineItem>,
    video_index: usize,
    video_ended_generation: u64,
    pending_stream: Option<(StreamSource, f64, bool)>,
    stream_restore: Option<SavedPlayback>,
    pending_music: Option<AudioCommand>,
    samples: [f32; 1024],
    held: bool,
    hold_pressed: bool,
    hold_consumed: bool,
    hold_brightness: Option<i32>,
    saver: Saver,
    auto_saver: bool,
    saver_brightness: Option<i32>,
    clock: String,
    clock_updated: Instant,
    rescan_pending: bool,
    menu_down: Option<Instant>,
    menu_used: bool,
    toast: Option<Toast>,
    last_activity: Instant,
    last_saved: Instant,
    rng: u64,
    benchmark_started: Option<f64>,
    benchmark_phase: usize,
    benchmark_step: u64,
    closed: bool,
    visual_time: f64,
    paused_at: f64,
    visual_assets: VisualAssets,
    status_watcher: betterpod_rust::status::Watcher,
    visual_menu: MenuVisual,
    player_visual: PlayerVisual,
    feedback: Feedback,
}

impl App {
    pub fn new(
        music: PathBuf,
        data: PathBuf,
        assets: PathBuf,
        import: Option<PathBuf>,
        renderer: &Renderer,
    ) -> Result<Self, String> {
        let store = Store::new(data.clone())?;
        let settings = store.load_settings();
        let restore = store.load_playback();
        let game_saves = store
            .load_games_snapshot()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let extras = Extras::new(music.clone(), data.clone())?;
        let audio = AudioEngine::new()?;
        let _ = audio.command(AudioCommand::Volume(settings.volume));
        let mut platform = Platform::new();
        if let Err(error) = platform.set_performance(true) {
            eprintln!("Performance policy: {error}");
        }
        let mut app = Self {
            running: true,
            user_quit: false,
            screen_off: false,
            phase: "home",
            audio_snapshot: AudioSnapshot::default(),
            maintenance_request: None,
            maintenance: None,
            audio,
            volume: SystemVolume::new(),
            used_system_volume: false,
            platform,
            idle: IdleGuard::new(),
            idle_stopped: true,
            idle_play_pending: None,
            last_idle_error: None,
            art: Artwork::new(renderer, &assets)?,
            store,
            extras,
            settings,
            library: Arc::new(Library::default()),
            by_path: HashMap::new(),
            pages: Vec::new(),
            selection_layout: None,
            queue: Arc::new(Vec::new()),
            queue_index: 0,
            queue_original: None,
            queued_marks: HashMap::new(),
            detached_next: None,
            now_track: None,
            restored_position: 0.0,
            restored: false,
            restore,
            end_seen: 0,
            pending_audio: VecDeque::with_capacity(16),
            music_dir: music.clone(),
            data_dir: data.clone(),
            import,
            scan: None,
            views_songs: Arc::new(Vec::new()),
            views_albums: Arc::new(Vec::new()),
            views_artists: Arc::new(Vec::new()),
            views_album_tracks: Vec::new(),
            views_artist_albums: Vec::new(),
            search_rows: Arc::new(Vec::new()),
            search_views: std::array::from_fn(|_| Arc::new(Vec::new())),
            library_generation: 0,
            search_result: None,
            search_worker: None,
            search_pending: None,
            scanning: false,
            scan_worker: None,
            library_error: None,
            cover_selected: 0,
            cover_position: 0.0,
            cover_card: None,
            cover_flip: 0.0,
            meter: 0.0,
            preview_rows: Vec::new(),
            preview_focus: false,
            preview_selected: 0,
            np_mode: 0,
            np_option: 0,
            np_seek: None,
            cover_from_np: false,
            search_query: String::new(),
            keyboard: None,
            keyboard_context: KeyboardContext::Library,
            options: None,
            game: None,
            quiz: None,
            retired_quiz: None,
            retired_game: None,
            curation: Curation::new(),
            files: Files::new(music.clone(), data.clone()),
            library_info_revision: 0,
            quiz_restore: None,
            puzzle: None,
            game_saves,
            video: None,
            video_open: false,
            video_visual: VideoVisual::new(),
            video_texture: None,
            video_overlay: Instant::now(),
            video_title: String::new(),
            video_cover: None,
            video_origin: String::new(),
            video_audio_only: false,
            inline_preview: false,
            pending_stream_paused: false,
            wired_headphones: false,
            last_bt_audio: None,
            video_queue: Vec::new(),
            video_index: 0,
            video_ended_generation: u64::MAX,
            pending_stream: None,
            stream_restore: None,
            pending_music: None,
            samples: [0.0; 1024],
            held: false,
            hold_pressed: false,
            hold_consumed: false,
            hold_brightness: None,
            saver: Saver::new(),
            auto_saver: false,
            saver_brightness: None,
            clock: local_clock(),
            clock_updated: Instant::now(),
            rescan_pending: false,
            menu_down: None,
            menu_used: false,
            toast: None,
            last_activity: Instant::now(),
            last_saved: Instant::now(),
            rng: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64),
            benchmark_started: None,
            benchmark_phase: usize::MAX,
            benchmark_step: 0,
            closed: false,
            visual_time: 0.0,
            paused_at: 0.0,
            visual_assets: VisualAssets::new(renderer, &assets)?,
            status_watcher: betterpod_rust::status::Watcher::new()?,
            visual_menu: MenuVisual::new(),
            player_visual: PlayerVisual::new(),
            feedback: Feedback::new(),
        };
        app.extras.set_youtube(app.settings.youtube);
        app.extras
            .set_sharing_name(app.settings.sharing_name.clone());
        app.saver.configure_assets(&assets)?;
        app.saver.set_assets(&app.visual_assets);
        app.configure_saver(false);
        app.pages.push(app.page(Screen::Home));
        app.rescan();
        Ok(app)
    }

    fn screen(&self) -> Screen {
        self.pages.last().map_or(Screen::Home, |p| p.screen)
    }
    pub(crate) fn dark(&self) -> bool {
        if self.settings.theme == "auto" {
            #[cfg(unix)]
            unsafe {
                let now = libc::time(std::ptr::null_mut());
                let mut t: libc::tm = std::mem::zeroed();
                if !libc::localtime_r(&now, &mut t).is_null() {
                    return t.tm_hour < 7 || t.tm_hour >= 20;
                }
            }
        }
        self.settings.theme == "dark"
    }
    pub(crate) fn maintenance_library(&self) -> Arc<Library> {
        Arc::clone(&self.library)
    }
    fn colors(&self) -> (Color, Color, Color) {
        if self.dark() {
            (
                [22.0 / 255.0, 23.0 / 255.0, 25.0 / 255.0, 1.0],
                [238.0 / 255.0, 239.0 / 255.0, 241.0 / 255.0, 1.0],
                [150.0 / 255.0, 153.0 / 255.0, 158.0 / 255.0, 1.0],
            )
        } else {
            (BG, FG, MUTED)
        }
    }
    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + Duration::from_millis(1600),
        });
    }
    fn send(&mut self, command: AudioCommand) {
        match &command {
            AudioCommand::Play { .. } | AudioCommand::Resume => {
                self.idle_stopped = false;
                self.idle_play_pending = Some(Instant::now());
            }
            AudioCommand::TogglePause if !self.audio_snapshot.playing => {
                self.idle_stopped = false;
                self.idle_play_pending = Some(Instant::now());
            }
            AudioCommand::Stop => {
                self.idle_stopped = true;
                self.idle_play_pending = None;
            }
            AudioCommand::Pause => self.idle_play_pending = None,
            _ => {}
        }
        if !self.audio.command(command.clone()) {
            if self.pending_audio.len() >= 16 {
                self.pending_audio.pop_front();
            }
            self.pending_audio.push_back(command);
        }
    }
    fn rescan(&mut self) {
        if self.scanning {
            self.rescan_pending = true;
            return;
        }
        let music = self.music_dir.clone();
        let data = self.data_dir.clone();
        let import = self.import.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        match thread::Builder::new()
            .name("library-scan".into())
            .spawn(move || {
                let _ = tx.send(
                    Library::load_or_scan(&music, &data, import.as_deref())
                        .map(ScannedLibrary::new),
                );
            }) {
            Ok(worker) => {
                self.scan_worker = Some(worker);
                self.scan = Some(rx);
                self.scanning = true;
                self.library_error = None;
            }
            Err(error) => self.notify(format!("Cannot start scan: {error}")),
        }
    }
    fn install_library(&mut self, scanned: ScannedLibrary, renderer: &Renderer) {
        let ScannedLibrary {
            library,
            by_path,
            songs,
            albums,
            artists,
            album_tracks,
            artist_albums,
        } = scanned;
        let retired_views = (
            std::mem::replace(&mut self.views_songs, songs),
            std::mem::replace(&mut self.views_albums, albums),
            std::mem::replace(&mut self.views_artists, artists),
            std::mem::replace(&mut self.views_album_tracks, album_tracks),
            std::mem::replace(&mut self.views_artist_albums, artist_albums),
        );
        let previous = Arc::clone(&self.library);
        let pages = std::mem::take(&mut self.pages);
        self.art.invalidate(renderer);
        self.library = Arc::new(library);
        self.curation.set_library(Arc::clone(&self.library));
        self.extras.set_library_snapshot(Arc::clone(&self.library));
        if let Some(controller) = self.maintenance.as_mut() {
            controller.set_library(Arc::clone(&self.library));
        }
        if let Err(error) = self.files.refresh_library(Arc::clone(&self.library)) {
            self.notify(error);
        }
        let retired_index = std::mem::replace(&mut self.by_path, by_path);
        self.library_generation = self.library_generation.wrapping_add(1);
        if let Some(saved) = self.restore.take() {
            self.queue_original = saved.original_queue.map(|order| {
                Arc::new(
                    order
                        .into_iter()
                        .filter(|p| self.by_path.contains_key(p))
                        .collect::<Vec<_>>(),
                )
            });
            let current = saved.queue.get(saved.index).cloned();
            self.queue = saved
                .queue
                .into_iter()
                .filter(|p| self.by_path.contains_key(p))
                .collect::<Vec<_>>()
                .into();
            self.queue_index = current
                .and_then(|p| self.queue.iter().position(|q| *q == p))
                .unwrap_or_else(|| saved.index.min(self.queue.len().saturating_sub(1)));
            if let Some(path) = self.queue.get(self.queue_index) {
                self.now_track = self
                    .by_path
                    .get(path)
                    .map(|&i| self.library.tracks[i].clone());
                self.restored_position = saved.position;
                self.restored = self.now_track.is_some();
            }
        }
        self.cover_selected = self
            .cover_selected
            .min(self.library.albums.len().saturating_sub(1));
        self.cover_position = self.cover_selected as f32;
        if let Some(track) = self.now_track.as_ref() {
            if let Some(&i) = self.by_path.get(&track.path) {
                self.now_track = Some(self.library.tracks[i].clone());
            }
        }
        for old in &pages {
            let screen = match old.screen {
                Screen::Album(i) => previous
                    .albums
                    .get(i)
                    .and_then(|old| {
                        let key = old
                            .track_indices
                            .first()
                            .and_then(|&i| previous.tracks.get(i))
                            .map(betterpod_rust::library::album_key)?;
                        let surviving = old
                            .track_indices
                            .iter()
                            .filter_map(|&i| previous.tracks.get(i))
                            .filter_map(|track| self.by_path.get(&track.path).copied())
                            .collect::<Vec<_>>();
                        let track = surviving
                            .iter()
                            .find(|&&i| {
                                betterpod_rust::library::album_key(&self.library.tracks[i]) == key
                            })
                            .or_else(|| surviving.first())?;
                        self.library
                            .albums
                            .iter()
                            .position(|a| a.track_indices.contains(track))
                    })
                    .map(Screen::Album)
                    .unwrap_or(Screen::Albums),
                Screen::Artist(i) => previous
                    .artists
                    .get(i)
                    .and_then(|old| self.library.artists.iter().position(|a| a == old))
                    .map(Screen::Artist)
                    .unwrap_or(Screen::Artists),
                other => other,
            };
            let mut page = self.page(screen);
            page.selected = old.selected.min(page.rows.len().saturating_sub(1));
            page.scroll = old.scroll;
            page.target_scroll = old.target_scroll;
            page.enter = 0.0;
            let height = self.visual_menu_content_height(&page);
            page.reveal_selection_in(height);
            self.pages.push(page);
        }
        if self.pages.is_empty() {
            self.pages.push(self.page(Screen::Home));
        }
        let _ = thread::Builder::new()
            .name("library-retire".into())
            .spawn(move || drop((previous, pages, retired_views, retired_index)));
        if !self.search_query.is_empty() {
            self.start_search();
        }
        eprintln!(
            "Library ready: {} tracks / {} albums",
            self.library.tracks.len(),
            self.library.albums.len()
        );
    }
    fn page(&self, screen: Screen) -> Page {
        let mut rows = Vec::new();
        let mut cached: Option<Arc<Vec<Row>>> = None;
        let title: String = match screen {
            Screen::Home => {
                rows = self.reference_home_rows();
                if self.settings.apple {
                    "iPod".into()
                } else {
                    "betterPod".into()
                }
            }
            Screen::Music => {
                rows = self.reference_music_rows();
                "Music".into()
            }
            Screen::Albums => {
                cached = Some(Arc::clone(&self.views_albums));
                "Albums".into()
            }
            Screen::Artists => {
                cached = Some(Arc::clone(&self.views_artists));
                "Artists".into()
            }
            Screen::Artist(index) => {
                cached = self.views_artist_albums.get(index).cloned();
                "Artists".into()
            }
            Screen::Album(index) => {
                cached = self.views_album_tracks.get(index).cloned();
                "Albums".into()
            }
            Screen::Songs => {
                cached = Some(Arc::clone(&self.views_songs));
                "Songs".into()
            }
            Screen::Queue => {
                rows = self.reference_queue_rows();
                "Up Next".into()
            }
            Screen::Settings => {
                rows = self.reference_settings_rows();
                "Settings".into()
            }
            Screen::Apps => {
                rows = self.reference_apps_rows();
                "Apps".into()
            }
            Screen::Screensaver => {
                rows = self.reference_saver_rows();
                "Screensaver".into()
            }
            Screen::YouTubeSettings => {
                rows = self.reference_youtube_rows();
                "YouTube".into()
            }
            Screen::Library => {
                rows = self.reference_library_rows();
                "Library".into()
            }
            Screen::SearchHub => {
                rows = self.reference_search_rows();
                "Search".into()
            }
            Screen::Search => {
                if self.search_rows.is_empty() {
                    rows.push(app_search::search_field(&self.search_query));
                } else {
                    cached = Some(Arc::clone(&self.search_rows));
                }
                "Search".into()
            }
            Screen::SearchResults(category) => {
                let i = (category as usize).min(3);
                cached = Some(Arc::clone(&self.search_views[i]));
                ["Artists", "Albums", "Songs", "Playlists"][i].into()
            }
            Screen::Files => self.files.title().into(),
            Screen::Maintenance => self
                .maintenance
                .as_ref()
                .map_or("Maintenance", |m| m.title())
                .into(),
            Screen::Curation => "Library".into(),
            Screen::Quiz => "Guess the Song".into(),
            Screen::About => {
                rows = self.reference_about_rows();
                "About".into()
            }
            Screen::NowPlaying => "Now Playing".into(),
            Screen::CoverFlow => "Cover Flow".into(),
            Screen::Visualizer => "Visualizer".into(),
            Screen::Game => self.game.as_ref().map_or("Game", |g| g.title()).into(),
            Screen::Extra => self.extras.title().into(),
            Screen::Video => {
                if self.video_audio_only {
                    "Listen".into()
                } else {
                    self.video_title.clone()
                }
            }
            Screen::GameHistory => {
                if let Some(game) = self.game.as_ref() {
                    for entry in game.history_rows() {
                        let mut row = Row::new(entry.label, Intent::Noop);
                        row.detail = entry.detail;
                        row.visual.header = entry.header;
                        row.arrow = false;
                        rows.push(row);
                    }
                }
                "History".into()
            }
            Screen::GameClocks => {
                for name in ["2048", "Brick", "Solitaire", "Five Letters", "Guess Song"]
                    .into_iter()
                    .chain(PUZZLES.iter().map(|p| p.name))
                {
                    rows.push(Row::new(
                        format!(
                            "{name}: {}",
                            if self.settings.game_clock.iter().any(|n| n == name) {
                                "On"
                            } else {
                                "Off"
                            }
                        ),
                        Intent::GameClock(name.into()),
                    ));
                }
                "Time in Games".into()
            }
            Screen::Puzzles => {
                for (i, info) in PUZZLES.iter().enumerate() {
                    let mut row = Row::new(info.name, Intent::Puzzle(i));
                    row.detail = info.description.into();
                    row.visual.preview = app_visual_menu::PreviewKind::Icon {
                        asset: format!("pz_{}", info.id),
                        title: info.name.into(),
                        subtitle: String::new(),
                    };
                    rows.push(row);
                }
                "Puzzles".into()
            }
            Screen::Puzzle => self.puzzle.as_ref().map_or("Puzzle", |p| p.title()).into(),
        };
        Page {
            kind: screen.default_kind(),
            mini: matches!(
                screen,
                Screen::Home
                    | Screen::Music
                    | Screen::Apps
                    | Screen::Settings
                    | Screen::Artists
                    | Screen::Artist(_)
                    | Screen::Albums
                    | Screen::Album(_)
                    | Screen::Songs
                    | Screen::SearchResults(_)
            ),
            screen,
            title,
            rows: cached.unwrap_or_else(|| Arc::new(rows)),
            selected: if matches!(screen, Screen::Album(_) | Screen::Artist(_)) {
                1
            } else {
                0
            },
            scroll: 0.0,
            target_scroll: 0.0,
            enter: 1.0,
        }
    }
    fn open(&mut self, screen: Screen) {
        if screen == Screen::Visualizer {
            self.configure_saver(true);
        }
        if screen == Screen::CoverFlow && self.library.albums.is_empty() {
            self.notify("No albums yet");
            return;
        }
        if screen == Screen::NowPlaying && self.now_track.is_none() {
            self.notify("Choose a song first");
            return;
        }
        if screen == Screen::Queue && self.queue.is_empty() {
            self.notify("The queue is empty");
        }
        if self.screen() != screen {
            if self.screen() == Screen::Extra {
                self.extras.retain_current_visual();
            }
            let previous = self.navigation_page();
            let page = if screen == Screen::Extra {
                self.extras
                    .visual_page()
                    .unwrap_or_else(|| self.page(screen))
            } else {
                self.page(screen)
            };
            self.pages.push(page);
            self.visual_menu_navigation(previous, true);
        }
        self.preview_focus = false;
        if screen == Screen::Settings {
            self.refresh_setting_preview();
        }
        if screen == Screen::Search {
            self.keyboard_context = KeyboardContext::Library;
            self.keyboard = Some(Keyboard::new(self.search_query.clone()));
            self.start_search();
        }
    }
    /// The controller's actual page owns its panel kind, not the App wrapper.
    fn projected_page(&mut self) -> Option<Page> {
        match self.screen() {
            Screen::Extra => self.extras.visual_page(),
            Screen::Files => Some(self.files.visual_page()),
            Screen::Curation => self.curation.visual_page(),
            Screen::Maintenance => self.maintenance.as_mut().and_then(|m| m.visual_page()),
            _ => self.pages.last().cloned(),
        }
    }
    fn navigation_page(&mut self) -> Option<Page> {
        self.projected_page()
            .or_else(|| {
                self.visual_menu_active_page()
                    .filter(|p| p.screen == self.screen())
            })
            .or_else(|| self.pages.last().cloned())
    }
    /// Every exit must retarget both panels and retire the outgoing view.
    fn pop_navigation_page(&mut self) {
        if self.pages.len() <= 1 {
            return;
        }
        let previous = self.navigation_page();
        self.pages.pop();
        if let Some(current) = self.projected_page() {
            if let Some(last) = self.pages.last_mut() {
                *last = current;
            }
        }
        self.visual_menu_navigation(previous, false);
    }
    fn refresh_page(&mut self) {
        let screen = self.screen();
        let selected = self.pages.last().map_or(0, |p| p.selected);
        let scroll = self.pages.last().map_or(0.0, |p| p.scroll);
        let mut page = self.page(screen);
        page.selected = selected.min(page.rows.len().saturating_sub(1));
        page.scroll = scroll;
        page.target_scroll = scroll;
        page.enter = 0.0;
        let height = self.visual_menu_content_height(&page);
        page.reveal_selection_in(height);
        if let Some(last) = self.pages.last_mut() {
            *last = page;
        }
        if screen == Screen::Settings {
            self.refresh_setting_preview();
        }
    }
    fn save_settings(&mut self) {
        if let Err(error) = self.store.save_settings(&self.settings) {
            self.notify(error);
        }
    }
    fn save_playback(&mut self) {
        let saved = self.quiz_restore.as_ref().or(self.stream_restore.as_ref());
        let (queue, index, position, paused) = if let Some(saved) = saved {
            (
                Arc::clone(&saved.queue),
                saved.index,
                saved.position,
                saved.paused,
            )
        } else {
            (
                Arc::clone(&self.queue),
                self.queue_index,
                if self.detached_next.is_some() {
                    0.0
                } else if self.restored {
                    self.restored_position
                } else {
                    self.audio_snapshot.position
                },
                self.detached_next.is_some() || !self.audio_snapshot.playing,
            )
        };
        if let Err(error) = self.store.save_playback_shared_order(
            queue,
            index,
            position,
            paused,
            self.queue_original.clone(),
        ) {
            eprintln!("Playback state: {error}");
        }
        self.last_saved = Instant::now();
    }
    fn set_volume(&mut self, delta: f32) {
        self.visual_player_volume_changed(self.visual_time);
        self.saver.volume_changed(self.visual_time);
        if self.screen() != Screen::NowPlaying {
            self.visual_menu_volume_changed(self.visual_time);
        }
        if self.screen() == Screen::Video {
            self.visual_video_volume_changed(self.visual_time);
        }
        self.settings.volume = (self.settings.volume + delta).clamp(0.0, 1.0);
        if self.volume.set(self.settings.volume) {
            self.send(AudioCommand::Volume(1.0));
        } else {
            self.send(AudioCommand::Volume(self.settings.volume));
        }
        if let Some(video) = self.video.as_ref() {
            video.command(VideoCommand::Volume(if self.volume.get().is_some() {
                1.0
            } else {
                self.settings.volume
            }));
        }
        self.save_settings();
        if self.screen() == Screen::Settings {
            self.refresh_page();
        }
    }
    fn current_index(&self) -> Option<usize> {
        if let Some(track) = self.now_track.as_ref() {
            return self.by_path.get(&track.path).copied();
        }
        self.queue
            .get(self.queue_index)
            .and_then(|p| self.by_path.get(p))
            .copied()
    }
    fn play_current(&mut self, start: f64) {
        self.play_current_inner(start, false);
    }
    fn play_current_inner(&mut self, start: f64, preserve_video_resolution: bool) {
        if self.inline_preview {
            self.inline_preview = false;
            self.extras.stop_preview();
        }
        if !preserve_video_resolution {
            self.extras.cancel_media();
        }
        self.np_seek = None;
        self.np_mode = 0;
        self.stream_restore = None;
        self.pending_stream = None;
        let Some(path) = self.queue.get(self.queue_index).cloned() else {
            self.notify("The queue is empty");
            return;
        };
        let track = self
            .by_path
            .get(&path)
            .map(|&i| self.library.tracks[i].clone())
            .unwrap_or_else(|| Track {
                title: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
                ..Default::default()
            });
        self.end_seen = self.audio_snapshot.ended;
        let command = AudioCommand::Play {
            path: track.path.clone(),
            sample_rate: track.sample_rate,
            start,
        };
        if self.video_open
            || self.video.as_ref().is_some_and(|v| {
                let s = v.snapshot();
                s.playing || s.loading
            })
        {
            if let Some(video) = self.video.as_ref() {
                video.command(VideoCommand::Stop);
            }
            self.video_open = false;
            self.pending_music = Some(command);
        } else {
            self.send(command);
        }
        self.queued_marks.remove(&track.path);
        self.detached_next = None;
        self.now_track = Some(track);
        self.refresh_playback_menus();
        self.restored = false;
        self.restored_position = 0.0;
        if let Some(path) = self.queue.get(self.queue_index + 1) {
            self.send(AudioCommand::Prepare { path: path.clone() });
        }
        self.save_playback();
    }
    fn song(&mut self, index: usize) {
        if self.library.tracks.get(index).is_some_and(|t| {
            self.now_track
                .as_ref()
                .is_some_and(|now| now.path == t.path)
        }) && (self.audio_snapshot.playing || self.audio_snapshot.paused || self.restored)
        {
            self.open(Screen::NowPlaying);
            return;
        }
        self.queue_original = None;
        self.queued_marks.clear();
        self.visual_player_track_direction(1);
        let screen = self.screen();
        let mut order: Vec<usize> = match screen {
            Screen::Album(album) => self
                .library
                .albums
                .get(album)
                .map(|a| a.track_indices.clone())
                .unwrap_or_default(),
            Screen::Songs => (0..self.library.tracks.len()).collect(),
            Screen::Search | Screen::SearchResults(2) => self
                .pages
                .last()
                .map(|p| {
                    p.rows
                        .iter()
                        .filter_map(|r| {
                            if let Intent::Song(i) = r.intent {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if order.is_empty() {
            order = self
                .library
                .albums
                .iter()
                .find(|a| a.track_indices.contains(&index))
                .map(|a| a.track_indices.clone())
                .unwrap_or_else(|| vec![index]);
        }
        let selected = order.iter().position(|&i| i == index).unwrap_or(0);
        self.queue = order
            .into_iter()
            .filter_map(|i| self.library.tracks.get(i).map(|t| t.path.clone()))
            .collect::<Vec<_>>()
            .into();
        self.queue_index = selected;
        if self.settings.shuffle {
            self.shuffle_remaining();
        }
        self.play_current(0.0);
        self.open(Screen::NowPlaying);
    }
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    fn shuffle_remaining(&mut self) {
        self.reference_shuffle_remaining();
    }
    fn next(&mut self, manual: bool) {
        if self.inline_preview {
            return;
        }
        self.visual_player_track_direction(1);
        self.saver.set_track_direction(1);
        if let Some(quiz) = self.quiz.as_mut() {
            let effect = quiz.next(&self.library);
            self.quiz_effect(effect);
            return;
        }
        if self.inline_preview || self.video_open || self.screen() == Screen::Video {
            self.video_next(1);
            return;
        }
        if self.queue.is_empty() {
            if self.now_track.is_some() {
                self.show_end_queue(manual);
            }
            return;
        }
        if let Some(next) = self.detached_next.take() {
            if next < self.queue.len() {
                self.queue_index = next;
                self.play_current(0.0);
                return;
            }
            self.queue_index = self.queue.len() - 1;
        } else if !manual && self.settings.repeat == "one" {
            self.play_current(0.0);
            return;
        }
        if self.queue_index + 1 < self.queue.len() {
            self.queue_index += 1;
            self.play_current(0.0);
        } else if self.settings.repeat == "all" {
            self.queue_index = 0;
            self.play_current(0.0);
        } else {
            self.show_end_queue(manual);
        }
    }
    fn show_end_queue(&mut self, manual: bool) {
        if self.now_track.is_none() || self.library.tracks.is_empty() {
            return;
        }
        if !manual {
            self.send(AudioCommand::Stop);
            self.save_playback();
        }
        if !self.auto_saver && !self.held {
            self.options = Some(Page {
                kind: ViewKind::Full,
                mini: false,
                screen: self.screen(),
                title: "End of the Queue".into(),
                rows: vec![
                    Row::new("Genius", Intent::Genius),
                    Row::new("Go to Music", Intent::EndQueueMusic),
                    Row::new("Not Now", Intent::CloseOptions),
                ]
                .into(),
                selected: 0,
                scroll: 0.0,
                target_scroll: 0.0,
                enter: 0.0,
            });
        }
    }
    fn previous(&mut self) {
        if self.inline_preview {
            return;
        }
        self.visual_player_track_direction(-1);
        self.saver.set_track_direction(-1);
        if self.quiz.is_some() {
            self.send(AudioCommand::Pause);
            return;
        }
        if self.inline_preview || self.video_open || self.screen() == Screen::Video {
            self.video_next(-1);
            return;
        }
        if self.queue.is_empty() {
            if self
                .now_track
                .as_ref()
                .is_some_and(|t| self.audio_snapshot.path.as_ref() == Some(&t.path))
            {
                self.send(AudioCommand::Seek(0.0));
            }
            return;
        }
        if self.audio_snapshot.position > 3.0 {
            self.send(AudioCommand::Seek(0.0));
        } else {
            self.queue_index = self.queue_index.saturating_sub(1);
            self.play_current(0.0);
        }
    }
    fn play_pause(&mut self) {
        if self.inline_preview {
            return;
        }
        if self.visual_video_media(Action::MediaPlayPause) {
            return;
        }
        if self.quiz.is_some() {
            self.send(AudioCommand::TogglePause);
            return;
        }
        if self.inline_preview || self.video_open || self.pending_stream.is_some() {
            if self.pending_stream.is_some() {
                self.pending_stream_paused = !self.pending_stream_paused;
                return;
            }
            if let Some(video) = self.video.as_ref() {
                let state = video.snapshot();
                video.command(VideoCommand::Pause(!state.paused));
                self.video_overlay = Instant::now();
            }
            return;
        }
        if self.restored {
            self.play_current(self.restored_position);
        } else if self.audio_snapshot.path.is_some() {
            self.send(AudioCommand::TogglePause);
        } else if !self.queue.is_empty() {
            self.play_current(0.0);
        } else if !self.library.tracks.is_empty() {
            self.song(0);
        }
    }
    fn execute(&mut self, intent: Intent) {
        match intent {
            Intent::PuzzleNew => {
                if let Some(puzzle) = self.puzzle.as_mut() {
                    puzzle.new_game();
                }
            }
            Intent::PuzzleUndo => {
                if let Some(puzzle) = self.puzzle.as_mut() {
                    puzzle.undo();
                }
            }
            Intent::QuitPuzzle => {
                self.save_games();
                if self.screen() == Screen::Puzzle && self.pages.len() > 1 {
                    let previous = self.pages.pop();
                    self.visual_menu_navigation(previous, false);
                }
            }
            Intent::StopQuiz => self.quiz_effect(QuizEffect::Exit),
            Intent::QuizNewGame => {
                if let Some(quiz) = self.quiz.as_mut() {
                    let effect = quiz.new_game(&self.library);
                    self.quiz_effect(effect);
                }
            }
            Intent::GameCommand(GameCommand::History) => self.open(Screen::GameHistory),
            Intent::GameCommand(command) => {
                if let Some(game) = self.game.as_mut() {
                    game.command(command);
                }
            }
            Intent::Noop => {}
            Intent::Quit => {
                self.user_quit = true;
                self.running = false;
            }
            Intent::QuitColumn | Intent::SoftwareUpdate | Intent::SharingSettings => {
                self.preview_focus = true;
            }
            Intent::YouTubeSettings => {
                self.open_youtube_settings();
            }
            Intent::SharingName => {
                self.keyboard_context = KeyboardContext::Sharing;
                self.keyboard = Some(Keyboard::new(self.settings.sharing_name.clone()));
                self.refresh_setting_preview();
            }
            Intent::ToggleBeta => {
                self.settings.beta_updates = !self.settings.beta_updates;
                if let Some(m) = self.maintenance.as_mut() {
                    m.set_beta(self.settings.beta_updates);
                }
                self.save_settings();
                self.refresh_setting_preview();
            }
            Intent::Open(screen) => {
                if screen == Screen::Visualizer {
                    self.configure_saver(true);
                }
                self.open(screen)
            }
            Intent::Song(i) => self.song(i),
            Intent::QueueSong(i) => {
                if i < self.queue.len() {
                    for path in self.queue.iter().take(i).skip(self.queue_index + 1) {
                        self.queued_marks.remove(path);
                    }
                    self.queue_index = i;
                    self.play_current(0.0);
                    self.open(Screen::NowPlaying);
                }
            }
            Intent::PlayAlbum(i) => {
                self.queue_original = None;
                self.queued_marks.clear();
                if let Some(a) = self.library.albums.get(i) {
                    self.queue = a
                        .track_indices
                        .iter()
                        .map(|&i| self.library.tracks[i].path.clone())
                        .collect::<Vec<_>>()
                        .into();
                    self.queue_index = 0;
                    self.play_current(0.0);
                    self.open(Screen::NowPlaying);
                }
            }
            Intent::ShuffleSongs => {
                self.queue_original = None;
                self.queued_marks.clear();
                self.queue = self
                    .library
                    .tracks
                    .iter()
                    .map(|t| t.path.clone())
                    .collect::<Vec<_>>()
                    .into();
                self.queue_index = 0;
                if !self.queue.is_empty() {
                    let i = self.random() as usize % self.queue.len();
                    Arc::make_mut(&mut self.queue).swap(0, i);
                    self.shuffle_remaining();
                    self.play_current(0.0);
                    self.open(Screen::NowPlaying);
                }
            }
            Intent::Shuffle => {
                let mode = match self.shuffle_label() {
                    "Off" => "songs",
                    "Songs" => "albums",
                    _ => "off",
                };
                self.set_shuffle_mode(mode);
                self.save_settings();
                self.refresh_page();
            }
            Intent::Repeat => {
                self.settings.repeat = match self.settings.repeat.as_str() {
                    "off" => "all",
                    "all" => "one",
                    _ => "off",
                }
                .into();
                self.save_settings();
                self.refresh_page();
            }
            Intent::Files(mode) => {
                self.files.open(mode, Arc::clone(&self.library));
                self.open(Screen::Files);
            }
            Intent::Setting(value) => self.apply_setting(value),
            Intent::AskClearUpcoming => self.reference_confirm(
                "Clear the songs up next?",
                "Clear Up Next",
                Intent::ClearUpcoming,
            ),
            Intent::AskClearNowPlaying => self.reference_confirm(
                "Stop the music and clear Now Playing?",
                "Clear Now Playing",
                Intent::ClearNowPlaying,
            ),
            Intent::EndQueueMusic => {
                self.pages.clear();
                self.pages.push(self.page(Screen::Home));
                self.open(Screen::Music);
            }
            Intent::ClearUpcoming => {
                let keep = self.detached_next.unwrap_or(self.queue_index + 1);
                self.queue = Arc::new(self.queue.iter().take(keep).cloned().collect());
                self.queue_index = self.queue_index.min(self.queue.len().saturating_sub(1));
                self.queue_original = None;
                self.queued_marks.clear();
                self.save_playback();
                self.refresh_page();
            }
            Intent::ClearNowPlaying => {
                self.send(AudioCommand::Stop);
                self.pending_music = None;
                self.queue = Arc::new(Vec::new());
                self.queue_index = 0;
                self.now_track = None;
                self.queued_marks.clear();
                self.queue_original = None;
                self.detached_next = None;
                self.refresh_playback_menus();
                self.restored = false;
                self.save_playback();
                for _ in 0..2 {
                    if matches!(self.screen(), Screen::NowPlaying | Screen::Queue)
                        && self.pages.len() > 1
                    {
                        let previous = self.pages.pop();
                        self.visual_menu_navigation(previous, false);
                    }
                }
                self.refresh_page();
            }
            Intent::Playlist(playlist) => {
                self.extras.open_playlist(playlist);
                self.open(Screen::Extra);
            }
            Intent::MergeSong(track) => {
                if let Some(t) = self.library.tracks.get(track) {
                    self.curation.open(
                        CurationMode::Merge {
                            paths: vec![t.path.clone()],
                        },
                        Arc::clone(&self.library),
                    );
                    self.open(Screen::Curation);
                }
            }
            Intent::SeparateSong(track) => {
                if let Some(t) = self.library.tracks.get(track) {
                    self.extras
                        .curation_task(betterpod_rust::online::Task::Unmerge {
                            paths: vec![t.path.clone()],
                        });
                    self.open(Screen::Extra);
                }
            }
            Intent::Unmerge(album) => {
                if let Some(a) = self.library.albums.get(album) {
                    let paths = a
                        .track_indices
                        .iter()
                        .map(|&i| self.library.tracks[i].path.clone())
                        .collect();
                    self.extras
                        .curation_task(betterpod_rust::online::Task::Unmerge { paths });
                    self.open(Screen::Extra);
                }
            }
            Intent::Maintenance(mode) => {
                let modal = matches!(mode, MaintenanceMode::DeleteAlbum(_));
                if let Some(maintenance) = self.maintenance.as_mut() {
                    maintenance.open(mode);
                    if !modal {
                        self.open(Screen::Maintenance);
                    }
                }
            }
            Intent::AddAlbumPlaylist(album) => {
                if let Some(a) = self.library.albums.get(album) {
                    self.extras.add_to_playlist(
                        a.track_indices
                            .iter()
                            .map(|&i| self.library.tracks[i].path.clone())
                            .collect(),
                    );
                    self.open(Screen::Extra);
                }
            }
            Intent::AddQueuePlaylist => {
                self.extras.add_to_playlist(self.queue.as_ref().clone());
                self.open(Screen::Extra);
            }
            Intent::QueueAlbum(album, next) => {
                if let Some(album) = self.library.albums.get(album) {
                    self.enqueue_paths(
                        album
                            .track_indices
                            .iter()
                            .map(|&i| self.library.tracks[i].path.clone())
                            .collect(),
                        next,
                    );
                }
            }
            Intent::RemoveQueue(index) => self.remove_queued(index),
            Intent::MoveQueueNext(index) => self.move_queued_next(index),
            Intent::Curation(CurationMode::Covers {
                paths,
                album,
                artist,
            }) => {
                self.extras.open_cover_search_all(paths, album, artist);
                self.open(Screen::Extra);
            }
            Intent::Curation(mode) => {
                self.curation.open(mode, Arc::clone(&self.library));
                self.open(Screen::Curation);
            }
            Intent::Quiz => self.start_quiz(),
            Intent::Genius => self.genius(),
            Intent::Theme => {
                self.settings.theme = match self.settings.theme.as_str() {
                    "light" => "dark",
                    "dark" => "auto",
                    _ => "light",
                }
                .into();
                self.save_settings();
                self.refresh_page();
            }
            Intent::Volume => self.set_volume(0.05),
            Intent::Rescan => {
                self.rescan();
                self.notify("Scanning music in the background");
            }
            Intent::SearchKeyboard => {
                self.keyboard_context = KeyboardContext::Library;
                self.keyboard = Some(Keyboard::new(self.search_query.clone()))
            }
            Intent::Game(kind) => {
                let mut game = Games::new(kind);
                if kind != GameKind::Brick {
                    if let Some(saved) = self.game_saves.get(game.title()) {
                        let _ = game.restore_snapshot(saved);
                    }
                }
                game.set_assets(&self.visual_assets);
                game.set_clock_shown(
                    self.settings
                        .game_clock
                        .iter()
                        .any(|name| name == game.title()),
                );
                self.game = Some(game);
                self.open(Screen::Game);
            }
            Intent::AddNext(i) | Intent::AddLater(i) => {
                if let Some(t) = self.library.tracks.get(i) {
                    self.enqueue_paths(vec![t.path.clone()], matches!(intent, Intent::AddNext(_)));
                }
            }
            Intent::Stop => {
                self.send(AudioCommand::Stop);
                self.queue = Arc::new(Vec::new());
                self.now_track = None;
                self.restored = false;
                self.save_playback();
            }
            Intent::Puzzle(i) => {
                let saved = PUZZLES
                    .get(i)
                    .and_then(|p| self.game_saves.get(p.id))
                    .cloned();
                match PuzzleGame::with_save(i, saved) {
                    Ok(puzzle) => {
                        self.puzzle = Some(puzzle);
                        self.open(Screen::Puzzle);
                    }
                    Err(error) => self.notify(error),
                }
            }
            Intent::Effects => {
                self.settings.effects = if self.settings.effects == "light" {
                    "full"
                } else {
                    "light"
                }
                .into();
                self.save_settings();
                self.refresh_page();
            }
            Intent::Apple => {
                self.settings.apple = !self.settings.apple;
                self.save_settings();
                self.refresh_page();
            }
            Intent::YouTube => {
                self.settings.youtube = !self.settings.youtube;
                self.extras.set_youtube(self.settings.youtube);
                self.save_settings();
                self.refresh_page();
            }
            Intent::Podcasts => {
                self.settings.hide_podcasts = !self.settings.hide_podcasts;
                self.save_settings();
                self.refresh_page();
            }
            Intent::StartSaver => self.enter_saver(),
            Intent::SaverDim => {
                self.settings.screensaver_dim = if self.settings.screensaver_dim == 0 {
                    15
                } else {
                    0
                };
                self.save_settings();
                self.refresh_page();
            }
            Intent::SaverTime => {
                self.settings.screensaver_seconds = match self.settings.screensaver_seconds {
                    0 => 60,
                    60 => 120,
                    120 => 300,
                    _ => 0,
                };
                self.save_settings();
                self.refresh_page();
            }
            Intent::SaverStyle => {
                self.settings.screensaver_style = if self.settings.screensaver_style == "cover" {
                    "viz"
                } else {
                    "cover"
                }
                .into();
                self.save_settings();
                self.refresh_page();
            }
            Intent::SaverEffect => {
                self.settings.screensaver_effect = (self.settings.screensaver_effect + 1)
                    % saver::effect_count(&self.settings.screensaver_style);
                self.configure_saver(false);
                self.save_settings();
                self.refresh_page();
            }
            Intent::SaverColors => {
                self.settings.screensaver_colors =
                    (self.settings.screensaver_colors + 1) % saver::palette_count();
                self.configure_saver(false);
                self.save_settings();
                self.refresh_page();
            }
            Intent::GameClock(name) => {
                if let Some(i) = self.settings.game_clock.iter().position(|n| *n == name) {
                    self.settings.game_clock.remove(i);
                } else {
                    self.settings.game_clock.push(name);
                }
                self.save_settings();
                self.refresh_page();
            }
            Intent::ShareTrack(i) => {
                if let Some(track) = self.library.tracks.get(i) {
                    let paths = vec![track.path.clone()];
                    let info = betterpod_rust::share::ShareInfo {
                        v: 1,
                        from: self.settings.sharing_name.clone(),
                        album: track.album.clone(),
                        artist: track.artist.clone(),
                        title: track.title.clone(),
                        songs: 1,
                        bytes: 0,
                    };
                    self.extras.send_share(paths, info, track.cover.clone());
                    self.open(Screen::Extra);
                }
            }
            Intent::ShareAlbum(i) => {
                if let Some(album) = self.library.albums.get(i) {
                    let paths = album
                        .track_indices
                        .iter()
                        .map(|&i| self.library.tracks[i].path.clone())
                        .collect();
                    let info = betterpod_rust::share::ShareInfo {
                        v: 1,
                        from: self.settings.sharing_name.clone(),
                        album: album.name.clone(),
                        artist: album.artist.clone(),
                        songs: album.track_indices.len(),
                        ..Default::default()
                    };
                    self.extras.send_share(paths, info, album.cover.clone());
                    self.open(Screen::Extra);
                }
            }
            Intent::SongInfo(i) => {
                if let Some(track) = self.library.tracks.get(i) {
                    let edit = betterpod_rust::metadata::TagEdit {
                        title: Some(track.title.clone()),
                        album: Some(track.album.clone()),
                        artist: Some(track.artist.clone()),
                        album_artist: Some(track.album_artist.clone()),
                        year: (track.year > 0).then_some(track.year),
                        genre: Some(track.genre.clone()),
                        track: (track.track > 0).then_some(track.track),
                        disc: (track.disc > 0).then_some(track.disc),
                        ..Default::default()
                    };
                    self.extras
                        .edit_album(vec![track.path.clone()], track.title.clone(), edit);
                    self.open(Screen::Extra);
                }
            }
            Intent::VideoRestart => self.visual_video_restart(),
            Intent::VideoDownloadMenu => self.visual_video_download_options(),
            Intent::DownloadVideoChapters => {
                let url = self.visual_video_download_origin().to_owned();
                self.extras
                    .download(betterpod_rust::downloads::DownloadRequest::YouTube {
                        url,
                        video: false,
                        split_chapters: true,
                    });
                self.open(Screen::Extra);
            }
            Intent::DownloadMedia(video) => {
                let origin = self.visual_video_download_origin().to_owned();
                let title = self.visual_video_download_title().to_owned();
                if origin.contains("youtube.com/") || origin.contains("youtu.be/") {
                    self.extras
                        .download(betterpod_rust::downloads::DownloadRequest::YouTube {
                            url: origin,
                            video,
                            split_chapters: false,
                        });
                } else {
                    self.extras
                        .download(betterpod_rust::downloads::DownloadRequest::File {
                            url: origin,
                            name: title,
                            category: if video { "videos" } else { "audio" }.into(),
                            tags: None,
                        });
                }
                self.open(Screen::Extra);
                self.notify("Choose a cover or follow download progress");
            }
            Intent::Extra(page) => {
                self.extras.open(page);
                self.open(Screen::Extra);
            }
            Intent::AddToPlaylist(i) => {
                if let Some(track) = self.library.tracks.get(i) {
                    self.extras.add_to_playlist(vec![track.path.clone()]);
                    self.open(Screen::Extra);
                }
            }
            Intent::AlbumInfo(i) => {
                if let Some(track) = self.library.tracks.get(i) {
                    let paths = self
                        .library
                        .albums
                        .iter()
                        .find(|a| a.track_indices.contains(&i))
                        .map(|a| {
                            a.track_indices
                                .iter()
                                .map(|&j| self.library.tracks[j].path.clone())
                                .collect()
                        })
                        .unwrap_or_else(|| vec![track.path.clone()]);
                    let edit = betterpod_rust::metadata::TagEdit {
                        album: Some(track.album.clone()),
                        album_artist: Some(track.album_artist.clone()),
                        artist: None,
                        genre: Some(track.genre.clone()),
                        year: (track.year > 0).then_some(track.year),
                        ..Default::default()
                    };
                    self.extras.edit_album(paths, track.album.clone(), edit);
                    self.open(Screen::Extra);
                }
            }
            Intent::CloseOptions => {}
        }
    }

    pub fn handle(&mut self, action: Action) {
        if self.visual_video_media(action) {
            return;
        }
        if !matches!(action, Action::Headphones(_)) {
            self.last_activity = Instant::now();
        }
        match action {
            Action::HoldDown => {
                self.hold_pressed = true;
                self.hold_consumed = false;
                return;
            }
            Action::HoldUp => {
                if self.hold_pressed {
                    self.hold_pressed = false;
                    if self.hold_consumed {
                        return;
                    }
                    self.held = !self.held;
                    if self.held {
                        self.leave_saver();
                        self.hold_brightness = self.platform.brightness();
                        let _ = self
                            .platform
                            .set_brightness(self.settings.hold_brightness as i32);
                    } else if let Some(value) = self.hold_brightness.take() {
                        let _ = self.platform.set_brightness(value);
                    }
                    self.notify(if self.held { "Hold On" } else { "Hold Off" });
                }
                return;
            }
            Action::Headphones(connected) => {
                self.wired_headphones = connected;
                self.visual_menu.status.headphones =
                    connected || self.last_bt_audio.unwrap_or(false);
                return;
            }
            Action::MediaUnplug | Action::MediaPause => {
                self.pause_media();
                return;
            }
            Action::MediaVolumeUp | Action::MediaVolumeDown => {
                self.set_volume(if action == Action::MediaVolumeUp {
                    0.04
                } else {
                    -0.04
                });
                return;
            }
            Action::MediaPlayPause => {
                self.play_pause();
                return;
            }
            Action::MediaPlay => {
                if self.inline_preview {
                    return;
                }
                if self.video_open || self.pending_stream.is_some() {
                    self.pending_stream_paused = false;
                    if let Some(video) = self.video.as_ref() {
                        video.command(VideoCommand::Pause(false));
                    }
                } else if !self.audio_snapshot.playing {
                    self.play_pause();
                }
                return;
            }
            Action::MediaNext => {
                self.next(true);
                return;
            }
            Action::MediaPrevious => {
                self.previous();
                return;
            }
            _ => {}
        }
        if self.hold_pressed && !matches!(action, Action::HoldDown | Action::HoldUp) {
            self.hold_consumed = true;
            match action {
                Action::Start => {
                    self.user_quit = true;
                    self.running = false;
                    return;
                }
                Action::Options => {
                    if !self.held {
                        self.enter_saver();
                    }
                    return;
                }
                _ => {}
            }
        }
        if self.held && matches!(action, Action::Up | Action::Down) {
            let level = (self.settings.hold_brightness as i32
                + if action == Action::Up { 5 } else { -5 })
            .clamp(1, 100);
            self.settings.hold_brightness = level as u8;
            let _ = self.platform.set_brightness(level);
            self.save_settings();
            return;
        }
        if !self.held && !self.screen_off && self.visual_menu_confirm_handle(action) {
            return;
        }
        if !self.held && !self.auto_saver && !self.screen_off && self.visual_player_handle(action) {
            return;
        }
        if !self.held
            && !self.auto_saver
            && !self.screen_off
            && self.options.is_none()
            && self.visual_video_handle(action)
        {
            return;
        }
        if !self.held
            && !self.screen_off
            && self.options.is_none()
            && (self.auto_saver || self.screen() == Screen::Visualizer)
            && matches!(
                action,
                Action::Up
                    | Action::Down
                    | Action::Left
                    | Action::Right
                    | Action::Select
                    | Action::Back
                    | Action::Options
                    | Action::Start
                    | Action::SelectHeld
                    | Action::BackHeld
            )
        {
            let event = self.saver.input(if action == Action::BackHeld {
                Action::Back
            } else {
                action
            });
            if self.settings.screensaver_effect != self.saver.effect()
                || self.settings.screensaver_colors != self.saver.palette()
            {
                self.settings.screensaver_effect = self.saver.effect();
                self.settings.screensaver_colors = self.saver.palette();
                self.save_settings();
            }
            if let Some(event) = event {
                self.saver_action(event);
            }
            return;
        }
        if matches!(action, Action::SelectHeld | Action::BackHeld) {
            if self.held {
                return;
            }
            if self.keyboard.is_some() {
                self.keyboard_action(action);
                return;
            }
            if self.screen() == Screen::Extra && self.extras.is_typing() {
                self.extras.handle(action);
                return;
            }
            if action == Action::SelectHeld {
                if self.screen() == Screen::Files {
                    if let Some(effect) = self.files.handle(action) {
                        self.files_effect(effect);
                    }
                    return;
                }
                if self.screen() == Screen::Game {
                    if let Some(g) = self.game.as_mut() {
                        g.handle(action);
                    }
                    return;
                }
                if self.screen() == Screen::Puzzle {
                    if let Some(g) = self.puzzle.as_mut() {
                        g.handle(action);
                    }
                    return;
                }
                if self.screen() == Screen::NowPlaying {
                    self.open(Screen::Queue);
                } else {
                    self.show_options();
                }
                return;
            }
            if !self.inline_preview {
                self.visual_video_stop();
            }
            self.stop_inline_preview();
            self.leave_saver();
            self.save_games();
            self.quiz_effect_if_active();
            self.game = None;
            self.puzzle = None;
            self.curation.close();
            self.files.leave();
            self.options = None;
            self.preview_focus = false;
            self.feedback = Feedback::new();
            self.pages.clear();
            self.pages.push(self.page(Screen::Home));
            self.visual_menu_snap_state(self.visual_time);
            return;
        }
        if self.held
            && !matches!(
                action,
                Action::MenuDown
                    | Action::MenuUp
                    | Action::Quit
                    | Action::VolumeUp
                    | Action::VolumeDown
                    | Action::BrightnessUp
                    | Action::BrightnessDown
            )
        {
            return;
        }
        if matches!(
            action,
            Action::Up
                | Action::Down
                | Action::Left
                | Action::Right
                | Action::Select
                | Action::Back
                | Action::Start
                | Action::Options
                | Action::PlayPause
                | Action::Next
                | Action::Previous
        ) {
            if self.keyboard.is_some() {
                self.keyboard_action(action);
                return;
            }
            if self.screen() == Screen::Extra && self.extras.is_typing() {
                self.extras.handle(action);
                return;
            }
        }
        if matches!(action, Action::Start | Action::Next | Action::Previous) {
            if self.screen() == Screen::Game {
                if action == Action::Start
                    && self.game.as_ref().is_some_and(|g| g.title() != "Solitaire")
                {
                    if let Some(game) = self.game.as_mut() {
                        let title = game.title().to_owned();
                        let mut rows = game
                            .menu_items()
                            .into_iter()
                            .map(|item| Row::new(item.label, Intent::GameCommand(item.command)))
                            .collect::<Vec<_>>();
                        rows.push(Row::new("Cancel", Intent::CloseOptions));
                        self.options = Some(Page {
                            kind: ViewKind::Full,
                            mini: false,
                            screen: Screen::Game,
                            title,
                            rows: Arc::new(rows),
                            selected: 0,
                            scroll: 0.0,
                            target_scroll: 0.0,
                            enter: 0.0,
                        });
                    }
                } else if let Some(game) = self.game.as_mut() {
                    game.handle(action);
                }
                return;
            }
            if self.screen() == Screen::Puzzle {
                if action == Action::Start {
                    let title = self
                        .puzzle
                        .as_ref()
                        .map(|p| p.title().to_owned())
                        .unwrap_or_default();
                    let rows = vec![
                        Row::new("New Game", Intent::PuzzleNew),
                        Row::new("Undo", Intent::PuzzleUndo),
                        Row::new(
                            if self.settings.game_clock.contains(&title) {
                                "Hide the Time"
                            } else {
                                "Show the Time"
                            },
                            Intent::GameClock(title.clone()),
                        ),
                        Row::new(format!("Quit {title}"), Intent::QuitPuzzle),
                        Row::new("Cancel", Intent::CloseOptions),
                    ];
                    self.options = Some(Page {
                        kind: ViewKind::Full,
                        mini: false,
                        screen: Screen::Puzzle,
                        title,
                        rows: Arc::new(rows),
                        selected: 0,
                        scroll: 0.0,
                        target_scroll: 0.0,
                        enter: 0.0,
                    });
                } else if let Some(puzzle) = self.puzzle.as_mut() {
                    puzzle.handle(action);
                }
                return;
            }
        }
        if self.screen() == Screen::Quiz && self.options.is_none() {
            if let Some(effect) = self
                .quiz
                .as_mut()
                .and_then(|q| q.handle(action, &self.library))
            {
                self.quiz_effect(effect);
            }
            if matches!(
                action,
                Action::Start
                    | Action::Up
                    | Action::Down
                    | Action::Left
                    | Action::Right
                    | Action::Select
                    | Action::Back
                    | Action::PlayPause
                    | Action::Next
                    | Action::Previous
                    | Action::Options
            ) {
                return;
            }
        }
        if action == Action::Start
            && (self.auto_saver || self.screen() == Screen::Visualizer)
            && (self.screen() == Screen::Visualizer || self.settings.screensaver_style == "viz")
        {
            self.saver.input(action);
            return;
        }
        if action == Action::Start {
            self.play_pause();
            return;
        }
        if action == Action::Back {
            if self.screen() == Screen::Game
                && self.game.as_mut().is_some_and(|game| game.back_consumed())
            {
                return;
            }
            if self.screen() == Screen::Puzzle
                && self
                    .puzzle
                    .as_mut()
                    .is_some_and(|game| game.back_consumed())
            {
                return;
            }
        }

        match action {
            Action::Quit => {
                self.user_quit = true;
                self.running = false;
                return;
            }
            Action::MenuDown => {
                if self.menu_down.is_none() {
                    self.menu_down = Some(Instant::now());
                    self.menu_used = false;
                }
                return;
            }
            Action::MenuUp => {
                if let Some(start) = self.menu_down.take() {
                    if start.elapsed() >= Duration::from_secs(5) {
                        self.user_quit = true;
                        self.running = false;
                    } else if !self.menu_used {
                        self.screen_off = !self.screen_off;
                        if let Err(e) = self.platform.set_screen_off(self.screen_off) {
                            self.notify(e);
                        }
                    }
                }
                return;
            }
            Action::VolumeUp => {
                self.menu_used |= self.menu_down.is_some();
                self.set_volume(0.05);
                return;
            }
            Action::VolumeDown => {
                self.menu_used |= self.menu_down.is_some();
                self.set_volume(-0.05);
                return;
            }
            Action::BrightnessUp | Action::BrightnessDown => {
                self.menu_used |= self.menu_down.is_some();
                let value = self.platform.brightness().unwrap_or(70)
                    + if action == Action::BrightnessUp {
                        5
                    } else {
                        -5
                    };
                if let Err(e) = self.platform.set_brightness(value) {
                    self.notify(e);
                } else {
                    self.visual_menu_brightness_changed(value.clamp(1, 100), self.visual_time);
                    self.saver
                        .brightness_changed(value.clamp(1, 100), self.visual_time);
                }
                return;
            }
            Action::PlayPause => {
                self.play_pause();
                return;
            }
            Action::Next => {
                self.next(true);
                return;
            }
            Action::Previous => {
                self.previous();
                return;
            }
            _ => {}
        }
        if self.screen_off {
            return;
        }
        if self.screen() == Screen::Quiz && self.quiz.is_some() {
            let effect = self
                .quiz
                .as_mut()
                .and_then(|quiz| quiz.handle(action, &self.library));
            if let Some(effect) = effect {
                self.quiz_effect(effect);
            }
            return;
        }
        if (self.auto_saver || self.screen() == Screen::Visualizer) && self.options.is_none() {
            let effect = self.saver.input(action);
            if self.settings.screensaver_effect != self.saver.effect()
                || self.settings.screensaver_colors != self.saver.palette()
            {
                self.settings.screensaver_effect = self.saver.effect();
                self.settings.screensaver_colors = self.saver.palette();
                self.save_settings();
            }
            if let Some(effect) = effect {
                self.saver_action(effect);
            }
            return;
        }

        if self.keyboard.is_some() {
            self.keyboard_action(action);
            return;
        }
        if self.options.is_some() {
            match action {
                Action::Back | Action::Options => self.options = None,
                Action::Up => {
                    if let Some(p) = self.options.as_mut() {
                        p.selected = p.selected.saturating_sub(1);
                    }
                }
                Action::Down => {
                    if let Some(p) = self.options.as_mut() {
                        p.selected = (p.selected + 1).min(p.rows.len().saturating_sub(1));
                    }
                }
                Action::Select => {
                    let intent = self
                        .options
                        .as_ref()
                        .and_then(|p| p.rows.get(p.selected))
                        .map(|r| r.intent.clone());
                    self.options = None;
                    if let Some(intent) = intent {
                        self.execute(intent);
                    }
                }
                _ => {}
            }
            return;
        }
        if self.maintenance.as_ref().is_some_and(|m| m.has_options()) {
            if let Some(effect) = self.maintenance.as_mut().and_then(|m| m.handle(action)) {
                self.maintenance_effect(effect);
            }
            return;
        }
        if self.screen() == Screen::Files {
            if let Some(effect) = self.files.handle(action) {
                self.files_effect(effect);
            }
            return;
        }
        if self.screen() == Screen::Maintenance {
            if let Some(effect) = self.maintenance.as_mut().and_then(|m| m.handle(action)) {
                self.maintenance_effect(effect);
            }
            return;
        }
        if self.screen() == Screen::Curation && self.curation.active() {
            if let Some(effect) = self.curation.handle(action) {
                self.curation_effect(effect);
            }
            return;
        }
        if self.screen() == Screen::Extra {
            if let Some(page) = self.extras.visual_page() {
                let height = self.visual_menu_content_height(&page);
                self.extras.set_viewport_height(height);
            }
            self.extras.handle(action);
            return;
        }
        if self.handle_setting_preview(action) {
            return;
        }
        if action == Action::Back {
            if self.pages.len() > 1 {
                let leaving = self.screen();
                let previous = self.pages.last().cloned();
                self.save_games();
                self.pages.pop();
                self.visual_menu_navigation(previous, false);
                // Retain custom view models while their original exit transition draws.
                let _ = leaving;
                self.refresh_page();
            }
            return;
        }
        if action == Action::Search {
            self.keyboard_context = KeyboardContext::Library;
            self.open(Screen::Search);
            self.keyboard = Some(Keyboard::new(self.search_query.clone()));
            return;
        }
        match self.screen() {
            Screen::Game => {
                if let Some(game) = self.game.as_mut() {
                    game.handle(action);
                }
            }
            Screen::CoverFlow => {
                if let Some(selected) = self.cover_card {
                    let tracks = self
                        .library
                        .albums
                        .get(self.cover_selected)
                        .map(|a| a.track_indices.as_slice())
                        .unwrap_or(&[]);
                    match action {
                        Action::Up => self.cover_card = Some(selected.saturating_sub(1)),
                        Action::Down => {
                            self.cover_card =
                                Some((selected + 1).min(tracks.len().saturating_sub(1)))
                        }
                        Action::Select => {
                            if let Some(&track) = tracks.get(selected) {
                                self.song(track);
                            }
                        }
                        Action::Left | Action::Right => self.cover_card = None,
                        Action::Options => self.show_album_options(self.cover_selected),
                        _ => {}
                    }
                } else {
                    match action {
                        Action::Left | Action::Up => {
                            self.cover_selected = self.cover_selected.saturating_sub(1)
                        }
                        Action::Right | Action::Down => {
                            self.cover_selected = (self.cover_selected + 1)
                                .min(self.library.albums.len().saturating_sub(1))
                        }
                        Action::Select => self.cover_card = Some(0),
                        Action::Options => self.show_album_options(self.cover_selected),
                        _ => {}
                    }
                }
            }
            Screen::NowPlaying => match action {
                Action::Up | Action::Down | Action::Left | Action::Right => {
                    let positive = matches!(action, Action::Up | Action::Right);
                    if self.np_mode == 1 {
                        let position = self.np_seek.map_or(self.audio_snapshot.position, |s| s.0);
                        let duration = self
                            .now_track
                            .as_ref()
                            .map_or(self.audio_snapshot.duration, |t| {
                                self.audio_snapshot.duration.max(t.duration)
                            });
                        self.np_seek = Some((
                            (position + if positive { 1.0 } else { -1.0 })
                                .clamp(0.0, if duration > 0.0 { duration } else { f64::MAX }),
                            Instant::now() + Duration::from_millis(250),
                        ));
                    } else if self.np_mode == 2 {
                        if positive {
                            self.np_option = (self.np_option + 1) % 2;
                        } else {
                            self.np_option = (self.np_option + 1) % 2;
                        }
                    } else {
                        self.set_volume(if positive { 0.05 } else { -0.05 });
                    }
                }
                Action::Select => {
                    if self.np_mode == 2 {
                        match self.np_option {
                            0 => self.execute(Intent::Shuffle),
                            1 => self.execute(Intent::Repeat),
                            _ => self.np_mode = 0,
                        }
                    } else {
                        self.np_mode += 1;
                    }
                }
                Action::Options => self.open(Screen::Queue),
                _ => {}
            },
            Screen::Visualizer => {
                if action == Action::Select {
                    self.play_pause();
                }
            }
            Screen::Puzzle => {
                if let Some(puzzle) = self.puzzle.as_mut() {
                    puzzle.handle(action);
                }
            }
            Screen::About | Screen::Extra | Screen::Video | Screen::Quiz | Screen::Curation => {}
            _ => match action {
                Action::Up => {
                    if let Some(p) = self.pages.last_mut() {
                        p.selected = p.selected.saturating_sub(1);
                    }
                }
                Action::Down => {
                    if let Some(p) = self.pages.last_mut() {
                        p.selected = (p.selected + 1).min(p.rows.len().saturating_sub(1));
                    }
                }
                Action::Left => {
                    if self.pages.len() > 1 {
                        let previous = self.pages.pop();
                        self.visual_menu_navigation(previous, false);
                        self.refresh_page();
                    }
                }
                Action::Right | Action::Select => {
                    let intent = self
                        .pages
                        .last()
                        .and_then(|p| p.rows.get(p.selected))
                        .map(|r| r.intent.clone());
                    if let Some(intent) = intent {
                        self.execute(intent);
                    }
                }
                Action::Options => self.show_options(),
                _ => {}
            },
        }
        self.ensure_selection_viewport();
        if self.screen() == Screen::Settings && !self.preview_focus {
            self.refresh_setting_preview();
        }
    }
    fn files_effect(&mut self, effect: FilesEffect) {
        match effect {
            FilesEffect::Exit => {
                if self.screen() == Screen::Files && self.pages.len() > 1 {
                    self.pop_navigation_page();
                }
            }
            FilesEffect::Notice(message) => self.notify(message),
            FilesEffect::Rescan => self.rescan(),
            FilesEffect::Deleted(paths) => {
                let removed = |p: &PathBuf| paths.iter().any(|r| p == r || p.starts_with(r));
                let old_index = self.detached_next.unwrap_or(self.queue_index);
                let before = self
                    .queue
                    .iter()
                    .take(old_index)
                    .filter(|p| !removed(p))
                    .count();
                let current = self.now_track.as_ref().map(|t| t.path.clone());
                self.queue = Arc::new(self.queue.iter().filter(|p| !removed(p)).cloned().collect());
                self.queue_original = None;
                self.queued_marks.retain(|p, _| !removed(p));
                self.detached_next = if current.as_ref().is_some_and(removed) {
                    Some(before)
                } else {
                    self.detached_next.map(|_| before)
                };
                self.queue_index = current
                    .and_then(|p| self.queue.iter().position(|q| q == &p))
                    .unwrap_or_else(|| before.min(self.queue.len().saturating_sub(1)));
                self.save_playback();
            }
        }
    }
    fn quiz_effect_if_active(&mut self) {
        if self.quiz.is_some() {
            self.quiz_effect(QuizEffect::Exit);
        }
    }
    fn flip_now_playing(&mut self) {
        if let Some(track) = self.current_index() {
            if let Some((album, position)) =
                self.library.albums.iter().enumerate().find_map(|(i, a)| {
                    a.track_indices
                        .iter()
                        .position(|&j| j == track)
                        .map(|p| (i, p))
                })
            {
                self.cover_selected = album;
                self.cover_position = album as f32;
                self.cover_card = Some(position);
                self.cover_from_np = true;
                self.open(Screen::CoverFlow);
            }
        }
    }
    pub fn audio_telemetry(&self) -> crate::audio::AudioTelemetry {
        self.audio.telemetry()
    }
    pub fn video_telemetry(&self) -> betterpod_rust::video::VideoTelemetry {
        self.video
            .as_ref()
            .map_or_else(Default::default, |v| v.telemetry())
    }
    pub fn install_maintenance(&mut self, mut controller: MaintenanceController) {
        let visible = controller.has_visible_transfer();
        controller.set_beta(self.settings.beta_updates);
        if !self.scanning {
            controller.set_library(Arc::clone(&self.library));
        }
        self.maintenance = Some(controller);
        if visible {
            self.open(Screen::Maintenance);
        }
    }
    fn maintenance_effect(&mut self, effect: MaintenanceEffect) {
        match effect {
            MaintenanceEffect::Exit => {
                if self.screen() == Screen::Maintenance && self.pages.len() > 1 {
                    self.pop_navigation_page();
                }
            }
            MaintenanceEffect::UpdateAvailable(available) => {
                if available {
                    eprintln!("Independent Rust update available");
                }
                if self.screen() == Screen::Home {
                    self.refresh_page();
                }
            }
            MaintenanceEffect::Notice(message) => self.notify(message),
            MaintenanceEffect::PauseMusic => self.pause_media(),
            MaintenanceEffect::Run(request) => {
                self.maintenance_request = Some(request);
                self.running = false;
            }
        }
    }
    fn curation_effect(&mut self, effect: CurationEffect) {
        match effect {
            CurationEffect::Close => {
                self.close_curation_page();
            }
            CurationEffect::StartCoverSearch(albums) => {
                match self.extras.start_missing_covers(albums) {
                    Ok(handle) => self.curation.cover_search_started(handle),
                    Err(error) => self.curation.cover_search_failed(error),
                }
            }
            CurationEffect::Rescan => self.rescan(),
            CurationEffect::OpenAlbum(index) => self.open(Screen::Album(index)),
            CurationEffect::AlbumOptions(index) => self.show_album_options(index),
            CurationEffect::EditInfo { paths, title, edit } => {
                self.extras.edit_album(paths, title, edit);
                self.open(Screen::Extra);
            }
            CurationEffect::SearchCovers {
                paths,
                album,
                artist,
                provider,
            } => {
                self.extras
                    .open_cover_search(paths, album, artist, provider);
                self.open(Screen::Extra);
            }
            CurationEffect::SearchCoversAll {
                paths,
                album,
                artist,
            } => {
                self.extras.open_cover_search_all(paths, album, artist);
                self.open(Screen::Extra);
            }
            CurationEffect::SearchInput(query) => {
                self.keyboard_context = KeyboardContext::Curation;
                self.keyboard = Some(Keyboard::new(query));
            }
            CurationEffect::Task(task) => {
                self.extras.curation_task(task);
                self.open(Screen::Extra);
            }
        }
    }
    fn start_quiz(&mut self) {
        if self.inline_preview || self.video_open || self.pending_stream.is_some() {
            self.notify("Close the video or stream before starting Guess Song");
            return;
        }
        let seed = self.random();
        match Quiz::new(&self.library, seed) {
            Err(error) => self.notify(error),
            Ok(mut quiz) => {
                quiz.set_best(
                    self.game_saves
                        .get("Guess the Song Best")
                        .and_then(|b| serde_json::from_slice(b).ok())
                        .unwrap_or(0),
                );
                self.quiz_restore = Some(SavedPlayback {
                    queue: self.queue.clone(),
                    index: self.queue_index,
                    position: if self.restored {
                        self.restored_position
                    } else {
                        self.audio_snapshot.position
                    },
                    paused: !self.audio_snapshot.playing,
                });
                let effect = quiz.next(&self.library);
                self.quiz = Some(quiz);
                self.open(Screen::Quiz);
                self.quiz_effect(effect);
            }
        }
    }
    fn quiz_effect(&mut self, effect: QuizEffect) {
        match effect {
            QuizEffect::Play(command) => {
                self.end_seen = self.audio_snapshot.ended;
                self.send(command);
            }
            QuizEffect::Pause => self.send(AudioCommand::Pause),
            QuizEffect::Menu => {
                let mut rows = Vec::new();
                if self.quiz.as_ref().is_some_and(Quiz::has_songs) {
                    rows.push(Row::new("New Game", Intent::QuizNewGame));
                }
                rows.push(Row::new(
                    if self
                        .settings
                        .game_clock
                        .iter()
                        .any(|n| n == "Guess the Song")
                    {
                        "Hide the Time"
                    } else {
                        "Show the Time"
                    },
                    Intent::GameClock("Guess the Song".into()),
                ));
                rows.push(Row::new("Quit Guess the Song", Intent::StopQuiz));
                rows.push(Row::new("Cancel", Intent::CloseOptions));
                self.options = Some(Page {
                    screen: Screen::Quiz,
                    kind: ViewKind::Full,
                    mini: false,
                    title: "Guess the Song".into(),
                    rows: Arc::new(rows),
                    selected: 0,
                    scroll: 0.0,
                    target_scroll: 0.0,
                    enter: 0.0,
                });
            }
            QuizEffect::Exit => {
                self.send(AudioCommand::Stop);
                self.retired_quiz = self.quiz.take();
                if let Some(saved) = self.quiz_restore.take() {
                    self.queue = saved.queue;
                    self.queue_index = saved.index;
                    self.restored_position = saved.position;
                    self.restored = true;
                    self.now_track = self.current_index().map(|i| self.library.tracks[i].clone());
                    if !saved.paused && !self.queue.is_empty() {
                        self.play_current(saved.position);
                    }
                }
                if self.screen() == Screen::Quiz && self.pages.len() > 1 {
                    let previous = self.pages.pop();
                    self.visual_menu_navigation(previous, false);
                }
            }
        }
    }
    fn genius(&mut self) {
        self.queue_original = None;
        self.queued_marks.clear();
        let mut pool: Vec<usize> = self
            .library
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                !t.genre.eq_ignore_ascii_case("podcast")
                    && !t.path.components().any(|c| {
                        c.as_os_str()
                            .to_string_lossy()
                            .eq_ignore_ascii_case("podcasts")
                    })
            })
            .map(|(i, _)| i)
            .collect();
        for i in (1..pool.len()).rev() {
            let j = self.random() as usize % (i + 1);
            pool.swap(i, j);
        }
        pool.truncate(50);
        for i in 1..pool.len() {
            if self.library.tracks[pool[i]]
                .artist
                .eq_ignore_ascii_case(&self.library.tracks[pool[i - 1]].artist)
            {
                if let Some(j) = (i + 1..pool.len()).find(|&j| {
                    !self.library.tracks[pool[j]]
                        .artist
                        .eq_ignore_ascii_case(&self.library.tracks[pool[i - 1]].artist)
                }) {
                    pool.swap(i, j);
                }
            }
        }
        if pool.is_empty() {
            self.notify("No music in your library yet");
            return;
        }
        self.queue = pool
            .into_iter()
            .map(|i| self.library.tracks[i].path.clone())
            .collect::<Vec<_>>()
            .into();
        self.queue_index = 0;
        self.play_current(0.0);
        self.open(Screen::NowPlaying);
    }
    fn show_album_options(&mut self, album: usize) {
        let Some(a) = self.library.albums.get(album) else {
            return;
        };
        let mut rows = vec![
            Row::new("Play Next", Intent::QueueAlbum(album, true)),
            Row::new("Add to End of Queue", Intent::QueueAlbum(album, false)),
            Row::new("Add to Playlist…", Intent::AddAlbumPlaylist(album)),
            Row::new("Share", Intent::ShareAlbum(album)),
        ];
        rows[0].visual.queue_mark = "queue_next".into();
        rows[1].visual.queue_mark = "queue_last".into();
        if let Some(&index) = a.track_indices.first() {
            rows.push(Row::new("Edit", Intent::AlbumInfo(index)));
        }
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(Page {
            kind: ViewKind::Full,
            mini: false,
            screen: self.screen(),
            title: a.name.clone(),
            rows: Arc::new(rows),
            selected: 0,
            scroll: 0.0,
            target_scroll: 0.0,
            enter: 0.0,
        });
    }
    fn show_options(&mut self) {
        let album = match self.screen() {
            Screen::Albums | Screen::Artist(_) => self
                .pages
                .last()
                .and_then(|p| p.rows.get(p.selected))
                .and_then(|r| {
                    if let Intent::Open(Screen::Album(i)) = r.intent {
                        Some(i)
                    } else {
                        None
                    }
                }),
            _ => None,
        };
        if let Some(album) = album {
            self.show_album_options(album);
            return;
        }
        let selected = self
            .pages
            .last()
            .and_then(|p| p.rows.get(p.selected))
            .and_then(|r| match r.intent {
                Intent::Song(i) => Some(i),
                Intent::QueueSong(i) => {
                    self.queue.get(i).and_then(|p| self.by_path.get(p)).copied()
                }
                _ => None,
            });
        let mut rows = Vec::new();
        if self.screen() == Screen::Queue {
            if let Some((index, title)) = self
                .pages
                .last()
                .and_then(|p| p.rows.get(p.selected))
                .and_then(|r| {
                    if let Intent::QueueSong(i) = r.intent {
                        Some((i, r.label.clone()))
                    } else {
                        None
                    }
                })
            {
                let mut next = Row::new("Play Next", Intent::MoveQueueNext(index));
                next.visual.queue_mark = "queue_next".into();
                self.options = Some(Page {
                    screen: Screen::Queue,
                    kind: ViewKind::Full,
                    mini: false,
                    title,
                    rows: Arc::new(vec![
                        next,
                        Row::new("Remove from Up Next", Intent::RemoveQueue(index)),
                        Row::new("Cancel", Intent::CloseOptions),
                    ]),
                    selected: 0,
                    scroll: 0.0,
                    target_scroll: 0.0,
                    enter: 0.0,
                });
            }
            return;
        }
        if let Some(i) = selected {
            self.visual_player_song_options(i);
            return;
        }
        let now_playing = self
            .pages
            .last()
            .and_then(|p| p.rows.get(p.selected))
            .is_some_and(|r| matches!(r.intent, Intent::Open(Screen::NowPlaying)));
        if now_playing {
            if self.visual_menu_ask_clear_now_playing() {
                return;
            }
            rows.push(Row::new("Clear Now Playing", Intent::ClearNowPlaying));
        }
        if rows.is_empty() {
            return;
        }
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(Page {
            kind: ViewKind::Full,
            mini: false,
            screen: Screen::Home,
            title: "Options".into(),
            rows: Arc::new(rows),
            selected: 0,
            scroll: 0.0,
            target_scroll: 0.0,
            enter: 0.0,
        });
    }
    fn start_search(&mut self) {
        if self.search_worker.is_some() {
            self.search_pending = Some(self.search_query.clone());
            return;
        }
        let library = Arc::clone(&self.library);
        let playlists = self.extras.playlists_snapshot();
        let query = self.search_query.clone();
        let generation = self.library_generation;
        let (tx, rx) = mpsc::sync_channel(1);
        match thread::Builder::new()
            .name("library-search".into())
            .spawn(move || {
                let result = app_search::search(&library, &playlists, &query);
                let _ = tx.send((generation, query, result));
            }) {
            Ok(worker) => {
                self.search_result = Some(rx);
                self.search_worker = Some(worker);
            }
            Err(e) => self.notify(format!("Search: {e}")),
        }
    }
    fn keyboard_action(&mut self, action: Action) {
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        let effect = keyboard.handle(action);
        let query = keyboard.query.clone();
        match effect {
            KeyboardEffect::Submit | KeyboardEffect::Close => {
                keyboard.hide(self.visual_time);
                match self.keyboard_context {
                    KeyboardContext::Curation => {
                        if matches!(effect, KeyboardEffect::Submit) {
                            self.curation.search_changed(&query);
                        }
                    }
                    KeyboardContext::Sharing => {
                        self.settings.sharing_name = query.trim().chars().take(32).collect();
                        self.extras
                            .set_sharing_name(self.settings.sharing_name.clone());
                        self.save_settings();
                        self.refresh_page();
                        self.refresh_setting_preview();
                    }
                    KeyboardContext::Library => {
                        self.search_query = query;
                        self.start_search();
                        self.refresh_page();
                    }
                }
            }
            KeyboardEffect::Changed => match self.keyboard_context {
                KeyboardContext::Curation => self.curation.search_changed(&query),
                KeyboardContext::Sharing => self.refresh_setting_preview(),
                KeyboardContext::Library => {
                    self.search_query = query;
                    self.start_search();
                }
            },
            KeyboardEffect::None => {}
        }
    }

    pub fn tick(&mut self, dt: f32, renderer: &Renderer) {
        self.visual_time += dt as f64;
        self.visual_video_tick(self.visual_time);
        if self.visual_menu_outgoing_page().is_none() {
            self.retired_quiz = None;
            self.retired_game = None;
            self.extras.release_previous_visual();
        }
        if let Some(status) = self.status_watcher.poll() {
            self.visual_menu.status.battery = status.battery;
            self.visual_menu.status.charging = status.charging;
            self.visual_menu.status.external_power = status.external_power;
            self.visual_menu.status.wifi = status.wifi;
        }

        if let Some((generation, query, result)) = self
            .search_result
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.search_result = None;
            if let Some(worker) = self.search_worker.take() {
                let _ = worker.join();
            }
            if generation == self.library_generation && query == self.search_query {
                self.search_rows = result.rows;
                self.search_views = result.categories;
            } else {
                self.search_pending = Some(self.search_query.clone());
            }
            if self.screen() == Screen::Search {
                self.refresh_page();
            }
            if let Some(query) = self.search_pending.take() {
                self.search_query = query;
                self.start_search();
            }
        }
        if let Some((position, deadline)) = self.np_seek {
            if Instant::now() >= deadline {
                self.np_seek = None;
                if self.restored {
                    self.restored_position = position;
                    self.save_playback();
                } else {
                    self.send(AudioCommand::Seek(position));
                }
            }
        }
        if self.held
            && self.last_activity.elapsed() > Duration::from_secs(5)
            && self.now_track.is_some()
            && !self.video_open
            && self.quiz.is_none()
            && self.screen() != Screen::NowPlaying
        {
            self.leave_saver();
            self.open(Screen::NowPlaying);
        }
        let mut drained = 0;
        while drained < 4 {
            let Some(command) = self.pending_audio.pop_front() else {
                break;
            };
            if !self.audio.command(command.clone()) {
                self.pending_audio.push_front(command);
                break;
            }
            drained += 1;
        }
        let was_playing = self.audio_snapshot.playing;
        self.audio_snapshot = self.audio.snapshot();
        if was_playing && !self.audio_snapshot.playing {
            self.paused_at = self.visual_time;
        }
        if was_playing != self.audio_snapshot.playing {
            self.refresh_playback_menus();
        }
        if self.idle_play_pending.is_some_and(|start| {
            self.audio_snapshot.playing
                || self.audio_snapshot.loading
                || self.audio_snapshot.error.is_some()
                || start.elapsed() > Duration::from_secs(2)
        }) {
            self.idle_play_pending = None;
        }
        if self.audio_snapshot.ended != self.end_seen {
            self.end_seen = self.audio_snapshot.ended;
            if self.quiz.is_none() {
                self.next(false);
            }
        }
        if let Some(effect) = self
            .quiz
            .as_mut()
            .and_then(|quiz| quiz.tick(&self.audio_snapshot))
        {
            self.quiz_effect(effect);
        }
        if let Some(value) = self.volume.get() {
            if !self.used_system_volume {
                self.send(AudioCommand::Volume(1.0));
                self.used_system_volume = true;
            }
            self.settings.volume = value;
        }
        if let Some(result) = self.scan.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.scan = None;
            if let Some(worker) = self.scan_worker.take() {
                let _ = worker.join();
            }
            self.scanning = false;
            match result {
                Ok(library) => self.install_library(library, renderer),
                Err(error) => {
                    self.library_error = Some(error.clone());
                    self.notify(error);
                }
            }
            if self.rescan_pending {
                self.rescan_pending = false;
                self.rescan();
            }
        }
        self.curation.set_time(self.visual_time);
        self.files.set_time(self.visual_time);
        let effects = self.files.tick();
        for effect in effects {
            self.files_effect(effect);
        }
        if self.screen() == Screen::Files {
            if let Some((previous, forward, started)) = self.files.take_navigation() {
                let current = self.files.visual_page();
                let depth = self.files.navigation_depth();
                self.visual_menu_external_navigation(previous, current, depth, forward, started);
            }
        }
        let revision = self.files.library_revision();
        if revision != self.library_info_revision {
            self.library_info_revision = revision;
            if self.screen() == Screen::Library {
                self.refresh_page();
            }
        }
        if let Some(maintenance) = self.maintenance.as_mut() {
            let effects = maintenance.tick();
            for effect in effects {
                self.maintenance_effect(effect);
            }
        }
        let effects = self.extras.tick(dt);
        while let Some(event) = self.extras.poll_cover_search() {
            if let Some(effect) = self.curation.cover_search_event(event) {
                self.curation_effect(effect);
            }
        }
        if let Some(connected) = self.extras.bluetooth_audio_connected() {
            if self.last_bt_audio == Some(true) && !connected {
                self.pause_media();
            }
            self.last_bt_audio = Some(connected);
            self.visual_menu.status.bluetooth = connected;
            self.visual_menu.status.headphones = connected || self.wired_headphones;
        }
        for effect in effects {
            match effect {
                ExtraEffect::Exit => {
                    self.stop_inline_preview();
                    if self.screen() == Screen::Extra && self.pages.len() > 1 {
                        self.pop_navigation_page();
                    }
                }
                ExtraEffect::PreviewOpening { item, queue, index } => {
                    self.begin_inline_preview(item, queue, index)
                }
                ExtraEffect::VideoOpening {
                    item,
                    queue,
                    index,
                    start,
                } => self.visual_video_opening(item, queue, index, start),
                ExtraEffect::VideoResolveError { origin, error } => {
                    self.visual_video_resolve_failed(&origin, error)
                }
                ExtraEffect::Notice(message) => self.notify(message),
                ExtraEffect::MediaArtwork { origin, path } => {
                    if !self.visual_video_artwork(&origin, path.clone())
                        && origin == self.video_origin
                    {
                        self.video_cover = Some(path);
                    }
                }
                ExtraEffect::SharingName(name) => {
                    self.settings.sharing_name = name;
                    self.save_settings();
                }
                ExtraEffect::Rescan => self.rescan(),
                ExtraEffect::DeleteAlbum(paths) => {
                    self.execute(Intent::Maintenance(MaintenanceMode::DeleteAlbum(paths)))
                }
                ExtraEffect::MergeAlbum(paths) => {
                    self.curation
                        .open(CurationMode::Merge { paths }, Arc::clone(&self.library));
                    self.open(Screen::Curation);
                }
                ExtraEffect::ChooseAlbumCover {
                    paths,
                    album,
                    artist,
                } => {
                    self.extras.open_cover_search_all(paths, album, artist);
                }
                ExtraEffect::MergeApplied => {
                    if self.screen() == Screen::Extra && self.pages.len() > 1 {
                        self.pop_navigation_page();
                    }
                    self.close_curation_page();
                }
                ExtraEffect::MergeFailed(error) => {
                    if self.screen() == Screen::Extra && self.pages.len() > 1 {
                        self.pop_navigation_page();
                    }
                    self.notify(error);
                }
                ExtraEffect::Queue { paths, next } => self.enqueue_paths(paths, next),
                ExtraEffect::SharePaths(paths) => self.extras.send_paths_share(paths),
                ExtraEffect::Play { paths, index } => {
                    self.queue_original = None;
                    self.queued_marks.clear();
                    self.queue = Arc::new(paths);
                    self.queue_index = index.min(self.queue.len().saturating_sub(1));
                    self.play_current(0.0);
                    self.open(Screen::NowPlaying);
                }
                ExtraEffect::YouTubeEnabled(enabled) => {
                    self.settings.youtube = enabled;
                    self.save_settings();
                }
                ExtraEffect::Preview {
                    source,
                    origin,
                    queue,
                    index,
                } => {
                    if !self.inline_preview || !self.extras.preview_can_open(&origin) {
                        continue;
                    }
                    if let Err(error) =
                        self.begin_stream(source, false, origin, 0.0, queue, index, true, renderer)
                    {
                        self.inline_preview_failed(error);
                    }
                }
                ExtraEffect::TogglePreview => {
                    if let Some(video) = self.video.as_ref() {
                        let state = video.snapshot();
                        let paused = !state.paused;
                        if video.command(VideoCommand::Pause(paused)) {
                            self.pending_stream_paused = paused;
                            self.extras.preview_set_paused(paused);
                        }
                    }
                }
                ExtraEffect::StopPreview => self.stop_inline_preview(),
                ExtraEffect::Stream {
                    source,
                    video,
                    origin,
                    start,
                    queue,
                    index,
                } => {
                    let opening_origin = origin.clone();
                    if let Err(error) = self
                        .begin_stream(source, video, origin, start, queue, index, false, renderer)
                    {
                        self.visual_video_resolve_failed(&opening_origin, error);
                    }
                }
            }
        }
        if self.screen() == Screen::Extra {
            if let Some((previous, forward, started)) = self.extras.take_navigation() {
                if let Some(current) = self.extras.visual_page() {
                    let depth = self.extras.navigation_depth();
                    self.visual_menu_external_navigation(
                        previous, current, depth, forward, started,
                    );
                }
            }
        }
        if self.screen() == Screen::Curation {
            if let Some((previous, forward, started)) = self.curation.take_navigation() {
                if let Some(current) = self.curation.visual_page() {
                    let depth = self.curation.navigation_depth();
                    self.visual_menu_external_navigation(
                        previous, current, depth, forward, started,
                    );
                }
            }
        }
        if self.screen() == Screen::Extra {
            if let Some(snapshot) = self.extras.visual_page() {
                if let Some(page) = self.pages.last_mut() {
                    *page = snapshot;
                }
            }
        } else if self.screen() == Screen::Files {
            let snapshot = self.files.visual_page();
            if let Some(page) = self.pages.last_mut() {
                *page = snapshot;
            }
        } else if self.screen() == Screen::Curation {
            if let Some(snapshot) = self.curation.visual_page() {
                if let Some(page) = self.pages.last_mut() {
                    *page = snapshot;
                }
            }
        } else if self.screen() == Screen::Maintenance {
            if let Some(snapshot) = self.maintenance.as_mut().and_then(|m| m.visual_page()) {
                if let Some(page) = self.pages.last_mut() {
                    *page = snapshot;
                }
            }
        }

        if self.pending_stream.is_some()
            && !self.audio_snapshot.playing
            && !self.audio_snapshot.loading
        {
            let old_active = self.video.as_ref().is_some_and(|v| {
                let state = v.snapshot();
                state.playing || state.loading
            });
            if !old_active {
                if let Some((source, start, audio_only)) = self.pending_stream.take() {
                    if let Some(video) = self.video.as_ref() {
                        if video.command(VideoCommand::Open {
                            source: source.clone(),
                            start,
                            audio_only,
                        }) {
                            self.video_open = !self.inline_preview;
                            if self.pending_stream_paused {
                                video.command(VideoCommand::Pause(true));
                            }
                            video.command(VideoCommand::Volume(if self.volume.get().is_some() {
                                1.0
                            } else {
                                self.settings.volume
                            }));
                        } else {
                            self.pending_stream = Some((source, start, audio_only));
                        }
                    }
                }
            }
        }
        if self.pending_music.is_some() {
            let old_active = self.video.as_ref().is_some_and(|v| {
                let state = v.snapshot();
                state.playing || state.loading
            });
            if !old_active {
                if let Some(command) = self.pending_music.take() {
                    self.send(command);
                }
            }
        }
        if let Some(video) = self.video.as_ref() {
            if let Some(frame) = video.frame() {
                if let Some(texture) = self
                    .video_texture
                    .filter(|t| t.width == frame.width && t.height == frame.height)
                {
                    let _ = renderer.update_texture(texture, &frame.pixels);
                } else {
                    if let Some(texture) = self.video_texture.take() {
                        renderer.delete_texture(texture);
                    }
                    match renderer.upload(frame.width, frame.height, &frame.pixels) {
                        Ok(texture) => self.video_texture = Some(texture),
                        Err(error) => eprintln!("Video texture: {error}"),
                    }
                }
            }
        }
        if (self.video_open || self.inline_preview) && self.pending_stream.is_none() {
            if let Some(video) = self.video.as_ref() {
                let state = video.snapshot();
                let accepted = if self.inline_preview {
                    self.extras.preview_transport_accepts(state.generation)
                } else {
                    self.screen() == Screen::Video && self.visual_video_auto_next()
                };
                if accepted && state.ended && state.generation != self.video_ended_generation {
                    self.video_ended_generation = state.generation;
                    self.video_next(1);
                }
            }
        }
        if self.inline_preview {
            if let Some(video) = self.video.as_ref() {
                let state = video.snapshot();
                let accepted = self.extras.preview_transport_accepts(state.generation);
                self.extras.set_preview_state(&self.video_origin, &state);
                if accepted
                    && state.error.is_some()
                    && !state.loading
                    && self.pending_stream.is_none()
                {
                    let error = state.error.clone().unwrap();
                    self.inline_preview_failed(error);
                }
            }
        }
        betterpod_rust::weather::poll_icons(renderer);
        self.art.poll(renderer);
        if let Some(page) = self.pages.last_mut() {
            page.enter = (page.enter - dt / 0.3).max(0.0);
            page.scroll += (page.target_scroll - page.scroll) * (1.0 - (-dt * 22.0).exp());
            if (page.target_scroll - page.scroll).abs() < 0.05 {
                page.scroll = page.target_scroll;
            }
        }
        self.cover_flip += ((if self.cover_card.is_some() { 1.0 } else { 0.0 }) - self.cover_flip)
            * (1.0 - (-dt * 20.0).exp());
        self.cover_position +=
            (self.cover_selected as f32 - self.cover_position) * (1.0 - (-dt * 14.0).exp());
        if self.screen() == Screen::Game || self.visual_menu_outgoing_screen() == Some(Screen::Game)
        {
            if let Some(game) = self.game.as_mut() {
                game.set_menu_open(self.options.is_some());
                game.tick(dt);
            }
        }
        if let Some(effect) = self.game.as_mut().and_then(Games::take_effect) {
            match effect {
                GameEffect::Quit => {
                    self.save_games();
                    self.retired_game = self.game.take();
                    if self.pages.len() > 1 {
                        let previous = self.pages.pop();
                        self.visual_menu_navigation(previous, false);
                    }
                }
                GameEffect::ToggleClock => {
                    if let Some(game) = self.game.as_ref() {
                        self.execute(Intent::GameClock(game.title().into()));
                    }
                }
            }
        }
        if self.screen() == Screen::Puzzle
            || self.visual_menu_outgoing_screen() == Some(Screen::Puzzle)
        {
            if let Some(puzzle) = self.puzzle.as_mut() {
                puzzle.tick(dt);
            }
        }
        if self.auto_saver || matches!(self.screen(), Screen::Visualizer | Screen::NowPlaying) {
            self.saver.set_time(self.visual_time);
            self.saver.set_playback(
                self.audio_snapshot.playing,
                self.audio_snapshot.paused,
                self.settings.volume,
            );
            self.saver.set_dark(self.dark());
            let status = self.visual_menu.status;
            self.saver.set_status(saver::SaverStatus {
                battery: status.battery.map(i32::from),
                charging: status.charging,
                wifi: status.wifi,
                bluetooth: status.bluetooth,
                headphones: status.headphones,
                held: self.held,
            });
            self.audio.visualizer(&mut self.samples);
            let rms = (self.samples.iter().map(|s| s * s).sum::<f32>() / self.samples.len() as f32)
                .sqrt();
            self.meter += ((rms * 3.0).min(1.0) - self.meter) * (1.0 - (-dt * 16.0).exp());
            if self.auto_saver || self.screen() == Screen::Visualizer {
                self.saver.samples(&self.samples, self.audio_snapshot.rate);
                if let Some(palette) = self
                    .art
                    .palette(self.now_track.as_ref().and_then(|t| t.cover.as_deref()))
                {
                    self.saver.set_album_palette(&palette);
                }
                self.saver.update(dt, &self.library, self.current_index());
            }
        }
        if let Some(action) = self.saver.poll() {
            self.saver_action(action);
        }
        let preview_waiting = self.inline_preview && self.extras.preview_waiting();
        let resolving_video = self.visual_video_waiting();
        let video_active = preview_waiting
            || resolving_video
            || self.pending_stream.is_some()
            || (self.video_open || self.inline_preview)
                && self.video.as_ref().is_some_and(|video| {
                    let state = video.snapshot();
                    state.playing || state.loading
                });
        let video_paused = if self.video_open || self.inline_preview {
            self.video.as_ref().map(|video| video.snapshot().paused)
        } else {
            None
        };
        let paused_intent = idle_pause_intent(
            self.audio_snapshot.paused,
            video_paused,
            preview_waiting
                || resolving_video
                || self.pending_stream.is_some()
                || self.pending_music.is_some()
                || self.idle_play_pending.is_some(),
            self.idle_stopped,
        );
        if let Err(error) = self.idle.update(
            self.audio_snapshot.playing
                || self.audio_snapshot.loading
                || video_active
                || self.pending_music.is_some()
                || self.idle_play_pending.is_some(),
            paused_intent,
        ) {
            if self.last_idle_error.as_deref() != Some(&error) {
                eprintln!("Idle guard: {error}");
                self.last_idle_error = Some(error);
            }
        }
        if self
            .menu_down
            .is_some_and(|start| start.elapsed() >= Duration::from_secs(5))
        {
            self.user_quit = true;
            self.running = false;
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|t| Instant::now() >= t.until)
        {
            self.toast = None;
        }
        if let Some(best) = self.quiz.as_mut().and_then(Quiz::take_best) {
            if let Ok(bytes) = serde_json::to_vec(&best) {
                self.game_saves.insert("Guess the Song Best".into(), bytes);
                self.save_games();
            }
        }
        if self.audio_snapshot.playing && self.last_saved.elapsed() > Duration::from_secs(15) {
            self.save_playback();
            self.save_games();
        }
        if self.settings.screensaver_seconds > 0
            && self.audio_snapshot.playing
            && self.quiz.is_none()
            && !self.screen_off
            && !self.extras.prevents_screensaver()
            && self.last_activity.elapsed()
                > Duration::from_secs(self.settings.screensaver_seconds as u64)
            && self.screen() != Screen::Visualizer
        {
            if !self.held && !self.video_open && !self.auto_saver {
                self.enter_saver();
            }
        }
        if self.clock_updated.elapsed() >= Duration::from_secs(1) {
            self.clock = local_clock();
            self.clock_updated = Instant::now();
        }
        self.phase = match self.screen() {
            Screen::CoverFlow => "coverflow",
            Screen::NowPlaying => "nowplaying",
            Screen::Visualizer => "visualizer",
            Screen::Game => "game",
            Screen::Albums => "albums",
            Screen::Video => "video",
            Screen::Extra => "services",
            _ => "home",
        };
    }

    pub fn draw(&mut self, renderer: &mut Renderer, elapsed: f64) {
        self.visual_time = elapsed;
        self.visual_player_tick(elapsed, 0.0);

        if self.screen() == Screen::Extra {
            if let Some(page) = self.extras.visual_page() {
                let height = self.visual_menu_content_height(&page);
                self.extras.set_viewport_height(height);
            }
        }
        self.ensure_selection_viewport();
        self.visual_menu_tick(elapsed);
        let (bg, fg, muted) = self.colors();
        renderer.begin(if self.screen_off {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            bg
        });
        if self.screen_off {
            return;
        }
        let screen = self.screen();
        let shared_split =
            screen == Screen::Extra && self.extras.visual_page().is_some_and(|page| page.split());
        let shared_menu = app_visual_menu::is_menu(screen) || shared_split;
        if self.auto_saver {
            if self.screen() == Screen::NowPlaying {
                self.visual_player_saver_hide(self.saver.fly_hides_np());
                self.visual_player_draw_now_playing(renderer);
                let title = self
                    .pages
                    .last()
                    .map(|p| p.title.clone())
                    .unwrap_or_default();
                self.visual_menu_draw_header(renderer, &title, 0.0, 640.0, true);
            } else if shared_menu {
                self.visual_menu_draw(renderer, elapsed, fg, muted);
            }
            self.saver.set_time(elapsed);
            let current = self.current_index();
            if let Err(error) =
                self.saver
                    .draw(renderer, &mut self.art, &self.library, current, &self.clock)
            {
                eprintln!("Screensaver: {error}");
            }
        } else {
            let custom = !shared_menu;
            if custom && screen != Screen::CoverFlow {
                self.visual_menu_draw_outgoing(renderer);
                self.visual_draw_outgoing_custom(renderer, elapsed, fg, muted);
                let offset = self.visual_menu_active_offset();
                let whole = self
                    .visual_menu_outgoing_page()
                    .is_none_or(|(_, _, whole)| whole);
                renderer.push_clip(
                    0.0,
                    if whole { 0.0 } else { 40.0 },
                    640.0,
                    if whole { 480.0 } else { 440.0 },
                );
                renderer.push_transform(offset, 0.0, 1.0, 1.0, 1.0);
                renderer.rect(0.0, 0.0, 640.0, 480.0, bg);
            }
            self.visual_draw_screen(screen, renderer, elapsed, fg, muted);
        }
        if !self.auto_saver
            && !shared_menu
            && screen != Screen::CoverFlow
            && self
                .visual_menu_outgoing_page()
                .is_some_and(|(_, _, whole)| !whole)
        {
            renderer.pop_transform();
            renderer.pop_clip();
            renderer.push_clip(0.0, 0.0, 640.0, 480.0);
            renderer.push_transform(0.0, 0.0, 1.0, 1.0, 1.0);
        }
        let title = self
            .pages
            .last()
            .map(|p| p.title.as_str())
            .unwrap_or("betterPod")
            .to_owned();
        if !self.auto_saver
            && !shared_menu
            && !matches!(self.screen(), Screen::Visualizer)
            && self.screen() != Screen::Video
        {
            let title = if self.screen() == Screen::CoverFlow && self.cover_from_np {
                "Now Playing".to_owned()
            } else {
                title.clone()
            };
            let width = if self.pages.last().is_some_and(Page::split) {
                320.0
            } else {
                640.0
            };
            self.visual_menu_draw_header(renderer, &title, 0.0, width, true);
        }
        if !self.auto_saver {
            if !shared_menu && screen != Screen::CoverFlow {
                renderer.pop_transform();
                renderer.pop_clip();
            }
            if screen == Screen::CoverFlow {
                self.visual_menu_draw_outgoing(renderer);
            }
        }
        let dark = self.dark();
        if let Some(keyboard) = self.keyboard.as_mut() {
            keyboard.draw(renderer, &self.visual_assets, dark, elapsed);
        }
        if self.keyboard.as_ref().is_some_and(|k| k.finished(elapsed)) {
            self.keyboard = None;
        }
        self.visual_feedback_draw(renderer);
        if let Some(start) = self.menu_down {
            if start.elapsed() >= Duration::from_secs(2) {
                renderer.rect(100.0, 190.0, 440.0, 86.0, [0.04, 0.05, 0.07, 0.95]);
                renderer.centered("Keep holding MENU to exit", 110.0, 205.0, 420.0, 20, WHITE);
                renderer.rect(
                    120.0,
                    246.0,
                    400.0 * (start.elapsed().as_secs_f32() / 5.0).min(1.0),
                    5.0,
                    BLUE,
                );
            }
        }
    }
    pub fn benchmark(&mut self, elapsed: f64) {
        if self.scanning || self.library.tracks.is_empty() {
            return;
        }
        let start = *self.benchmark_started.get_or_insert(elapsed);
        let t = (elapsed - start).max(0.0);
        let phase = (t / 4.0) as usize;
        if phase != self.benchmark_phase {
            self.benchmark_phase = phase;
            self.benchmark_step = 0;
            if self.audio_snapshot.path.is_none() && !self.audio_snapshot.loading {
                self.queue = self
                    .library
                    .tracks
                    .iter()
                    .map(|t| t.path.clone())
                    .collect::<Vec<_>>()
                    .into();
                self.queue_index = 0;
                self.play_current(0.0);
            }
            self.pages.clear();
            self.pages.push(self.page(Screen::Home));
            let screen = match phase % 5 {
                0 => Screen::Home,
                1 => Screen::CoverFlow,
                2 => Screen::NowPlaying,
                3 => Screen::Albums,
                _ => Screen::Visualizer,
            };
            if screen != Screen::Home {
                self.open(screen);
            }
            eprintln!("Rust benchmark phase: {screen:?}");
        }
        let step = (t * 8.0) as u64;
        if step != self.benchmark_step {
            self.benchmark_step = step;
            match phase % 5 {
                1 => {
                    if self.library.albums.len() > 1 {
                        let span = (self.library.albums.len() - 1).min(8);
                        let at = (step as usize) % (span * 2);
                        self.cover_selected = if at <= span { at } else { span * 2 - at };
                    }
                }
                3 => {
                    let len = self.pages.last().map_or(0, |p| p.rows.len());
                    if len > 0 {
                        let span = (len - 1).min(18);
                        let at = if span > 0 {
                            step as usize % (span * 2)
                        } else {
                            0
                        };
                        let height = self
                            .pages
                            .last()
                            .map(|page| self.visual_menu_content_height(page))
                            .unwrap_or(440.0);
                        if let Some(page) = self.pages.last_mut() {
                            page.selected = if at <= span { at } else { span * 2 - at };
                            page.reveal_selection_in(height);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn enter_saver(&mut self) {
        if !self.auto_saver && self.settings.screensaver_dim > 0 {
            self.saver_brightness = self.platform.brightness();
            if let Some(level) = self.saver_brightness {
                let _ = self
                    .platform
                    .set_brightness(level.min(self.settings.screensaver_dim as i32));
            }
        }
        self.auto_saver = true;
        self.configure_saver(false);
        self.saver.restart();
        if self.screen() == Screen::NowPlaying && self.visual_player_can_fly() {
            let tilt = self.visual_player_begin_saver();
            self.saver.begin_from_np(tilt);
        }
    }
    fn saver_action(&mut self, action: SaverAction) {
        match action {
            SaverAction::Exit => {
                if self.auto_saver {
                    self.leave_saver();
                } else if self.pages.len() > 1 {
                    let previous = self.pages.pop();
                    self.visual_menu_navigation(previous, false);
                }
            }
            SaverAction::OpenAlbum(index) => {
                self.leave_saver();
                self.open(Screen::Album(index));
            }
            SaverAction::AlbumOptions(index) => self.show_album_options(index),
            SaverAction::SongOptions(index) => self.visual_player_song_options(index),
            SaverAction::PlayTrack(index) => {
                let pages = self.pages.clone();
                self.song(index);
                self.pages = pages;
            }
        }
    }
    fn leave_saver(&mut self) {
        self.visual_player_end_saver(self.audio_snapshot.playing);
        self.auto_saver = false;
        if let Some(level) = self.saver_brightness.take() {
            let _ = self.platform.set_brightness(level);
        }
    }
    fn close_curation_page(&mut self) {
        if self.screen() == Screen::Curation && self.pages.len() > 1 {
            self.pop_navigation_page();
        }
        self.curation.close();
    }
    fn configure_saver(&mut self, visualizer: bool) {
        self.saver.set_style(if visualizer {
            "viz"
        } else {
            &self.settings.screensaver_style
        });
        self.saver.set_dark(self.dark());
        self.saver.set_effect(self.settings.screensaver_effect);
        self.saver.set_palette(self.settings.screensaver_colors);
    }
    fn restore_stream_music(&mut self) {
        let preserve_video_resolution = self.visual_video_has_opening();
        if let Some(saved) = self.stream_restore.take() {
            self.queue = saved.queue;
            self.queue_index = saved.index;
            self.restored_position = saved.position;
            self.restored = true;
            self.now_track = self.current_index().map(|i| self.library.tracks[i].clone());
            if !saved.paused && !self.queue.is_empty() {
                self.play_current_inner(saved.position, preserve_video_resolution);
            }
        }
    }
    fn video_next(&mut self, delta: isize) {
        if self.inline_preview {
            if delta > 0 {
                self.extras.advance_preview();
            }
            return;
        }
        let index = self.video_index as isize + delta;
        if index < 0 || index as usize >= self.video_queue.len() {
            return;
        }
        let item = self.video_queue[index as usize].clone();
        let video = item.kind != "audio";
        self.extras
            .resolve_media(item, self.video_queue.clone(), index as usize, video, 0.0);
    }
    fn video_overlay_visible(&self) -> bool {
        self.video_audio_only
            || self.video_overlay.elapsed() < Duration::from_millis(2500)
            || self.pending_stream.is_some()
            || self.video.as_ref().is_some_and(|v| {
                let s = v.snapshot();
                s.paused || s.loading || s.error.is_some() || s.ended
            })
    }
    fn remember_video(&mut self) {
        if self.video_open && !self.video_origin.is_empty() {
            if let Some(video) = self.video.as_ref() {
                let state = video.snapshot();
                self.extras.remember_video(
                    self.video_origin.clone(),
                    self.video_title.clone(),
                    state.position,
                    state.duration,
                );
            }
        }
    }
    fn draw_video(&mut self, r: &mut Renderer) {
        self.visual_video_draw(r, self.visual_time);
    }
    fn save_games(&mut self) {
        if let Some(game) = self.game.as_ref() {
            if let Ok(snapshot) = game.serialize_snapshot() {
                self.game_saves.insert(game.title().into(), snapshot);
            }
        }
        if let Some(puzzle) = self.puzzle.as_ref() {
            if let Some(snapshot) = puzzle.serialize_snapshot() {
                self.game_saves.insert(puzzle.id().into(), snapshot);
            }
        }
        if let Ok(bytes) = serde_json::to_vec(&self.game_saves) {
            if let Err(error) = self.store.save_games_snapshot(&bytes) {
                eprintln!("Game save: {error}");
            }
        }
    }
    pub fn shutdown(&mut self, renderer: &Renderer) -> Result<(), String> {
        if let Some(request) = self
            .maintenance
            .as_mut()
            .and_then(|m| m.quiesce_for_shutdown())
        {
            if self.maintenance_request.is_none() {
                self.maintenance_request = Some(request);
            }
        }
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.leave_saver();
        self.scan.take();
        self.search_result.take();
        if let Some(worker) = self.search_worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.scan_worker.take() {
            let _ = worker.join();
        }
        self.save_games();
        self.save_playback();
        self.save_settings();
        self.remember_video();
        if let Some(video) = self.video.as_mut() {
            video.shutdown();
        }
        if let Some(texture) = self.video_texture.take() {
            renderer.delete_texture(texture);
        }
        self.audio.shutdown();
        self.extras.shutdown();
        betterpod_rust::weather::close_icons(renderer);
        self.files.shutdown();
        self.curation.shutdown();
        self.volume.shutdown();
        self.idle.close();
        self.saver.close(renderer);
        self.art.close(renderer);
        self.visual_assets.close(renderer);
        self.status_watcher.close();
        if self.screen_off {
            let _ = self.platform.set_screen_off(false);
            self.screen_off = false;
        }
        self.platform.set_performance(false)?;
        self.store.flush()
    }
}

fn time_label(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn local_clock() -> String {
    let mut now: libc::time_t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&mut now, &mut parts) }.is_null() {
        return String::new();
    }
    format!("{:02}:{:02}", parts.tm_hour, parts.tm_min)
}
fn idle_pause_intent(
    audio_paused: bool,
    video_paused: Option<bool>,
    handoff_pending: bool,
    stopped: bool,
) -> bool {
    if handoff_pending {
        false
    } else {
        video_paused.unwrap_or(audio_paused || stopped)
    }
}
#[cfg(test)]
mod idle_intent_tests {
    use super::*;
    #[test]
    fn natural_track_handoff_keeps_guard_grace() {
        assert!(!idle_pause_intent(false, None, false, false));
        assert!(!idle_pause_intent(true, None, true, false));
    }
    #[test]
    fn video_pause_is_independent_of_suspended_local_music() {
        assert!(!idle_pause_intent(true, Some(false), false, false));
        assert!(idle_pause_intent(true, Some(true), false, false));
        assert!(!idle_pause_intent(true, Some(true), true, false));
    }
    #[test]
    fn explicit_music_pause_and_stop_release_guard() {
        assert!(idle_pause_intent(true, None, false, false));
        assert!(idle_pause_intent(false, None, false, true));
    }
}

#[cfg(test)]
#[path = "app_visual_qa.rs"]
mod visual_qa;
