//! Original Settings columns. Choices are rebuilt only on a state change.
use super::*;
#[derive(Clone)]
pub(crate) enum SettingValue {
    ShuffleMode(&'static str),
    Repeat(&'static str),
    Theme(&'static str),
    Apple(bool),
    YouTube(bool),
    Podcasts(bool),
    Delay(u32),
}
fn choice(label: impl Into<String>, value: SettingValue, selected: bool) -> Row {
    let mut row = Row::new(label, Intent::Setting(value));
    row.arrow = false;
    row.visual.checked = selected;
    row
}
fn summary(label: &str, value: impl Into<String>, detail: impl Into<String>) -> Row {
    let mut row = Row::new(label, Intent::Noop);
    row.arrow = false;
    row.visual.value = value.into();
    row.detail = detail.into();
    row
}
impl App {
    pub(super) fn apply_setting(&mut self, value: SettingValue) {
        match value {
            SettingValue::ShuffleMode(v) => self.set_shuffle_mode(v),
            SettingValue::Repeat(v) => self.settings.repeat = v.into(),
            SettingValue::Theme(v) => self.settings.theme = v.into(),
            SettingValue::Apple(v) => self.settings.apple = v,
            SettingValue::YouTube(v) => {
                self.settings.youtube = v;
                self.extras.set_youtube(v);
            }
            SettingValue::Podcasts(v) => self.settings.hide_podcasts = !v,
            SettingValue::Delay(v) => self.settings.screensaver_seconds = v,
        }
        self.configure_saver(false);
        self.save_settings();
        self.refresh_page();
        // Home labels and visibility change when its menu isn't on top too.
        for i in 0..self.pages.len() {
            if self.pages[i].screen == Screen::Home {
                let selected = self.pages[i]
                    .rows
                    .get(self.pages[i].selected)
                    .map(|r| r.label.clone());
                let mut page = self.page(Screen::Home);
                page.selected = selected
                    .and_then(|l| page.rows.iter().position(|r| r.label == l))
                    .unwrap_or(0);
                page.enter = 0.0;
                self.pages[i] = page;
            }
        }
    }
    pub(super) fn refresh_setting_preview(&mut self) {
        if self.screen() != Screen::Settings {
            self.preview_focus = false;
            return;
        }
        let intent = self
            .pages
            .last()
            .and_then(|p| p.rows.get(p.selected))
            .map(|r| r.intent.clone());
        let previous = self.preview_selected;
        self.preview_rows = match intent {
            Some(Intent::Shuffle) => [("Off", "off"), ("Songs", "songs"), ("Albums", "albums")]
                .into_iter()
                .map(|(l, v)| choice(l, SettingValue::ShuffleMode(v), self.shuffle_label() == l))
                .collect(),
            Some(Intent::Repeat) => [("Off", "off"), ("One", "one"), ("All", "all")]
                .into_iter()
                .map(|(l, v)| choice(l, SettingValue::Repeat(v), self.settings.repeat == v))
                .collect(),
            Some(Intent::Theme) => [("Light", "light"), ("Dark", "dark"), ("Automatic", "auto")]
                .into_iter()
                .map(|(l, v)| {
                    let mut r = choice(l, SettingValue::Theme(v), self.settings.theme == v);
                    if v == "auto" {
                        r.detail = "Dark from 20:00 to 7:00".into();
                    }
                    r
                })
                .collect(),
            Some(Intent::Apple) => [false, true]
                .into_iter()
                .map(|v| {
                    let mut r = choice(
                        if v { "On" } else { "Off" },
                        SettingValue::Apple(v),
                        self.settings.apple == v,
                    );
                    r.detail = if v {
                        "The menu says iPod"
                    } else {
                        "The menu says betterPod"
                    }
                    .into();
                    r
                })
                .collect(),
            Some(Intent::Podcasts) => [true, false]
                .into_iter()
                .map(|v| {
                    let mut r = choice(
                        if v { "On" } else { "Off" },
                        SettingValue::Podcasts(v),
                        !self.settings.hide_podcasts == v,
                    );
                    r.detail = if v {
                        "In the main menu"
                    } else {
                        "Episodes stay in Music"
                    }
                    .into();
                    r
                })
                .collect(),
            Some(Intent::SoftwareUpdate) => vec![
                Row::new(
                    "Check for Updates",
                    Intent::Maintenance(MaintenanceMode::Update),
                ),
                {
                    let mut r = Row::new("Beta Versions", Intent::ToggleBeta);
                    r.visual.value = if self.settings.beta_updates {
                        "On"
                    } else {
                        "Off"
                    }
                    .into();
                    r.arrow = false;
                    r
                },
                summary("Version", env!("CARGO_PKG_VERSION"), ""),
            ],
            Some(Intent::QuitColumn) => {
                let mut r = Row::new("Quit Now", Intent::Quit);
                r.arrow = false;
                vec![r]
            }
            Some(Intent::SharingSettings) => vec![
                {
                    let mut r = Row::new("Device Name", Intent::SharingName);
                    r.visual.typing = matches!(self.keyboard_context, KeyboardContext::Sharing)
                        && self.keyboard.as_ref().is_some_and(|k| !k.is_exiting());
                    r.visual.value = if matches!(self.keyboard_context, KeyboardContext::Sharing) {
                        self.keyboard
                            .as_ref()
                            .filter(|k| !k.is_exiting())
                            .map(|k| k.with_caret())
                    } else {
                        None
                    }
                    .unwrap_or_else(|| {
                        if self.settings.sharing_name.is_empty() {
                            "Not set".into()
                        } else {
                            self.settings.sharing_name.clone()
                        }
                    });
                    r.detail = "Your friends see it when you share with them".into();
                    r.arrow = false;
                    r
                },
                summary(
                    "How it works",
                    "",
                    "[Y] on an album or a song → Share; Music → Receive",
                ),
            ],
            Some(Intent::Open(Screen::Library)) => vec![
                if self.scanning {
                    summary("Scanning…", crate::library::scan_progress_text(), "")
                } else {
                    summary("Songs", self.library.tracks.len().to_string(), "")
                },
                summary("Albums", self.library.albums.len().to_string(), ""),
                summary("Artists", self.library.artists.len().to_string(), ""),
            ],
            Some(Intent::Open(Screen::WifiLibrary)) => vec![
                summary(
                    "Edit from your Mac",
                    "",
                    "Connect with the code shown on betterPod",
                ),
                summary(
                    "Local Wi-Fi only",
                    "",
                    "The Mac and betterPod must use the same network",
                ),
            ],
            Some(Intent::Open(Screen::Screensaver)) => vec![
                summary(
                    "Starts",
                    if self.settings.screensaver_seconds == 0 {
                        "Never".into()
                    } else {
                        format!("{} min", self.settings.screensaver_seconds / 60)
                    },
                    "",
                ),
                summary(
                    "Style",
                    if self.settings.screensaver_style == "viz" {
                        "Visualizer"
                    } else {
                        "Cover Flip"
                    },
                    "",
                ),
                summary(
                    "Dim the Screen",
                    if self.settings.screensaver_dim > 0 {
                        "On"
                    } else {
                        "Off"
                    },
                    "",
                ),
            ],
            Some(Intent::Maintenance(MaintenanceMode::Database)) => vec![summary(
                &self.database_label(),
                "",
                self.data_dir.display().to_string(),
            )],
            Some(Intent::Extra(ExtraPage::Bluetooth)) => {
                vec![summary("No headphones connected", "", "")]
            }
            Some(Intent::YouTubeSettings) => vec![
                choice(
                    "YouTube",
                    SettingValue::YouTube(!self.settings.youtube),
                    false,
                ),
                Row::new("Install / Update Tools", Intent::Extra(ExtraPage::Tools)),
            ],
            Some(Intent::Open(Screen::About)) => self.reference_about_rows(),
            _ => Vec::new(),
        };
        self.preview_selected = if self.preview_focus {
            previous.min(self.preview_rows.len().saturating_sub(1))
        } else {
            self.preview_rows
                .iter()
                .position(|r| r.visual.checked)
                .unwrap_or(0)
        };
    }
    pub(super) fn handle_setting_preview(&mut self, action: Action) -> bool {
        if self.screen() != Screen::Settings {
            return false;
        }
        let intent = self
            .pages
            .last()
            .and_then(|p| p.rows.get(p.selected))
            .map(|r| r.intent.clone());
        let is_column = matches!(
            intent,
            Some(
                Intent::Shuffle
                    | Intent::Repeat
                    | Intent::Theme
                    | Intent::Apple
                    | Intent::Podcasts
                    | Intent::SoftwareUpdate
                    | Intent::SharingSettings
                    | Intent::QuitColumn
            )
        );
        if self.preview_focus {
            match action {
                Action::Left | Action::Back => self.preview_focus = false,
                Action::Up => self.preview_selected = self.preview_selected.saturating_sub(1),
                Action::Down => {
                    self.preview_selected =
                        (self.preview_selected + 1).min(self.preview_rows.len().saturating_sub(1))
                }
                Action::Select => {
                    if let Some(intent) = self
                        .preview_rows
                        .get(self.preview_selected)
                        .map(|r| r.intent.clone())
                    {
                        self.execute(intent);
                    }
                }
                _ => {}
            }
            return true;
        }
        if is_column && matches!(action, Action::Select | Action::Right) {
            self.preview_focus = true;
            return true;
        }
        if action == Action::Left {
            if self.pages.len() > 1 {
                let previous = self.pages.pop();
                self.visual_menu_navigation(previous, false);
            }
            return true;
        }
        false
    }
}
