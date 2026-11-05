//! UI for background library services. No task or menu callback performs I/O.
#[path = "app_cover.rs"]
mod app_cover;
use super::{draw_keyboard_title, keyboard_char, Keyboard, FG, MUTED, WHITE};
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
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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
    VideoHistory,
    Weather,
    Receive,
    Sharing,
}
#[derive(Clone)]
pub(super) enum Effect {
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
    SharingName(String),
    MediaArtwork {
        origin: String,
        path: PathBuf,
    },
}
#[derive(Clone)]
enum Choice {
    Page(ExtraPage),
    NewPlaylist,
    Playlist(Playlist),
    PlaylistSong(Arc<Playlist>, usize),
    AppendPlaylist(Playlist),
    DeletePlaylist(Playlist),
    RemovePlaylist(Arc<Playlist>, usize),
    Search(Provider),
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
    WeatherSearch,
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
}
#[derive(Clone, Copy)]
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
struct Item {
    title: String,
    detail: String,
    choice: Choice,
}
impl Item {
    fn new(title: impl Into<String>, choice: Choice) -> Self {
        Self {
            title: title.into(),
            detail: String::new(),
            choice,
        }
    }
}
struct Page {
    title: String,
    rows: Vec<Item>,
    selected: usize,
    scroll: f32,
    target: f32,
}
impl Page {
    fn new(title: impl Into<String>, rows: Vec<Item>) -> Self {
        Self {
            title: title.into(),
            rows,
            selected: 0,
            scroll: 0.0,
            target: 0.0,
        }
    }
}
#[derive(Clone)]
enum Purpose {
    Search {
        provider: Provider,
        query: String,
        page: u32,
        metadata: bool,
        generation: u64,
    },
    Details {
        metadata: bool,
    },
    Feed,
    Download,
    Downloads,
    Tags,
    Tools,
    Subscriptions,
    Remember,
    History,
    Cover,
    Preview(String),
    Stream {
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
    Search(Provider, bool),
    Tag(Field),
    Weather,
    Receive,
    SharingName,
}

pub(super) struct Extras {
    playlists: PlaylistClient,
    lists: Arc<Vec<Playlist>>,
    services: Services,
    jobs: HashMap<u64, (JobHandle, Purpose)>,
    pages: Vec<Page>,
    keyboard: Option<(Keyboard, Typing)>,
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
    forecast: Option<usize>,
    forecast_depth: usize,
    forecast_day: usize,
    share: ShareClient,
    cover_search: bool,
    cover_override: bool,
    youtube: bool,
    sharing_name: String,
    previews: HashMap<String, Option<PathBuf>>,
    active_media_art: Option<(String, String)>,
    cover_editor: Option<CoverEditor>,
    detail_item: Option<OnlineItem>,
    media_queue: Vec<OnlineItem>,
    media_generation: u64,
}
impl Extras {
    pub(super) fn new(music: PathBuf, data: PathBuf) -> Result<Self, String> {
        let mut extras = Self {
            playlists: PlaylistClient::new(music.clone(), data.clone()),
            lists: Arc::new(Vec::new()),
            services: Services::new_with_music(data.clone(), music)?,
            weather: Weather::new(data.clone())?,
            weather_state: WeatherSnapshot::default(),
            forecast: None,
            forecast_depth: 0,
            forecast_day: 0,
            share: ShareClient::new(data),
            cover_search: false,
            cover_override: false,
            youtube: false,
            sharing_name: "betterPod Rust".into(),
            previews: HashMap::new(),
            active_media_art: None,
            cover_editor: None,
            detail_item: None,
            media_queue: Vec::new(),
            media_generation: 0,
            jobs: HashMap::new(),
            pages: Vec::new(),
            keyboard: None,
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
        Ok(extras)
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
        self.media_generation = self.media_generation.wrapping_add(1);
        let generation = self.media_generation;
        for (handle, purpose) in self.jobs.values() {
            if matches!(purpose, Purpose::Stream { .. }) {
                handle.cancel();
            }
        }
        let origin = item.url.clone();
        self.active_media_art = Some((origin.clone(), item.cover.clone()));
        self.request_preview(item.cover.clone());
        if origin.contains("youtube.com/") || origin.contains("youtu.be/") {
            self.task(
                Task::ResolveStream { item, video },
                Purpose::Stream {
                    video,
                    origin,
                    start,
                    queue,
                    index,
                    generation,
                },
            );
        } else {
            self.effects.push(Effect::Stream {
                source: StreamSource {
                    video_url: item.url.clone(),
                    audio_url: None,
                    headers: Vec::new(),
                    duration: item.seconds,
                    title: item.title,
                    available_at: None,
                },
                video,
                origin,
                start,
                queue,
                index,
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
        self.pages.clear();
        self.push("Album Changes", vec![Item::new("Cancel", Choice::Cancel)]);
        self.task(task, Purpose::Tags);
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
    }
    pub(super) fn cancel_typing(&mut self) {
        self.keyboard = None;
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
    pub(super) fn remember_video(&mut self, url: String, title: String, position: f64) {
        match self.services.submit(Task::RememberVideo {
            url,
            title,
            position,
        }) {
            Ok(handle) => {
                self.jobs.insert(handle.id, (handle, Purpose::Remember));
            }
            Err(error) => eprintln!("Remember video: {error}"),
        }
    }
    pub(super) fn send_share(
        &mut self,
        paths: Vec<PathBuf>,
        info: ShareInfo,
        cover: Option<PathBuf>,
    ) {
        self.open(ExtraPage::Receive);
        self.replace(
            "Send Music",
            vec![Item::new("Cancel Transfer", Choice::ShareCancel)],
        );
        if let Err(error) = self
            .share
            .submit(share::Command::Send { paths, info, cover })
        {
            self.effects.push(Effect::Notice(error));
        }
    }
    fn push(&mut self, title: impl Into<String>, rows: Vec<Item>) {
        self.pages.push(Page::new(title, rows));
    }
    fn replace(&mut self, title: impl Into<String>, rows: Vec<Item>) {
        if !self.pages.is_empty() {
            self.pages.pop();
        }
        self.push(title, rows);
    }
    fn task(&mut self, task: Task, purpose: Purpose) {
        match self.services.submit(task) {
            Ok(handle) => {
                self.jobs.insert(handle.id, (handle, purpose));
                self.status = "Working...".into();
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
        self.open(ExtraPage::AlbumInfo);
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
                    vec![Item::new("Create Playlist", Choice::NewPlaylist)],
                );
                self.playlist_command(playlists::Command::List);
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
                self.push(
                    provider.name(),
                    vec![Item::new("Search", Choice::Search(provider))],
                );
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: String::new(),
                    },
                    Typing::Search(provider, false),
                ));
            }
            ExtraPage::Podcasts => {
                self.push(
                    "Podcasts",
                    vec![Item::new(
                        "Find Podcasts",
                        Choice::Page(ExtraPage::Online(Provider::Podcast)),
                    )],
                );
                self.task(Task::Subscriptions, Purpose::Subscriptions);
            }
            ExtraPage::Downloads => {
                self.push("Downloaded", Vec::new());
                self.task(Task::Downloaded, Purpose::Downloads);
            }
            ExtraPage::Tools => self.push(
                "YouTube Tools",
                vec![
                    Item::new("Install / Update Tools", Choice::Tools),
                    Item::new(
                        "YouTube Search",
                        Choice::Page(ExtraPage::Online(Provider::YouTube)),
                    ),
                ],
            ),
            ExtraPage::Bluetooth => {
                self.push("Bluetooth", Vec::new());
                self.bluetooth.request(BtRequest::Refresh);
                self.status = "Reading Bluetooth devices...".into();
            }
            ExtraPage::AlbumInfo => self.push("Album Info", self.tag_rows()),
            ExtraPage::Videos => self.push(
                "Videos",
                vec![
                    Item::new(
                        "Search YouTube",
                        Choice::Page(ExtraPage::Online(Provider::YouTube)),
                    ),
                    Item::new(
                        "YouTube Playlists",
                        Choice::Page(ExtraPage::Online(Provider::YouTubePlaylists)),
                    ),
                    Item::new("Recently Watched", Choice::Page(ExtraPage::VideoHistory)),
                    Item::new("Downloaded", Choice::Page(ExtraPage::Downloads)),
                ],
            ),
            ExtraPage::Weather => self.push("Weather", self.weather_rows()),
            ExtraPage::Sharing => self.push(
                "Sharing",
                vec![
                    Item::new(
                        format!("Name: {}", self.sharing_name),
                        Choice::EditSharingName,
                    ),
                    Item::new("Receive Music", Choice::Page(ExtraPage::Receive)),
                ],
            ),
            ExtraPage::Receive => self.push(
                "Receive Music",
                vec![
                    Item::new("Enter 8-Digit Code", Choice::ShareReceive),
                    Item::new("Cancel Transfer", Choice::ShareCancel),
                ],
            ),
            ExtraPage::VideoHistory => {
                self.push("Recently Watched", Vec::new());
                self.task(Task::VideoHistory, Purpose::History);
            }
        }
    }
    fn tag_rows(&self) -> Vec<Item> {
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
        let mut rows = vec![Item::new("Create Playlist", Choice::NewPlaylist)];
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
        playlist
            .tracks
            .iter()
            .enumerate()
            .map(|(index, path)| {
                Item::new(
                    path.file_stem().unwrap_or_default().to_string_lossy(),
                    Choice::PlaylistSong(Arc::clone(&playlist), index),
                )
            })
            .collect()
    }
    pub(super) fn handle(&mut self, action: Action) {
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
                Action::Left => {
                    self.forecast_day = self.forecast_day.saturating_sub(1);
                    return;
                }
                Action::Right => {
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
                Action::Options => {
                    self.push(
                        "Weather Options",
                        vec![
                            Item::new("Refresh", Choice::WeatherRefresh(index)),
                            Item::new("Change Units", Choice::WeatherUnits),
                            Item::new("Remove City", Choice::WeatherRemove(index)),
                        ],
                    );
                    return;
                }
                Action::Back => self.forecast = None,
                _ => {}
            }
        }
        if action == Action::Back {
            if self.pages.len() > 1 {
                self.pages.pop();
            } else {
                self.effects.push(Effect::Exit);
            }
            return;
        }
        if action == Action::Options {
            if let Some(page) = self.pages.last() {
                if let Some(row) = page.rows.get(page.selected) {
                    match &row.choice {
                        Choice::Feed(podcast) => {
                            self.push(
                                "Podcast Options",
                                vec![Item::new(
                                    "Delete Downloaded Episodes",
                                    Choice::ConfirmTask(
                                        Task::DeletePodcast {
                                            podcast: podcast.clone(),
                                        },
                                        "Delete only this app’s downloaded episodes?".into(),
                                    ),
                                )],
                            );
                            return;
                        }
                        Choice::Episode(episode, podcast) => {
                            self.push(
                                "Episode Options",
                                vec![Item::new(
                                    "Delete Downloaded Episode",
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
                        Choice::Playlist(playlist) if !playlist.read_only => {
                            self.push(
                                "Playlist Options",
                                vec![Item::new(
                                    "Delete Playlist",
                                    Choice::DeletePlaylist(playlist.clone()),
                                )],
                            );
                            return;
                        }
                        Choice::PlaylistSong(playlist, index) if !playlist.read_only => {
                            self.push(
                                "Song Options",
                                vec![Item::new(
                                    "Remove from Playlist",
                                    Choice::RemovePlaylist(Arc::clone(playlist), *index),
                                )],
                            );
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
                    Self::reveal(page);
                }
            }
            Action::Down => {
                if let Some(page) = self.pages.last_mut() {
                    page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1));
                    Self::reveal(page);
                }
            }
            Action::Select => {
                let choice = self
                    .pages
                    .last()
                    .and_then(|p| p.rows.get(p.selected))
                    .map(|r| r.choice.clone());
                if let Some(choice) = choice {
                    self.choose(choice);
                }
            }
            _ => {}
        }
    }
    fn reveal(page: &mut Page) {
        let top = page.selected as f32 * 52.0;
        if top < page.target {
            page.target = top;
        }
        if top + 52.0 > page.target + 352.0 {
            page.target = top + 52.0 - 352.0;
        }
        page.target = page.target.max(0.0);
    }
    fn search(&mut self, provider: Provider, query: String, page: u32, metadata: bool) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.push(provider.name(), Vec::new());
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
            Choice::Dismiss => {
                self.pages.pop();
            }
            Choice::ConfirmTask(task, title) => self.push(
                title,
                vec![
                    Item::new("Cancel", Choice::Dismiss),
                    Item::new("Delete Downloaded Files", Choice::RunTask(task)),
                ],
            ),
            Choice::RunTask(task) => self.task(task, Purpose::Tags),
            Choice::EditSharingName => {
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: self.sharing_name.clone(),
                    },
                    Typing::SharingName,
                ));
            }
            Choice::Page(page) => self.push_page(page),
            Choice::NewPlaylist => {
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: String::new(),
                    },
                    Typing::NewPlaylist,
                ))
            }
            Choice::Playlist(playlist) => {
                self.push(&playlist.name, self.playlist_rows(&playlist));
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
            Choice::DeletePlaylist(playlist) => self.playlist_command(playlists::Command::Delete {
                path: playlist.path,
            }),
            Choice::RemovePlaylist(playlist, index) => {
                self.playlist_command(playlists::Command::Remove {
                    path: playlist.path.clone(),
                    index,
                })
            }
            Choice::Search(provider) => {
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: String::new(),
                    },
                    Typing::Search(provider, false),
                ))
            }
            Choice::NextSearch(provider, query, page, metadata) => {
                self.search(provider, query, page, metadata)
            }
            Choice::Online(item, metadata) => {
                if item.provider == Provider::Podcast {
                    match serde_json::from_value::<Podcast>(item.raw.clone()) {
                        Ok(podcast) => self.task(Task::Feed(podcast), Purpose::Feed),
                        Err(error) => self.effects.push(Effect::Notice(error.to_string())),
                    }
                } else {
                    self.task(Task::Details(item), Purpose::Details { metadata });
                }
            }
            Choice::ResumeVideo(item, start) => {
                self.resolve_media(item.clone(), vec![item], 0, true, start);
            }
            Choice::Recent(item) => {
                let start = item.raw["position"].as_f64().unwrap_or(0.0);
                self.push(
                    &item.title,
                    vec![
                        Item::new(
                            format!("Continue from {}", super::time_label(start)),
                            Choice::ResumeVideo(item.clone(), start),
                        ),
                        Item::new(
                            "Watch from the Start",
                            Choice::ResumeVideo(item.clone(), 0.0),
                        ),
                        Item::new(
                            "Download Video",
                            Choice::Download(DownloadRequest::YouTube {
                                url: item.url.clone(),
                                video: true,
                                split_chapters: false,
                            }),
                        ),
                    ],
                );
            }
            Choice::Download(request) => self.start_download(request),
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
                let (item, queue, index) = if let Some(index) = index {
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
                self.resolve_media(item, queue, index, video, 0.0);
            }
            Choice::Feed(podcast) => self.task(Task::Feed(podcast), Purpose::Feed),
            Choice::Subscribe(podcast) => self.task(Task::RememberPodcast(podcast), Purpose::Tags),
            Choice::Unsubscribe(podcast) => {
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
                let request = DownloadRequest::Episodes {
                    podcast,
                    episodes: vec![episode.clone()],
                };
                self.push(
                    episode.title.clone(),
                    vec![
                        Item::new(
                            "Listen",
                            Choice::Media(episode.url.clone(), episode.title.clone(), false),
                        ),
                        Item::new("Download Episode", Choice::Download(request)),
                    ],
                );
            }
            Choice::DownloadEpisodes(podcast, episodes) => self.task(
                Task::Download(DownloadRequest::Episodes { podcast, episodes }),
                Purpose::Download,
            ),
            Choice::TagField(field) => {
                let query = self.field_value(field);
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query,
                    },
                    Typing::Tag(field),
                ));
            }
            Choice::SaveTags => {
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
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: self.edit_title.clone(),
                    },
                    Typing::Search(provider, true),
                ))
            }
            Choice::ApplyMetadata(item) => {
                self.pending_metadata = Some(item.clone());
                if !item.title.is_empty() {
                    self.edit.album = Some(item.title);
                }
                if !item.artist.is_empty() {
                    self.edit.artist = Some(item.artist.clone());
                    self.edit.album_artist = Some(item.artist);
                }
                if let Ok(year) = item.year.chars().take(4).collect::<String>().parse() {
                    self.edit.year = Some(year);
                }
                self.push("Album Info", self.tag_rows());
            }
            Choice::BtPower(on) => {
                self.bluetooth.request(BtRequest::Power(on));
                self.status = "Changing Bluetooth power...".into();
            }
            Choice::BtScan => {
                self.bluetooth.request(BtRequest::Scan);
                self.status = "Looking for Bluetooth devices...".into();
            }
            Choice::BtDevice(address) => {
                self.bt_selected = Some(address.clone());
                let device = self.bt_state.devices.iter().find(|d| d.address == address);
                let name = device.map_or(address.clone(), |d| d.name.clone());
                let connected = device.is_some_and(|d| d.connected);
                self.push(
                    name,
                    vec![
                        Item::new(
                            if connected { "Disconnect" } else { "Connect" },
                            if connected {
                                Choice::BtDisconnect(address.clone())
                            } else {
                                Choice::BtConnect(address.clone())
                            },
                        ),
                        Item::new("Forget Device", Choice::BtForget(address)),
                    ],
                );
            }
            Choice::BtConnect(address) => {
                self.bluetooth.request(BtRequest::Connect(address));
                self.status = "Connecting...".into();
            }
            Choice::BtDisconnect(address) => {
                self.bluetooth.request(BtRequest::Disconnect(address));
                self.status = "Disconnecting...".into();
            }
            Choice::BtForget(address) => {
                self.bluetooth.request(BtRequest::Forget(address));
                self.status = "Removing pairing...".into();
            }
            Choice::CoverSearch(provider) => {
                self.cover_search = true;
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: self.edit_title.clone(),
                    },
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
            Choice::WeatherSearch => {
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 0,
                        query: String::new(),
                    },
                    Typing::Weather,
                ))
            }
            Choice::WeatherPlace(index) => {
                if let Some(place) = self.weather_state.places.get(index) {
                    let title = place.name.clone();
                    self.forecast = Some(index);
                    self.forecast_day = 0;
                    self.push(title, Vec::new());
                    self.forecast_depth = self.pages.len();
                }
            }
            Choice::WeatherAdd(place) => {
                self.weather.command(WeatherCommand::Add(place));
                self.status = "Adding city...".into();
            }
            Choice::WeatherRefresh(index) => {
                self.weather.command(WeatherCommand::Refresh(index));
                self.status = "Refreshing forecast...".into();
            }
            Choice::WeatherRemove(index) => {
                self.weather.command(WeatherCommand::Remove(index));
                self.forecast = None;
                self.pages.clear();
                self.push("Weather", self.weather_rows());
            }
            Choice::WeatherUnits => {
                self.weather
                    .command(WeatherCommand::Units(!self.weather_state.fahrenheit));
            }
            Choice::ShareReceive => {
                self.keyboard = Some((
                    Keyboard {
                        layout: 0,
                        selected: 26,
                        query: String::new(),
                    },
                    Typing::Receive,
                ))
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
            Choice::Nothing => {}
        }
    }
    fn field_value(&self, field: Field) -> String {
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
        let action = match action {
            Action::Start => {
                keyboard.selected = 39;
                Action::Select
            }
            Action::PlayPause => {
                keyboard.query.push(' ');
                return;
            }
            Action::Options => {
                keyboard.layout = (keyboard.layout + 1) % 3;
                return;
            }
            Action::Back if !keyboard.query.is_empty() => {
                keyboard.query.pop();
                return;
            }
            a => a,
        };
        match action {
            Action::Up => keyboard.selected = keyboard.selected.saturating_sub(10),
            Action::Down => keyboard.selected = (keyboard.selected + 10).min(39),
            Action::Left => keyboard.selected = keyboard.selected.saturating_sub(1),
            Action::Right => keyboard.selected = (keyboard.selected + 1).min(39),
            Action::Back => {
                self.keyboard = None;
            }
            Action::Select => {
                let selected = keyboard.selected;
                if selected < 36 {
                    if keyboard.query.chars().count() < 120 {
                        keyboard
                            .query
                            .push(keyboard_char(keyboard.layout, selected));
                    }
                } else {
                    match selected {
                        36 => keyboard.query.push(' '),
                        37 => {
                            keyboard.query.pop();
                        }
                        38 => keyboard.query.clear(),
                        _ => {
                            let (keyboard, typing) = self.keyboard.take().unwrap();
                            match typing {
                                Typing::NewPlaylist => {
                                    self.playlist_command(playlists::Command::Create {
                                        name: keyboard.query,
                                    })
                                }
                                Typing::Search(provider, metadata) => {
                                    self.search(provider, keyboard.query, 1, metadata)
                                }
                                Typing::Weather => {
                                    self.weather.command(WeatherCommand::Search(keyboard.query));
                                    self.status = "Finding cities...".into();
                                }
                                Typing::Receive => {
                                    if keyboard.query.len() != 8
                                        || !keyboard.query.bytes().all(|c| c.is_ascii_digit())
                                    {
                                        self.effects
                                            .push(Effect::Notice("Enter exactly 8 digits".into()));
                                    } else if let Err(error) =
                                        self.share.submit(share::Command::Receive {
                                            code: keyboard.query,
                                            name: self.sharing_name.clone(),
                                        })
                                    {
                                        self.effects.push(Effect::Notice(error));
                                    } else {
                                        self.status = "Waiting for the sender...".into();
                                    }
                                }
                                Typing::SharingName => {
                                    let name = keyboard.query.trim().to_string();
                                    if !name.is_empty() {
                                        self.sharing_name = name.clone();
                                        self.effects.push(Effect::SharingName(name));
                                        self.replace(
                                            "Sharing",
                                            vec![Item::new(
                                                format!("Name: {}", self.sharing_name),
                                                Choice::EditSharingName,
                                            )],
                                        );
                                    }
                                }
                                Typing::Tag(field) => {
                                    self.set_field(field, keyboard.query);
                                    self.replace("Album Info", self.tag_rows());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
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
            Provider::Archive => {
                if let Some(groups) = item.raw["groups"].as_object() {
                    for (format, files) in groups {
                        rows.push(Item::new(
                            format!("Download {format}"),
                            Choice::Download(DownloadRequest::Archive {
                                item: item.clone(),
                                format: format.clone(),
                            }),
                        ));
                        if let Some(file) = files.as_array().and_then(|a| a.first()) {
                            if let Some(url) = file["download_url"].as_str() {
                                rows.push(Item::new(
                                    format!("Listen ({format})"),
                                    Choice::Media(url.into(), item.title.clone(), false),
                                ));
                            }
                        }
                    }
                }
            }
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
        let mut rows = vec![
            Item::new(
                format!(
                    "Bluetooth: {}",
                    if self.bt_state.powered { "On" } else { "Off" }
                ),
                Choice::BtPower(!self.bt_state.powered),
            ),
            Item::new("Find Devices", Choice::BtScan),
        ];
        for d in &self.bt_state.devices {
            let mut row = Item::new(&d.name, Choice::BtDevice(d.address.clone()));
            row.detail = if d.connected {
                "Connected".into()
            } else if d.paired {
                "Paired".into()
            } else {
                d.address.clone()
            };
            if let Some(battery) = d.battery {
                row.detail.push_str(&format!(" - {battery}%"));
            }
            rows.push(row);
        }
        rows
    }
    pub(super) fn tick(&mut self, dt: f32) -> Vec<Effect> {
        for _ in 0..16 {
            let Some(event) = self.weather.poll() else {
                break;
            };
            match event {
                WeatherEvent::State(state) => {
                    self.weather_state = state;
                    self.status.clear();
                    if self.pages.first().is_some_and(|p| p.title == "Weather")
                        && self.forecast.is_none()
                    {
                        self.pages.clear();
                        self.push("Weather", self.weather_rows());
                    }
                }
                WeatherEvent::SearchResults { query, places } => {
                    self.status.clear();
                    self.push(
                        format!("Cities: {query}"),
                        places
                            .into_iter()
                            .map(|place| {
                                let detail = place.location();
                                let mut row =
                                    Item::new(&place.name, Choice::WeatherAdd(place.clone()));
                                row.detail = detail;
                                row
                            })
                            .collect(),
                    );
                }
                WeatherEvent::Error { context, message } => {
                    self.status.clear();
                    self.effects
                        .push(Effect::Notice(format!("{context}: {message}")));
                }
            }
        }
        for _ in 0..16 {
            let Some(event) = self.share.poll() else {
                break;
            };
            match event {
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
                    self.bt_state = state;
                    self.status.clear();
                    if self.pages.first().is_some_and(|p| p.title == "Bluetooth") {
                        self.pages.clear();
                        self.push("Bluetooth", self.bluetooth_rows());
                    }
                }
                Err(error) => {
                    self.status.clear();
                    self.effects.push(Effect::Notice(error));
                }
            }
        }
        for _ in 0..16 {
            let Some(event) = self.services.poll() else {
                break;
            };
            match event {
                Event::Progress {
                    status,
                    done,
                    total,
                    id,
                } => {
                    if self
                        .jobs
                        .get(&id)
                        .is_some_and(|(_, p)| matches!(p, Purpose::Preview(_)))
                    {
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
                    if let Some(Purpose::Preview(url)) = purpose.as_ref() {
                        if let Ok(Output::Files(files)) = &result {
                            if let Some(path) = files.first() {
                                self.previews.insert(url.clone(), Some(path.clone()));
                                self.emit_media_art();
                            }
                        }
                        continue;
                    }
                    self.status.clear();
                    match result {
                        Err(error) => self.effects.push(Effect::Notice(error)),
                        Ok(output) => match output {
                            Output::CurationApplied { matched, total } => {
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
                                    let mut rows = vec![Item::new(
                                        format!("Search: {query}"),
                                        Choice::Search(provider),
                                    )];
                                    for item in items {
                                        let mut row = Item::new(
                                            &item.title,
                                            if metadata && self.cover_search {
                                                Choice::UseCover(item.clone())
                                            } else {
                                                Choice::Online(item.clone(), metadata)
                                            },
                                        );
                                        row.detail = if item.year.is_empty() {
                                            item.artist.clone()
                                        } else {
                                            format!("{} - {}", item.artist, item.year)
                                        };
                                        rows.push(row);
                                    }
                                    if total > page as usize * 25 {
                                        rows.push(Item::new(
                                            "More Results",
                                            Choice::NextSearch(provider, query, page + 1, metadata),
                                        ));
                                    }
                                    self.replace(provider.name(), rows);
                                }
                            }
                            Output::Item(item) => {
                                if matches!(
                                    item.provider,
                                    Provider::YouTube | Provider::YouTubePlaylists
                                ) {
                                    self.detail_item = Some(item.clone());
                                }
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

                                let metadata =
                                    matches!(purpose, Some(Purpose::Details { metadata: true }));
                                self.push(&item.title, self.details_rows(&item, metadata));
                            }
                            Output::Feed { podcast, episodes } => {
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

                                let mut rows = vec![
                                    Item::new("Subscribe", Choice::Subscribe(podcast.clone())),
                                    Item::new("Unsubscribe", Choice::Unsubscribe(podcast.clone())),
                                    Item::new(
                                        "Download All Episodes",
                                        Choice::DownloadEpisodes(podcast.clone(), episodes.clone()),
                                    ),
                                ];
                                for episode in episodes {
                                    let mut row = Item::new(
                                        &episode.title,
                                        Choice::Episode(episode.clone(), podcast.clone()),
                                    );
                                    row.detail = format!(
                                        "{}{}",
                                        if podcast.files.contains_key(episode.key()) {
                                            "Downloaded — "
                                        } else {
                                            ""
                                        },
                                        episode.date.chars().take(10).collect::<String>()
                                    );
                                    rows.push(row);
                                }
                                self.push(&podcast.name, rows);
                            }
                            Output::Subscriptions(podcasts) => {
                                let mut rows = vec![Item::new(
                                    "Find Podcasts",
                                    Choice::Page(ExtraPage::Online(Provider::Podcast)),
                                )];
                                for podcast in podcasts {
                                    rows.push(Item::new(
                                        &podcast.name,
                                        Choice::Feed(podcast.clone()),
                                    ));
                                }
                                self.replace("Podcasts", rows);
                            }
                            Output::Files(files) => {
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
                                }
                            }
                            Output::Saved => {
                                if matches!(purpose, Some(Purpose::Remember)) {
                                    continue;
                                }
                                self.effects.push(Effect::Notice("Changes saved".into()));
                                self.effects.push(Effect::Rescan);
                            }
                            Output::Stream(source) => {
                                if let Some(Purpose::Stream {
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
                                    self.effects.push(Effect::Stream {
                                        source,
                                        video,
                                        origin,
                                        start,
                                        queue,
                                        index,
                                    });
                                    self.emit_media_art();
                                }
                            }
                            Output::ToolsReady => self
                                .effects
                                .push(Effect::Notice("YouTube tools are ready".into())),
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
                    .skip((page.scroll / 52.0).max(0.0) as usize)
                    .take(8)
                    .filter_map(|r| choice_art(&r.choice))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for url in preview_urls {
            self.request_preview(url);
        }
        if let Some(page) = self.pages.last_mut() {
            page.scroll += (page.target - page.scroll) * (1.0 - (-dt * 22.0).exp());
        }
        std::mem::take(&mut self.effects)
    }
    fn weather_rows(&self) -> Vec<Item> {
        let mut rows = vec![
            Item::new("Add City", Choice::WeatherSearch),
            Item::new(
                format!(
                    "Units: {}",
                    if self.weather_state.fahrenheit {
                        "Fahrenheit"
                    } else {
                        "Celsius"
                    }
                ),
                Choice::WeatherUnits,
            ),
        ];
        for (index, place) in self.weather_state.places.iter().enumerate() {
            let mut row = Item::new(&place.name, Choice::WeatherPlace(index));
            row.detail = place.location();
            rows.push(row);
        }
        rows
    }
    pub(super) fn draw(
        &mut self,
        r: &mut Renderer,
        dark: bool,
        elapsed: f64,
        art: &mut crate::art::Artwork,
    ) {
        if let Some(editor) = self.cover_editor.as_ref() {
            editor.draw(r, art, dark);
            return;
        }
        if let Some(index) = self
            .forecast
            .filter(|_| self.pages.len() == self.forecast_depth)
        {
            if let Some(place) = self.weather_state.places.get(index) {
                betterpod_rust::weather::draw_forecast(
                    r,
                    place,
                    self.forecast_day,
                    self.weather_state.fahrenheit,
                    elapsed as f32,
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
        if let Some((keyboard, typing)) = &self.keyboard {
            let title = match typing {
                Typing::NewPlaylist => "New Playlist",
                Typing::Search(_, true) => "Find Album Information",
                Typing::Search(_, false) => "Search Online",
                Typing::Tag(_) => "Edit Information",
                Typing::Weather => "Find a City",
                Typing::Receive => "Enter the 8-Digit Share Code",
                Typing::SharingName => "Your Player’s Sharing Name",
            };
            draw_keyboard_title(r, keyboard, dark, title);
        }
    }
    pub(super) fn shutdown(&mut self) {
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
        Choice::Online(i, _)
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
