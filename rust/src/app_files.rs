//! File browser and statistics controller. Directory access, disk statistics and
//! confirmed deletion belong exclusively to the FileClient worker.
use betterpod_rust::{
    files::{self, FileClient},
    input::Action,
    library::Library,
    render::{Renderer, BLACK, WHITE},
};
use std::{path::PathBuf, sync::Arc, time::Instant};
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
    Play(PathBuf),
    Rescan,
}
use files::ViewRow as Row;
pub struct Files {
    client: FileClient,
    music: PathBuf,
    title: String,
    mode: Mode,
    rows: Arc<Vec<Row>>,
    selected: usize,
    scroll: f32,
    current: PathBuf,
    history: Vec<PathBuf>,
    pending: bool,
    confirmation: Option<files::Entry>,
    last: Instant,
}
impl Files {
    pub fn new(music: PathBuf, data: PathBuf) -> Self {
        Self {
            client: FileClient::new(music.clone(), data),
            current: music.clone(),
            music,
            title: "Browse Files".into(),
            mode: Mode::Browse,
            rows: Arc::new(Vec::new()),
            selected: 0,
            scroll: 0.0,
            history: Vec::new(),
            pending: false,
            confirmation: None,
            last: Instant::now(),
        }
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn open(&mut self, mode: Mode, library: Arc<Library>) {
        self.mode = mode;
        self.rows = Arc::new(Vec::new());
        self.selected = 0;
        self.scroll = 0.0;
        self.history.clear();
        self.confirmation = None;
        self.pending = false;
        match mode {
            Mode::Browse => {
                self.title = "Browse Files".into();
                self.current = self.music.clone();
                self.request(files::Command::Browse(self.current.clone()));
            }
            Mode::Stats => {
                self.title = "Library Statistics".into();
                self.request(files::Command::StatsLibrary(library));
            }
            Mode::Scanned => {
                self.title = "Scanned Folders".into();
                self.request(files::Command::Scanned(library));
            }
        }
    }
    fn request(&mut self, command: files::Command) {
        match self.client.submit(command) {
            Ok(()) => self.pending = true,
            Err(error) => {
                self.pending = false;
                self.rows = Arc::new(vec![text("File operation failed", error)]);
            }
        }
    }
    pub fn handle(&mut self, action: Action) -> Option<Effect> {
        if let Some(entry) = self.confirmation.clone() {
            return match action {
                Action::Back | Action::MenuDown => {
                    self.confirmation = None;
                    None
                }
                Action::Select => {
                    self.confirmation = None;
                    self.request(files::Command::Delete {
                        path: entry.path,
                        confirmed: true,
                    });
                    None
                }
                _ => None,
            };
        }
        match action {
            Action::Up => self.selected = self.selected.saturating_sub(1),
            Action::Down => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1))
            }
            Action::Back | Action::MenuDown => {
                if matches!(self.mode, Mode::Browse) {
                    if let Some(path) = self.history.pop() {
                        self.request(files::Command::Browse(path));
                    } else {
                        return Some(Effect::Exit);
                    }
                } else {
                    return Some(Effect::Exit);
                }
            }
            Action::Select if !self.pending => {
                let entry = self.rows.get(self.selected)?.entry.clone()?;
                if entry.directory {
                    self.history.push(self.current.clone());
                    self.request(files::Command::Browse(entry.path));
                } else if entry.audio {
                    return Some(Effect::Play(entry.path));
                }
            }
            Action::Options if !self.pending && matches!(self.mode, Mode::Browse) => {
                self.confirmation = self.rows.get(self.selected).and_then(|r| r.entry.clone());
            }
            _ => {}
        }
        None
    }
    pub fn tick(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        while let Some(event) = self.client.poll() {
            self.pending = false;
            match event {
                files::Event::Listed { path, rows } => {
                    if matches!(self.mode, Mode::Browse) {
                        self.current = path;
                        self.rows = rows;
                        self.selected = 0;
                        self.scroll = 0.0;
                    }
                }
                files::Event::Info { scanned, rows } => {
                    if scanned == matches!(self.mode, Mode::Scanned)
                        && !matches!(self.mode, Mode::Browse)
                    {
                        self.rows = rows;
                        self.selected = 0;
                        self.scroll = 0.0;
                    }
                }
                files::Event::Space(_) => {}
                files::Event::Deleted => {
                    effects.push(Effect::Rescan);
                    effects.push(Effect::Notice("Deleted".into()));
                    self.request(files::Command::Browse(self.current.clone()));
                }
                files::Event::Error(error) => effects.push(Effect::Notice(error)),
            }
        }
        effects
    }
    pub fn shutdown(&mut self) {
        self.client.shutdown();
    }
    pub fn draw(&mut self, r: &mut Renderer, dark: bool) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let target = self.selected.saturating_sub(5) as f32 * 57.0;
        self.scroll += (target - self.scroll) * (1.0 - (-18.0 * dt).exp());
        let (bg, fg, muted) = if dark {
            ([0.035, 0.045, 0.06, 1.0], WHITE, [0.62, 0.67, 0.74, 1.0])
        } else {
            ([0.95, 0.96, 0.98, 1.0], BLACK, [0.38, 0.42, 0.48, 1.0])
        };
        r.rect(0.0, 0.0, 640.0, 480.0, bg);
        r.centered(&self.title, 10.0, 9.0, 620.0, 24, fg);
        r.set_clip(0.0, 45.0, 640.0, 393.0);
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
            r.text(
                &row.title,
                17.0,
                y + 3.0,
                21,
                if selected { WHITE } else { fg },
                595.0,
            );
            r.text(
                &row.subtitle,
                17.0,
                y + 31.0,
                15,
                if selected {
                    [0.86, 0.93, 1.0, 1.0]
                } else {
                    muted
                },
                595.0,
            );
        }
        if self.rows.is_empty() {
            r.centered(
                if self.pending {
                    "Loading…"
                } else {
                    "No items in this view"
                },
                15.0,
                220.0,
                610.0,
                22,
                muted,
            );
        }
        r.reset_clip();
        r.rect(0.0, 444.0, 640.0, 36.0, bg);
        r.text(
            if self.pending {
                "Working…"
            } else if matches!(self.mode, Mode::Browse) {
                "A Open / Play    Y Delete    B Back"
            } else {
                "B Back"
            },
            14.0,
            453.0,
            16,
            muted,
            610.0,
        );
        if let Some(entry) = &self.confirmation {
            r.rect(0.0, 0.0, 640.0, 480.0, [0.0, 0.0, 0.0, 0.70]);
            r.gradient(
                40.0,
                140.0,
                560.0,
                205.0,
                [0.20, 0.07, 0.07, 1.0],
                [0.10, 0.04, 0.04, 1.0],
            );
            r.centered(
                if entry.directory {
                    "Delete this folder and its files?"
                } else {
                    "Delete this music file?"
                },
                52.0,
                159.0,
                536.0,
                24,
                WHITE,
            );
            r.centered(&entry.name, 60.0, 210.0, 520.0, 21, WHITE);
            r.centered(
                "This cannot be undone.",
                60.0,
                252.0,
                520.0,
                18,
                [0.95, 0.75, 0.75, 1.0],
            );
            r.centered("A Delete     B Cancel", 60.0, 299.0, 520.0, 21, WHITE);
        }
    }
}
fn text(title: &str, subtitle: String) -> Row {
    Row {
        title: title.into(),
        subtitle,
        entry: None,
    }
}
