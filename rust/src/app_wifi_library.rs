//! Explicit local library sessions. The UI only exchanges snapshots and events.
use super::*;
use betterpod_rust::wifi_library::{Config, Controller, Event};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Off,
    Starting,
    Ready(String),
    Connected(String),
    Failed(String),
}

pub(super) struct WifiLibrary {
    controller: Controller,
    phase: Phase,
    pairing_code: Option<String>,
    connection_notice: Option<String>,
    endpoints: Vec<String>,
    protected: Vec<PathBuf>,
    requested_path: Option<PathBuf>,
    library_generation: u64,
    last_update: Option<String>,
}

impl WifiLibrary {
    pub(super) fn new(music: PathBuf, data: PathBuf, name: String) -> Self {
        Self {
            controller: Controller::new(Config {
                music,
                data,
                name: if name.trim().is_empty() {
                    "betterPod Rust".into()
                } else {
                    name
                },
            }),
            phase: Phase::Off,
            pairing_code: None,
            connection_notice: None,
            endpoints: Vec::new(),
            protected: Vec::new(),
            requested_path: None,
            library_generation: u64::MAX,
            last_update: None,
        }
    }

    pub(super) fn active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Starting | Phase::Ready(_) | Phase::Connected(_)
        )
    }

    pub(super) fn audio_stopped(&mut self) {
        self.requested_path = None;
    }

    pub(super) fn shutdown(&mut self) {
        self.controller.shutdown();
        self.phase = Phase::Off;
        self.protected.clear();
    }
}

fn command_path(command: &AudioCommand) -> Option<&Path> {
    match command {
        AudioCommand::Play { path, .. } => Some(path.as_path()),
        _ => None,
    }
}

fn fact(label: impl Into<String>, detail: impl Into<String>) -> Row {
    let mut row = Row::new(label, Intent::Noop);
    row.detail = detail.into();
    row.arrow = false;
    row
}

fn rows(
    phase: &Phase,
    last_update: Option<&str>,
    connection_notice: Option<&str>,
    endpoints: &[String],
) -> Vec<Row> {
    let active = matches!(
        phase,
        Phase::Starting | Phase::Ready(_) | Phase::Connected(_)
    );
    let mut action = Row::new(
        if *phase == Phase::Starting {
            "Cancel"
        } else if active {
            "Stop Connection"
        } else {
            "Start Connection"
        },
        if active {
            Intent::StopWifiLibrary
        } else {
            Intent::StartWifiLibrary
        },
    );
    action.arrow = false;
    let mut rows = vec![action];
    match phase {
        Phase::Off => rows.push(fact(
            "Edit from your Mac",
            "Open the betterPod tab in the Mac app, then start a connection here",
        )),
        Phase::Starting => rows.push(fact(
            "Starting connection…",
            "The pairing code will appear here",
        )),
        Phase::Ready(code) => {
            rows.push(fact(
                grouped_code(code),
                "Pairing code · expires after ten minutes",
            ));
            rows.push(fact(
                "On your Mac",
                "betterPod tab → choose this device → enter the code",
            ));
        }
        Phase::Connected(name) => {
            rows.push(fact(
                name,
                "Connected Mac · tags and covers can be edited remotely",
            ));
            rows.push(fact(
                "Music keeps playing",
                "Stop playback before editing the current song",
            ));
        }
        Phase::Failed(error) => rows.push(fact("Connection unavailable", error)),
    }
    if let Some(update) = last_update {
        rows.push(fact(
            update,
            "The library refreshes automatically after saved changes",
        ));
    }
    rows.push(fact(
        "Use the same Wi-Fi",
        "This connection stays local and stops when betterPod closes",
    ));
    if let Some(notice) = connection_notice {
        rows.push(fact("Ready to reconnect", notice));
    }
    if active {
        for endpoint in endpoints {
            rows.push(fact(
                endpoint,
                "Manual connection address if the Mac cannot find this betterPod",
            ));
        }
    }
    if active {
        rows.push(fact(
            "You can leave this page",
            "Return here to stop the connection at any time",
        ));
    }
    rows
}

fn grouped_code(code: &str) -> String {
    if code.len() == 8 && code.bytes().all(|c| c.is_ascii_digit()) {
        format!("{} {}", &code[..4], &code[4..])
    } else {
        code.to_owned()
    }
}

impl App {
    #[cfg(test)]
    pub(super) fn wifi_library_fixture(&mut self, phase: &str) {
        self.wifi_library.phase = match phase {
            "ready" => Phase::Ready("00123456".into()),
            "connected" => Phase::Connected("Studio Mac".into()),
            "failed" => Phase::Failed("No Wi-Fi connection".into()),
            "starting" => Phase::Starting,
            _ => Phase::Off,
        };
        self.wifi_library.endpoints = if matches!(phase, "ready" | "connected") {
            vec!["192.168.1.20:41234".into()]
        } else {
            Vec::new()
        };
        self.refresh_page();
    }
    pub(super) fn wifi_library_handle(&mut self, action: Action) -> bool {
        if self.screen() != Screen::WifiLibrary || self.options.is_some() {
            return false;
        }
        match action {
            Action::Select => {
                if self.wifi_library.active() {
                    self.wifi_library_stop();
                } else {
                    self.wifi_library_start();
                }
            }
            Action::Back if self.wifi_library.phase == Phase::Starting => self.wifi_library_stop(),
            Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::Options
            | Action::SelectHeld => {}
            _ => return false,
        }
        true
    }
    pub(super) fn wifi_library_draw(&self, r: &mut Renderer, now: f64) {
        let (bg, fg, sub) = self.colors();
        r.rect(0., 40., 640., 440., bg);
        let center = |r: &mut Renderer, text: &str, y: f32, size: f32, weight: u16, color| {
            let text = app_visual_menu::ellipsize(r, text, size, weight, 576.);
            let width = r.text_width_weighted(&text, size, weight);
            r.text_baseline(
                &text,
                (640. - width) * 0.5,
                y + crate::render::line_box(size).1,
                size,
                weight,
                color,
                576.,
            );
        };
        let state = &self.wifi_library;
        let (title, lines): (&str, Vec<&str>) = match &state.phase {
            Phase::Off => (
                "Your library, on your Mac",
                vec![
                    "Connect both devices to the same Wi-Fi.",
                    "Open betterPod Share on your Mac.",
                    "Choose betterPod, then select this player.",
                ],
            ),
            Phase::Starting => ("Starting connection…", vec!["Preparing your pairing code"]),
            Phase::Ready(_) => (
                "Enter this code on your Mac",
                vec![
                    "betterPod Share → betterPod → this player",
                    "Pairing code expires after ten minutes",
                ],
            ),
            Phase::Connected(name) => (
                "Connected",
                vec![
                    name,
                    "Edit tags and covers from your Mac.",
                    "Saved changes appear here automatically.",
                ],
            ),
            Phase::Failed(_) => (
                "Connection unavailable",
                vec!["Check Wi-Fi on both devices, then try again."],
            ),
        };
        center(r, title, 78., 32., 700, fg);
        match &state.phase {
            Phase::Ready(code) => {
                center(r, &grouped_code(code), 140., 64., 600, fg);
            }
            Phase::Starting => app_visual_menu::draw_spinner_in(r, 320., 174., 22., now, sub),
            _ => {
                r.push_transform(266., 132., 3., 3., 1.);
                app_visual_menu::draw_wifi(r, 0., 0., 3, sub);
                r.pop_transform();
            }
        }
        for (i, line) in lines.iter().enumerate() {
            center(r, line, 224. + i as f32 * 30., 24., 400, sub);
        }
        if let Phase::Failed(error) = &state.phase {
            center(r, error, 284., 24., 500, fg);
        } else if let Some(update) = state
            .last_update
            .as_deref()
            .or(state.connection_notice.as_deref())
        {
            center(r, update, 324., 22., 500, fg);
        }
        if state.active() {
            if let Some(endpoint) = state.endpoints.first() {
                center(
                    r,
                    &format!("Manual address: {endpoint}"),
                    354.,
                    22.,
                    400,
                    sub,
                );
            }
            center(
                r,
                if state.phase == Phase::Starting {
                    "[B] Cancel connection"
                } else {
                    "[B] Back · connection stays available"
                },
                382.,
                20.,
                400,
                sub,
            );
        }
        app_services::draw_job_button(
            r,
            if state.phase == Phase::Starting {
                "Cancel"
            } else if state.active() {
                "Stop Connection"
            } else if matches!(state.phase, Phase::Failed(_)) {
                "Try Again"
            } else {
                "Start Connection"
            },
            true,
            self.dark(),
            fg,
        );
    }
    pub(super) fn wifi_library_rows(&self) -> Vec<Row> {
        rows(
            &self.wifi_library.phase,
            self.wifi_library.last_update.as_deref(),
            self.wifi_library.connection_notice.as_deref(),
            &self.wifi_library.endpoints,
        )
    }

    pub(super) fn wifi_library_start(&mut self) {
        if self.wifi_library.active() {
            return;
        }
        if self
            .maintenance
            .as_ref()
            .is_some_and(|controller| controller.has_active_library_transfer())
        {
            self.wifi_library.phase = Phase::Failed(
                "Wait for the music transfer to finish before connecting the Mac.".into(),
            );
            if self.screen() == Screen::WifiLibrary {
                self.refresh_page();
            }
            return;
        }
        self.wifi_library.phase = Phase::Starting;
        self.wifi_library.endpoints.clear();
        self.wifi_library.last_update = None;
        self.wifi_library.connection_notice = None;
        self.wifi_library.pairing_code = None;
        self.wifi_library.endpoints.clear();
        self.wifi_library.library_generation = u64::MAX;
        self.wifi_library_sync(None);
        let result = self.wifi_library.controller.start(
            Arc::clone(&self.library),
            self.wifi_library.protected.clone(),
        );
        if let Err(error) = result {
            self.wifi_library.phase = Phase::Failed(error);
        }
        if self.screen() == Screen::WifiLibrary {
            self.refresh_page();
        }
    }

    pub(super) fn wifi_library_stop(&mut self) {
        self.wifi_library.controller.stop();
        self.wifi_library.phase = Phase::Off;
        self.wifi_library.last_update = None;
        self.wifi_library.connection_notice = None;
        self.wifi_library.pairing_code = None;
        self.wifi_library.protected.clear();
        self.wifi_library.library_generation = u64::MAX;
        if self.screen() == Screen::WifiLibrary {
            self.refresh_page();
        }
    }

    /// Keep the new Play target protected until the worker's snapshot catches
    /// up. Existing paused decoders remain protected through their path too.
    pub(super) fn wifi_library_sync(&mut self, requested: Option<&PathBuf>) {
        if let Some(path) = requested {
            self.wifi_library.requested_path = Some(path.clone());
        }
        if !self.wifi_library.active() {
            return;
        }
        let video_path = (self.video_open && self.video_origin.starts_with('/'))
            .then(|| Path::new(&self.video_origin));
        let paths = self
            .audio_snapshot
            .path
            .as_deref()
            .into_iter()
            .chain(self.wifi_library.requested_path.as_deref())
            .chain(self.pending_audio.iter().filter_map(command_path))
            .chain(self.pending_music.as_ref().and_then(command_path))
            .chain(video_path);
        // Do not allocate or publish the library on every animation frame.
        if self.wifi_library.library_generation == self.library_generation
            && paths
                .clone()
                .eq(self.wifi_library.protected.iter().map(PathBuf::as_path))
        {
            return;
        }
        let protected: Vec<PathBuf> = paths.map(Path::to_path_buf).collect();
        self.wifi_library
            .controller
            .update_snapshot(Arc::clone(&self.library), protected.clone());
        self.wifi_library.protected = protected;
        self.wifi_library.library_generation = self.library_generation;
    }

    pub(super) fn wifi_library_refresh_snapshot(&mut self) {
        self.wifi_library.library_generation = u64::MAX;
        self.wifi_library_sync(None);
    }

    pub(super) fn wifi_library_tick(&mut self) {
        let mut refresh = false;
        let mut changed = false;
        for _ in 0..8 {
            let Some(event) = self.wifi_library.controller.poll() else {
                break;
            };
            match event {
                Event::Ready {
                    code,
                    port,
                    addresses,
                    ..
                } => {
                    if !self.wifi_library.active() {
                        continue;
                    }
                    self.wifi_library.pairing_code = Some(code.clone());
                    self.wifi_library.endpoints = addresses
                        .into_iter()
                        .map(|address| format!("{address}:{port}"))
                        .collect();
                    if self.wifi_library.endpoints.is_empty() {
                        self.wifi_library
                            .endpoints
                            .push(format!("Connection port: {port}"));
                    }
                    self.wifi_library.phase = Phase::Ready(code);
                }
                Event::Connected { name } => {
                    if !self.wifi_library.active() {
                        continue;
                    }
                    self.wifi_library.phase = Phase::Connected(name);
                    self.wifi_library.connection_notice = None;
                }
                Event::Disconnected { reason } => {
                    if !self.wifi_library.active() {
                        continue;
                    }
                    if let Some(code) = self.wifi_library.pairing_code.as_ref() {
                        self.wifi_library.phase = Phase::Ready(code.clone());
                        self.wifi_library.connection_notice = Some(reason);
                    }
                }
                Event::Changed { paths } => {
                    self.wifi_library.last_update = Some(format!(
                        "Updated {} {} from Mac",
                        paths.len(),
                        if paths.len() == 1 { "song" } else { "songs" }
                    ));
                    changed = true;
                }
                Event::Stopped => {
                    self.wifi_library.pairing_code = None;
                    self.wifi_library.connection_notice = None;
                    if !matches!(self.wifi_library.phase, Phase::Failed(_)) {
                        self.wifi_library.phase = Phase::Off;
                    }
                }
                Event::Error(error) => {
                    if !self.wifi_library.active() {
                        continue;
                    }
                    self.wifi_library.phase = Phase::Failed(error);
                    self.wifi_library.connection_notice = None;
                }
            }
            refresh = true;
        }
        if changed {
            self.rescan();
        }
        if refresh && self.screen() == Screen::WifiLibrary {
            self.refresh_page();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_page_never_starts_a_connection_and_has_an_explicit_action() {
        let rows = rows(&Phase::Off, None, None, &[]);
        assert!(matches!(rows[0].intent, Intent::StartWifiLibrary));
        assert!(rows.iter().any(|row| row.label == "Use the same Wi-Fi"));
        assert!(!app_visual_menu::is_menu(Screen::WifiLibrary));
        assert_eq!(Screen::WifiLibrary.default_kind(), ViewKind::Full);
    }

    #[test]
    fn pairing_page_groups_all_eight_digits_and_can_always_stop() {
        let rows = rows(&Phase::Ready("00123456".into()), None, None, &[]);
        assert!(matches!(rows[0].intent, Intent::StopWifiLibrary));
        assert!(rows.iter().any(|row| row.label == "0012 3456"));
        assert!(rows
            .iter()
            .any(|row| row.detail.contains("expires after ten minutes")));
    }

    #[test]
    fn connected_page_names_the_mac_and_reports_saved_changes() {
        let rows = rows(
            &Phase::Connected("Studio Mac".into()),
            Some("Updated 2 songs from Mac"),
            None,
            &[],
        );
        assert!(matches!(rows[0].intent, Intent::StopWifiLibrary));
        assert!(rows.iter().any(|row| row.label == "Studio Mac"));
        assert!(rows
            .iter()
            .any(|row| row.label == "Updated 2 songs from Mac"));
        assert!(rows.iter().any(|row| row.detail.contains("Stop playback")));
    }

    #[test]
    fn disconnected_page_keeps_the_code_visible_for_reconnection() {
        let rows = rows(
            &Phase::Ready("00123456".into()),
            None,
            Some("The Mac disconnected"),
            &[],
        );
        assert!(rows.iter().any(|row| row.label == "0012 3456"));
        assert!(rows.iter().any(|row| row.label == "Ready to reconnect"));
        assert!(matches!(rows[0].intent, Intent::StopWifiLibrary));
    }

    #[test]
    fn pairing_page_shows_the_manual_address_without_exposing_the_code_in_it() {
        let rows = rows(
            &Phase::Ready("00123456".into()),
            None,
            None,
            &["192.168.1.20:41234".into()],
        );
        assert!(rows.iter().any(|row| row.label == "192.168.1.20:41234"));
        assert!(rows
            .iter()
            .any(|row| row.detail.contains("Manual connection address")));
    }

    #[test]
    fn play_commands_protect_the_new_path_before_the_audio_snapshot_arrives() {
        let command = AudioCommand::Play {
            path: PathBuf::from("/music/01.flac"),
            sample_rate: 44100,
            start: 0.,
        };
        assert_eq!(command_path(&command), Some(Path::new("/music/01.flac")));
        assert_eq!(command_path(&AudioCommand::Pause), None);
        assert_eq!(command_path(&AudioCommand::Stop), None);
    }
}
