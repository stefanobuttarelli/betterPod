//! Source Go file-browser controls and immutable list snapshots. The worker
//! alone reads directories and removes explicitly confirmed music paths.
use super::{Action, Intent, Page, Row, Screen, ViewKind};
use betterpod_rust::{
    files::{self, FileClient},
    library::Library,
};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
};
#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Browse,
    Stats,
    Scanned,
}
#[derive(Clone, Debug)]
pub enum Effect {
    Exit,
    Notice(String),
    Rescan,
    Deleted(Vec<PathBuf>),
}
#[derive(Clone)]
enum Sheet {
    Entry { entry: files::Entry, long: bool },
    Confirm { paths: Vec<PathBuf>, what: String },
}
pub struct Files {
    client: FileClient,
    music: PathBuf,
    current: PathBuf,
    mode: Mode,
    entries: Arc<Vec<files::ViewRow>>,
    selected: usize,
    scroll: f32,
    history: Vec<(PathBuf, usize, f32)>,
    restore: Option<(usize, f32)>,
    pending: bool,
    selecting: bool,
    picked: HashSet<PathBuf>,
    sheet: Option<Sheet>,
    sheet_selected: usize,
    sheet_rows: Option<Arc<Vec<Row>>>,
    deleting: VecDeque<PathBuf>,
    inflight: Option<PathBuf>,
    deleted: Vec<PathBuf>,
    last_error: Option<String>,
    rows: Option<Arc<Vec<Row>>>,
    free: Option<u64>,
    visual_time: f64,
    requested_navigation: Option<(Page, bool)>,
    pending_navigation: Option<(Page, bool, f64)>,
    library_info: Arc<Vec<files::ViewRow>>,
    library_revision: u64,
}
impl Files {
    pub fn new(music: PathBuf, data: PathBuf) -> Self {
        Self {
            client: FileClient::new(music.clone(), data),
            current: music.clone(),
            music,
            mode: Mode::Browse,
            entries: Arc::new(Vec::new()),
            selected: 0,
            scroll: 0.0,
            history: Vec::new(),
            restore: None,
            pending: false,
            selecting: false,
            picked: HashSet::new(),
            sheet: None,
            sheet_selected: 0,
            sheet_rows: None,
            deleting: VecDeque::new(),
            inflight: None,
            deleted: Vec::new(),
            last_error: None,
            rows: None,
            free: None,
            visual_time: 0.0,
            requested_navigation: None,
            pending_navigation: None,
            library_info: Arc::new(Vec::new()),
            library_revision: 0,
        }
    }
    pub fn title(&self) -> &str {
        match self.mode {
            Mode::Browse => {
                if self.current == self.music {
                    "Music Folder"
                } else {
                    self.current
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Files")
                }
            }
            Mode::Stats => "Library",
            Mode::Scanned => "Scanned Folders",
        }
    }
    pub fn open(&mut self, mode: Mode, library: Arc<Library>) {
        self.client.cancel_browse();
        self.requested_navigation = None;
        self.pending_navigation = None;
        self.mode = mode;
        self.entries = Arc::new(Vec::new());
        self.rows = None;
        self.selected = 0;
        self.scroll = 0.0;
        self.history.clear();
        self.sheet = None;
        self.selecting = false;
        self.picked.clear();
        self.last_error = None;
        match mode {
            Mode::Browse => {
                self.current = self.music.clone();
                self.request(files::Command::Browse(self.current.clone()));
                let _ = self.client.submit(files::Command::Stats);
            }
            Mode::Stats => self.request(files::Command::StatsLibrary(library)),
            Mode::Scanned => self.request(files::Command::Scanned(library)),
        }
    }
    fn request(&mut self, command: files::Command) {
        self.last_error = None;
        match self.client.submit(command) {
            Ok(()) => self.pending = true,
            Err(error) => {
                self.last_error = Some(error);
                self.pending = false;
            }
        }
        self.rows = None;
    }
    fn top_rows(&self) -> usize {
        if self.selecting {
            2
        } else {
            1
        }
    }
    fn entry(&self) -> Option<files::Entry> {
        self.selected
            .checked_sub(self.top_rows())
            .and_then(|i| self.entries.get(i))
            .and_then(|r| r.entry.clone())
    }
    fn dismiss(&mut self) {
        self.sheet = None;
        self.sheet_rows = None;
        self.sheet_selected = 0;
    }
    fn confirm(&mut self, paths: Vec<PathBuf>, what: String) {
        self.sheet = Some(Sheet::Confirm { paths, what });
        self.sheet_selected = 0;
        self.sheet_rows = None;
    }
    pub fn handle(&mut self, action: Action) -> Option<Effect> {
        if let Some(sheet) = self.sheet.clone() {
            let length = match &sheet {
                Sheet::Entry { long: true, .. } => 3,
                _ => 2,
            };
            match action {
                Action::Back | Action::Options => self.dismiss(),
                Action::Up => self.sheet_selected = self.sheet_selected.saturating_sub(1),
                Action::Down => self.sheet_selected = (self.sheet_selected + 1).min(length - 1),
                Action::Select => {
                    if self.sheet_selected == length - 1 {
                        self.dismiss();
                        return None;
                    }
                    match sheet {
                        Sheet::Entry { entry, long: true } if self.sheet_selected == 1 => {
                            self.selecting = true;
                            self.picked = HashSet::from([entry.path]);
                            self.selected += 1;
                            self.rows = None;
                            self.dismiss();
                        }
                        Sheet::Entry { entry, .. } => {
                            self.confirm(vec![entry.path], format!("“{}”", entry.name))
                        }
                        Sheet::Confirm { paths, .. } => {
                            self.dismiss();
                            self.deleting = paths.into();
                            self.deleted.clear();
                            self.delete_next();
                        }
                    }
                }
                _ => {}
            }
            return None;
        }
        match action {
            Action::Up => self.selected = self.selected.saturating_sub(1),
            Action::Down => {
                let n = self.visual_page().rows.len();
                self.selected = (self.selected + 1).min(n.saturating_sub(1));
            }
            Action::Back | Action::MenuDown => {
                if self.selecting {
                    self.selecting = false;
                    self.picked.clear();
                    self.selected = self.selected.saturating_sub(1);
                    self.rows = None;
                } else if let Some((path, selected, scroll)) = self.history.pop() {
                    let previous = self.visual_page();
                    self.requested_navigation = Some((previous, false));
                    self.restore = Some((selected, scroll));
                    self.request(files::Command::Browse(path));
                } else {
                    self.client.cancel_browse();
                    return Some(Effect::Exit);
                }
            }
            Action::Select if !self.pending => {
                if !matches!(self.mode, Mode::Browse) {
                    return None;
                }
                if self.selecting && self.selected < 2 {
                    if self.selected == 0 && !self.picked.is_empty() {
                        let paths = self
                            .entries
                            .iter()
                            .filter_map(|r| r.entry.as_ref())
                            .filter(|e| self.picked.contains(&e.path))
                            .map(|e| e.path.clone())
                            .collect::<Vec<_>>();
                        self.confirm(
                            paths,
                            format!(
                                "{} {}",
                                self.picked.len(),
                                if self.picked.len() == 1 {
                                    "item"
                                } else {
                                    "items"
                                }
                            ),
                        );
                    } else if self.selected == 1 {
                        self.selecting = false;
                        self.picked.clear();
                        self.selected = 1;
                        self.rows = None;
                    }
                } else if let Some(entry) = self.entry() {
                    if self.selecting {
                        if !self.picked.remove(&entry.path) {
                            self.picked.insert(entry.path);
                        }
                        self.rows = None;
                    } else if entry.directory {
                        let previous = self.visual_page();
                        self.requested_navigation = Some((previous, true));
                        self.history
                            .push((self.current.clone(), self.selected, self.scroll));
                        self.request(files::Command::Browse(entry.path));
                    } else {
                        self.sheet = Some(Sheet::Entry { entry, long: false });
                        self.sheet_selected = 0;
                        self.sheet_rows = None;
                    }
                }
            }
            Action::Options | Action::SelectHeld
                if !self.pending && !self.selecting && matches!(self.mode, Mode::Browse) =>
            {
                if let Some(entry) = self.entry() {
                    self.sheet = Some(Sheet::Entry { entry, long: true });
                    self.sheet_selected = 0;
                    self.sheet_rows = None;
                }
            }
            _ => {}
        }
        None
    }
    fn delete_next(&mut self) {
        if let Some(path) = self.deleting.pop_front() {
            self.inflight = Some(path.clone());
            self.request(files::Command::Delete {
                path,
                confirmed: true,
            });
        }
    }
    pub fn tick(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        for _ in 0..16 {
            let Some(event) = self.client.poll() else {
                break;
            };
            match event {
                files::Event::Listed {
                    path,
                    rows,
                    generation,
                    initial,
                } => {
                    if matches!(self.mode, Mode::Browse)
                        && generation == self.client.browse_generation()
                    {
                        self.pending = false;
                        self.current = path;
                        self.entries = rows;
                        self.rows = None;
                        if initial {
                            let (selected, scroll) = self
                                .restore
                                .take()
                                .unwrap_or((usize::from(!self.entries.is_empty()), 0.0));
                            self.selected = selected;
                            self.scroll = scroll;
                            if let Some((previous, forward)) = self.requested_navigation.take() {
                                self.pending_navigation =
                                    Some((previous, forward, self.visual_time));
                            }
                        }
                    }
                }
                files::Event::Info { scanned, rows } => {
                    if !scanned {
                        self.library_info = Arc::clone(&rows);
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                    if scanned == matches!(self.mode, Mode::Scanned)
                        && !matches!(self.mode, Mode::Browse)
                    {
                        self.pending = false;
                        self.entries = rows;
                        self.rows = None;
                        self.selected = 0;
                        self.scroll = 0.0;
                    }
                }
                files::Event::Space(space) => {
                    self.free = Some(space.free);
                    self.rows = None;
                }
                files::Event::Deleted => {
                    self.pending = false;
                    if let Some(path) = self.inflight.take() {
                        self.deleted.push(path);
                    }
                    if self.deleting.is_empty() {
                        self.selecting = false;
                        self.picked.clear();
                        effects.push(Effect::Deleted(std::mem::take(&mut self.deleted)));
                        effects.push(Effect::Rescan);
                        effects.push(Effect::Notice("Deleted".into()));
                        self.request(files::Command::Browse(self.current.clone()));
                    } else {
                        self.delete_next();
                    }
                }
                files::Event::Error(error) => {
                    self.pending = false;
                    self.last_error = Some(error.clone());
                    self.rows = None;
                    effects.push(Effect::Notice(error));
                    if self.inflight.take().is_some() {
                        if self.deleting.is_empty() {
                            if !self.deleted.is_empty() {
                                effects.push(Effect::Deleted(std::mem::take(&mut self.deleted)));
                                effects.push(Effect::Rescan);
                            }
                            self.request(files::Command::Browse(self.current.clone()));
                        } else {
                            self.delete_next();
                        }
                    }
                }
            }
        }
        effects
    }
    pub fn visual_page(&mut self) -> Page {
        if self.rows.is_none() {
            let mut rows = Vec::new();
            if matches!(self.mode, Mode::Browse) {
                if self.inflight.is_some() {
                    rows.push(info("Deleting…", ""));
                } else if self.selecting {
                    rows.push(info(
                        "Delete Selected",
                        if self.picked.is_empty() {
                            "Press [A] on the items to select them".into()
                        } else {
                            format!(
                                "{} {} selected",
                                self.picked.len(),
                                if self.picked.len() == 1 {
                                    "item"
                                } else {
                                    "items"
                                }
                            )
                        },
                    ));
                    rows.push(info("Done Selecting", ""));
                } else if self.pending && self.entries.is_empty() {
                    rows.push(info("Loading…", ""));
                } else {
                    rows.push(info(
                        format!(
                            "{} {}",
                            self.entries.len(),
                            if self.entries.len() == 1 {
                                "item"
                            } else {
                                "items"
                            }
                        ),
                        self.free
                            .map(|free| format!("{} free on the card", files::format_size(free)))
                            .unwrap_or_default(),
                    ));
                }
            }
            if let Some(error) = self.last_error.as_ref() {
                rows.push(info(error, ""));
            }
            for entry in self.entries.iter() {
                let mut row = info(&entry.title, &entry.subtitle);
                row.visual.value = entry.value.clone();
                if let Some(entry) = entry.entry.as_ref() {
                    row.detail = if entry.directory {
                        row.detail
                    } else {
                        file_kind(&entry.name)
                    };
                    row.arrow = entry.directory && !self.selecting;
                    row.visual.checked = self.selecting && self.picked.contains(&entry.path);
                }
                rows.push(row);
            }
            self.rows = Some(Arc::new(rows));
        }
        let mut page = Page {
            screen: Screen::Files,
            kind: ViewKind::Full,
            mini: false,
            title: self.title().into(),
            rows: Arc::clone(self.rows.as_ref().unwrap()),
            selected: self.selected,
            scroll: self.scroll,
            target_scroll: self.scroll,
            enter: 0.0,
        };
        page.reveal_selection();
        self.selected = page.selected;
        self.scroll = page.scroll;
        page
    }
    pub fn visual_options(&mut self) -> Option<Page> {
        let sheet = self.sheet.as_ref()?;
        let (title, labels): (String, Vec<String>) = match sheet {
            Sheet::Entry { entry, long } => {
                let mut labels = vec![if *long {
                    if entry.directory {
                        "Delete folder and everything in it".into()
                    } else {
                        "Delete file".into()
                    }
                } else {
                    "Delete".into()
                }];
                if *long {
                    labels.push("Select Several".into());
                }
                labels.push("Cancel".into());
                (entry.name.clone(), labels)
            }
            Sheet::Confirm { what, .. } => (
                format!("Delete {what}? This can't be undone."),
                vec!["Delete".into(), "Cancel".into()],
            ),
        };
        if self.sheet_rows.is_none() {
            self.sheet_rows = Some(Arc::new(
                labels
                    .into_iter()
                    .map(|label| Row::new(label, Intent::Noop))
                    .collect(),
            ));
        }
        Some(Page {
            screen: Screen::Files,
            kind: ViewKind::Full,
            mini: false,
            title,
            rows: Arc::clone(self.sheet_rows.as_ref().unwrap()),
            selected: self.sheet_selected,
            scroll: 0.0,
            target_scroll: 0.0,
            enter: 0.0,
        })
    }
    pub fn set_time(&mut self, now: f64) {
        self.visual_time = now;
    }
    pub fn take_navigation(&mut self) -> Option<(Page, bool, f64)> {
        self.pending_navigation.take()
    }
    pub fn navigation_depth(&self) -> usize {
        self.history.len()
    }
    pub fn refresh_library(&self, library: Arc<Library>) -> Result<(), String> {
        self.client.submit(files::Command::StatsLibrary(library))
    }
    pub fn library_information(&self) -> &Arc<Vec<files::ViewRow>> {
        &self.library_info
    }
    pub fn library_revision(&self) -> u64 {
        self.library_revision
    }
    pub fn leave(&mut self) {
        self.client.cancel_browse();
        self.requested_navigation = None;
        self.pending_navigation = None;
    }
    pub fn shutdown(&mut self) {
        self.client.shutdown();
    }
}
fn info(title: impl Into<String>, detail: impl Into<String>) -> Row {
    let mut row = Row::new(title, Intent::Noop);
    row.detail = detail.into();
    row.arrow = false;
    row
}
fn file_kind(name: &str) -> String {
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let label = match extension.to_ascii_lowercase().as_str() {
        "mp3" => "MP3 audio",
        "flac" => "FLAC audio",
        "m4a" | "mp4" => "AAC / ALAC audio",
        "wav" => "WAV audio",
        "aif" | "aiff" => "AIFF audio",
        "ogg" | "opus" => "Ogg audio",
        "jpg" | "jpeg" => "JPEG image",
        "png" => "PNG image",
        "m3u" | "m3u8" => "Playlist",
        "part" => "Unfinished download",
        "lrc" => "Lyrics",
        _ => "",
    };
    if !label.is_empty() {
        label.into()
    } else if name.starts_with("._") {
        "macOS leftover".into()
    } else if !extension.is_empty() {
        format!("{} file", extension.to_uppercase())
    } else {
        "File".into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_enter_and_back_retain_real_previous_page_and_selection() {
        let root = std::env::temp_dir().join(format!("betterpod-files-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let music = root.join("music");
        let data = root.join("private");
        std::fs::create_dir_all(music.join("Album")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(music.join("Album/cover.jpg"), b"cover").unwrap();
        let mut files = Files::new(music.clone(), data);
        files.set_time(3.);
        files.open(Mode::Browse, Arc::new(Library::default()));
        fn ready(files: &mut Files) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while files.pending && std::time::Instant::now() < deadline {
                files.tick();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(!files.pending);
        }
        ready(&mut files);
        assert_eq!(files.visual_page().rows[files.selected].label, "Album");
        files.handle(Action::Select);
        ready(&mut files);
        assert_eq!(files.navigation_depth(), 1);
        let (previous, forward, started) = files.take_navigation().unwrap();
        assert!(forward);
        assert_eq!(started, 3.);
        assert_eq!(previous.rows[previous.selected].label, "Album");
        assert_eq!(files.current, music.join("Album"));
        assert!(files
            .visual_page()
            .rows
            .iter()
            .any(|r| r.label == "cover.jpg"));
        files.handle(Action::Back);
        ready(&mut files);
        let (previous, forward, _) = files.take_navigation().unwrap();
        assert!(!forward);
        assert_eq!(previous.title, "Album");
        assert_eq!(files.navigation_depth(), 0);
        assert_eq!(files.visual_page().rows[files.selected].label, "Album");
        assert!(matches!(files.handle(Action::Back), Some(Effect::Exit)));
        files.shutdown();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn file_descriptions_follow_source_including_non_audio_and_leftovers() {
        assert_eq!(file_kind("lyrics.LRC"), "Lyrics");
        assert_eq!(file_kind("track.flac.part"), "Unfinished download");
        assert_eq!(file_kind("._song"), "macOS leftover");
        assert_eq!(file_kind("book.txt"), "TXT file");
        assert_eq!(file_kind("README"), "File");
    }
}
