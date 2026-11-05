//! Maintenance menus. Mutations are handed to main only after App is closed.
use crate::{input::Action, maintenance as m, render::Renderer, updater as u};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Mode {
    Music,
    Database,
    Maintenance,
    Update,
    DeleteAlbum(Vec<PathBuf>),
}
#[derive(Clone, Debug)]
pub enum Request {
    Maintenance {
        config: m::Config,
        command: m::Command,
        label: String,
    },
    Update {
        config: u::Config,
        manifest: u::Manifest,
    },
}
pub enum Effect {
    Exit,
    Notice(String),
    Run(Request),
    UpdateAvailable(bool),
}
#[derive(Clone, Copy)]
enum Destination {
    SelectMusic,
    CopyMusic,
    MoveMusic,
    MoveDatabase,
    UseDatabase,
}
#[derive(Clone)]
enum Choice {
    Browse(PathBuf, Destination),
    Select(PathBuf, Destination),
    Confirm(Request),
    Run(Request),
    Pending,
    Resume(m::PendingTransfer),
    Check,
    Apply(u::Manifest),
    Back,
}
struct Row {
    label: String,
    detail: String,
    choice: Choice,
}
impl Row {
    fn new(label: impl Into<String>, choice: Choice) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            choice,
        }
    }
}
struct Page {
    title: String,
    rows: Vec<Row>,
    selected: usize,
}
pub struct Controller {
    config: m::Config,
    update_config: u::Config,
    worker: m::Maintenance,
    updater: u::Updater,
    pages: Vec<Page>,
    browsing: Option<(u64, PathBuf, Destination)>,
    status: String,
    available: Option<u::Manifest>,
    check_after: Option<std::time::Instant>,
}
impl Controller {
    pub fn new(config: m::Config, update_config: u::Config) -> Result<Self, String> {
        let updater = u::Updater::new(update_config.clone())?;
        let check_after = if update_config.feed.is_some() {
            Some(std::time::Instant::now() + std::time::Duration::from_secs(30))
        } else {
            updater.command(u::Command::Check);
            None
        };
        Ok(Self {
            worker: m::Maintenance::new(config.clone())?,
            updater,
            config,
            update_config,
            pages: Vec::new(),
            browsing: None,
            status: String::new(),
            available: None,
            check_after,
        })
    }
    fn push(&mut self, title: impl Into<String>, rows: Vec<Row>) {
        self.pages.push(Page {
            title: title.into(),
            rows,
            selected: 0,
        });
    }
    fn request(&self, command: m::Command, label: impl Into<String>) -> Request {
        Request::Maintenance {
            config: self.config.clone(),
            command,
            label: label.into(),
        }
    }
    pub fn open(&mut self, mode: Mode) {
        self.pages.clear();
        self.status.clear();
        let root = self
            .config
            .music
            .parent()
            .unwrap_or(&self.config.music)
            .to_path_buf();
        match mode {
            Mode::Music => self.push(
                "Music folder",
                vec![
                    Row::new(
                        "Choose music folder",
                        Choice::Browse(root.clone(), Destination::SelectMusic),
                    ),
                    Row::new(
                        "Copy music to folder",
                        Choice::Browse(root.clone(), Destination::CopyMusic),
                    ),
                    Row::new(
                        "Move music to folder",
                        Choice::Browse(root, Destination::MoveMusic),
                    ),
                    Row::new("Resume interrupted transfer", Choice::Pending),
                ],
            ),
            Mode::Database => self.push(
                "Database",
                vec![
                    Row::new(
                        "Move database",
                        Choice::Browse(
                            self.config
                                .data
                                .parent()
                                .unwrap_or(&self.config.data)
                                .to_path_buf(),
                            Destination::MoveDatabase,
                        ),
                    ),
                    Row::new(
                        "Use existing Rust database",
                        Choice::Browse(
                            self.config
                                .data
                                .parent()
                                .unwrap_or(&self.config.data)
                                .to_path_buf(),
                            Destination::UseDatabase,
                        ),
                    ),
                ],
            ),
            Mode::Maintenance => {
                let clear = self.request(
                    m::Command::ClearArtwork { quiescent: true },
                    "Clear artwork cache",
                );
                let prefs = self.request(
                    m::Command::Reset {
                        scope: m::ResetScope::Preferences,
                        confirmed: true,
                        quiescent: true,
                    },
                    "Reset preferences",
                );
                let data = self.request(
                    m::Command::Reset {
                        scope: m::ResetScope::AppData,
                        confirmed: true,
                        quiescent: true,
                    },
                    "Clear app data",
                );
                let all = self.request(
                    m::Command::Reset {
                        scope: m::ResetScope::EverythingIncludingTools,
                        confirmed: true,
                        quiescent: true,
                    },
                    "Reset this Rust app",
                );
                self.push(
                    "Maintenance",
                    vec![
                        Row::new("Clear artwork cache", Choice::Confirm(clear)),
                        Row::new("Reset preferences", Choice::Confirm(prefs)),
                        Row::new("Clear app data", Choice::Confirm(data)),
                        Row::new("Reset Rust app and downloads", Choice::Confirm(all)),
                    ],
                );
            }
            Mode::Update => {
                let mut rows = vec![Row::new("Check for updates", Choice::Check)];
                if let Some(manifest) = &self.available {
                    rows.insert(
                        0,
                        Row::new(
                            format!("Install {}", manifest.version),
                            Choice::Apply(manifest.clone()),
                        ),
                    );
                }
                self.push("Software Update", rows);
                self.updater.command(u::Command::Check);
                self.status = "Checking…".into();
            }
            Mode::DeleteAlbum(paths) => {
                let label = format!("Delete {} music files", paths.len());
                let request = self.request(
                    m::Command::DeleteAlbum {
                        paths,
                        confirmed: true,
                    },
                    label.clone(),
                );
                self.confirm(request, &label);
            }
        }
    }
    pub fn title(&self) -> &str {
        self.pages
            .last()
            .map_or("Maintenance", |p| p.title.as_str())
    }
    pub fn update_available(&self) -> bool {
        self.available.is_some()
    }
    fn confirm(&mut self, request: Request, label: &str) {
        self.push(
            "Confirm",
            vec![
                Row::new("Cancel", Choice::Back),
                Row {
                    label: label.into(),
                    detail: "Playback stops while this operation runs".into(),
                    choice: Choice::Run(request),
                },
            ],
        );
    }
    fn browse(&mut self, path: PathBuf, kind: Destination) {
        if let Some(id) = self.worker.command(m::Command::Browse(path.clone())) {
            self.browsing = Some((id, path, kind));
            self.status = "Reading folders…".into();
        } else {
            self.status = "Folder worker is busy".into();
        }
    }
    pub fn handle(&mut self, action: Action) -> Option<Effect> {
        let Some(page) = self.pages.last_mut() else {
            return Some(Effect::Exit);
        };
        match action {
            Action::Up => page.selected = page.selected.saturating_sub(1),
            Action::Down => {
                page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1))
            }
            Action::Back => {
                self.pages.pop();
                if self.pages.is_empty() {
                    return Some(Effect::Exit);
                }
            }
            Action::Select => {
                let Some(choice) = page.rows.get(page.selected).map(|r| r.choice.clone()) else {
                    return None;
                };
                match choice {
                    Choice::Back => {
                        self.pages.pop();
                        if self.pages.is_empty() {
                            return Some(Effect::Exit);
                        }
                    }
                    Choice::Browse(path, kind) => self.browse(path, kind),
                    Choice::Select(path, kind) => {
                        let (command, label) = match kind {
                            Destination::SelectMusic => {
                                (m::Command::SelectMusic(path), "Use this music folder")
                            }
                            Destination::CopyMusic => (
                                m::Command::TransferMusic {
                                    destination: path,
                                    remove_source: false,
                                    confirmed: true,
                                },
                                "Copy music and use destination",
                            ),
                            Destination::MoveMusic => (
                                m::Command::TransferMusic {
                                    destination: path,
                                    remove_source: true,
                                    confirmed: true,
                                },
                                "Move music and remove verified sources",
                            ),
                            Destination::MoveDatabase => (
                                m::Command::MoveDatabase {
                                    destination: path,
                                    quiescent: true,
                                },
                                "Move database here",
                            ),
                            Destination::UseDatabase => (
                                m::Command::UseDatabase {
                                    directory: path,
                                    quiescent: true,
                                },
                                "Use this Rust database",
                            ),
                        };
                        let req = self.request(command, label);
                        self.confirm(req, label);
                    }
                    Choice::Confirm(req) => self.confirm(req, "Continue"),
                    Choice::Run(req) => return Some(Effect::Run(req)),
                    Choice::Pending => {
                        self.worker.command(m::Command::PendingTransfers);
                        self.status = "Reading transfers…".into();
                    }
                    Choice::Resume(t) => {
                        let req = self.request(
                            m::Command::ResumeTransfer {
                                manifest: t.manifest,
                                confirmed: true,
                            },
                            "Resume music transfer",
                        );
                        self.confirm(req, "Resume verified transfer");
                    }
                    Choice::Check => {
                        self.updater.command(u::Command::Check);
                        self.status = "Checking…".into();
                    }
                    Choice::Apply(manifest) => {
                        let req = Request::Update {
                            config: self.update_config.clone(),
                            manifest,
                        };
                        self.confirm(req, "Install Rust update");
                    }
                }
            }
            _ => {}
        }
        None
    }
    pub fn tick(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self
            .check_after
            .is_some_and(|at| std::time::Instant::now() >= at)
            && self.updater.command(u::Command::CheckIfDue {
                state: self.config.data.join("update-check.json"),
            })
        {
            self.check_after =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(3600));
        }
        while let Some(event) = self.worker.poll() {
            match event {
                m::Event::Folders { id, entries } => {
                    if self.browsing.as_ref().is_none_or(|b| b.0 != id) {
                        continue;
                    }
                    let (_, path, kind) = self.browsing.take().unwrap();
                    let mut rows = vec![Row::new(
                        "Use this folder",
                        Choice::Select(path.clone(), kind),
                    )];
                    if matches!(kind, Destination::MoveDatabase) {
                        rows.push(Row::new(
                            "New betterpod-rust folder here",
                            Choice::Select(path.join("betterpod-rust"), kind),
                        ));
                    }
                    if matches!(kind, Destination::CopyMusic | Destination::MoveMusic) {
                        rows.push(Row::new(
                            "New music folder here",
                            Choice::Select(path.join("betterpod-music"), kind),
                        ));
                    }
                    if let Some(parent) = path.parent() {
                        rows.push(Row::new("..", Choice::Browse(parent.into(), kind)));
                    }
                    rows.extend(entries.into_iter().map(|e| Row {
                        label: e.name,
                        detail: if e.is_current_music {
                            "Current music".into()
                        } else if e.is_current_data {
                            "Current database".into()
                        } else {
                            String::new()
                        },
                        choice: Choice::Browse(e.path, kind),
                    }));
                    self.push(path.display().to_string(), rows);
                    self.status.clear();
                }
                m::Event::PendingTransfers { transfers, .. } => {
                    self.status = if transfers.is_empty() {
                        "No interrupted transfers".into()
                    } else {
                        String::new()
                    };
                    self.push(
                        "Interrupted transfers",
                        transfers
                            .into_iter()
                            .map(|t| Row {
                                label: t.destination.display().to_string(),
                                detail: t.phase.clone(),
                                choice: Choice::Resume(t),
                            })
                            .collect(),
                    );
                }
                m::Event::Error { message, .. } => {
                    self.status = message.clone();
                    effects.push(Effect::Notice(message));
                }
                _ => {}
            }
        }
        while let Some(event) = self.updater.poll() {
            match event {
                u::Event::Status(u::Status::LocalBuild) => {
                    self.status = "Local experiment: no published Rust update channel".into();
                    self.available = None;
                    effects.push(Effect::UpdateAvailable(false));
                }
                u::Event::Status(u::Status::UpToDate) => {
                    self.status = "Up to date".into();
                    self.available = None;
                    effects.push(Effect::UpdateAvailable(false));
                }
                u::Event::Status(u::Status::Available(manifest)) => {
                    self.status = manifest.notes.clone();
                    self.available = Some(manifest.clone());
                    effects.push(Effect::UpdateAvailable(true));
                    if !self.pages.is_empty() {
                        self.push(
                            "Update available",
                            vec![Row::new(
                                format!("Install {}", manifest.version),
                                Choice::Apply(manifest),
                            )],
                        );
                    }
                }
                u::Event::Error(message) => self.status = message,
                _ => {}
            }
        }
        effects
    }
    pub fn draw(&mut self, r: &mut Renderer, dark: bool) {
        let fg = if dark {
            [0.94, 0.94, 0.96, 1.]
        } else {
            [0.1, 0.11, 0.13, 1.]
        };
        let muted = if dark {
            [0.65, 0.66, 0.7, 1.]
        } else {
            [0.4, 0.42, 0.47, 1.]
        };
        r.text(self.title(), 20., 60., 24, fg, 600.);
        if let Some(page) = self.pages.last() {
            let first = page.selected.saturating_sub(5);
            for (i, row) in page.rows.iter().enumerate().skip(first).take(6) {
                let y = 106. + (i - first) as f32 * 46.;
                let selected = i == page.selected;
                if selected {
                    r.rect(12., y - 4., 616., 44., [0.12, 0.48, 0.88, 1.]);
                }
                let color = if selected { [1.; 4] } else { fg };
                r.text(&row.label, 24., y, 20, color, 585.);
                if !row.detail.is_empty() {
                    r.text(
                        &row.detail,
                        24.,
                        y + 23.,
                        13,
                        if selected { [1.; 4] } else { muted },
                        585.,
                    );
                }
            }
        }
        r.text(&self.status, 20., 400., 16, muted, 600.);
        r.text("A Select   B Back", 20., 454., 16, muted, 600.);
    }
}

/// The App and every writer must already have been dropped before entering.
pub fn run(
    request: Request,
    r: &mut Renderer,
    input: &mut crate::input::Input,
    window: &sdl2::video::Window,
) -> Result<bool, String> {
    let mut maintenance = None;
    let mut updater = None;
    let mut id = 0;
    let label;
    match request {
        Request::Maintenance {
            config,
            command,
            label: message,
        } => {
            let mut worker = m::Maintenance::new(config)?;
            id = worker
                .command(command)
                .ok_or("Maintenance worker is busy")?;
            maintenance = Some(worker);
            label = message;
        }
        Request::Update { config, manifest } => {
            let worker = u::Updater::new(config)?;
            if !worker.command(u::Command::Apply {
                manifest,
                quiescent: true,
            }) {
                return Err("Update worker is busy".into());
            }
            updater = Some(worker);
            label = "Installing Rust update".into();
        }
    }
    let mut phase = String::from("Preparing…");
    let mut done = 0;
    let mut total = 0;
    let mut complete = None;
    let mut cancelled = false;
    let mut failure: Option<String> = None;
    let mut clock = crate::timing::FrameClock::new(60.);
    loop {
        for action in input.poll() {
            if failure.is_some() && matches!(action, Action::Back | Action::Select | Action::Quit) {
                return Err(failure.take().unwrap());
            }
            if matches!(action, Action::Back | Action::Quit) {
                if let Some(worker) = &maintenance {
                    worker.cancel(id);
                    cancelled = true;
                } else {
                    phase = "Finishing safe update transaction…".into();
                }
            }
        }
        if crate::platform::quit_requested() {
            if let Some(worker) = &maintenance {
                worker.cancel(id);
                cancelled = true;
            }
        }
        if let Some(worker) = &mut maintenance {
            while let Some(event) = worker.poll() {
                match event {
                    m::Event::Progress {
                        phase: p,
                        done: d,
                        total: t,
                        ..
                    } => {
                        phase = p.into();
                        done = d;
                        total = t;
                    }
                    m::Event::MusicSelected { id, .. } | m::Event::DatabaseSelected { id, .. } => {
                        worker.acknowledge_switch(id)
                    }
                    m::Event::Completed { .. } => complete = Some(Ok(false)),
                    m::Event::Error { message, .. } => complete = Some(Err(message)),
                    _ => {}
                }
            }
        }
        if let Some(worker) = &mut updater {
            while let Some(event) = worker.poll() {
                match event {
                    u::Event::Progress {
                        phase: p,
                        done: d,
                        total: t,
                    } => {
                        phase = p.into();
                        done = d;
                        total = t;
                    }
                    u::Event::Applied { .. } => complete = Some(Ok(true)),
                    u::Event::Error(e) => complete = Some(Err(e)),
                    _ => {}
                }
            }
        }
        r.begin([0.96, 0.96, 0.97, 1.]);
        r.text(&label, 28., 140., 26, [0.1, 0.11, 0.13, 1.], 584.);
        r.text(&phase, 28., 196., 19, [0.4, 0.42, 0.47, 1.], 584.);
        let progress = if total > 0 {
            done as f32 / total as f32
        } else {
            0.
        };
        r.rect(28., 250., 584., 8., [0.8, 0.82, 0.85, 1.]);
        r.rect(
            28.,
            250.,
            584. * progress.clamp(0., 1.),
            8.,
            [0.12, 0.48, 0.88, 1.],
        );
        r.text(
            if failure.is_some() {
                "A / B Return"
            } else if cancelled {
                "Cancelling safely…"
            } else {
                "B Cancel"
            },
            28.,
            430.,
            16,
            [0.4, 0.42, 0.47, 1.],
            584.,
        );
        r.finish();
        window.gl_swap_window();
        if let Some(result) = complete.take() {
            match result {
                Ok(restart) => return Ok(restart),
                Err(error) => {
                    phase = error.clone();
                    failure = Some(error);
                }
            }
        }
        if failure.is_some() && crate::platform::quit_requested() {
            return Err(failure.take().unwrap());
        }
        clock.wait(std::time::Instant::now());
    }
}
