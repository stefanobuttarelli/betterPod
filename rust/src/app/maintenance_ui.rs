//! Maintenance menus. Mutations are handed to main only after App is closed.
fn project_rows(rows: &[Row]) -> std::sync::Arc<Vec<crate::app::Row>> {
    std::sync::Arc::new(
        rows.iter()
            .map(|row| {
                let mut v = crate::app::Row::new(&row.label, crate::app::Intent::Noop);
                v.detail = row.detail.clone();
                v.visual.value = row.value.clone();
                v.visual.checked = row.checked;
                v.arrow = !matches!(
                    row.choice,
                    Choice::Noop
                        | Choice::Select(_, _)
                        | Choice::Database(_)
                        | Choice::CreateDatabase(_)
                        | Choice::Back
                );
                v
            })
            .collect(),
    )
}
use crate::app::{Intent, Page as VisualPage, Row as VisualRow, Screen, ViewKind};
use crate::{input::Action, maintenance as m, render::Renderer, updater as u};
use betterpod_rust::visual_assets::VisualAssets;
use std::path::PathBuf;
use std::sync::Arc;
mod move_visual;
mod update_visual;
fn size(bytes: u64) -> String {
    crate::files::format_size(bytes)
}
fn is_beta(version: &str) -> bool {
    version.split('+').next().unwrap_or(version).contains('-')
}
mod discovery;

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
    /// Only main may handle this after joining the old App's writers.
    FinalizeTransfer {
        config: m::Config,
        manifest: PathBuf,
        visible: bool,
    },
    Update {
        config: u::Config,
        manifest: u::Manifest,
    },
}
pub enum Effect {
    Exit,
    PauseMusic,
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
    Noop,
    StopMove,
    HideMove,
    Card(discovery::Card),
    Database(PathBuf),
    CreateDatabase(PathBuf),
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
    value: String,
    checked: bool,
}
impl Row {
    fn new(label: impl Into<String>, choice: Choice) -> Self {
        Self {
            label: label.into(),
            detail: String::new(),
            choice,
            value: String::new(),
            checked: false,
        }
    }
}
struct Page {
    title: String,
    rows: Vec<Row>,
    selected: usize,
    visual: Arc<Vec<VisualRow>>,
}
struct ActiveMove {
    id: u64,
    request: Request,
    view: move_visual::MoveView,
    visible: bool,
    stopping: bool,
    committing: bool,
    committed: bool,
    manifest: Option<PathBuf>,
    render_library: Arc<crate::library::Library>,
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
    sheet: Option<Page>,
    status_visual: Option<(String, Arc<Vec<VisualRow>>)>,
    discovery: discovery::Discovery,
    mode: Option<Mode>,
    cards: Vec<discovery::Card>,
    pending_count: usize,
    auto_pending: bool,
    probe: Option<PathBuf>,
    creating: Option<PathBuf>,
    update_view: update_visual::UpdateView,
    active_move: Option<ActiveMove>,
    library: Arc<crate::library::Library>,
}
impl Controller {
    #[cfg(test)]
    pub(crate) fn visual_fixture(
        key: &str,
        music: PathBuf,
        data: PathBuf,
        library: Arc<crate::library::Library>,
    ) -> Result<Self, String> {
        let config = m::Config {
            bootstrap: data.clone(),
            music: music.clone(),
            data,
            protected: Vec::new(),
        };
        let updates = u::Config {
            installation: music.join("fixture-ports"),
            current_version: "1.1".into(),
            feed: None,
            beta_feed: None,
            allow_beta: false,
        };
        let mut controller = Self::new(config, updates)?;
        controller.set_library(Arc::clone(&library));
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while controller.updater.poll().is_none() {
            if std::time::Instant::now() > limit {
                return Err("Local fixture updater did not initialize".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        match key {
            "maintenance-update-available" => {
                controller.mode = Some(Mode::Update);
                controller.push("Software Update", Vec::new());
                controller.available=Some(u::Manifest{application:"betterpod-rust".into(),version:"1.2".into(),arch:"aarch64".into(),url:"https://example.invalid/local-fixture.zip".into(),size:12_345_678,sha256:"0".repeat(64),notes:"Playback and interface refinements\nMore reliable Bluetooth playback\nImproved album browsing".into()});
                controller.update_view.available();
            }
            "maintenance-move-copy" | "maintenance-move-verify" => {
                controller.mode = Some(Mode::Music);
                controller.push("Moving Music", Vec::new());
                let request = controller.request(
                    m::Command::TransferMusic {
                        destination: "/fixture-sd2/music".into(),
                        remove_source: true,
                        confirmed: true,
                    },
                    "Moving Music",
                );
                controller.active_move = Some(ActiveMove {
                    id: u64::MAX,
                    request,
                    view: move_visual::MoveView::visual_fixture(
                        music,
                        &library,
                        if key.ends_with("verify") {
                            "verify"
                        } else {
                            "copy"
                        },
                    ),
                    visible: true,
                    stopping: false,
                    committing: false,
                    committed: false,
                    manifest: None,
                    render_library: Arc::clone(&controller.library),
                });
            }
            _ => return Err(format!("Unknown maintenance fixture {key}")),
        }
        Ok(controller)
    }
    pub fn new(config: m::Config, update_config: u::Config) -> Result<Self, String> {
        let updater = u::Updater::new(update_config.clone())?;
        let check_after = if update_config.feed.is_some() || update_config.beta_feed.is_some() {
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
            sheet: None,
            status_visual: None,
            discovery: discovery::Discovery::new(),
            mode: None,
            cards: Vec::new(),
            pending_count: 0,
            auto_pending: false,
            probe: None,
            creating: None,
            update_view: update_visual::UpdateView::new(),
            active_move: None,
            library: Arc::new(crate::library::Library::default()),
        })
    }
    fn push(&mut self, title: impl Into<String>, rows: Vec<Row>) {
        self.status_visual = None;
        let visual = project_rows(&rows);
        self.pages.push(Page {
            title: title.into(),
            rows,
            selected: 0,
            visual,
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
        self.probe = None;
        self.creating = None;
        self.mode = Some(mode.clone());
        self.sheet = None;
        self.status_visual = None;
        self.pages.clear();
        self.status.clear();
        if matches!(mode, Mode::Music) && self.active_move.is_some() {
            self.active_move.as_mut().unwrap().visible = true;
            self.push("Moving Music", Vec::new());
            return;
        }
        let root = self
            .config
            .music
            .parent()
            .unwrap_or(&self.config.music)
            .to_path_buf();
        match mode {
            Mode::Music => {
                let mut now = Row::new("Now", Choice::Noop);
                now.detail = self.config.music.display().to_string();
                self.push(
                    "Move Music",
                    vec![now, Row::new("Looking for the cards…", Choice::Noop)],
                );
                self.discovery
                    .submit(discovery::Query::Cards(self.config.clone()));
                self.auto_pending = true;
                self.worker.command(m::Command::PendingTransfers);
            }
            Mode::Database => {
                self.database_rows(Vec::new());
                self.discovery
                    .submit(discovery::Query::Cards(self.config.clone()));
            }
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
                self.push("Software Update", Vec::new());
                self.update_view.open(self.available.as_ref());
                if self.available.is_none() {
                    self.check_updates();
                }
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
    pub fn database_label(&self) -> String {
        if self.config.data == self.config.bootstrap {
            "Port folder".into()
        } else if self.config.data == self.config.music.join(".betterpod-rust") {
            "Music folder".into()
        } else {
            self.config
                .data
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        }
    }
    pub fn title(&self) -> &str {
        if self.move_visible() {
            return "Moving Music";
        }
        self.pages
            .last()
            .map_or("Maintenance", |p| p.title.as_str())
    }
    pub fn update_available(&self) -> bool {
        self.available.is_some()
    }
    fn replace_rows(&mut self, title: &str, rows: Vec<Row>) {
        self.status_visual = None;
        if let Some(page) = self.pages.first_mut() {
            page.title = title.into();
            page.selected = page.selected.min(rows.len().saturating_sub(1));
            page.visual = project_rows(&rows);
            page.rows = rows;
        } else {
            self.push(title, rows)
        }
    }
    fn music_rows(&mut self) {
        let mut rows = vec![Row {
            label: "Now".into(),
            detail: self.config.music.display().to_string(),
            choice: Choice::Noop,
            value: String::new(),
            checked: false,
        }];
        for card in &self.cards {
            let mut row = Row::new(
                &card.name,
                if card.here {
                    Choice::Noop
                } else {
                    Choice::Card(card.clone())
                },
            );
            row.detail = format!(
                "{} free of {} · {}",
                size(card.free),
                size(card.total),
                card.music.display()
            );
            if card.here {
                row.value = "Here".into();
            }
            rows.push(row);
        }
        if self.cards.len() <= 1 {
            let mut row = Row::new("No other card found", Choice::Noop);
            row.detail = "Put a second card in (formatted with Knulli's Format A Disk)".into();
            rows.push(row);
        }
        if self.pending_count > 0 {
            let mut row = Row::new("Resume Interrupted Transfer", Choice::Pending);
            row.detail = format!(
                "{} {} waiting",
                self.pending_count,
                if self.pending_count == 1 {
                    "transfer"
                } else {
                    "transfers"
                }
            );
            rows.push(row);
        }
        self.replace_rows("Move Music", rows);
        self.status.clear();
    }
    fn database_rows(&mut self, cards: Vec<discovery::Card>) {
        let mut candidates = vec![
            ("Port folder".into(), self.config.bootstrap.clone()),
            (
                "Music folder".into(),
                self.config.music.join(".betterpod-rust"),
            ),
        ];
        for card in cards {
            if card.root.starts_with("/media") || !card.here {
                candidates.push((
                    card.root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    card.root.join("betterpod-rust"),
                ));
            }
        }
        let known = candidates.iter().any(|(_, p)| *p == self.config.data);
        if !known {
            candidates.push((
                self.config
                    .data
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                self.config.data.clone(),
            ));
        }
        let mut rows = Vec::new();
        for (label, path) in candidates {
            let mut row = Row::new(label, Choice::Database(path.clone()));
            row.detail = path.display().to_string();
            row.checked = path == self.config.data;
            rows.push(row);
        }
        rows.push(Row::new(
            "Choose a folder…",
            Choice::Browse(
                if cfg!(target_os = "linux") {
                    PathBuf::from("/userdata")
                } else {
                    self.config
                        .data
                        .parent()
                        .unwrap_or(&self.config.data)
                        .to_path_buf()
                },
                Destination::MoveDatabase,
            ),
        ));
        if !self.cards.is_empty() {
            rows.push(Row::new(
                "Browse cards and drives…",
                Choice::Browse(PathBuf::from("/media"), Destination::MoveDatabase),
            ));
        }
        self.replace_rows("Database", rows);
        self.status.clear();
    }
    fn show_choices(&mut self, title: String, rows: Vec<Row>) {
        self.sheet = Some(Page {
            title,
            visual: project_rows(&rows),
            rows,
            selected: 0,
        });
    }
    fn choose_card(&mut self, card: discovery::Card) {
        let mut rows = vec![
            Row::new(
                "Move Music",
                Choice::Run(self.request(
                    m::Command::TransferMusic {
                        destination: card.music.clone(),
                        remove_source: true,
                        confirmed: true,
                    },
                    "Moving Music",
                )),
            ),
            Row::new(
                "Copy Music (keep the originals)",
                Choice::Run(self.request(
                    m::Command::TransferMusic {
                        destination: card.music.clone(),
                        remove_source: false,
                        confirmed: true,
                    },
                    "Copying Music",
                )),
            ),
        ];
        if card.has_music {
            rows.push(Row::new(
                "Use the Music Already There",
                Choice::Run(self.request(
                    m::Command::SelectMusic(card.music),
                    "Use the Music Already There",
                )),
            ));
        }
        rows.push(Row::new("Cancel", Choice::Back));
        self.show_choices(
            format!("Move music to {}? Keep the console charging.", card.name),
            rows,
        );
    }
    fn choose_database(&mut self, info: discovery::Destination) {
        if info.path == self.config.data {
            return;
        }
        if !info.writable {
            self.status = "Can't write there".into();
            return;
        }
        if info.foreign {
            self.status = "Choose an empty folder or a database belonging to this app".into();
            return;
        }
        let mut rows = if info.owned {
            vec![
                Row::new(
                    "Use database there",
                    Choice::Run(self.request(
                        m::Command::UseDatabase {
                            directory: info.path.clone(),
                            quiescent: true,
                        },
                        "Switching Database",
                    )),
                ),
                Row::new(
                    "Replace with current",
                    Choice::Run(self.request(
                        m::Command::ReplaceDatabase {
                            destination: info.path.clone(),
                            quiescent: true,
                            confirmed: true,
                        },
                        "Replacing Database",
                    )),
                ),
            ]
        } else {
            vec![Row::new(
                "Move database here",
                Choice::Run(self.request(
                    m::Command::MoveDatabase {
                        destination: info.path.clone(),
                        quiescent: true,
                    },
                    "Moving Database",
                )),
            )]
        };
        rows.push(Row::new("Cancel", Choice::Back));
        self.show_choices(info.path.display().to_string(), rows);
    }
    pub fn set_beta(&mut self, allow: bool) {
        if self.update_config.allow_beta == allow {
            return;
        }
        self.update_config.allow_beta = allow;
        self.updater.command(u::Command::SetBeta(allow));
        if !allow && self.available.as_ref().is_some_and(|m| is_beta(&m.version)) {
            self.available = None;
            self.update_view.clear_available();
        }
        self.check_updates();
    }
    fn check_updates(&mut self) {
        if self.updater.command(u::Command::Check) || self.updater.alive() {
            // queued, or a check already under way (the start-up check, or
            // one trying GitHub again after a 5xx): its answer comes
            self.update_view.checking();
        } else {
            self.update_view
                .fail("Software Update stopped; reopen betterPod Rust".into());
        }
    }
    /// Called only after the old App is joined and PrepareTransferredClients
    /// has completed, before installing this Controller into the new App.
    pub fn adopt_verified_transfer(
        &mut self,
        manifest: PathBuf,
        library: Arc<crate::library::Library>,
        visible: bool,
    ) -> Result<(), String> {
        if self.active_move.is_some() {
            return Err("Music is already being moved".into());
        }
        let command = m::Command::FinishTransferredMusic {
            manifest: manifest.clone(),
            clients_ready: true,
        };
        let request = self.request(command.clone(), "Moving Music");
        let id = self.worker.command(command).ok_or("Music worker is busy")?;
        self.active_move = Some(ActiveMove {
            id,
            request,
            view: move_visual::MoveView::new(),
            visible,
            stopping: false,
            committing: false,
            committed: true,
            manifest: Some(manifest),
            render_library: library,
        });
        self.mode = Some(Mode::Music);
        self.pages.clear();
        self.push("Moving Music", Vec::new());
        Ok(())
    }
    pub fn has_visible_transfer(&self) -> bool {
        self.move_visible()
    }

    pub fn has_active_library_transfer(&self) -> bool {
        self.active_move
            .as_ref()
            .is_some_and(|job| !job.view.finished())
    }
    /// Stop live mutations before the Store's final flush. A verified switch
    /// queued after the last frame must still be finalized by main, even Quit.
    pub fn quiesce_for_shutdown(&mut self) -> Option<Request> {
        let events = self.worker.shutdown_collect();
        let job = self.active_move.as_mut()?;
        let mut switched = false;
        for event in events {
            if event.id() != job.id {
                continue;
            }
            match event {
                m::Event::TransferPlan { manifest, .. } => job.manifest = Some(manifest),
                m::Event::MusicSelected { .. } => switched = true,
                _ => {}
            }
        }
        if switched && !job.committed && !job.committing {
            let manifest = job.manifest.clone()?;
            job.committing = true;
            Some(Request::FinalizeTransfer {
                config: self.config.clone(),
                manifest,
                visible: job.visible,
            })
        } else {
            None
        }
    }
    pub fn set_library(&mut self, library: Arc<crate::library::Library>) {
        self.library = library;
    }
    fn move_visible(&self) -> bool {
        matches!(self.mode, Some(Mode::Music))
            && self.active_move.as_ref().is_some_and(|job| job.visible)
    }
    pub fn is_custom(&self) -> bool {
        matches!(self.mode, Some(Mode::Update)) || self.move_visible()
    }
    pub fn draw_custom(
        &mut self,
        r: &mut Renderer,
        dark: bool,
        assets: &VisualAssets,
        art: &mut crate::art::Artwork,
    ) {
        if self.move_visible() {
            if let Some(job) = self.active_move.as_mut() {
                job.view.draw(r, art, assets, dark);
            }
        } else {
            self.update_view.draw(
                r,
                dark,
                assets,
                &self.update_config.current_version,
                self.update_config.allow_beta,
                self.available.as_ref(),
            );
        }
    }
    fn launch(&mut self, request: Request) -> Option<Effect> {
        if self
            .active_move
            .as_ref()
            .is_some_and(|job| !job.view.finished())
        {
            return Some(Effect::Notice(
                "Wait for the music move to finish before changing the library or updating".into(),
            ));
        }
        if let Request::Maintenance { command, .. } = &request {
            if matches!(
                command,
                m::Command::TransferMusic { .. } | m::Command::ResumeTransfer { .. }
            ) {
                if self.active_move.is_some() {
                    return Some(Effect::Notice("Music is already being moved".into()));
                }
                if let Some(id) = self.worker.command(command.clone()) {
                    self.active_move = Some(ActiveMove {
                        id,
                        request,
                        view: move_visual::MoveView::new(),
                        visible: true,
                        stopping: false,
                        committing: false,
                        committed: false,
                        manifest: None,
                        render_library: Arc::clone(&self.library),
                    });
                    self.mode = Some(Mode::Music);
                    self.pages.clear();
                    self.push("Moving Music", Vec::new());
                    return Some(Effect::PauseMusic);
                }
                return Some(Effect::Notice("Music worker is busy".into()));
            }
        }
        Some(Effect::Run(request))
    }
    fn hide_move(&mut self) -> Option<Effect> {
        if let Some(job) = self.active_move.as_mut() {
            job.visible = false;
        }
        Some(Effect::Exit)
    }
    fn consume_move_event(&mut self, event: m::Event) -> Option<Effect> {
        let job = self.active_move.as_mut()?;
        match event {
            m::Event::TransferPlan {
                manifest,
                source,
                destination,
                remove_source,
                files,
                ..
            } => {
                job.manifest = Some(manifest);
                if job.committed {
                    job.view.prepare_relocated(
                        source,
                        destination,
                        remove_source,
                        files,
                        Arc::clone(&job.render_library),
                    );
                } else {
                    job.view.prepare(
                        source,
                        destination,
                        remove_source,
                        files,
                        Arc::clone(&job.render_library),
                    );
                }
            }
            m::Event::Progress {
                phase,
                done,
                total,
                path,
                ..
            } => job.view.progress(phase, done, total, path),
            // The initial worker never receives destructive acknowledgement.
            // main joins its clients and prepares metadata before a new App
            // starts the dedicated cleanup worker and can acknowledge it.
            m::Event::MusicSelected { id, .. } => {
                if job.committed {
                    self.worker.acknowledge_switch(id);
                } else if !job.committing {
                    let Some(manifest) = job.manifest.clone() else {
                        job.view
                            .fail("Missing verified transfer manifest".into(), false);
                        return Some(Effect::Notice(
                            "The originals were kept: missing transfer manifest".into(),
                        ));
                    };
                    job.committing = true;
                    return Some(Effect::Run(Request::FinalizeTransfer {
                        config: self.config.clone(),
                        manifest,
                        visible: job.visible,
                    }));
                }
            }
            m::Event::Completed { .. } => job.view.done(),
            m::Event::Error { message, .. } => job.view.fail(message, job.stopping),
            _ => {}
        }
        None
    }
    fn confirm(&mut self, request: Request, label: &str) {
        let rows = vec![
            Row {
                label: label.into(),
                detail: String::new(),
                choice: Choice::Run(request),
                value: String::new(),
                checked: false,
            },
            Row::new("Cancel", Choice::Back),
        ];
        self.sheet = Some(Page {
            title: label.into(),
            visual: project_rows(&rows),
            rows,
            selected: 0,
        });
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
        if matches!(self.mode, Some(Mode::Update)) && self.sheet.is_none() {
            match self.update_view.handle(action) {
                Some(update_visual::Button::Check) => self.check_updates(),
                Some(update_visual::Button::Install) => {
                    if let Some(manifest) = self.available.clone() {
                        return self.launch(Request::Update {
                            config: self.update_config.clone(),
                            manifest,
                        });
                    }
                }
                Some(update_visual::Button::Exit) => return Some(Effect::Exit),
                None => {}
            }
            return None;
        }

        if let Some(sheet) = self.sheet.as_mut() {
            match action {
                Action::Up | Action::Left => sheet.selected = sheet.selected.saturating_sub(1),
                Action::Down | Action::Right => {
                    sheet.selected = (sheet.selected + 1).min(sheet.rows.len() - 1)
                }
                Action::Back => self.sheet = None,
                Action::Select => {
                    let sheet = self.sheet.take().unwrap();
                    match sheet.rows.get(sheet.selected).map(|r| r.choice.clone()) {
                        Some(Choice::Run(request)) => return self.launch(request),
                        Some(Choice::StopMove) => {
                            if let Some(job) = self.active_move.as_mut() {
                                job.stopping = true;
                                self.worker.cancel(job.id);
                            }
                        }
                        Some(Choice::HideMove) => return self.hide_move(),
                        _ => {}
                    }
                }
                _ => {}
            }
            return if self.sheet.is_none() && self.pages.is_empty() {
                Some(Effect::Exit)
            } else {
                None
            };
        }
        if self.move_visible() {
            if self
                .active_move
                .as_ref()
                .is_some_and(|job| job.view.finished())
            {
                if matches!(action, Action::Select | Action::Back) {
                    self.active_move = None;
                    return Some(Effect::Exit);
                }
                return None;
            }
            if action == Action::Back {
                let deleting = self
                    .active_move
                    .as_ref()
                    .is_some_and(|job| job.view.deleting());
                if deleting {
                    self.show_choices(
                        "Deleting the originals: the music is already on the destination card."
                            .into(),
                        vec![
                            Row::new("Hide (it goes on)", Choice::HideMove),
                            Row::new("Cancel", Choice::Back),
                        ],
                    );
                } else {
                    self.show_choices(
                        "Stop moving the music? What's copied stays; nothing is deleted.".into(),
                        vec![
                            Row::new("Stop", Choice::StopMove),
                            Row::new("Hide (it goes on)", Choice::HideMove),
                            Row::new("Cancel", Choice::Back),
                        ],
                    );
                }
            }
            return None;
        }
        let Some(page) = self.pages.last_mut() else {
            return Some(Effect::Exit);
        };
        match action {
            Action::Up => page.selected = page.selected.saturating_sub(1),
            Action::Down => {
                page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1))
            }
            Action::Back => {
                self.probe = None;
                self.creating = None;
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
                    Choice::Noop | Choice::StopMove | Choice::HideMove => {}
                    Choice::Card(card) => self.choose_card(card),
                    Choice::Database(path) => {
                        if path != self.config.data {
                            self.probe = Some(path.clone());
                            self.discovery.submit(discovery::Query::Inspect(path));
                        }
                    }
                    Choice::CreateDatabase(path) => {
                        let mut protected = self.config.protected.clone();
                        protected.push(self.config.data.clone());
                        self.creating = Some(path.clone());
                        self.discovery
                            .submit(discovery::Query::Create { path, protected });
                    }
                    Choice::Back => {
                        self.pages.pop();
                        if self.pages.is_empty() {
                            return Some(Effect::Exit);
                        }
                    }
                    Choice::Browse(path, kind) => self.browse(path, kind),
                    Choice::Select(path, kind) => {
                        if matches!(kind, Destination::MoveDatabase | Destination::UseDatabase) {
                            self.probe = Some(path.clone());
                            self.discovery.submit(discovery::Query::Inspect(path));
                            return None;
                        }
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
                    Choice::Run(req) => return self.launch(req),
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
                        self.check_updates();
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
    pub fn has_options(&self) -> bool {
        self.sheet.is_some()
    }
    pub fn visual_options(&self) -> Option<VisualPage> {
        let page = self.sheet.as_ref()?;
        Some(VisualPage {
            screen: Screen::Maintenance,
            kind: ViewKind::Full,
            mini: false,
            title: page.title.clone(),
            rows: Arc::clone(&page.visual),
            selected: page.selected,
            scroll: 0.,
            target_scroll: 0.,
            enter: 0.,
        })
    }
    pub fn visual_page(&mut self) -> Option<VisualPage> {
        let page = self.pages.last()?;
        let rows = if self.status.is_empty() {
            Arc::clone(&page.visual)
        } else {
            if self
                .status_visual
                .as_ref()
                .is_none_or(|(status, _)| status != &self.status)
            {
                let mut rows = page.visual.as_ref().clone();
                let mut row = VisualRow::new("Status", Intent::Noop);
                row.detail = self.status.clone();
                row.arrow = false;
                rows.insert(0, row);
                self.status_visual = Some((self.status.clone(), Arc::new(rows)));
            }
            Arc::clone(&self.status_visual.as_ref().unwrap().1)
        };
        let mut visual = VisualPage {
            screen: Screen::Maintenance,
            kind: ViewKind::Full,
            mini: false,
            title: self.title().into(),
            rows,
            selected: page.selected + usize::from(!self.status.is_empty()),
            scroll: 0.,
            target_scroll: 0.,
            enter: 0.,
        };
        visual.reveal_selection();
        Some(visual)
    }
    pub fn tick(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        while let Some(event) = self.discovery.poll() {
            match event {
                discovery::Event::Cards(cards) => {
                    self.cards = cards.clone();
                    if matches!(self.mode, Some(Mode::Music)) {
                        self.music_rows();
                    } else if matches!(self.mode, Some(Mode::Database)) {
                        self.database_rows(cards);
                    }
                }
                discovery::Event::Destination(info) => {
                    if self.probe.as_ref() == Some(&info.path) {
                        self.probe = None;
                        self.choose_database(info);
                    }
                }
                discovery::Event::Created(path) => {
                    if self.creating.as_ref() == Some(&path) {
                        self.creating = None;
                        self.browse(path, Destination::MoveDatabase);
                    }
                }
                discovery::Event::Error(message) => {
                    self.probe = None;
                    self.creating = None;
                    self.status = message.clone();
                    effects.push(Effect::Notice(message));
                }
            }
        }
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
            if self
                .active_move
                .as_ref()
                .is_some_and(|job| job.id == event.id())
            {
                if let Some(effect) = self.consume_move_event(event) {
                    effects.push(effect);
                }
                continue;
            }
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
                            "New folder “betterpod-rust”",
                            Choice::CreateDatabase(path.join("betterpod-rust")),
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
                        value: String::new(),
                        checked: false,
                    }));
                    if let Some(first) = rows.first_mut() {
                        first.label = "Use this folder".into();
                        first.detail = path.display().to_string();
                    }
                    self.push(
                        path.file_name()
                            .map_or_else(|| "/".into(), |s| s.to_string_lossy().into_owned()),
                        rows,
                    );
                    self.status.clear();
                }
                m::Event::PendingTransfers { transfers, .. } => {
                    self.pending_count = transfers.len();
                    if self.auto_pending {
                        self.auto_pending = false;
                        if matches!(self.mode, Some(Mode::Music)) {
                            self.music_rows();
                        }
                        continue;
                    }
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
                                value: String::new(),
                                checked: false,
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
                    self.update_view.local_build();
                    self.available = None;
                    effects.push(Effect::UpdateAvailable(false));
                }
                u::Event::Status(u::Status::UpToDate) => {
                    self.update_view.up_to_date();
                    self.available = None;
                    effects.push(Effect::UpdateAvailable(false));
                }
                u::Event::Status(u::Status::Available(manifest)) => {
                    // A check already in flight may finish after beta versions were disabled.
                    if !self.update_config.allow_beta && is_beta(&manifest.version) {
                        continue;
                    }
                    self.update_view.available();
                    self.available = Some(manifest);
                    effects.push(Effect::UpdateAvailable(true));
                }
                u::Event::Error(message) => self.update_view.fail(message),
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

#[cfg(test)]
mod controller_tests {
    use super::*;
    use std::time::Instant;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn fixture() -> (PathBuf, Controller) {
        let root = std::env::temp_dir().join(format!(
            "betterpod-maint-controller-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let music = root.join("music");
        let data = root.join("data");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let config = m::Config {
            bootstrap: data.clone(),
            music,
            data,
            protected: vec![],
        };
        let updates = u::Config {
            installation: root.join("ports"),
            current_version: "1.1.0".into(),
            feed: None,
            beta_feed: None,
            allow_beta: false,
        };
        (root, Controller::new(config, updates).unwrap())
    }
    #[test]
    fn existing_owned_database_offers_use_or_explicit_replace_and_stable_sheet_identity() {
        let (root, mut c) = fixture();
        c.open(Mode::Database);
        let destination = root.join("owned");
        c.choose_database(discovery::Destination {
            path: destination.clone(),
            writable: true,
            owned: true,
            foreign: false,
            has_files: true,
        });
        let one = c.visual_options().unwrap();
        let two = c.visual_options().unwrap();
        assert!(Arc::ptr_eq(&one.rows, &two.rows));
        assert_eq!(
            one.rows
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Use database there", "Replace with current", "Cancel"]
        );
        c.handle(Action::Down);
        match c.handle(Action::Select) {
            Some(Effect::Run(Request::Maintenance {
                command:
                    m::Command::ReplaceDatabase {
                        destination: path,
                        quiescent: true,
                        confirmed: true,
                    },
                ..
            })) => assert_eq!(path, destination),
            _ => panic!("replace must retain the lifecycle request"),
        }
        drop(c);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn foreign_database_is_never_offered_for_replacement() {
        let (root, mut c) = fixture();
        c.open(Mode::Database);
        c.choose_database(discovery::Destination {
            path: root.join("foreign"),
            writable: true,
            owned: false,
            foreign: true,
            has_files: true,
        });
        assert!(!c.has_options());
        assert!(c.status.contains("belonging to this app"));
        drop(c);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn music_card_choice_preserves_all_three_original_actions() {
        let (root, mut c) = fixture();
        c.open(Mode::Music);
        c.choose_card(discovery::Card {
            name: "SD 2".into(),
            root: root.clone(),
            music: root.join("existing-music"),
            free: 100,
            total: 200,
            here: false,
            has_music: true,
        });
        let page = c.visual_options().unwrap();
        assert_eq!(
            page.rows
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Move Music",
                "Copy Music (keep the originals)",
                "Use the Music Already There",
                "Cancel"
            ]
        );
        c.handle(Action::Back);
        assert!(!c.has_options());
        assert!(matches!(c.mode, Some(Mode::Music)));
        drop(c);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn beta_detection_ignores_hyphens_in_build_metadata() {
        assert!(!is_beta("1.1.0+local-build"));
        assert!(is_beta("1.2.0-beta.3+local-build"));
    }
    #[test]
    fn quit_collects_verified_switch_queued_after_last_frame_before_store_flush() {
        use crate::storage::{Playback, Store};
        let (root, mut controller) = fixture();
        let config = controller.config.clone();
        let source = config.music.clone();
        let destination = root.join("second/music");
        std::fs::create_dir_all(source.join("Album")).unwrap();
        let song = source.join("Album/song.wav");
        std::fs::write(&song, b"retained original").unwrap();
        let store = Store::new(config.data.clone()).unwrap();
        let request = controller.request(
            m::Command::TransferMusic {
                destination: destination.clone(),
                remove_source: true,
                confirmed: true,
            },
            "Moving Music",
        );
        assert!(matches!(
            controller.launch(request),
            Some(Effect::PauseMusic)
        ));
        // Do not tick: the critical event stays queued, as when Quit arrives
        // after the last frame and verification publishes during shutdown.
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while m::resolve_paths(&config.bootstrap, &source, &config.data).0 != destination {
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let Some(Request::FinalizeTransfer { manifest, .. }) = controller.quiesce_for_shutdown()
        else {
            panic!("queued switch was lost during shutdown")
        };
        assert!(song.exists());
        assert!(controller
            .worker
            .command(m::Command::Browse(source.clone()))
            .is_none());
        store
            .save_playback(&Playback {
                queue: vec![song.clone()],
                index: 0,
                position: 4.0,
                paused: true,
                ..Default::default()
            })
            .unwrap();
        store.flush().unwrap();
        drop(store);
        drop(controller);
        let mut switched = config.clone();
        switched.music = destination.clone();
        prepare_verified_transfer_clients(switched, manifest).unwrap();
        let data = Store::new(config.data.clone()).unwrap();
        assert!(data.load_playback().is_none());
        assert_eq!(std::fs::read(&song).unwrap(), b"retained original");
        assert_eq!(
            std::fs::read(destination.join("Album/song.wav")).unwrap(),
            b"retained original"
        );
        drop(data);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn verified_transfer_reopens_clients_before_background_cleanup_and_preserves_hide() {
        use crate::storage::{Playback, Store};
        for hidden in [false, true] {
            let (root, mut old) = fixture();
            let source = old.config.music.clone();
            let destination = root.join("second-card/music");
            let config = old.config.clone();
            let updates = old.update_config.clone();
            std::fs::create_dir_all(source.join("Album")).unwrap();
            let original = source.join("Album/song.wav");
            let bytes = vec![0x6a; 32768];
            std::fs::write(&original, &bytes).unwrap();
            let old_store = Store::new(config.data.clone()).unwrap();
            let playback = Playback {
                queue: vec![original.clone()],
                index: 0,
                position: 3.0,
                paused: true,
                ..Default::default()
            };
            old_store.save_playback(&playback).unwrap();
            old_store.flush().unwrap();
            let playlist = config.data.join("playlists/late.m3u");
            std::fs::create_dir_all(playlist.parent().unwrap()).unwrap();
            std::fs::write(&playlist, format!("#EXTM3U\n{}\n", original.display())).unwrap();
            let library = Arc::new(crate::library::Library::from_tracks(vec![
                crate::library::Track {
                    path: original.clone(),
                    title: "Song".into(),
                    album: "Album".into(),
                    ..Default::default()
                },
            ]));
            old.set_library(Arc::clone(&library));
            let request = old.request(
                m::Command::TransferMusic {
                    destination: destination.clone(),
                    remove_source: true,
                    confirmed: true,
                },
                "Moving Music",
            );
            assert!(matches!(old.launch(request), Some(Effect::PauseMusic)));
            if hidden {
                old.handle(Action::Back);
                old.handle(Action::Down);
                assert!(matches!(old.handle(Action::Select), Some(Effect::Exit)));
            }
            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            let finalize = loop {
                let mut finalize = None;
                for effect in old.tick() {
                    if let Effect::Run(request) = effect {
                        finalize = Some(request);
                    }
                }
                if let Some(request) = finalize {
                    break request;
                }
                assert!(
                    Instant::now() < deadline,
                    "verified copy did not reach client barrier"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            };
            assert_eq!(std::fs::read(&original).unwrap(), bytes);
            assert_eq!(
                std::fs::read(destination.join("Album/song.wav")).unwrap(),
                bytes
            );
            // Reproduce late writes by clients that were alive during the first
            // verification. No new client may read these old paths yet.
            let mut late = playback.clone();
            late.position = 4.0;
            old_store.save_playback(&late).unwrap();
            old_store.flush().unwrap();
            assert!(std::fs::read_to_string(config.data.join("playback.json"))
                .unwrap()
                .contains(original.to_str().unwrap()));
            std::fs::write(&playlist, format!("#EXTM3U\n{}\n", original.display())).unwrap();
            drop(old_store);
            drop(old);
            let Request::FinalizeTransfer {
                manifest, visible, ..
            } = finalize
            else {
                panic!(
                    "verified transfer must reopen clients instead of entering a blocking runner"
                )
            };
            assert_eq!(visible, !hidden);
            let mut new_config = config.clone();
            new_config.music = destination.clone();
            prepare_verified_transfer_clients(new_config.clone(), manifest.clone()).unwrap();
            assert!(original.exists(), "preparation must not delete originals");
            let cleared: Playback =
                serde_json::from_slice(&std::fs::read(config.data.join("playback.json")).unwrap())
                    .unwrap();
            assert!(cleared.queue.is_empty());
            let text = std::fs::read_to_string(&playlist).unwrap();
            assert!(text.contains(destination.to_str().unwrap()));
            assert!(!text.contains(source.to_str().unwrap()));
            // The newly opened Store is operable while cleanup runs. Its
            // current destination playback must survive the cleanup worker.
            let new_store = Store::new(config.data.clone()).unwrap();
            assert!(new_store.load_playback().is_none());
            let current = Playback {
                queue: vec![destination.join("Album/song.wav")],
                index: 0,
                position: 9.0,
                paused: false,
                ..Default::default()
            };
            new_store.save_playback(&current).unwrap();
            new_store.flush().unwrap();
            let mut new = Controller::new(new_config, updates).unwrap();
            new.adopt_verified_transfer(manifest, library, visible)
                .unwrap();
            assert_eq!(new.has_visible_transfer(), !hidden);
            if !hidden {
                new.handle(Action::Back);
                assert!(new.has_options());
                new.handle(Action::Down);
                assert!(matches!(new.handle(Action::Select), Some(Effect::Exit)));
                assert!(!new.has_visible_transfer());
            }
            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            while !new.active_move.as_ref().unwrap().view.finished() {
                for effect in new.tick() {
                    assert!(
                        !matches!(effect, Effect::Run(_)),
                        "adopted cleanup must stay in the operable App"
                    );
                }
                assert!(Instant::now() < deadline, "background cleanup stalled");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(!original.exists());
            assert_eq!(
                std::fs::read(destination.join("Album/song.wav")).unwrap(),
                bytes
            );
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(config.data.join("playback.json")).unwrap())
                    .unwrap();
            assert_eq!(
                saved["queue"][0],
                current.queue[0].to_string_lossy().as_ref()
            );
            assert_eq!(saved["position"], 9.0);
            assert!(!new.has_visible_transfer());
            drop(new);
            drop(new_store);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

/// The old App and every writer must be joined before this metadata barrier.
/// No deletion is acknowledged here. The next App may safely read the files.
pub fn prepare_verified_transfer_clients(
    config: m::Config,
    manifest: PathBuf,
) -> Result<(), String> {
    let mut worker = m::Maintenance::new(config)?;
    let id = worker
        .command(m::Command::PrepareTransferredClients {
            manifest,
            quiescent: true,
        })
        .ok_or("Music worker is busy")?;
    loop {
        // This metadata barrier must finish even on Quit: the selected
        // pointer is already committed. It never acknowledges source deletion.
        if let Some(event) = worker.poll() {
            match event {
                m::Event::Completed { id: done } if done == id => {
                    worker.shutdown();
                    return Ok(());
                }
                m::Event::Error {
                    id: failed,
                    message,
                } if failed == id => {
                    worker.shutdown();
                    return Err(message);
                }
                _ => {}
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

/// The App and every writer must already have been dropped before entering.
pub fn run(
    request: Request,
    r: &mut Renderer,
    input: &mut crate::input::Input,
    window: &sdl2::video::Window,
    assets_path: &std::path::Path,
    dark: bool,
    library: Arc<crate::library::Library>,
) -> Result<bool, String> {
    // The App has already joined its writers. Sprites are loaded before this
    // new interaction loop and released on both success and failure.
    let mut assets = VisualAssets::new(r, assets_path)?;
    let mut art = match crate::art::Artwork::new(r, assets_path) {
        Ok(art) => art,
        Err(error) => {
            assets.close(r);
            return Err(error);
        }
    };
    let result = run_inner(request, r, input, window, &assets, dark, &mut art, library);
    art.close(r);
    assets.close(r);
    result
}
fn run_inner(
    request: Request,
    r: &mut Renderer,
    input: &mut crate::input::Input,
    window: &sdl2::video::Window,
    assets: &VisualAssets,
    dark: bool,
    art: &mut crate::art::Artwork,
    library: Arc<crate::library::Library>,
) -> Result<bool, String> {
    let mut maintenance = None;
    let mut updater = None;
    let mut id = 0;
    let label;
    let mut update_view = update_visual::UpdateView::new();
    let mut update_manifest = None;
    let mut update_version = String::new();
    let mut update_beta = false;
    let mut update_done = None;
    let mut movement = None;
    match request {
        Request::FinalizeTransfer { .. } => {
            return Err("Transferred clients must be reopened before cleanup".into())
        }
        Request::Maintenance {
            config,
            command,
            label: message,
        } => {
            if matches!(
                &command,
                m::Command::TransferMusic { .. } | m::Command::ResumeTransfer { .. }
            ) {
                movement = Some(move_visual::MoveView::new());
            }
            let mut worker = m::Maintenance::new(config)?;
            id = worker
                .command(command)
                .ok_or("Maintenance worker is busy")?;
            maintenance = Some(worker);
            label = message;
        }
        Request::Update { config, manifest } => {
            update_view.download();
            update_manifest = Some(manifest.clone());
            update_version = config.current_version.clone();
            update_beta = config.allow_beta;
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
    // read every two seconds, as the app's status watcher does
    let mut runner_battery = betterpod_rust::status::current_battery();
    let mut battery_read = std::time::Instant::now();
    loop {
        if battery_read.elapsed() >= std::time::Duration::from_secs(2) {
            runner_battery = betterpod_rust::status::current_battery();
            battery_read = std::time::Instant::now();
        }
        for action in input.poll() {
            if movement.as_ref().is_some_and(|view| view.finished())
                && matches!(action, Action::Select | Action::Back)
            {
                return failure.take().map_or(Ok(false), Err);
            }
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
                    m::Event::TransferPlan {
                        source,
                        destination,
                        remove_source,
                        files,
                        ..
                    } => {
                        if let Some(view) = movement.as_mut() {
                            view.prepare(
                                source,
                                destination,
                                remove_source,
                                files,
                                Arc::clone(&library),
                            );
                        }
                    }
                    m::Event::Progress {
                        phase: p,
                        done: d,
                        total: t,
                        path,
                        ..
                    } => {
                        phase = p.into();
                        done = d;
                        total = t;
                        if let Some(view) = movement.as_mut() {
                            view.progress(p, d, t, path);
                        }
                    }

                    m::Event::MusicSelected { id, .. } | m::Event::DatabaseSelected { id, .. } => {
                        worker.acknowledge_switch(id)
                    }
                    m::Event::Completed { .. } => {
                        if let Some(view) = movement.as_mut() {
                            view.done();
                        } else {
                            complete = Some(Ok(false));
                        }
                    }
                    m::Event::Error { message, .. } => {
                        if let Some(view) = movement.as_mut() {
                            view.fail(message.clone(), cancelled);
                        }
                        complete = Some(Err(message));
                    }
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
                        update_view.progress(p, d, t);
                    }
                    u::Event::Applied { .. } => {
                        update_view.done();
                        update_done = Some(
                            std::time::Instant::now() + std::time::Duration::from_millis(1500),
                        );
                    }
                    u::Event::Error(e) => {
                        update_view.fail(e.clone());
                        complete = Some(Err(e));
                    }
                    _ => {}
                }
            }
        }
        if updater.is_some() {
            r.begin(if dark {
                [22. / 255., 23. / 255., 25. / 255., 1.]
            } else {
                [1.; 4]
            });
            update_view.draw(
                r,
                dark,
                assets,
                &update_version,
                update_beta,
                update_manifest.as_ref(),
            );
            draw_runner_header(r, "Software Update", dark, assets, runner_battery);
        } else if let Some(view) = movement.as_mut() {
            art.poll(r);
            r.begin(if dark {
                [22. / 255., 23. / 255., 25. / 255., 1.]
            } else {
                [1.; 4]
            });
            view.draw(r, art, assets, dark);
            draw_runner_header(r, "Moving Music", dark, assets, runner_battery);
        } else {
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
        }
        if update_done.is_some_and(|done| std::time::Instant::now() >= done) {
            complete = Some(Ok(true));
        }
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

fn draw_runner_header(
    r: &mut Renderer,
    title: &str,
    dark: bool,
    assets: &VisualAssets,
    battery: Option<u8>,
) {
    let rgb = |a: u8, b: u8, c: u8| [a as f32 / 255., b as f32 / 255., c as f32 / 255., 1.];
    let (top, bottom, border, text) = if dark {
        (
            rgb(78, 81, 86),
            rgb(36, 38, 42),
            rgb(10, 11, 12),
            rgb(240, 241, 243),
        )
    } else {
        (
            rgb(254, 255, 255),
            rgb(177, 182, 185),
            rgb(121, 149, 163),
            rgb(0, 0, 0),
        )
    };
    r.gradient(0., 0., 640., 38., top, bottom);
    r.rect(0., 38., 640., 2., border);
    r.text_baseline(
        title,
        12.,
        3. + crate::render::line_box(26.).1,
        26.,
        700,
        text,
        530.,
    );
    if let Some(shell) = assets.get(if dark {
        "battery_shell__dark"
    } else {
        "battery_shell"
    }) {
        r.image(shell, 576., 7., 52., 24., 1.);
    }
    // the console's level, as the header elsewhere (it was always full)
    let pct = battery.unwrap_or(100).max(15);
    if let Some(fill) = assets.get(if pct <= 20 {
        "battery_fill_warn"
    } else {
        "battery_fill_ok"
    }) {
        let w = (44.0 * (pct as f32 / 85.0).min(1.0)).round();
        r.image_uv(fill, [578., 9., w, 20.], [0., 0., w / 44., 1.], 1.);
    }
}
