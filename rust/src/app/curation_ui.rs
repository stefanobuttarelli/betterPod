//! Cached library curation screens. Submitting network/filesystem work is an
//! effect consumed by Extras; this controller performs no I/O.
use super::{Intent, Page as VisualPage, Row as VisualRow, Screen, ViewKind};
mod covers;
use betterpod_rust::{
    curation::CoverQuery,
    input::Action,
    library::{Album, Library},
    metadata::TagEdit,
    online::Task,
    render::Renderer,
};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};
#[derive(Clone, Debug)]
pub enum Mode {
    MissingInfo,
    MissingCovers,
    Merge { paths: Vec<PathBuf> },
    MergeQuery { paths: Vec<PathBuf>, query: String },
}
#[derive(Clone, Debug)]
pub enum CurationEffect {
    Close,
    AlbumOptions(usize),
    EditInfo {
        paths: Vec<PathBuf>,
        title: String,
        edit: TagEdit,
    },
    SearchCoversAll {
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
    },
    SearchInput(String),
    Task(Task),
    StartCoverSearch(Vec<CoverQuery>),
    Rescan,
}
#[derive(Clone)]
enum RowAction {
    Search,
    EditInfo {
        paths: Vec<PathBuf>,
        title: String,
        edit: TagEdit,
    },
    Covers {
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
    },
    Submit(Arc<Mutex<Option<Task>>>),
    Merge {
        target_paths: Vec<PathBuf>,
        album: String,
        artist: String,
        disc: u32,
    },
}
#[derive(Clone)]
struct Row {
    title: String,
    subtitle: String,
    action: RowAction,
    cover: Option<PathBuf>,
    thumb: bool,
    album_index: Option<usize>,
}
struct ScreenState {
    title: String,
    empty: &'static str,
    rows: Arc<Vec<Row>>,
    selected: usize,
    scroll: f32,
    visual: Arc<Vec<VisualRow>>,
}
pub struct Curation {
    active: bool,
    title: String,
    empty: &'static str,
    rows: Arc<Vec<Row>>,
    selected: usize,
    scroll: f32,
    history: Vec<ScreenState>,
    merge_source: Arc<Vec<PathBuf>>,
    worker: CurationWorker,
    generation: u64,
    library: Arc<Library>,
    visual: Arc<Vec<VisualRow>>,
    merge_query: String,
    sheet: Option<(String, Arc<Vec<VisualRow>>, usize, RowAction)>,
    auto_covers: Option<covers::Covers>,
    auto_visible: bool,
    time: f64,
    navigation: Option<(VisualPage, bool, f64)>,
    retired_covers: Option<(covers::Covers, f64)>,
}
impl Default for Curation {
    fn default() -> Self {
        Self::new()
    }
}
impl Curation {
    #[cfg(test)]
    pub(crate) fn visual_fixture(key: &str, library: Arc<Library>) -> Result<Self, String> {
        if !matches!(
            key,
            "curation-covers-ready" | "curation-covers-running" | "curation-covers-done"
        ) {
            return Err(format!("Unknown curation fixture {key}"));
        }
        let albums = library
            .albums
            .iter()
            .map(|a| CoverQuery::from_album(a, &library))
            .collect();
        let mut curation = Self::new();
        curation.open(Mode::MissingCovers, library);
        curation.enter_auto(None);
        curation.auto_covers = Some(covers::Covers::visual_fixture(albums, key));
        curation.navigation = None;
        Ok(curation)
    }
    pub fn new() -> Self {
        Self {
            active: false,
            title: String::new(),
            empty: "Nothing to show",
            rows: Arc::new(Vec::new()),
            selected: 0,
            scroll: 0.0,
            history: Vec::new(),
            merge_source: Arc::new(Vec::new()),
            worker: CurationWorker::new(),
            generation: 0,
            library: Arc::new(Library::default()),
            visual: Arc::new(Vec::new()),
            merge_query: String::new(),
            sheet: None,
            auto_covers: None,
            auto_visible: false,
            time: 0.,
            navigation: None,
            retired_covers: None,
        }
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn close(&mut self) {
        self.auto_visible = false;
        self.active = false;
        self.history.clear();
        self.rows = Arc::new(Vec::new());
        self.merge_source = Arc::new(Vec::new());
        self.visual = Arc::new(Vec::new());
        self.sheet = None;
    }
    pub fn open(&mut self, mode: Mode, library: Arc<Library>) {
        self.auto_visible = false;
        self.merge_query.clear();
        self.sheet = None;
        self.history.clear();
        self.library = library;
        self.queue(mode);
    }
    fn queue(&mut self, mode: Mode) {
        self.active = true;
        self.selected = 0;
        self.scroll = 0.0;
        self.rows = Arc::new(Vec::new());
        self.empty = "Loading…";
        let mut loading = VisualRow::new("Loading…", Intent::Noop);
        loading.arrow = false;
        self.visual = Arc::new(vec![loading]);
        self.title = match &mode {
            Mode::MissingInfo => "Missing Info",
            Mode::MissingCovers => "Missing Covers",
            Mode::Merge { .. } => "Add to Album",
            Mode::MergeQuery { .. } => "Add to Album",
        }
        .into();
        self.generation = self.generation.wrapping_add(1);
        if self
            .worker
            .submit(self.generation, mode, self.library.clone())
            .is_err()
        {
            self.empty = "Library tools are busy; reopen this view";
        }
    }
    fn poll_rows(&mut self) {
        while let Some((generation, state, source)) = self.worker.poll() {
            if generation == self.generation && self.active {
                self.title = state.title;
                self.empty = state.empty;
                self.rows = state.rows;
                self.visual = state.visual;
                self.selected = state.selected;
                self.scroll = state.scroll;
                self.merge_source = source;
            }
        }
    }
    pub fn shutdown(&mut self) {
        if let Some(view) = &self.auto_covers {
            view.cancel();
        }
        self.worker.shutdown();
    }
    pub fn visual_page(&mut self) -> Option<VisualPage> {
        self.poll_rows();
        if !self.active {
            return None;
        }
        let mut page = VisualPage {
            mini: false,
            screen: Screen::Curation,
            kind: ViewKind::Full,
            title: self.title.clone(),
            rows: Arc::clone(&self.visual),
            selected: self.selected,
            scroll: self.scroll,
            target_scroll: self.scroll,
            enter: 0.,
        };
        page.reveal_selection();
        self.selected = page.selected;
        self.scroll = page.scroll;
        Some(page)
    }
    pub fn set_library(&mut self, library: Arc<Library>) {
        if Arc::ptr_eq(&self.library, &library) {
            return;
        }
        self.library = library;
        if self.active && !self.auto_visible && self.title == "Missing Covers" {
            self.refresh_missing_keep_rows();
        }
    }
    fn enter_auto(&mut self, albums: Option<Vec<CoverQuery>>) {
        let previous = self.visual_page();
        self.history.push(ScreenState {
            title: self.title.clone(),
            empty: self.empty,
            rows: Arc::clone(&self.rows),
            selected: self.selected,
            scroll: self.scroll,
            visual: Arc::clone(&self.visual),
        });
        if let Some(albums) = albums {
            self.auto_covers = Some(covers::Covers::new(albums));
        }
        self.auto_visible = true;
        self.title = "Find Missing Covers".into();
        self.visual = Arc::new(Vec::new());
        self.generation = self.generation.wrapping_add(1);
        if let Some(previous) = previous {
            self.navigation = Some((previous, true, self.time));
        }
    }
    fn refresh_missing_keep_rows(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        let _ = self.worker.submit(
            self.generation,
            Mode::MissingCovers,
            Arc::clone(&self.library),
        );
    }
    pub fn set_time(&mut self, now: f64) {
        self.time = now;
        if self
            .retired_covers
            .as_ref()
            .is_some_and(|(_, until)| now > *until)
        {
            self.retired_covers = None;
        }
    }
    pub fn take_navigation(&mut self) -> Option<(VisualPage, bool, f64)> {
        self.navigation.take()
    }
    pub fn navigation_depth(&self) -> usize {
        self.history.len()
    }
    pub fn draw_previous(
        &mut self,
        page: &VisualPage,
        r: &mut Renderer,
        dark: bool,
        assets: &betterpod_rust::visual_assets::VisualAssets,
        now: f64,
    ) -> bool {
        if page.title != "Find Missing Covers" {
            return false;
        }
        if let Some(view) = self
            .auto_covers
            .as_mut()
            .or_else(|| self.retired_covers.as_mut().map(|(view, _)| view))
        {
            view.draw(r, assets, dark, now);
            true
        } else {
            false
        }
    }
    pub fn is_custom(&self) -> bool {
        self.active && self.auto_visible
    }
    pub fn draw_custom(
        &mut self,
        r: &mut Renderer,
        dark: bool,
        assets: &betterpod_rust::visual_assets::VisualAssets,
        now: f64,
    ) {
        if let Some(view) = self.auto_covers.as_mut() {
            view.draw(r, assets, dark, now);
        }
    }
    pub fn cover_search_started(&mut self, handle: betterpod_rust::online::JobHandle) {
        if let Some(view) = self.auto_covers.as_mut() {
            view.started(handle);
        } else {
            handle.cancel();
        }
    }
    pub fn cover_search_failed(&mut self, message: String) {
        if let Some(view) = self.auto_covers.as_mut() {
            view.failed(message);
        }
    }
    pub fn cover_search_event(
        &mut self,
        event: betterpod_rust::online::Event,
    ) -> Option<CurationEffect> {
        let changed = matches!(&event,betterpod_rust::online::Event::Finished{result:Ok(betterpod_rust::online::Output::CurationApplied{matched,..}),..}if *matched>0);
        let view_changed = self
            .auto_covers
            .as_mut()
            .is_some_and(|view| view.event(event));
        if changed || view_changed {
            Some(CurationEffect::Rescan)
        } else {
            None
        }
    }
    pub fn visual_options(&self) -> Option<VisualPage> {
        let (title, labels, selected, _) = self.sheet.as_ref()?;
        Some(VisualPage {
            mini: false,
            screen: Screen::Curation,
            kind: ViewKind::Full,
            title: title.clone(),
            rows: Arc::clone(labels),
            selected: *selected,
            scroll: 0.,
            target_scroll: 0.,
            enter: 0.,
        })
    }
    pub fn has_options(&self) -> bool {
        self.sheet.is_some()
    }
    pub fn dismiss_options(&mut self) {
        self.sheet = None;
    }
    pub fn search_changed(&mut self, query: &str) {
        self.merge_query = query.into();
        self.queue(Mode::MergeQuery {
            paths: self.merge_source.as_ref().clone(),
            query: query.into(),
        });
    }
    pub fn handle(&mut self, action: Action) -> Option<CurationEffect> {
        self.poll_rows();
        if self.auto_visible {
            match self
                .auto_covers
                .as_mut()
                .and_then(|view| view.handle(action))
            {
                Some(covers::Response::Start(albums)) => {
                    return Some(CurationEffect::StartCoverSearch(albums))
                }
                Some(covers::Response::Back) => {
                    let previous = self.visual_page();
                    self.auto_visible = false;
                    if self
                        .auto_covers
                        .as_ref()
                        .is_some_and(|view| !view.running())
                    {
                        self.retired_covers =
                            self.auto_covers.take().map(|view| (view, self.time + 0.35));
                    }
                    if let Some(previous) = self.history.pop() {
                        self.title = previous.title;
                        self.empty = previous.empty;
                        self.rows = previous.rows;
                        self.selected = previous.selected;
                        self.scroll = previous.scroll;
                        self.visual = previous.visual;
                    }
                    self.refresh_missing_keep_rows();
                    if let Some(previous) = previous {
                        self.navigation = Some((previous, false, self.time));
                    }
                }
                None => {}
            }
            return None;
        }

        if let Some((_, labels, selected, _)) = self.sheet.as_mut() {
            match action {
                Action::Up | Action::Left => *selected = selected.saturating_sub(1),
                Action::Down | Action::Right => *selected = (*selected + 1).min(labels.len() - 1),
                Action::Back => self.sheet = None,
                Action::Select => {
                    let (_, _, selected, choice) = self.sheet.as_ref().unwrap().clone();
                    if selected == 0 {
                        match choice {
                            RowAction::Merge {
                                target_paths,
                                album,
                                artist,
                                ..
                            } => {
                                return Some(CurationEffect::Task(Task::MergeAlbums {
                                    paths: self.merge_source.as_ref().clone(),
                                    target_paths,
                                    album,
                                    artist,
                                }))
                            }
                            RowAction::Search => {
                                self.sheet = None;
                                self.search_changed("");
                            }
                            _ => self.sheet = None,
                        }
                    } else {
                        self.sheet = None;
                    }
                }
                _ => {}
            }
            return None;
        }
        if !self.active {
            return None;
        }
        match action {
            Action::Options | Action::SelectHeld => {
                if let Some(row) = self.rows.get(self.selected) {
                    if matches!(row.action, RowAction::Search) {
                        self.sheet = Some((
                            "Search".into(),
                            visual_labels(vec!["Clear search".into(), "Cancel".into()]),
                            0,
                            RowAction::Search,
                        ));
                    } else if let Some(index) = row.album_index {
                        return Some(CurationEffect::AlbumOptions(index));
                    }
                }
            }
            Action::Up => self.selected = self.selected.saturating_sub(1),
            Action::Down => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1))
            }
            Action::Back | Action::MenuDown => {
                if let Some(previous) = self.history.pop() {
                    self.generation = self.generation.wrapping_add(1);
                    self.title = previous.title;
                    self.empty = previous.empty;
                    self.rows = previous.rows;
                    self.visual = previous.visual;
                    self.selected = previous.selected;
                    self.scroll = previous.scroll;
                } else {
                    self.close();
                    return Some(CurationEffect::Close);
                }
            }
            Action::Select => {
                let row = self.rows.get(self.selected)?.action.clone();
                match row {
                    RowAction::Search => {
                        return Some(CurationEffect::SearchInput(self.merge_query.clone()))
                    }
                    RowAction::EditInfo { paths, title, edit } => {
                        return Some(CurationEffect::EditInfo { paths, title, edit })
                    }
                    RowAction::Covers {
                        paths,
                        album,
                        artist,
                    } => {
                        return Some(CurationEffect::SearchCoversAll {
                            paths,
                            album,
                            artist,
                        });
                    }
                    RowAction::Submit(task) => {
                        if self.auto_covers.as_ref().is_some_and(|view| view.running()) {
                            self.enter_auto(None);
                            return None;
                        }
                        let task = {
                            let mut queued = task.lock().ok()?;
                            if matches!(queued.as_ref(), Some(Task::FindMissingCovers { .. })) {
                                queued.take()?
                            } else {
                                queued.as_ref()?.clone()
                            }
                        };
                        if let Task::FindMissingCovers { albums } = task {
                            self.enter_auto(Some(albums));
                            return None;
                        }
                        return Some(CurationEffect::Task(task));
                    }
                    RowAction::Merge {
                        target_paths,
                        album,
                        artist,
                        disc,
                    } => {
                        let what = if self.merge_source.len() == 1 {
                            "1 song".into()
                        } else {
                            format!("{} songs", self.merge_source.len())
                        };
                        let title = format!("Add {what} to {album}?");
                        let label = if disc > 0 {
                            format!("Add as Disc {disc}")
                        } else {
                            "Add to Album".into()
                        };
                        self.sheet = Some((
                            title,
                            visual_labels(vec![label, "Cancel".into()]),
                            0,
                            RowAction::Merge {
                                target_paths,
                                album,
                                artist,
                                disc,
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
        None
    }
}
fn build_rows(mode: Mode, library: &Library) -> (ScreenState, Arc<Vec<PathBuf>>) {
    let mut rows = Vec::new();
    let mut source = Vec::new();
    let title: String;
    let empty: &'static str;
    let query = if let Mode::MergeQuery { query, .. } = &mode {
        query.clone()
    } else {
        String::new()
    };
    let album_row = |album: &Album, subtitle: String, action: RowAction| {
        let index = (album as *const Album as usize)
            .checked_sub(library.albums.as_ptr() as usize)
            .map(|bytes| bytes / std::mem::size_of::<Album>())
            .filter(|i| {
                library
                    .albums
                    .get(*i)
                    .is_some_and(|a| std::ptr::eq(a, album))
            });
        Row {
            title: album.name.clone(),
            subtitle,
            action,
            cover: album.cover.clone(),
            thumb: true,
            album_index: index,
        }
    };
    match mode {
        Mode::MissingInfo => {
            title = "Missing Info".into();
            empty = "Every album has its artist and track numbers";
            for album in &library.albums {
                if !album_missing_info(album, library) {
                    continue;
                }
                let tracks = album
                    .track_indices
                    .iter()
                    .filter_map(|i| library.tracks.get(*i))
                    .collect::<Vec<_>>();
                let no_artist = !usable_artist(&album.artist)
                    || tracks.iter().any(|t| !usable_artist(&t.artist));
                let no_number = tracks.iter().filter(|t| t.track == 0).count();
                let mut missing = Vec::new();
                if no_artist {
                    missing.push("no artist".into())
                }
                if no_number > 0 {
                    missing.push(format!("{no_number} without a number"));
                }
                let first = tracks.first().copied();
                let mut edit = TagEdit {
                    album: Some(album.name.clone()),
                    ..Default::default()
                };
                if let Some(first) = first {
                    if tracks.iter().all(|t| t.artist == first.artist)
                        && usable_artist(&first.artist)
                    {
                        edit.artist = Some(first.artist.clone());
                    }
                    if tracks.iter().all(|t| t.album_artist == first.album_artist)
                        && !first.album_artist.is_empty()
                    {
                        edit.album_artist = Some(first.album_artist.clone());
                    }
                    if first.year > 0 && tracks.iter().all(|t| t.year == first.year) {
                        edit.year = Some(first.year);
                    }
                    if !first.genre.is_empty() && tracks.iter().all(|t| t.genre == first.genre) {
                        edit.genre = Some(first.genre.clone());
                    }
                }
                rows.push(album_row(
                    album,
                    missing.join(" · "),
                    RowAction::EditInfo {
                        paths: album_paths(album, library),
                        title: album.name.clone(),
                        edit,
                    },
                ));
            }
            rows.sort_by_key(|r| r.title.to_lowercase());
        }
        Mode::MissingCovers => {
            title = "Missing Covers".into();
            empty = "Every album has a cover";
            let missing = library
                .albums
                .iter()
                .filter(|a| {
                    !a.name.trim().is_empty()
                        && !a.track_indices.is_empty()
                        && a.cover.as_ref().is_none_or(|path| !path.is_file())
                })
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                let albums = missing
                    .iter()
                    .map(|a| CoverQuery::from_album(a, library))
                    .collect::<Vec<_>>();
                let count = albums.len();
                rows.push(Row {
                    title: "Search All Automatically".into(),
                    subtitle: format!(
                        "{count} {}, best matches only",
                        if count == 1 { "album" } else { "albums" }
                    ),
                    action: RowAction::Submit(Arc::new(Mutex::new(Some(
                        Task::FindMissingCovers { albums },
                    )))),
                    cover: None,
                    thumb: false,
                    album_index: None,
                });
            }
            for album in missing {
                rows.push(album_row(
                    album,
                    album_subtitle(album),
                    RowAction::Covers {
                        paths: album_paths(album, library),
                        album: album.name.clone(),
                        artist: album.artist.clone(),
                    },
                ));
            }
        }
        Mode::Merge { paths } | Mode::MergeQuery { paths, query: _ } => {
            source = paths;
            title = "Add to Album".into();
            empty = "No other albums";
            let paths = source.iter().collect::<HashSet<_>>();
            let from = library.albums.iter().find(|a| {
                a.track_indices.iter().any(|i| {
                    library
                        .tracks
                        .get(*i)
                        .is_some_and(|t| paths.contains(&t.path))
                })
            });
            let artist = from.map_or("", |a| a.artist.as_str());
            let from_stem = from.map(|a| album_stem(&a.name)).unwrap_or_default();
            let needle = query.trim().to_lowercase();
            let mut near = Vec::new();
            let mut rest = Vec::new();
            let moving = library
                .tracks
                .iter()
                .filter(|t| paths.contains(&t.path))
                .map(|t| (t.disc, t.track))
                .collect::<Vec<_>>();
            for album in &library.albums {
                let target_paths = album_paths(album, library);
                if from.is_some_and(|from| std::ptr::eq(from, album))
                    || target_paths.is_empty()
                    || target_paths.iter().all(|p| paths.contains(p))
                {
                    continue;
                }
                if !needle.is_empty()
                    && !format!("{} {}", album.name, album.artist)
                        .to_lowercase()
                        .contains(&needle)
                {
                    continue;
                }
                let target = album
                    .track_indices
                    .iter()
                    .filter_map(|i| library.tracks.get(*i))
                    .map(|t| (t.disc, t.track))
                    .collect::<Vec<_>>();
                let disc = merge_disc(&target, &moving);
                let row = album_row(
                    album,
                    album_subtitle(album),
                    RowAction::Merge {
                        target_paths,
                        album: album.name.clone(),
                        artist: album.artist.clone(),
                        disc,
                    },
                );
                if usable_artist(artist) && album.artist.eq_ignore_ascii_case(artist)
                    || !from_stem.is_empty() && album_stem(&album.name) == from_stem
                {
                    near.push(row)
                } else {
                    rest.push(row)
                }
            }
            let count = near.len() + rest.len();
            rows.push(Row {
                title: query.clone(),
                subtitle: if query.is_empty() {
                    String::new()
                } else {
                    format!("{count} {}", if count == 1 { "album" } else { "albums" })
                },
                action: RowAction::Search,
                cover: None,
                thumb: false,
                album_index: None,
            });
            rows.extend(near);
            rows.extend(rest);
        }
    }
    let visual = if rows.is_empty() {
        let mut row = VisualRow::new(empty, Intent::Noop);
        row.arrow = false;
        vec![row]
    } else {
        rows.iter()
            .map(|r| {
                let mut v = VisualRow::new(&r.title, Intent::Noop);
                v.detail = r.subtitle.clone();
                v.cover = r.cover.clone();
                if r.thumb && r.cover.is_none() {
                    v.visual.icon = "default_album_artwork".into();
                }
                if matches!(r.action, RowAction::Search) {
                    v.visual.search_field = true;
                    v.visual.placeholder = "Search albums".into();
                    v.arrow = false;
                }
                v
            })
            .collect()
    };
    (
        ScreenState {
            title,
            empty,
            rows: Arc::new(rows),
            selected: 0,
            scroll: 0.,
            visual: Arc::new(visual),
        },
        Arc::new(source),
    )
}
type Ready = (u64, ScreenState, Arc<Vec<PathBuf>>);
struct CurationWorker {
    tx: Option<mpsc::SyncSender<(u64, Mode, Arc<Library>)>>,
    rx: Option<mpsc::Receiver<Ready>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl CurationWorker {
    fn new() -> Self {
        let (tx, jobs) = mpsc::sync_channel::<(u64, Mode, Arc<Library>)>(4);
        let (results, rx) = mpsc::sync_channel(4);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let worker = thread::Builder::new()
            .name("curation-views".into())
            .spawn(move || {
                let mut retained = VecDeque::new();
                while let Ok((id, mode, library)) = jobs.recv() {
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let (state, source) = build_rows(mode, &library);
                    retained.push_back((
                        state.rows.clone(),
                        state.visual.clone(),
                        source.clone(),
                        library,
                    ));
                    while retained.len() > 3 {
                        retained.pop_front();
                    }
                    if results.send((id, state, source)).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self {
            tx: Some(tx),
            rx: Some(rx),
            cancelled,
            worker,
        }
    }
    fn submit(&self, id: u64, mode: Mode, library: Arc<Library>) -> Result<(), ()> {
        self.tx
            .as_ref()
            .ok_or(())?
            .try_send((id, mode, library))
            .map_err(|_| ())
    }
    fn poll(&self) -> Option<Ready> {
        self.rx.as_ref()?.try_recv().ok()
    }
    fn shutdown(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.tx.take();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for CurationWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn album_paths(album: &Album, library: &Library) -> Vec<PathBuf> {
    album
        .track_indices
        .iter()
        .filter_map(|&i| library.tracks.get(i).map(|t| t.path.clone()))
        .collect()
}
fn album_subtitle(album: &Album) -> String {
    let count = album.track_indices.len();
    let songs = format!("{count} {}", if count == 1 { "song" } else { "songs" });
    if album.artist.trim().is_empty() {
        songs
    } else {
        format!("{} · {songs}", album.artist)
    }
}
fn visual_labels(labels: Vec<String>) -> Arc<Vec<VisualRow>> {
    Arc::new(
        labels
            .into_iter()
            .map(|s| VisualRow::new(s, Intent::Noop))
            .collect(),
    )
}
fn album_stem(name: &str) -> String {
    let mut name = name.to_lowercase();
    for sep in [" (", " [", " - ", " cd", " disc", " disk", ":"] {
        if let Some(i) = name.find(sep).filter(|i| *i > 0) {
            name.truncate(i);
        }
    }
    name.trim().into()
}
fn merge_disc(target: &[(u32, u32)], moving: &[(u32, u32)]) -> u32 {
    let have = target
        .iter()
        .map(|(d, n)| (1.max(*d), *n))
        .collect::<HashSet<_>>();
    let last = target.iter().map(|(d, _)| *d).max().unwrap_or(1).max(1);
    if moving
        .iter()
        .any(|(d, n)| *n > 0 && have.contains(&(1.max(*d), *n)))
    {
        last + 1
    } else {
        0
    }
}
fn usable_artist(artist: &str) -> bool {
    let artist = artist.trim();
    !artist.is_empty()
        && !matches!(
            artist.to_lowercase().as_str(),
            "unknown artist" | "unknown" | "<unknown>" | "unknown album artist"
        )
}
fn album_missing_info(album: &Album, library: &Library) -> bool {
    !usable_artist(&album.artist)
        || album
            .track_indices
            .iter()
            .filter_map(|&i| library.tracks.get(i))
            .any(|t| !usable_artist(&t.artist) || t.track == 0)
}
#[cfg(test)]
mod tests {
    use super::*;
    use betterpod_rust::library::Track;
    fn wait(c: &mut Curation) {
        let start = std::time::Instant::now();
        while c.rows.is_empty() && start.elapsed() < std::time::Duration::from_secs(2) {
            c.poll_rows();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn fixture() -> Library {
        Library::from_tracks(vec![
            Track {
                path: "/music/one/one.flac".into(),
                title: "One".into(),
                artist: "Unknown Artist".into(),
                album: "One".into(),
                ..Default::default()
            },
            Track {
                path: "/music/two/two.flac".into(),
                title: "Two".into(),
                artist: "Artist".into(),
                album: "Two".into(),
                track: 1,
                ..Default::default()
            },
        ])
    }
    #[test]
    fn missing_info_opens_edit_and_missing_cover_search_carries_cached_paths() {
        let library = fixture();
        let mut c = Curation::new();
        c.open(Mode::MissingInfo, Arc::new(library.clone()));
        wait(&mut c);
        assert_eq!(c.rows.len(), 1);
        assert!(matches!(
            c.handle(Action::Select),
            Some(CurationEffect::EditInfo { .. })
        ));
        c.open(Mode::MissingCovers, Arc::new(library.clone()));
        wait(&mut c);
        assert!(c.handle(Action::Select).is_none());
        assert!(c.is_custom());
        match c.handle(Action::Select).unwrap() {
            CurationEffect::StartCoverSearch(albums) => assert_eq!(albums.len(), 2),
            _ => panic!("automatic search requires the explicit Find Covers action"),
        }
    }
    #[test]
    fn merge_picker_excludes_source_and_emits_logical_merge() {
        let library = fixture();
        let source = vec![PathBuf::from("/music/one/one.flac")];
        let mut c = Curation::new();
        c.open(
            Mode::Merge {
                paths: source.clone(),
            },
            Arc::new(library),
        );
        wait(&mut c);
        assert_eq!(c.rows.len(), 2);
        assert!(matches!(
            c.handle(Action::Select),
            Some(CurationEffect::SearchInput(_))
        ));
        c.handle(Action::Down);
        assert!(c.handle(Action::Select).is_none());
        assert!(c.has_options());
        match c.handle(Action::Select).unwrap() {
            CurationEffect::Task(Task::MergeAlbums {
                paths,
                target_paths,
                album,
                ..
            }) => {
                assert_eq!(paths, source);
                assert_eq!(target_paths, vec![PathBuf::from("/music/two/two.flac")]);
                assert_eq!(album, "Two");
            }
            _ => panic!("wrong task"),
        }
    }
    #[test]
    fn source_disc_collision_only_moves_numbered_collisions() {
        assert_eq!(merge_disc(&[(1, 1), (2, 4)], &[(1, 1)]), 3);
        assert_eq!(merge_disc(&[(0, 1)], &[(0, 2)]), 0);
        assert_eq!(merge_disc(&[(0, 0)], &[(0, 0)]), 0);
    }
    #[test]
    fn merging_one_song_excludes_its_entire_current_album() {
        let mut tracks = fixture().tracks;
        tracks.push(Track {
            path: "/music/one/second.flac".into(),
            title: "Second".into(),
            artist: "Unknown Artist".into(),
            album: "One".into(),
            ..Default::default()
        });
        let mut c = Curation::new();
        c.open(
            Mode::Merge {
                paths: vec!["/music/one/one.flac".into()],
            },
            Arc::new(Library::from_tracks(tracks)),
        );
        wait(&mut c);
        assert_eq!(c.rows.len(), 2);
        assert_eq!(c.rows[1].title, "Two");
    }
    #[test]
    fn automatic_cover_view_emits_nested_navigation_and_returns_to_cached_picker() {
        let mut c = Curation::new();
        c.set_time(1.);
        c.open(Mode::MissingCovers, Arc::new(fixture()));
        wait(&mut c);
        let picker = Arc::clone(&c.visual);
        c.handle(Action::Select);
        let (previous, forward, started) = c.take_navigation().unwrap();
        assert_eq!(previous.title, "Missing Covers");
        assert!(Arc::ptr_eq(&previous.rows, &picker));
        assert!(forward);
        assert_eq!(started, 1.);
        assert_eq!(c.navigation_depth(), 1);
        assert!(c.is_custom());
        c.set_time(1.1);
        c.handle(Action::Back);
        let (previous, forward, started) = c.take_navigation().unwrap();
        assert_eq!(previous.title, "Find Missing Covers");
        assert!(!forward);
        assert_eq!(started, 1.1);
        assert_eq!(c.navigation_depth(), 0);
        assert!(!c.is_custom());
        assert!(c.retired_covers.is_some());
        c.set_time(1.46);
        assert!(c.retired_covers.is_none());
    }
}
