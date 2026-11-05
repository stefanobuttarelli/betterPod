//! Cached library curation screens. Submitting network/filesystem work is an
//! effect consumed by Extras; this controller performs no I/O.
use betterpod_rust::{
    curation::CoverQuery,
    input::Action,
    library::{Album, Library},
    metadata::TagEdit,
    online::{Provider, Task},
    render::{Renderer, BLACK, WHITE},
};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Instant,
};
#[derive(Clone, Debug)]
pub enum Mode {
    MissingInfo,
    MissingCovers,
    Merge {
        paths: Vec<PathBuf>,
    },
    Covers {
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
    },
}
#[derive(Clone, Debug)]
pub enum CurationEffect {
    Close,
    OpenAlbum(usize),
    EditInfo {
        paths: Vec<PathBuf>,
        title: String,
        edit: TagEdit,
    },
    SearchCovers {
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
        provider: Provider,
    },
    Task(Task),
}
#[derive(Clone)]
enum RowAction {
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
    Provider {
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
        provider: Provider,
    },
    Submit(Arc<Mutex<Option<Task>>>),
    Merge {
        target_paths: Vec<PathBuf>,
        album: String,
        artist: String,
    },
}
#[derive(Clone)]
struct Row {
    title: String,
    subtitle: String,
    action: RowAction,
}
struct ScreenState {
    title: String,
    empty: &'static str,
    rows: Arc<Vec<Row>>,
    selected: usize,
    scroll: f32,
}
pub struct Curation {
    active: bool,
    title: String,
    empty: &'static str,
    rows: Arc<Vec<Row>>,
    selected: usize,
    scroll: f32,
    last_draw: Instant,
    history: Vec<ScreenState>,
    merge_source: Arc<Vec<PathBuf>>,
    worker: CurationWorker,
    generation: u64,
    library: Arc<Library>,
}
impl Default for Curation {
    fn default() -> Self {
        Self::new()
    }
}
impl Curation {
    pub fn new() -> Self {
        Self {
            active: false,
            title: String::new(),
            empty: "Nothing to show",
            rows: Arc::new(Vec::new()),
            selected: 0,
            scroll: 0.0,
            last_draw: Instant::now(),
            history: Vec::new(),
            merge_source: Arc::new(Vec::new()),
            worker: CurationWorker::new(),
            generation: 0,
            library: Arc::new(Library::default()),
        }
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn close(&mut self) {
        self.active = false;
        self.history.clear();
        self.rows = Arc::new(Vec::new());
        self.merge_source = Arc::new(Vec::new());
    }
    pub fn open(&mut self, mode: Mode, library: Arc<Library>) {
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
        self.title = match &mode {
            Mode::MissingInfo => "Missing Info",
            Mode::MissingCovers => "Missing Covers",
            Mode::Merge { .. } => "Add to Another Album",
            Mode::Covers { .. } => "Choose Cover",
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
                self.selected = state.selected;
                self.scroll = state.scroll;
                self.merge_source = source;
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.worker.shutdown();
    }
    pub fn handle(&mut self, action: Action) -> Option<CurationEffect> {
        self.poll_rows();
        if !self.active {
            return None;
        }
        match action {
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
                    RowAction::EditInfo { paths, title, edit } => {
                        return Some(CurationEffect::EditInfo { paths, title, edit })
                    }
                    RowAction::Covers {
                        paths,
                        album,
                        artist,
                    } => {
                        self.history.push(ScreenState {
                            title: self.title.clone(),
                            empty: self.empty,
                            rows: std::mem::take(&mut self.rows),
                            selected: self.selected,
                            scroll: self.scroll,
                        });
                        self.queue(Mode::Covers {
                            paths,
                            album,
                            artist,
                        });
                    }
                    RowAction::Provider {
                        paths,
                        album,
                        artist,
                        provider,
                    } => {
                        return Some(CurationEffect::SearchCovers {
                            paths,
                            album,
                            artist,
                            provider,
                        })
                    }
                    RowAction::Submit(task) => {
                        return task.lock().ok()?.take().map(CurationEffect::Task)
                    }
                    RowAction::Merge {
                        target_paths,
                        album,
                        artist,
                    } => {
                        return Some(CurationEffect::Task(Task::MergeAlbums {
                            paths: self.merge_source.as_ref().clone(),
                            target_paths,
                            album,
                            artist,
                        }))
                    }
                }
            }
            _ => {}
        }
        None
    }
    pub fn draw(&mut self, r: &mut Renderer, dark: bool) {
        self.poll_rows();
        if !self.active {
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_draw).as_secs_f32().min(0.1);
        self.last_draw = now;
        let target = (self.selected.saturating_sub(5)) as f32 * 57.0;
        self.scroll += (target - self.scroll) * (1.0 - (-dt * 18.0).exp());
        let (bg, fg, muted) = if dark {
            ([0.035, 0.045, 0.06, 1.0], WHITE, [0.60, 0.66, 0.73, 1.0])
        } else {
            ([0.94, 0.95, 0.97, 1.0], BLACK, [0.37, 0.41, 0.47, 1.0])
        };
        r.rect(0.0, 0.0, 640.0, 480.0, bg);
        r.gradient(
            0.0,
            0.0,
            640.0,
            42.0,
            if dark { [0.12, 0.14, 0.18, 1.0] } else { WHITE },
            bg,
        );
        r.centered(&self.title, 15.0, 9.0, 610.0, 24, fg);
        if self.rows.is_empty() {
            r.centered(self.empty, 20.0, 215.0, 600.0, 23, muted);
        }
        r.set_clip(0.0, 45.0, 640.0, 396.0);
        let start = (self.scroll / 57.0).floor().max(0.0) as usize;
        for (index, row) in self.rows.iter().enumerate().skip(start).take(9) {
            let y = 49.0 + index as f32 * 57.0 - self.scroll;
            let selected = index == self.selected;
            if selected {
                r.gradient(
                    5.0,
                    y - 2.0,
                    630.0,
                    55.0,
                    [0.17, 0.52, 0.95, 1.0],
                    [0.06, 0.32, 0.76, 1.0],
                );
            }
            let text = if selected { WHITE } else { fg };
            let secondary = if selected {
                [0.85, 0.92, 1.0, 1.0]
            } else {
                muted
            };
            r.text(&row.title, 17.0, y + 3.0, 22, text, 585.0);
            r.text(&row.subtitle, 17.0, y + 31.0, 15, secondary, 585.0);
            r.text("›", 611.0, y + 11.0, 26, text, 25.0);
        }
        r.reset_clip();
        r.rect(0.0, 445.0, 640.0, 35.0, bg);
        r.text("A Select     B Back", 16.0, 454.0, 16, muted, 460.0);
        if !self.rows.is_empty() {
            r.text(
                &format!("{}/{}", self.selected + 1, self.rows.len()),
                540.0,
                454.0,
                16,
                muted,
                90.0,
            );
        }
    }
}
struct BuildState {
    title: String,
    empty: &'static str,
    rows: Vec<Row>,
    merge_source: Vec<PathBuf>,
}
fn build_rows(mode: Mode, library: &Library) -> (ScreenState, Arc<Vec<PathBuf>>) {
    let mut state = BuildState {
        title: String::new(),
        empty: "No items",
        rows: Vec::new(),
        merge_source: Vec::new(),
    };
    match &mode {
        Mode::MissingInfo => {
            state.title = "Missing Info".into();
            state.empty = "Every album has artist and track information";
            for album in &library.albums {
                if !album_missing_info(album, library) {
                    continue;
                }
                let paths = album_paths(album, library);
                let title = album.name.clone();
                state.rows.push(Row {
                    title: title.clone(),
                    subtitle: album_subtitle(album),
                    action: RowAction::EditInfo {
                        paths,
                        title,
                        edit: TagEdit::default(),
                    },
                });
            }
        }
        Mode::MissingCovers => {
            state.title = "Missing Covers".into();
            state.empty = "Every album has a cover";
            let missing: Vec<_> = library
                .albums
                .iter()
                .filter(|a| a.cover.is_none() && !a.track_indices.is_empty())
                .collect();
            if !missing.is_empty() {
                let albums = missing
                    .iter()
                    .map(|a| CoverQuery {
                        paths: album_paths(a, library),
                        album: a.name.clone(),
                        artist: a.artist.clone(),
                    })
                    .collect::<Vec<_>>();
                state.rows.push(Row {
                    title: "Search All Automatically".into(),
                    subtitle: format!("{} albums · best matches only", albums.len()),
                    action: RowAction::Submit(Arc::new(Mutex::new(Some(
                        Task::FindMissingCovers { albums },
                    )))),
                });
            }
            for album in missing {
                state.rows.push(Row {
                    title: album.name.clone(),
                    subtitle: album_subtitle(album),
                    action: RowAction::Covers {
                        paths: album_paths(album, library),
                        album: album.name.clone(),
                        artist: album.artist.clone(),
                    },
                });
            }
        }
        Mode::Merge { paths } => {
            state.merge_source.clone_from(paths);
            state.title = "Add to Another Album".into();
            state.empty = "There is no other album to choose";
            let source: HashSet<_> = paths.iter().collect();
            for album in &library.albums {
                let target_paths = album_paths(album, library);
                if target_paths.is_empty() || target_paths.iter().all(|p| source.contains(p)) {
                    continue;
                }
                state.rows.push(Row {
                    title: album.name.clone(),
                    subtitle: album_subtitle(album),
                    action: RowAction::Merge {
                        target_paths,
                        album: album.name.clone(),
                        artist: album.artist.clone(),
                    },
                });
            }
        }
        Mode::Covers {
            paths,
            album,
            artist,
        } => {
            state.title = "Choose Cover".into();
            state.empty = "No cover providers available";
            for provider in [
                Provider::MusicBrainz,
                Provider::Itunes,
                Provider::Deezer,
                Provider::Wikipedia,
            ] {
                state.rows.push(Row {
                    title: provider.name().into(),
                    subtitle: format!("{} · {}", artist, album),
                    action: RowAction::Provider {
                        paths: paths.clone(),
                        album: album.clone(),
                        artist: artist.clone(),
                        provider,
                    },
                });
            }
        }
    }

    (
        ScreenState {
            title: state.title,
            empty: state.empty,
            rows: Arc::new(state.rows),
            selected: 0,
            scroll: 0.0,
        },
        Arc::new(state.merge_source),
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
                    retained.push_back((state.rows.clone(), source.clone(), library));
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
    format!("{} · {} songs", album.artist, album.track_indices.len())
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
        match c.handle(Action::Select).unwrap() {
            CurationEffect::Task(Task::FindMissingCovers { albums }) => assert_eq!(albums.len(), 2),
            _ => panic!("wrong task"),
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
        assert_eq!(c.rows.len(), 1);
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
}
