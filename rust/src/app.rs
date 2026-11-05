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
use crate::games::{GameKind, Games};
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

const BG: Color = [0.96, 0.96, 0.97, 1.0];
const FG: Color = [0.10, 0.11, 0.13, 1.0];
const MUTED: Color = [0.43, 0.45, 0.49, 1.0];
const WHITE: Color = [1.0; 4];
const BLUE: Color = [0.12, 0.48, 0.88, 1.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
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
    NowPlaying,
    CoverFlow,
    Visualizer,
    Game,
    Extra,
    Puzzles,
    Puzzle,
    Video,
    GameClocks,
    Quiz,
    Curation,
    Maintenance,
    Files,
}
#[derive(Clone)]
enum Intent {
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
    ClearNowPlaying,
    Setting(SettingValue),
    Files(FilesMode),
}
#[derive(Clone)]
struct Row {
    label: String,
    detail: String,
    cover: Option<PathBuf>,
    intent: Intent,
    arrow: bool,
}
impl Row {
    fn new(label: impl Into<String>, intent: Intent) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            cover: None,
            intent,
            arrow: true,
        }
    }
}
struct Page {
    screen: Screen,
    title: String,
    rows: Arc<Vec<Row>>,
    selected: usize,
    scroll: f32,
    target_scroll: f32,
    enter: f32,
}
impl Page {
    fn split(&self) -> bool {
        matches!(
            self.screen,
            Screen::Home | Screen::Music | Screen::Settings | Screen::Apps
        )
    }
    fn row_height(&self) -> f32 {
        if matches!(
            self.screen,
            Screen::Albums | Screen::Songs | Screen::Queue | Screen::Search
        ) {
            54.0
        } else {
            44.0
        }
    }
    fn reveal_selection(&mut self) {
        let h = self.row_height();
        let available = 350.0;
        let top = self.selected as f32 * h;
        if top < self.target_scroll {
            self.target_scroll = top;
        }
        if top + h > self.target_scroll + available {
            self.target_scroll = top + h - available;
        }
        self.target_scroll = self.target_scroll.max(0.0);
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
                    row.detail = t.artist.clone();
                    row.cover = t.cover.clone();
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
                Arc::new(
                    a.track_indices
                        .iter()
                        .filter_map(|&i| {
                            songs.get(i).cloned().map(|mut row| {
                                row.detail = time_label(library.tracks[i].duration);
                                row
                            })
                        })
                        .collect(),
                )
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
            .map(|a| Arc::new(by_artist.remove(a.as_str()).unwrap_or_default()))
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
struct Keyboard {
    layout: u8,
    selected: usize,
    query: String,
}
struct Toast {
    text: String,
    until: Instant,
}

pub struct App {
    pub running: bool,
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
    queue: Arc<Vec<PathBuf>>,
    queue_index: usize,
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
    library_generation: u64,
    search_result: Option<mpsc::Receiver<(u64, Arc<Vec<Row>>)>>,
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
    options: Option<Page>,
    game: Option<Games>,
    quiz: Option<Quiz>,
    curation: Curation,
    files: Files,
    quiz_restore: Option<SavedPlayback>,
    puzzle: Option<PuzzleGame>,
    game_saves: HashMap<String, Vec<u8>>,
    video: Option<VideoPlayer>,
    video_open: bool,
    video_texture: Option<Texture>,
    video_overlay: Instant,
    video_title: String,
    video_cover: Option<PathBuf>,
    video_origin: String,
    video_audio_only: bool,
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
            queue: Arc::new(Vec::new()),
            queue_index: 0,
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
            options: None,
            game: None,
            quiz: None,
            curation: Curation::new(),
            files: Files::new(music.clone(), data.clone()),
            quiz_restore: None,
            puzzle: None,
            game_saves,
            video: None,
            video_open: false,
            video_texture: None,
            video_overlay: Instant::now(),
            video_title: String::new(),
            video_cover: None,
            video_origin: String::new(),
            video_audio_only: false,
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
        };
        app.extras.set_youtube(app.settings.youtube);
        app.extras
            .set_sharing_name(app.settings.sharing_name.clone());
        app.configure_saver(false);
        app.pages.push(app.page(Screen::Home));
        app.rescan();
        Ok(app)
    }

    fn screen(&self) -> Screen {
        self.pages.last().map_or(Screen::Home, |p| p.screen)
    }
    fn dark(&self) -> bool {
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
    fn colors(&self) -> (Color, Color, Color) {
        if self.dark() {
            (
                [0.075, 0.08, 0.095, 1.0],
                [0.96, 0.97, 0.99, 1.0],
                [0.65, 0.67, 0.72, 1.0],
            )
        } else {
            (BG, FG, MUTED)
        }
    }
    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + Duration::from_secs(3),
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
        let retired_index = std::mem::replace(&mut self.by_path, by_path);
        self.library_generation = self.library_generation.wrapping_add(1);
        if let Some(saved) = self.restore.take() {
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
                        self.library
                            .albums
                            .iter()
                            .position(|a| a.name == old.name && a.artist == old.artist)
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
            page.reveal_selection();
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
                rows.push(Row::new("Cover Flow", Intent::Open(Screen::CoverFlow)));
                rows.push(Row::new("Music", Intent::Open(Screen::Music)));
                if !self.settings.hide_podcasts {
                    rows.push(Row::new("Podcasts", Intent::Extra(ExtraPage::Podcasts)));
                }
                if self.settings.youtube {
                    rows.push(Row::new("Videos", Intent::Extra(ExtraPage::Videos)));
                }
                if self
                    .maintenance
                    .as_ref()
                    .is_some_and(|m| m.update_available())
                {
                    rows.push(Row::new(
                        "Update Available",
                        Intent::Maintenance(MaintenanceMode::Update),
                    ));
                }
                rows.push(Row::new("Genius", Intent::Genius));
                rows.push(Row::new("Apps", Intent::Open(Screen::Apps)));
                rows.push(Row::new("Settings", Intent::Open(Screen::Settings)));
                if self.now_track.is_some() {
                    rows.push(Row::new("Now Playing", Intent::Open(Screen::NowPlaying)));
                }
                if self.settings.apple {
                    "iPod".into()
                } else {
                    "betterPod Rust".into()
                }
            }
            Screen::Music => {
                rows.push(Row::new("Cover Flow", Intent::Open(Screen::CoverFlow)));
                rows.push(Row::new("Playlists", Intent::Extra(ExtraPage::Playlists)));
                rows.push(Row::new("Albums", Intent::Open(Screen::Albums)));
                rows.push(Row::new("Artists", Intent::Open(Screen::Artists)));
                rows.push(Row::new("Songs", Intent::Open(Screen::Songs)));
                rows.push(Row::new("Search", Intent::Open(Screen::Search)));
                rows.push(Row::new(
                    "Search Online",
                    Intent::Extra(ExtraPage::SearchSources),
                ));
                rows.push(Row::new("Downloaded", Intent::Extra(ExtraPage::Downloads)));
                rows.push(Row::new("Receive Music", Intent::Extra(ExtraPage::Receive)));
                rows.push(Row::new("Shuffle Songs", Intent::ShuffleSongs));
                rows.push(Row::new("Up Next", Intent::Open(Screen::Queue)));
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
                self.library
                    .artists
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| "Artist".into())
            }
            Screen::Album(index) => {
                cached = self.views_album_tracks.get(index).cloned();
                self.library
                    .albums
                    .get(index)
                    .map_or("Album", |a| a.name.as_str())
                    .into()
            }
            Screen::Songs => {
                cached = Some(Arc::clone(&self.views_songs));
                "Songs".into()
            }
            Screen::Queue => {
                for (i, path) in self.queue.iter().enumerate() {
                    let track = self
                        .by_path
                        .get(path)
                        .and_then(|&i| self.library.tracks.get(i));
                    let label = track.map(|t| t.title.clone()).unwrap_or_else(|| {
                        path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    });
                    let mut row = Row::new(label, Intent::QueueSong(i));
                    row.detail = track.map(|t| t.artist.clone()).unwrap_or_default();
                    row.cover = track.and_then(|t| t.cover.clone());
                    rows.push(row);
                }
                rows.push(Row::new("Clear Up Next", Intent::ClearUpcoming));
                rows.push(Row::new("Clear Now Playing", Intent::ClearNowPlaying));
                "Up Next".into()
            }
            Screen::Settings => {
                rows.push(Row::new(
                    format!("Volume: {}%", (self.settings.volume * 100.0).round() as u32),
                    Intent::Volume,
                ));
                rows.push(Row::new(
                    format!(
                        "Shuffle: {}",
                        if self.settings.shuffle { "On" } else { "Off" }
                    ),
                    Intent::Shuffle,
                ));
                rows.push(Row::new(
                    format!(
                        "Repeat: {}",
                        match self.settings.repeat.as_str() {
                            "all" => "All",
                            "one" => "One",
                            _ => "Off",
                        }
                    ),
                    Intent::Repeat,
                ));
                rows.push(Row::new(
                    format!("Theme: {}", self.settings.theme),
                    Intent::Theme,
                ));
                if self.maintenance.is_some() {
                    rows.push(Row::new(
                        "Music Folder",
                        Intent::Maintenance(MaintenanceMode::Music),
                    ));
                    rows.push(Row::new(
                        "Database",
                        Intent::Maintenance(MaintenanceMode::Database),
                    ));
                    rows.push(Row::new(
                        "Maintenance",
                        Intent::Maintenance(MaintenanceMode::Maintenance),
                    ));
                    rows.push(Row::new(
                        "Software Update",
                        Intent::Maintenance(MaintenanceMode::Update),
                    ));
                }
                rows.push(Row::new(
                    format!("Sharing: {}", self.settings.sharing_name),
                    Intent::Extra(ExtraPage::Sharing),
                ));
                rows.push(Row::new("Bluetooth", Intent::Extra(ExtraPage::Bluetooth)));
                rows.push(Row::new("YouTube Tools", Intent::Extra(ExtraPage::Tools)));
                rows.push(Row::new(
                    "Library Information",
                    Intent::Files(FilesMode::Stats),
                ));
                rows.push(Row::new("Browse Files", Intent::Files(FilesMode::Browse)));
                rows.push(Row::new(
                    "Scanned Folders",
                    Intent::Files(FilesMode::Scanned),
                ));
                rows.push(Row::new("Rescan Library", Intent::Rescan));
                rows.push(Row::new(
                    "Missing Info",
                    Intent::Curation(CurationMode::MissingInfo),
                ));
                rows.push(Row::new(
                    "Missing Covers",
                    Intent::Curation(CurationMode::MissingCovers),
                ));
                rows.push(Row::new("Visualizer", Intent::Open(Screen::Visualizer)));
                rows.push(Row::new(
                    format!(
                        "Effects: {}",
                        if self.settings.effects == "light" {
                            "Light"
                        } else {
                            "Full"
                        }
                    ),
                    Intent::Effects,
                ));
                rows.push(Row::new(
                    format!(
                        "Turn It Apple: {}",
                        if self.settings.apple { "On" } else { "Off" }
                    ),
                    Intent::Apple,
                ));
                rows.push(Row::new(
                    format!(
                        "YouTube: {}",
                        if self.settings.youtube { "On" } else { "Off" }
                    ),
                    Intent::YouTube,
                ));
                rows.push(Row::new(
                    format!(
                        "Show Podcasts: {}",
                        if self.settings.hide_podcasts {
                            "Off"
                        } else {
                            "On"
                        }
                    ),
                    Intent::Podcasts,
                ));
                rows.push(Row::new(
                    format!(
                        "Screensaver: {}",
                        if self.settings.screensaver_seconds == 0 {
                            "Never".into()
                        } else {
                            format!("{} min", self.settings.screensaver_seconds / 60)
                        }
                    ),
                    Intent::SaverTime,
                ));
                rows.push(Row::new("Start Screensaver Now", Intent::StartSaver));
                rows.push(Row::new(
                    format!(
                        "Screensaver Dimming: {}",
                        if self.settings.screensaver_dim == 0 {
                            "Off".into()
                        } else {
                            format!("{}%", self.settings.screensaver_dim)
                        }
                    ),
                    Intent::SaverDim,
                ));
                rows.push(Row::new(
                    format!("Screensaver Style: {}", self.settings.screensaver_style),
                    Intent::SaverStyle,
                ));
                rows.push(Row::new(
                    format!(
                        "Saver Effect: {}",
                        saver::effect_name(
                            &self.settings.screensaver_style,
                            self.settings.screensaver_effect
                        )
                    ),
                    Intent::SaverEffect,
                ));
                rows.push(Row::new(
                    format!(
                        "Saver Colors: {}",
                        saver::palette_name(self.settings.screensaver_colors)
                    ),
                    Intent::SaverColors,
                ));
                rows.push(Row::new("Game Clocks", Intent::Open(Screen::GameClocks)));
                rows.push(Row::new("About", Intent::Open(Screen::About)));
                "Settings".into()
            }
            Screen::Apps => {
                rows.push(Row::new("Guess Song", Intent::Quiz));
                rows.push(Row::new("2048", Intent::Game(GameKind::TwentyFortyEight)));
                rows.push(Row::new("Brick", Intent::Game(GameKind::Brick)));
                rows.push(Row::new("Solitaire", Intent::Game(GameKind::Solitaire)));
                rows.push(Row::new(
                    "Five Letters",
                    Intent::Game(GameKind::FiveLetters),
                ));
                rows.push(Row::new("Puzzles", Intent::Open(Screen::Puzzles)));
                rows.push(Row::new("Weather", Intent::Extra(ExtraPage::Weather)));
                rows.push(Row::new("Visualizer", Intent::Open(Screen::Visualizer)));
                "Apps".into()
            }
            Screen::Search => {
                if self.search_rows.is_empty() {
                    rows.push(Row::new(
                        if self.search_query.is_empty() {
                            "Type a Search".into()
                        } else {
                            format!("Search: {}", self.search_query)
                        },
                        Intent::SearchKeyboard,
                    ));
                } else {
                    cached = Some(Arc::clone(&self.search_rows));
                }
                "Search".into()
            }
            Screen::Files => self.files.title().into(),
            Screen::Maintenance => self
                .maintenance
                .as_ref()
                .map_or("Maintenance", |m| m.title())
                .into(),
            Screen::Curation => "Library".into(),
            Screen::Quiz => "Guess Song".into(),
            Screen::About => "About".into(),
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
                    rows.push(Row::new(info.name, Intent::Puzzle(i)));
                }
                "Puzzles".into()
            }
            Screen::Puzzle => self.puzzle.as_ref().map_or("Puzzle", |p| p.title()).into(),
        };
        Page {
            screen,
            title,
            rows: cached.unwrap_or_else(|| Arc::new(rows)),
            selected: 0,
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
            self.pages.push(self.page(screen));
        }
        self.preview_focus = false;
        if screen == Screen::Settings {
            self.refresh_setting_preview();
        }
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
        page.reveal_selection();
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
                if self.restored {
                    self.restored_position
                } else {
                    self.audio_snapshot.position
                },
                !self.audio_snapshot.playing,
            )
        };
        if let Err(error) = self
            .store
            .save_playback_shared(queue, index, position, paused)
        {
            eprintln!("Playback state: {error}");
        }
        self.last_saved = Instant::now();
    }
    fn set_volume(&mut self, delta: f32) {
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
        self.notify(format!(
            "Volume {}%",
            (self.settings.volume * 100.0).round() as u32
        ));
    }
    fn current_index(&self) -> Option<usize> {
        self.queue
            .get(self.queue_index)
            .and_then(|p| self.by_path.get(p))
            .copied()
    }
    fn play_current(&mut self, start: f64) {
        self.extras.cancel_media();
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
        self.now_track = Some(track);
        self.restored = false;
        self.restored_position = 0.0;
        if let Some(path) = self.queue.get(self.queue_index + 1) {
            self.send(AudioCommand::Prepare { path: path.clone() });
        }
        self.save_playback();
    }
    fn song(&mut self, index: usize) {
        let screen = self.screen();
        let mut order: Vec<usize> = match screen {
            Screen::Album(album) => self
                .library
                .albums
                .get(album)
                .map(|a| a.track_indices.clone())
                .unwrap_or_default(),
            Screen::Songs => (0..self.library.tracks.len()).collect(),
            Screen::Search => self
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
        let first = self.queue_index.saturating_add(1);
        for i in (first + 1..self.queue.len()).rev() {
            let j = first + self.random() as usize % (i - first + 1);
            Arc::make_mut(&mut self.queue).swap(i, j);
        }
    }
    fn next(&mut self, manual: bool) {
        if let Some(quiz) = self.quiz.as_mut() {
            let effect = quiz.next(&self.library);
            self.quiz_effect(effect);
            return;
        }
        if self.video_open || self.screen() == Screen::Video {
            self.video_next(1);
            return;
        }
        if self.queue.is_empty() {
            return;
        }
        if !manual && self.settings.repeat == "one" {
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
            if !manual {
                self.send(AudioCommand::Stop);
                self.save_playback();
            }
            if !self.auto_saver && !self.held {
                self.options = Some(Page {
                    screen: self.screen(),
                    title: "End of the Queue".into(),
                    rows: vec![
                        Row::new("Genius — 50 Random Songs", Intent::Genius),
                        Row::new("Go to Music", Intent::Open(Screen::Music)),
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
    }
    fn previous(&mut self) {
        if self.quiz.is_some() {
            self.send(AudioCommand::Pause);
            return;
        }
        if self.video_open || self.screen() == Screen::Video {
            self.video_next(-1);
            return;
        }
        if self.queue.is_empty() {
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
        if self.quiz.is_some() {
            self.send(AudioCommand::TogglePause);
            return;
        }
        if self.video_open || self.pending_stream.is_some() {
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
            Intent::Open(screen) => {
                if screen == Screen::Visualizer {
                    self.configure_saver(true);
                }
                self.open(screen)
            }
            Intent::Song(i) => self.song(i),
            Intent::QueueSong(i) => {
                if i < self.queue.len() {
                    self.queue_index = i;
                    self.play_current(0.0);
                    self.open(Screen::NowPlaying);
                }
            }
            Intent::PlayAlbum(i) => {
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
                self.settings.shuffle = !self.settings.shuffle;
                if self.settings.shuffle {
                    self.shuffle_remaining();
                }
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
            Intent::ClearUpcoming => {
                if let Some(path) = self.queue.get(self.queue_index).cloned() {
                    self.queue = Arc::new(vec![path]);
                    self.queue_index = 0;
                } else {
                    self.queue = Arc::new(Vec::new());
                    self.queue_index = 0;
                }
                self.save_playback();
                self.refresh_page();
            }
            Intent::ClearNowPlaying => {
                self.send(AudioCommand::Stop);
                self.pending_music = None;
                self.queue = Arc::new(Vec::new());
                self.queue_index = 0;
                self.now_track = None;
                self.restored = false;
                self.save_playback();
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
                if let Some(maintenance) = self.maintenance.as_mut() {
                    maintenance.open(mode);
                    self.open(Screen::Maintenance);
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
                if let Some(a) = self.library.albums.get(album) {
                    let paths = a
                        .track_indices
                        .iter()
                        .map(|&i| self.library.tracks[i].path.clone())
                        .collect::<Vec<_>>();
                    let at = if next {
                        (self.queue_index + 1).min(self.queue.len())
                    } else {
                        self.queue.len()
                    };
                    Arc::make_mut(&mut self.queue).splice(at..at, paths);
                    self.save_playback();
                    self.notify(if next {
                        "Album will play next"
                    } else {
                        "Album added to queue"
                    });
                }
            }
            Intent::RemoveQueue(index) => {
                if index < self.queue.len() {
                    Arc::make_mut(&mut self.queue).remove(index);
                    if index < self.queue_index {
                        self.queue_index -= 1;
                    } else if index == self.queue_index {
                        if self.queue.is_empty() {
                            self.send(AudioCommand::Stop);
                            self.now_track = None;
                        } else {
                            self.queue_index = self.queue_index.min(self.queue.len() - 1);
                            self.play_current(0.0);
                        }
                    }
                    self.save_playback();
                    self.refresh_page();
                }
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
                self.keyboard = Some(Keyboard {
                    layout: 0,
                    selected: 0,
                    query: self.search_query.clone(),
                })
            }
            Intent::Game(kind) => {
                let mut game = Games::new(kind);
                if let Some(saved) = self.game_saves.get(game.title()) {
                    let _ = game.restore_snapshot(saved);
                }
                self.game = Some(game);
                self.open(Screen::Game);
            }
            Intent::AddNext(i) | Intent::AddLater(i) => {
                if let Some(t) = self.library.tracks.get(i) {
                    let path = t.path.clone();
                    if matches!(intent, Intent::AddNext(_)) && !self.queue.is_empty() {
                        let at = (self.queue_index + 1).min(self.queue.len());
                        Arc::make_mut(&mut self.queue).insert(at, path);
                    } else {
                        Arc::make_mut(&mut self.queue).push(path);
                    }
                    self.save_playback();
                    self.notify("Added to the queue");
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
                self.settings.screensaver_dim = match self.settings.screensaver_dim {
                    0 => 5,
                    5 => 15,
                    15 => 30,
                    30 => 50,
                    _ => 0,
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
            Intent::DownloadMedia(video) => {
                if self.video_origin.contains("youtube.com/")
                    || self.video_origin.contains("youtu.be/")
                {
                    self.extras
                        .download(betterpod_rust::downloads::DownloadRequest::YouTube {
                            url: self.video_origin.clone(),
                            video,
                            split_chapters: false,
                        });
                } else {
                    self.extras
                        .download(betterpod_rust::downloads::DownloadRequest::File {
                            url: self.video_origin.clone(),
                            name: self.video_title.clone(),
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
        self.last_activity = Instant::now();
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
            Action::MediaPlayPause => {
                self.play_pause();
                return;
            }
            Action::MediaPlay => {
                if self.video_open {
                    if let Some(video) = self.video.as_ref() {
                        video.command(VideoCommand::Pause(false));
                    }
                } else if self.quiz.is_some() {
                    self.send(AudioCommand::Resume);
                } else if self.restored {
                    self.play_current(self.restored_position);
                } else {
                    self.send(AudioCommand::Resume);
                }
                return;
            }
            Action::MediaPause => {
                if self.video_open {
                    if let Some(video) = self.video.as_ref() {
                        video.command(VideoCommand::Pause(true));
                    }
                } else {
                    self.send(AudioCommand::Pause);
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
                    self.running = false;
                    return;
                }
                Action::Options => {
                    self.enter_saver();
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
        if matches!(action, Action::SelectHeld | Action::BackHeld) {
            if self.held {
                return;
            }
            if self.keyboard.is_some() {
                self.keyboard = None;
                return;
            }
            if self.screen() == Screen::Extra && self.extras.is_typing() {
                self.extras.cancel_typing();
                return;
            }
            if action == Action::SelectHeld {
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
            self.leave_saver();
            self.save_games();
            self.quiz_effect_if_active();
            self.game = None;
            self.puzzle = None;
            self.curation.close();
            self.pages.clear();
            self.pages.push(self.page(Screen::Home));
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
                if let Some(game) = self.game.as_mut() {
                    game.handle(action);
                }
                return;
            }
            if self.screen() == Screen::Puzzle {
                if let Some(puzzle) = self.puzzle.as_mut() {
                    puzzle.handle(action);
                }
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
        if action == Action::Start && self.screen() == Screen::NowPlaying {
            self.flip_now_playing();
            return;
        }
        if action == Action::Start && self.screen() == Screen::CoverFlow && self.cover_from_np {
            self.cover_card = None;
            self.cover_from_np = false;
            if self.pages.len() > 1 {
                self.pages.pop();
            }
            return;
        }
        if action == Action::Start {
            self.play_pause();
            return;
        }
        if action == Action::Back
            && self.options.is_none()
            && self.screen() == Screen::NowPlaying
            && self.np_mode > 0
        {
            self.np_mode = 0;
            return;
        }
        if action == Action::Back
            && self.options.is_none()
            && self.screen() == Screen::CoverFlow
            && self.cover_card.is_some()
        {
            self.cover_card = None;
            if self.cover_from_np {
                self.cover_from_np = false;
                if self.pages.len() > 1 {
                    self.pages.pop();
                }
            }
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
        if self.screen() == Screen::Video && self.options.is_none() {
            self.video_overlay = Instant::now();
            match action {
                Action::PlayPause | Action::Select => {
                    if let Some(video) = self.video.as_ref() {
                        let snap = video.snapshot();
                        video.command(VideoCommand::Pause(!snap.paused));
                    }
                    return;
                }
                Action::Left | Action::Right => {
                    if let Some(video) = self.video.as_ref() {
                        let snap = video.snapshot();
                        video.command(VideoCommand::Seek(
                            (snap.position + if action == Action::Left { -10.0 } else { 10.0 })
                                .max(0.0),
                        ));
                    }
                    return;
                }
                Action::Back => {
                    self.remember_video();
                    self.pending_stream = None;
                    if let Some(video) = self.video.as_ref() {
                        video.command(VideoCommand::Stop);
                    }
                    self.video_open = false;
                    self.restore_stream_music();
                    if self.pages.len() > 1 {
                        self.pages.pop();
                    }
                    return;
                }
                Action::Options => {
                    self.options = Some(Page {
                        screen: Screen::Video,
                        title: "Download".into(),
                        rows: vec![
                            Row::new("As Video", Intent::DownloadMedia(true)),
                            Row::new("As Audio Only", Intent::DownloadMedia(false)),
                            Row::new("Cancel", Intent::CloseOptions),
                        ]
                        .into(),
                        selected: 0,
                        scroll: 0.0,
                        target_scroll: 0.0,
                        enter: 0.0,
                    });
                    return;
                }
                _ => {}
            }
        }
        match action {
            Action::Quit => {
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
        if self.auto_saver || self.screen() == Screen::Visualizer {
            let effect = self.saver.input(action);
            if self.settings.screensaver_effect != self.saver.effect()
                || self.settings.screensaver_colors != self.saver.palette()
            {
                self.settings.screensaver_effect = self.saver.effect();
                self.settings.screensaver_colors = self.saver.palette();
                self.save_settings();
            }
            if let Some(effect) = effect {
                match effect {
                    SaverAction::Exit => {
                        if self.auto_saver {
                            self.leave_saver();
                        } else if self.pages.len() > 1 {
                            self.pages.pop();
                        }
                    }
                    SaverAction::OpenAlbum(index) => {
                        self.leave_saver();
                        self.open(Screen::Album(index));
                    }
                    SaverAction::PlayTrack(index) => {
                        self.leave_saver();
                        self.song(index);
                    }
                }
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
            self.extras.handle(action);
            return;
        }
        if self.handle_setting_preview(action) {
            return;
        }
        if action == Action::Back {
            if self.pages.len() > 1 {
                let leaving = self.screen();
                self.save_games();
                self.pages.pop();
                if leaving == Screen::Game {
                    self.game = None;
                }
                if leaving == Screen::Puzzle {
                    self.puzzle = None;
                }
                self.refresh_page();
            }
            return;
        }
        if action == Action::Search {
            self.open(Screen::Search);
            self.keyboard = Some(Keyboard {
                layout: 0,
                selected: 0,
                query: self.search_query.clone(),
            });
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
                            self.np_option = (self.np_option + 1) % 3;
                        } else {
                            self.np_option = (self.np_option + 2) % 3;
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
                        p.reveal_selection();
                    }
                }
                Action::Down => {
                    if let Some(p) = self.pages.last_mut() {
                        p.selected = (p.selected + 1).min(p.rows.len().saturating_sub(1));
                        p.reveal_selection();
                    }
                }
                Action::Left | Action::Right => {
                    if self.screen() == Screen::Settings
                        && self.pages.last().is_some_and(|p| p.selected == 0)
                    {
                        self.set_volume(if action == Action::Left { -0.05 } else { 0.05 });
                    }
                }
                Action::Select => {
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
        if self.screen() == Screen::Settings && !self.preview_focus {
            self.refresh_setting_preview();
        }
    }
    fn files_effect(&mut self, effect: FilesEffect) {
        match effect {
            FilesEffect::Exit => {
                if self.screen() == Screen::Files && self.pages.len() > 1 {
                    self.pages.pop();
                }
            }
            FilesEffect::Notice(message) => self.notify(message),
            FilesEffect::Rescan => self.rescan(),
            FilesEffect::Play(path) => {
                self.queue = Arc::new(vec![path]);
                self.queue_index = 0;
                self.play_current(0.0);
                self.open(Screen::NowPlaying);
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
    pub fn install_maintenance(&mut self, controller: MaintenanceController) {
        self.maintenance = Some(controller);
    }
    fn maintenance_effect(&mut self, effect: MaintenanceEffect) {
        match effect {
            MaintenanceEffect::Exit => {
                if self.screen() == Screen::Maintenance && self.pages.len() > 1 {
                    self.pages.pop();
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
            MaintenanceEffect::Run(request) => {
                self.maintenance_request = Some(request);
                self.running = false;
            }
        }
    }
    fn curation_effect(&mut self, effect: CurationEffect) {
        match effect {
            CurationEffect::Close => {
                self.curation.close();
                if self.screen() == Screen::Curation && self.pages.len() > 1 {
                    self.pages.pop();
                }
            }
            CurationEffect::OpenAlbum(index) => self.open(Screen::Album(index)),
            CurationEffect::EditInfo { paths, title, edit } => {
                self.close_curation_page();
                self.extras.edit_album(paths, title, edit);
                self.open(Screen::Extra);
            }
            CurationEffect::SearchCovers {
                paths,
                album,
                artist,
                provider,
            } => {
                self.close_curation_page();
                self.extras
                    .open_cover_search(paths, album, artist, provider);
                self.open(Screen::Extra);
            }
            CurationEffect::Task(task) => {
                self.close_curation_page();
                self.extras.curation_task(task);
                self.open(Screen::Extra);
            }
        }
    }
    fn start_quiz(&mut self) {
        if self.video_open || self.pending_stream.is_some() {
            self.notify("Close the video or stream before starting Guess Song");
            return;
        }
        let seed = self.random();
        match Quiz::new(&self.library, seed) {
            Err(error) => self.notify(error),
            Ok(mut quiz) => {
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
            QuizEffect::Exit => {
                self.send(AudioCommand::Stop);
                self.quiz = None;
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
                    self.pages.pop();
                }
            }
        }
    }
    fn genius(&mut self) {
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
            Row::new("Play Album", Intent::PlayAlbum(album)),
            Row::new("Send Album", Intent::ShareAlbum(album)),
            Row::new("Play Next", Intent::QueueAlbum(album, true)),
            Row::new("Add to End of Queue", Intent::QueueAlbum(album, false)),
            Row::new("Add to Playlist", Intent::AddAlbumPlaylist(album)),
        ];
        if let Some(&track) = a.track_indices.first() {
            rows.push(Row::new("Album Info", Intent::AlbumInfo(track)));
            let paths = a
                .track_indices
                .iter()
                .map(|&i| self.library.tracks[i].path.clone())
                .collect::<Vec<_>>();
            rows.push(Row::new(
                "Choose Cover",
                Intent::Curation(CurationMode::Covers {
                    paths: paths.clone(),
                    album: a.name.clone(),
                    artist: a.artist.clone(),
                }),
            ));
            rows.push(Row::new(
                "Merge with Album",
                Intent::Curation(CurationMode::Merge { paths }),
            ));
        }
        if self.maintenance.is_some() {
            rows.push(Row::new(
                "Delete Album",
                Intent::Maintenance(MaintenanceMode::DeleteAlbum(
                    a.track_indices
                        .iter()
                        .map(|&i| self.library.tracks[i].path.clone())
                        .collect(),
                )),
            ));
        }
        if a.track_indices
            .iter()
            .any(|&i| !self.library.tracks[i].album_group.is_empty())
        {
            rows.push(Row::new("Separate Added Songs", Intent::Unmerge(album)));
        }
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(Page {
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
            Screen::Album(i) => Some(i),
            Screen::Albums => self
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
            if let Some(index) = self
                .pages
                .last()
                .map(|p| p.selected)
                .filter(|&i| i < self.queue.len())
            {
                rows.push(Row::new("Remove from Queue", Intent::RemoveQueue(index)));
            }
            rows.push(Row::new("Save Queue as Playlist", Intent::AddQueuePlaylist));
        }
        if let Some(i) = selected {
            rows.push(Row::new("Play Next", Intent::AddNext(i)));
            rows.push(Row::new("Play Later", Intent::AddLater(i)));
            rows.push(Row::new("Add to Playlist", Intent::AddToPlaylist(i)));
            rows.push(Row::new("Album Info", Intent::AlbumInfo(i)));
            rows.push(Row::new("Song Info", Intent::SongInfo(i)));
            rows.push(Row::new("Send Song", Intent::ShareTrack(i)));
            rows.push(Row::new("Add to Another Album", Intent::MergeSong(i)));
            if !self.library.tracks[i].album_group.is_empty() {
                rows.push(Row::new("Separate Added Song", Intent::SeparateSong(i)));
            }
        }
        if self.now_track.is_some() {
            rows.push(Row::new("Now Playing", Intent::Open(Screen::NowPlaying)));
            rows.push(Row::new("Up Next", Intent::Open(Screen::Queue)));
            rows.push(Row::new("Stop Playback", Intent::Stop));
        }
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(Page {
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
        self.search_rows = Arc::new(Vec::new());
        let (tx, rx) = mpsc::sync_channel(1);
        match thread::Builder::new()
            .name("library-search".into())
            .spawn(move || {
                let mut rows = vec![Row::new(
                    if query.is_empty() {
                        "Type a Search".into()
                    } else {
                        format!("Search: {query}")
                    },
                    Intent::SearchKeyboard,
                )];
                if !query.is_empty() {
                    let needle = query.to_lowercase();
                    for p in playlists
                        .iter()
                        .filter(|p| p.name.to_lowercase().contains(&needle))
                    {
                        let mut row = Row::new(&p.name, Intent::Playlist(p.clone()));
                        row.detail = format!("Playlist — {} songs", p.tracks.len());
                        rows.push(row);
                    }
                    for (i, t) in library
                        .tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            t.title.to_lowercase().contains(&needle)
                                || t.artist.to_lowercase().contains(&needle)
                                || t.album.to_lowercase().contains(&needle)
                        })
                        .take(500)
                    {
                        let mut row = Row::new(&t.title, Intent::Song(i));
                        row.detail = t.artist.clone();
                        row.cover = t.cover.clone();
                        rows.push(row);
                    }
                }
                let _ = tx.send((generation, Arc::new(rows)));
            }) {
            Ok(worker) => {
                self.search_result = Some(rx);
                self.search_worker = Some(worker);
            }
            Err(e) => self.notify(format!("Search: {e}")),
        }
    }
    fn keyboard_action(&mut self, action: Action) {
        let Some(k) = self.keyboard.as_mut() else {
            return;
        };
        let action = match action {
            Action::Start => {
                k.selected = 39;
                Action::Select
            }
            Action::PlayPause => {
                k.query.push(' ');
                return;
            }
            Action::Options => {
                k.layout = (k.layout + 1) % 3;
                return;
            }
            Action::Back if !k.query.is_empty() => {
                k.query.pop();
                return;
            }
            a => a,
        };
        match action {
            Action::Up => k.selected = k.selected.saturating_sub(10),
            Action::Down => k.selected = (k.selected + 10).min(39),
            Action::Left => k.selected = k.selected.saturating_sub(1),
            Action::Right => k.selected = (k.selected + 1).min(39),
            Action::Back => {
                self.keyboard = None;
                return;
            }
            Action::Select => match k.selected {
                0..=35 => {
                    if k.query.chars().count() < 120 {
                        k.query.push(keyboard_char(k.layout, k.selected));
                    }
                }
                36 => k.query.push(' '),
                37 => {
                    k.query.pop();
                }
                38 => k.query.clear(),
                _ => {
                    self.search_query = k.query.clone();
                    self.keyboard = None;
                    self.start_search();
                    self.refresh_page();
                    return;
                }
            },
            _ => {}
        }
    }

    pub fn tick(&mut self, dt: f32, renderer: &Renderer) {
        if let Some((generation, rows)) = self
            .search_result
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.search_result = None;
            if let Some(worker) = self.search_worker.take() {
                let _ = worker.join();
            }
            if generation == self.library_generation {
                self.search_rows = rows;
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
        self.audio_snapshot = self.audio.snapshot();
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
        let effects = self.files.tick();
        for effect in effects {
            self.files_effect(effect);
        }
        if let Some(maintenance) = self.maintenance.as_mut() {
            let effects = maintenance.tick();
            for effect in effects {
                self.maintenance_effect(effect);
            }
        }
        let effects = self.extras.tick(dt);
        for effect in effects {
            match effect {
                ExtraEffect::Exit => {
                    if self.screen() == Screen::Extra && self.pages.len() > 1 {
                        self.pages.pop();
                    }
                }
                ExtraEffect::Notice(message) => self.notify(message),
                ExtraEffect::MediaArtwork { origin, path } => {
                    if origin == self.video_origin {
                        self.video_cover = Some(path);
                    }
                }
                ExtraEffect::SharingName(name) => {
                    self.settings.sharing_name = name;
                    self.save_settings();
                }
                ExtraEffect::Rescan => self.rescan(),
                ExtraEffect::Play { paths, index } => {
                    self.queue = Arc::new(paths);
                    self.queue_index = index.min(self.queue.len().saturating_sub(1));
                    self.play_current(0.0);
                    self.open(Screen::NowPlaying);
                }
                ExtraEffect::Stream {
                    source,
                    video,
                    origin,
                    start,
                    queue,
                    index,
                } => {
                    self.leave_saver();
                    self.pending_music = None;
                    self.video_queue = queue;
                    self.video_index = index;
                    if self.video.is_none() {
                        match VideoPlayer::new() {
                            Ok(player) => self.video = Some(player),
                            Err(error) => {
                                self.notify(error);
                                continue;
                            }
                        }
                    }
                    self.remember_video();
                    if self.stream_restore.is_none() {
                        self.stream_restore = Some(SavedPlayback {
                            queue: self.queue.clone(),
                            index: self.queue_index,
                            position: if self.restored {
                                self.restored_position
                            } else {
                                self.audio_snapshot.position
                            },
                            paused: !self.audio_snapshot.playing,
                        });
                    }
                    self.send(AudioCommand::Pause);
                    if self.video_open {
                        if let Some(player) = self.video.as_ref() {
                            player.command(VideoCommand::Stop);
                        }
                    }
                    if let Some(texture) = self.video_texture.take() {
                        renderer.delete_texture(texture);
                    }
                    self.video_cover = None;
                    self.video_title = source.title.clone();
                    self.video_origin = origin;
                    self.video_audio_only = !video;
                    self.pending_stream = Some((source, start, !video));
                    self.video_overlay = Instant::now();
                    self.open(Screen::Video);
                    if let Some(page) = self.pages.last_mut() {
                        page.title = if self.video_audio_only {
                            "Listen".into()
                        } else {
                            self.video_title.clone()
                        };
                    }
                }
            }
        }
        if self.screen() == Screen::Extra {
            if let Some(page) = self.pages.last_mut() {
                page.title = self.extras.title().into();
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
                            self.video_open = true;
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
        if self.video_open && self.pending_stream.is_none() {
            if let Some(video) = self.video.as_ref() {
                let state = video.snapshot();
                if state.ended && state.generation != self.video_ended_generation {
                    self.video_ended_generation = state.generation;
                    self.video_next(1);
                }
            }
        }
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
        if self.screen() == Screen::Game {
            if let Some(game) = self.game.as_mut() {
                game.tick(dt);
            }
        }
        if self.screen() == Screen::Puzzle {
            if let Some(puzzle) = self.puzzle.as_mut() {
                puzzle.tick(dt);
            }
        }
        if self.auto_saver || matches!(self.screen(), Screen::Visualizer | Screen::NowPlaying) {
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
        let video_active = self.pending_stream.is_some()
            || self.video_open
                && self.video.as_ref().is_some_and(|video| {
                    let state = video.snapshot();
                    state.playing || state.loading
                });
        let video_paused = if self.video_open {
            self.video.as_ref().map(|video| video.snapshot().paused)
        } else {
            None
        };
        let paused_intent = idle_pause_intent(
            self.audio_snapshot.paused,
            video_paused,
            self.pending_stream.is_some()
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
            self.running = false;
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|t| Instant::now() >= t.until)
        {
            self.toast = None;
        }
        if self.audio_snapshot.playing && self.last_saved.elapsed() > Duration::from_secs(15) {
            self.save_playback();
            self.save_games();
        }
        if self.settings.screensaver_seconds > 0
            && self.audio_snapshot.playing
            && self.quiz.is_none()
            && !self.screen_off
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
            _ => "home",
        };
    }

    pub fn draw(&mut self, renderer: &mut Renderer, elapsed: f64) {
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
        if self.auto_saver {
            let current = self.current_index();
            if let Err(error) =
                self.saver
                    .draw(renderer, &mut self.art, &self.library, current, &self.clock)
            {
                eprintln!("Screensaver: {error}");
            }
        } else {
            match screen {
                Screen::CoverFlow => self.draw_coverflow(renderer, fg, muted),
                Screen::NowPlaying => self.draw_now_playing(renderer, elapsed, fg, muted),
                Screen::Visualizer => {
                    let current = self.current_index();
                    if let Err(error) = self.saver.draw(
                        renderer,
                        &mut self.art,
                        &self.library,
                        current,
                        &self.clock,
                    ) {
                        eprintln!("Visualizer: {error}");
                    }
                }
                Screen::Files => {
                    let dark = self.dark();
                    self.files.draw(renderer, dark);
                }
                Screen::Maintenance => {
                    let dark = self.dark();
                    if let Some(maintenance) = self.maintenance.as_mut() {
                        maintenance.draw(renderer, dark);
                    }
                }
                Screen::Curation => self.curation.draw(renderer, self.dark()),
                Screen::Quiz => {
                    if let Some(quiz) = self.quiz.as_ref() {
                        quiz.draw(renderer, &self.library, &self.audio_snapshot, self.dark());
                    }
                }
                Screen::Game => {
                    let dark = self.dark();
                    if let Some(game) = self.game.as_ref() {
                        game.draw(renderer, dark);
                    }
                }
                Screen::Video => self.draw_video(renderer),
                Screen::Puzzle => {
                    let dark = self.dark();
                    if let Some(puzzle) = self.puzzle.as_ref() {
                        puzzle.draw(renderer, dark);
                    }
                }
                Screen::About => self.draw_about(renderer, fg, muted),
                Screen::Extra => {
                    let dark = self.dark();
                    self.extras.draw(
                        renderer,
                        dark,
                        if self.settings.effects == "light" {
                            0.0
                        } else {
                            elapsed
                        },
                        &mut self.art,
                    );
                }
                _ => self.draw_menu(renderer, elapsed, fg, muted),
            }
        }
        let title = self
            .pages
            .last()
            .map(|p| p.title.as_str())
            .unwrap_or("betterPod Rust");
        if !self.auto_saver
            && !matches!(
                self.screen(),
                Screen::Visualizer | Screen::Curation | Screen::Files
            )
            && (self.screen() != Screen::Video || self.video_overlay_visible())
        {
            draw_header(
                renderer,
                title,
                self.audio_snapshot.playing,
                self.settings.volume,
                self.dark(),
            );
        }
        if matches!(self.screen(), Screen::Game | Screen::Puzzle | Screen::Quiz)
            && self.settings.game_clock.iter().any(|name| name == title)
        {
            renderer.centered(&self.clock, 270.0, 10.0, 120.0, 18, fg);
        }
        if self.held {
            renderer.centered(&self.clock, 422.0, 10.0, 73.0, 18, fg);
            renderer.rect(500.0, 8.0, 60.0, 25.0, [0.85, 0.62, 0.08, 1.0]);
            renderer.centered("HOLD", 501.0, 11.0, 58.0, 16, [0.1, 0.08, 0.0, 1.0]);
        }
        if let Some(options) = self.options.as_ref() {
            draw_options(renderer, options, self.dark());
        }
        if let Some(keyboard) = self.keyboard.as_ref() {
            draw_keyboard(renderer, keyboard, self.dark());
        }
        if let Some(toast) = self.toast.as_ref() {
            renderer.rect(50.0, 414.0, 540.0, 42.0, [0.06, 0.08, 0.11, 0.92]);
            renderer.centered(&toast.text, 60.0, 423.0, 520.0, 18, WHITE);
        }
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
    fn draw_menu(&mut self, r: &mut Renderer, elapsed: f64, fg: Color, muted: Color) {
        let Some(page) = self.pages.last() else {
            return;
        };
        let width = if page.split() { 320.0 } else { 640.0 };
        let row_h = page.row_height();
        let bottom = if self.now_track.is_some() {
            400.0
        } else {
            442.0
        };
        if page.split() {
            if page.screen == Screen::Settings {
                self.draw_setting_preview(r, fg, muted);
            } else {
                let album = page
                    .rows
                    .get(page.selected)
                    .and_then(|row| row.cover.as_deref())
                    .or_else(|| self.now_track.as_ref().and_then(|t| t.cover.as_deref()))
                    .or_else(|| {
                        self.library
                            .albums
                            .get((elapsed / 7.0) as usize % self.library.albums.len().max(1))
                            .and_then(|a| a.cover.as_deref())
                    });
                let texture = self.art.get(album);
                r.cover(
                    texture,
                    480.0,
                    216.0,
                    220.0,
                    if self.settings.effects == "full" {
                        0.14 * (elapsed as f32 * 0.5).sin()
                    } else {
                        0.0
                    },
                    1.0,
                    self.settings.effects == "full",
                );
                let label = page
                    .rows
                    .get(page.selected)
                    .map_or("Music", |row| row.label.as_str());
                r.centered(label, 330.0, 369.0, 300.0, 24, fg);
            }
            r.rect(319.0, 40.0, 1.0, bottom - 40.0, [0.45, 0.46, 0.48, 0.35]);
        }
        r.set_clip(0.0, 40.0, width, bottom - 40.0);
        let first = (page.scroll / row_h).floor().max(0.0) as usize;
        let visible = ((bottom - 40.0) / row_h).ceil() as usize + 1;
        for (i, row) in page.rows.iter().enumerate().skip(first).take(visible) {
            let y = 42.0 + i as f32 * row_h - page.scroll;
            let selected = i == page.selected;
            if selected {
                r.gradient(
                    0.0,
                    y,
                    width,
                    row_h,
                    [0.30, 0.62, 0.97, 1.0],
                    [0.06, 0.36, 0.77, 1.0],
                );
            }
            let mut x = 15.0;
            if !page.split() && row.cover.is_some() {
                let tex = self.art.get(row.cover.as_deref());
                r.image(tex, 9.0, y + 5.0, row_h - 10.0, row_h - 10.0, 1.0);
                x = row_h + 6.0;
            }
            let color = if selected { WHITE } else { fg };
            r.text(
                &row.label,
                x,
                y + if row.detail.is_empty() { 10.0 } else { 5.0 },
                22,
                color,
                width - x - 30.0,
            );
            if !row.detail.is_empty() {
                r.text(
                    &row.detail,
                    x,
                    y + 30.0,
                    16,
                    if selected { WHITE } else { muted },
                    width - x - 25.0,
                );
            }
            if row.arrow {
                r.text(">", width - 23.0, y + 10.0, 22, color, 18.0);
            }
            if page.screen == Screen::Queue && i == self.queue_index {
                r.rect(width - 42.0, y + 19.0, 8.0, 8.0, color);
            }
        }
        if page.rows.is_empty() {
            r.centered(
                if self.scanning {
                    "Reading your music..."
                } else {
                    "No music here yet"
                },
                10.0,
                175.0,
                width - 20.0,
                20,
                muted,
            );
        }
        r.reset_clip();
        if page.rows.len() as f32 * row_h > bottom - 40.0 {
            let full = page.rows.len() as f32 * row_h;
            let viewport = bottom - 40.0;
            let thumb = (viewport / full * viewport).max(15.0);
            let y = 40.0 + page.scroll / full * viewport;
            r.rect(width - 4.0, y, 3.0, thumb, muted);
        }
        if let Some(track) = self.now_track.as_ref() {
            r.gradient(
                0.0,
                401.0,
                640.0,
                78.0,
                if self.dark() {
                    [0.16, 0.17, 0.20, 1.0]
                } else {
                    [0.91, 0.92, 0.94, 1.0]
                },
                if self.dark() {
                    [0.08, 0.09, 0.11, 1.0]
                } else {
                    [0.80, 0.82, 0.85, 1.0]
                },
            );
            let texture = self.art.get(track.cover.as_deref());
            r.image(texture, 12.0, 412.0, 56.0, 56.0, 1.0);
            r.text(&track.title, 82.0, 410.0, 21, fg, 480.0);
            r.text(&track.artist, 82.0, 440.0, 17, muted, 480.0);
            r.text(
                if self.audio_snapshot.playing {
                    "II"
                } else {
                    ">"
                },
                594.0,
                424.0,
                26,
                fg,
                30.0,
            );
        } else {
            r.text(
                if self.scanning {
                    "Scanning library in background"
                } else {
                    "A: Select    B: Back    START: Play"
                },
                14.0,
                452.0,
                16,
                muted,
                610.0,
            );
        }
    }
    fn draw_coverflow(&mut self, r: &mut Renderer, fg: Color, muted: Color) {
        let center = self.cover_position.floor() as isize;
        for distance in (0..=5).rev() {
            for sign in [-1isize, 1] {
                if distance == 0 && sign == 1 {
                    continue;
                }
                let index = center + distance * sign;
                if index < 0 || index as usize >= self.library.albums.len() {
                    continue;
                }
                let album = &self.library.albums[index as usize];
                let texture = self.art.get(album.cover.as_deref());
                let d = index as f32 - self.cover_position;
                let outside = (d.abs() - 1.0).max(0.0) * 65.0;
                let x = 320.0 + d.signum() * (d.abs().min(1.0) * 175.0 + outside);
                let size = 280.0 - 50.0 * d.abs().min(1.0);
                if index as usize == self.cover_selected && self.cover_flip > 0.5 {
                    continue;
                }
                r.cover(
                    texture,
                    x,
                    217.0,
                    size,
                    if self.settings.effects == "full" {
                        if index as usize == self.cover_selected && self.cover_flip > 0.01 {
                            self.cover_flip * std::f32::consts::PI
                        } else {
                            -d.clamp(-1.0, 1.0) * 0.98
                        }
                    } else {
                        0.0
                    },
                    1.0,
                    true,
                );
            }
        }
        if let Some(album) = self.library.albums.get(self.cover_selected) {
            if self.cover_flip > 0.5 {
                let width = 304.0 * (-(self.cover_flip * std::f32::consts::PI).cos()).max(0.01);
                let x = 320.0 - width / 2.0;
                r.rect(
                    x,
                    71.0,
                    width,
                    292.0,
                    if self.dark() {
                        [0.13, 0.14, 0.17, 1.0]
                    } else {
                        [0.98, 0.98, 0.99, 1.0]
                    },
                );
                if self.cover_flip > 0.85 {
                    let selected = self.cover_card.unwrap_or(0);
                    let first = selected
                        .saturating_sub(4)
                        .min(album.track_indices.len().saturating_sub(8));
                    r.set_clip(x + 5.0, 77.0, width - 10.0, 280.0);
                    for (n, &i) in album.track_indices.iter().enumerate().skip(first).take(8) {
                        let y = 80.0 + (n - first) as f32 * 34.0;
                        if n == selected {
                            r.rect(x + 5.0, y, width - 10.0, 32.0, BLUE);
                        }
                        r.text(
                            &self.library.tracks[i].title,
                            x + 13.0,
                            y + 6.0,
                            18,
                            if n == selected { WHITE } else { fg },
                            width - 85.0,
                        );
                        if let Some(row) = self
                            .views_album_tracks
                            .get(self.cover_selected)
                            .and_then(|rows| rows.get(n))
                        {
                            r.text(
                                &row.detail,
                                x + width - 61.0,
                                y + 7.0,
                                16,
                                if n == selected { WHITE } else { muted },
                                53.0,
                            );
                        }
                    }
                    r.reset_clip();
                    if album.track_indices.len() > 8 {
                        let full = album.track_indices.len() as f32 * 34.0;
                        let thumb = (278.0 / full * 278.0).max(15.0);
                        r.rect(
                            x + width - 5.0,
                            78.0 + first as f32 * 34.0 / full * 278.0,
                            3.0,
                            thumb,
                            muted,
                        );
                    }
                }
            }
            r.centered(&album.name, 15.0, 395.0, 610.0, 24, fg);
            r.centered(&album.artist, 15.0, 428.0, 610.0, 18, muted);
        }
        r.text(
            "LEFT / RIGHT: Browse     A: Flip     Y: Options",
            16.0,
            459.0,
            16,
            muted,
            610.0,
        );
    }
    fn draw_now_playing(&mut self, r: &mut Renderer, elapsed: f64, fg: Color, muted: Color) {
        let Some(track) = self.now_track.as_ref() else {
            r.centered("Choose a song from Music", 10.0, 220.0, 620.0, 24, fg);
            return;
        };
        let texture = self.art.get(track.cover.as_deref());
        let angle = if self.audio_snapshot.playing && self.settings.effects == "full" {
            0.13 * (elapsed as f32 * 0.45).sin()
        } else {
            0.0
        };
        r.cover(texture, 177.0, 225.0, 285.0, angle, 1.0, true);
        marquee(r, &track.title, 345.0, 100.0, 26, fg, 280.0, elapsed);
        marquee(r, &track.artist, 345.0, 143.0, 21, muted, 280.0, elapsed);
        marquee(r, &track.album, 345.0, 178.0, 19, muted, 280.0, elapsed);
        if track.lossless {
            let rate = if track.sample_rate > 0 {
                track.sample_rate
            } else {
                self.audio_snapshot.rate
            };
            let badge = format!(
                "LOSSLESS{}{}",
                if track.bit_depth > 0 {
                    format!("  {}-bit", track.bit_depth)
                } else {
                    String::new()
                },
                if rate > 0 {
                    format!(" / {:.1} kHz", rate as f32 / 1000.0)
                } else {
                    String::new()
                }
            );
            r.text(&badge, 345.0, 221.0, 13, BLUE, 280.0);
        }
        for i in 0..20 {
            r.rect(
                347.0 + i as f32 * 13.7,
                316.0,
                10.0,
                4.0,
                if i as f32 / 20.0 < self.meter && self.audio_snapshot.playing {
                    if i > 16 {
                        [0.9, 0.3, 0.15, 1.0]
                    } else {
                        BLUE
                    }
                } else {
                    [0.45, 0.47, 0.5, 0.22]
                },
            );
        }
        let position = self.np_seek.map_or_else(
            || {
                if self.restored {
                    self.restored_position
                } else {
                    self.audio_snapshot.position
                }
            },
            |s| s.0,
        );
        let duration = if self.audio_snapshot.duration > 0.0 {
            self.audio_snapshot.duration
        } else {
            track.duration
        };
        r.rect(345.0, 259.0, 275.0, 8.0, [0.47, 0.49, 0.52, 0.5]);
        if duration > 0.0 {
            r.rect(
                345.0,
                259.0,
                275.0 * (position / duration).clamp(0.0, 1.0) as f32,
                8.0,
                BLUE,
            );
        }
        r.text(&time_label(position), 345.0, 281.0, 18, muted, 125.0);
        r.text(
            &format!("-{}", time_label((duration - position).max(0.0))),
            541.0,
            281.0,
            18,
            muted,
            90.0,
        );
        r.centered(
            if self.audio_snapshot.loading {
                "Loading..."
            } else if self.audio_snapshot.playing {
                "Playing"
            } else {
                "Paused"
            },
            345.0,
            330.0,
            275.0,
            22,
            fg,
        );
        if self.np_mode == 2 {
            for (i, label) in [
                format!(
                    "Shuffle: {}",
                    if self.settings.shuffle { "On" } else { "Off" }
                ),
                format!("Repeat: {}", self.settings.repeat),
                "Done".into(),
            ]
            .iter()
            .enumerate()
            {
                let y = 354.0 + i as f32 * 26.0;
                if i == self.np_option {
                    r.rect(342.0, y - 2.0, 283.0, 25.0, BLUE);
                }
                r.text(
                    label,
                    351.0,
                    y,
                    17,
                    if i == self.np_option { WHITE } else { fg },
                    268.0,
                );
            }
        } else {
            r.centered(
                if self.np_mode == 1 {
                    "<  Seek  >"
                } else {
                    "−  Volume  +"
                },
                345.0,
                371.0,
                275.0,
                22,
                fg,
            );
            r.centered("A: Next control   X: Pause", 345.0, 403.0, 275.0, 15, muted);
        }

        let status = format!(
            "{} of {}   {}{}",
            self.queue_index + 1,
            self.queue.len(),
            if self.settings.shuffle {
                "Shuffle  "
            } else {
                ""
            },
            match self.settings.repeat.as_str() {
                "one" => "Repeat One",
                "all" => "Repeat All",
                _ => "",
            }
        );
        r.centered(&status, 20.0, 438.0, 600.0, 17, muted);
        if let Some(error) = &self.audio_snapshot.error {
            r.rect(10.0, 399.0, 620.0, 34.0, [0.5, 0.06, 0.06, 0.95]);
            r.text(error, 18.0, 406.0, 16, WHITE, 602.0);
        }
    }
    fn draw_about(&self, r: &mut Renderer, fg: Color, muted: Color) {
        r.centered("betterPod Rust", 20.0, 108.0, 600.0, 36, fg);
        r.centered("Experimental local build", 20.0, 165.0, 600.0, 22, muted);
        let summary = format!(
            "{} songs  -  {} albums",
            self.library.tracks.len(),
            self.library.albums.len()
        );
        r.centered(&summary, 20.0, 220.0, 600.0, 22, fg);
        r.centered(
            "Independent app and private settings",
            20.0,
            267.0,
            600.0,
            20,
            muted,
        );
        r.centered(
            "MENU: screen off   Hold MENU: exit",
            20.0,
            311.0,
            600.0,
            20,
            muted,
        );
        r.text(&r.backend.clone(), 20.0, 398.0, 16, muted, 600.0);
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
                        if let Some(page) = self.pages.last_mut() {
                            page.selected = if at <= span { at } else { span * 2 - at };
                            page.reveal_selection();
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
    }
    fn leave_saver(&mut self) {
        self.auto_saver = false;
        if let Some(level) = self.saver_brightness.take() {
            let _ = self.platform.set_brightness(level);
        }
    }
    fn close_curation_page(&mut self) {
        self.curation.close();
        if self.screen() == Screen::Curation && self.pages.len() > 1 {
            self.pages.pop();
        }
    }
    fn configure_saver(&mut self, visualizer: bool) {
        self.saver.set_style(if visualizer {
            "viz"
        } else {
            &self.settings.screensaver_style
        });
        self.saver.set_effect(self.settings.screensaver_effect);
        self.saver.set_palette(self.settings.screensaver_colors);
    }
    fn restore_stream_music(&mut self) {
        self.extras.cancel_media();
        if let Some(saved) = self.stream_restore.take() {
            self.queue = saved.queue;
            self.queue_index = saved.index;
            self.restored_position = saved.position;
            self.restored = true;
            self.now_track = self.current_index().map(|i| self.library.tracks[i].clone());
            if !saved.paused && !self.queue.is_empty() {
                self.play_current(saved.position);
            }
        }
    }
    fn video_next(&mut self, delta: isize) {
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
                );
            }
        }
    }
    fn draw_video(&mut self, r: &mut Renderer) {
        r.rect(0.0, 0.0, 640.0, 480.0, [0.0, 0.0, 0.0, 1.0]);
        let Some(video) = self.video.as_ref() else {
            return;
        };
        let state = video.snapshot();
        if self.video_audio_only {
            let cover = self.art.get(self.video_cover.as_deref());
            r.cover(
                cover,
                180.0,
                225.0,
                275.0,
                0.0,
                1.0,
                self.settings.effects == "full",
            );
            r.text(&self.video_title, 340.0, 150.0, 26, WHITE, 280.0);
            r.text("Listening", 340.0, 214.0, 20, [0.65, 0.7, 0.77, 1.0], 280.0);
        } else if let Some(texture) = self.video_texture {
            let ratio = (640.0 / texture.width as f32).min(480.0 / texture.height as f32);
            let w = texture.width as f32 * ratio;
            let h = texture.height as f32 * ratio;
            r.image(texture, (640.0 - w) * 0.5, (480.0 - h) * 0.5, w, h, 1.0);
        }
        if self.video_overlay_visible() {
            r.rect(0.0, 400.0, 640.0, 80.0, [0.0, 0.0, 0.0, 0.7]);
            r.rect(20.0, 423.0, 600.0, 5.0, [0.4, 0.4, 0.4, 1.0]);
            if state.duration > 0.0 {
                r.rect(
                    20.0,
                    423.0,
                    600.0 * (state.position / state.duration).clamp(0.0, 1.0) as f32,
                    5.0,
                    BLUE,
                );
            }
            r.text(&time_label(state.position), 20.0, 440.0, 18, WHITE, 100.0);
            r.text(
                if state.loading || self.pending_stream.is_some() {
                    "Opening..."
                } else if state.paused {
                    "Paused"
                } else if state.ended {
                    "Finished"
                } else {
                    "Playing"
                },
                245.0,
                440.0,
                18,
                WHITE,
                180.0,
            );
            r.text(
                &if state.duration > 0.0 {
                    time_label(state.duration)
                } else {
                    "--:--".into()
                },
                540.0,
                440.0,
                18,
                WHITE,
                90.0,
            );
        }
        if let Some(error) = state.error {
            r.rect(20.0, 190.0, 600.0, 88.0, [0.35, 0.03, 0.03, 0.95]);
            r.text(&error, 35.0, 212.0, 18, WHITE, 570.0);
        }
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
        self.files.shutdown();
        self.curation.shutdown();
        self.volume.shutdown();
        self.idle.close();
        self.art.close(renderer);
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
fn draw_header(r: &mut Renderer, title: &str, playing: bool, volume: f32, dark: bool) {
    let (top, bottom, fg) = if dark {
        ([0.21, 0.22, 0.25, 1.0], [0.10, 0.11, 0.13, 1.0], WHITE)
    } else {
        ([0.99, 0.99, 1.0, 1.0], [0.75, 0.77, 0.80, 1.0], FG)
    };
    r.gradient(0.0, 0.0, 640.0, 40.0, top, bottom);
    r.rect(0.0, 39.0, 640.0, 1.0, [0.3, 0.31, 0.33, 0.65]);
    r.text(title, 14.0, 7.0, 24, fg, 510.0);
    if playing {
        r.text(">", 552.0, 9.0, 22, fg, 30.0);
    }
    r.text(
        &format!("{}%", (volume * 100.0).round() as u32),
        586.0,
        10.0,
        18,
        fg,
        51.0,
    );
}
fn draw_options(r: &mut Renderer, page: &Page, dark: bool) {
    r.rect(0.0, 40.0, 640.0, 440.0, [0.0, 0.0, 0.0, 0.55]);
    let height = 50.0 + page.rows.len() as f32 * 42.0;
    let y = (480.0 - height) * 0.5;
    let bg = if dark { [0.13, 0.14, 0.17, 1.0] } else { BG };
    let fg = if dark { WHITE } else { FG };
    r.rect(110.0, y, 420.0, height, bg);
    r.centered(&page.title, 120.0, y + 10.0, 400.0, 23, fg);
    for (i, row) in page.rows.iter().enumerate() {
        let yy = y + 47.0 + i as f32 * 42.0;
        if i == page.selected {
            r.rect(110.0, yy, 420.0, 42.0, BLUE);
        }
        r.text(
            &row.label,
            130.0,
            yy + 9.0,
            21,
            if i == page.selected { WHITE } else { fg },
            380.0,
        );
    }
}
fn draw_keyboard(r: &mut Renderer, k: &Keyboard, dark: bool) {
    draw_keyboard_title(r, k, dark, "Search your music");
}
fn draw_keyboard_title(r: &mut Renderer, k: &Keyboard, dark: bool, title: &str) {
    let bg = if dark { [0.08, 0.09, 0.11, 1.0] } else { BG };
    let fg = if dark { WHITE } else { FG };
    r.rect(0.0, 40.0, 640.0, 440.0, bg);
    r.text(title, 18.0, 55.0, 24, fg, 600.0);
    r.rect(
        16.0,
        103.0,
        608.0,
        48.0,
        if dark {
            [0.2, 0.21, 0.24, 1.0]
        } else {
            [0.86, 0.88, 0.91, 1.0]
        },
    );
    r.text(&k.query, 25.0, 116.0, 23, fg, 590.0);
    for index in 0..40 {
        let x = 16.0 + (index % 10) as f32 * 61.0;
        let y = 178.0 + (index / 10) as f32 * 61.0;
        let selected = index == k.selected;
        r.rect(
            x,
            y,
            55.0,
            52.0,
            if selected {
                BLUE
            } else if dark {
                [0.18, 0.19, 0.22, 1.0]
            } else {
                [0.84, 0.86, 0.89, 1.0]
            },
        );
        let key = match index {
            0..=35 => keyboard_char(k.layout, index).to_string(),
            36 => "Space".into(),
            37 => "Del".into(),
            38 => "Clear".into(),
            _ => "Done".into(),
        };
        r.centered(
            &key,
            x + 2.0,
            y + 16.0,
            51.0,
            if index < 36 { 22 } else { 14 },
            if selected { WHITE } else { fg },
        );
    }
    r.centered(
        "A: Type   X: Space   B: Delete   Y: Aa/123   START: Done",
        15.0,
        448.0,
        610.0,
        17,
        fg,
    );
}

fn local_clock() -> String {
    let mut now: libc::time_t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&mut now, &mut parts) }.is_null() {
        return String::new();
    }
    format!("{:02}:{:02}", parts.tm_hour, parts.tm_min)
}
fn marquee(
    r: &mut Renderer,
    text: &str,
    x: f32,
    y: f32,
    size: u16,
    color: Color,
    width: f32,
    elapsed: f64,
) {
    let full = r.text_width(text, size);
    if full <= width {
        r.text(text, x, y, size, color, width);
        return;
    }
    let travel = (full - width + 18.0) / 32.0;
    let phase = elapsed as f32 % (travel * 2.0 + 4.0);
    let offset = if phase < 2.0 {
        0.0
    } else if phase < 2.0 + travel {
        (phase - 2.0) * 32.0
    } else if phase < 4.0 + travel {
        full - width + 18.0
    } else {
        (travel * 2.0 + 4.0 - phase) * 32.0
    };
    r.set_clip(x, y - 2.0, width, size as f32 + 9.0);
    r.text(text, x - offset, y, size, color, full + 2.0);
    r.reset_clip();
}

fn keyboard_char(layout: u8, index: usize) -> char {
    let keys: &[u8] = match layout {
        1 => b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
        2 => br#"0123456789-._/:@?&=+#%()[]{}'",!;\|*"#,
        _ => b"abcdefghijklmnopqrstuvwxyz0123456789",
    };
    keys.get(index).copied().unwrap_or(b' ') as char
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
