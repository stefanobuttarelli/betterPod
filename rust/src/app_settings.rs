//! Cached two-column choices. Built only when navigation or a preference changes.
use super::*;
#[derive(Clone)]
pub(super) enum SettingValue {
    Shuffle(bool),
    Repeat(&'static str),
    Theme(&'static str),
    Effects(&'static str),
    Apple(bool),
    YouTube(bool),
    Podcasts(bool),
    Delay(u32),
    Dim(u8),
    Style(&'static str),
    Effect(u32),
    Palette(u32),
}
fn choice(label: impl Into<String>, value: SettingValue, selected: bool) -> Row {
    let mut row = Row::new(label, Intent::Setting(value));
    row.arrow = false;
    if selected {
        row.detail = "Selected".into();
    }
    row
}
impl App {
    pub(super) fn apply_setting(&mut self, value: SettingValue) {
        match value {
            SettingValue::Shuffle(v) => {
                self.settings.shuffle = v;
                if v {
                    self.shuffle_remaining();
                }
            }
            SettingValue::Repeat(v) => self.settings.repeat = v.into(),
            SettingValue::Theme(v) => self.settings.theme = v.into(),
            SettingValue::Effects(v) => self.settings.effects = v.into(),
            SettingValue::Apple(v) => self.settings.apple = v,
            SettingValue::YouTube(v) => {
                self.settings.youtube = v;
                self.extras.set_youtube(v);
            }
            SettingValue::Podcasts(v) => self.settings.hide_podcasts = !v,
            SettingValue::Delay(v) => self.settings.screensaver_seconds = v,
            SettingValue::Dim(v) => self.settings.screensaver_dim = v,
            SettingValue::Style(v) => self.settings.screensaver_style = v.into(),
            SettingValue::Effect(v) => self.settings.screensaver_effect = v,
            SettingValue::Palette(v) => self.settings.screensaver_colors = v,
        }
        self.configure_saver(false);
        self.save_settings();
        self.refresh_page();
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
        self.preview_rows = match intent {
            Some(Intent::Shuffle) => [false, true]
                .into_iter()
                .map(|v| {
                    choice(
                        if v { "On" } else { "Off" },
                        SettingValue::Shuffle(v),
                        self.settings.shuffle == v,
                    )
                })
                .collect(),
            Some(Intent::Repeat) => [("Off", "off"), ("One", "one"), ("All", "all")]
                .into_iter()
                .map(|(l, v)| choice(l, SettingValue::Repeat(v), self.settings.repeat == v))
                .collect(),
            Some(Intent::Theme) => [("Light", "light"), ("Dark", "dark"), ("Automatic", "auto")]
                .into_iter()
                .map(|(l, v)| choice(l, SettingValue::Theme(v), self.settings.theme == v))
                .collect(),
            Some(Intent::Effects) => [("Full", "full"), ("Light", "light")]
                .into_iter()
                .map(|(l, v)| choice(l, SettingValue::Effects(v), self.settings.effects == v))
                .collect(),
            Some(Intent::Apple) => [false, true]
                .into_iter()
                .map(|v| {
                    choice(
                        if v { "iPod" } else { "betterPod" },
                        SettingValue::Apple(v),
                        self.settings.apple == v,
                    )
                })
                .collect(),
            Some(Intent::YouTube) => [false, true]
                .into_iter()
                .map(|v| {
                    choice(
                        if v { "On" } else { "Off" },
                        SettingValue::YouTube(v),
                        self.settings.youtube == v,
                    )
                })
                .collect(),
            Some(Intent::Podcasts) => [false, true]
                .into_iter()
                .map(|v| {
                    choice(
                        if v { "Show" } else { "Hide" },
                        SettingValue::Podcasts(v),
                        self.settings.hide_podcasts != v,
                    )
                })
                .collect(),
            Some(Intent::SaverTime) => [
                ("Never", 0),
                ("1 minute", 60),
                ("2 minutes", 120),
                ("5 minutes", 300),
                ("10 minutes", 600),
            ]
            .into_iter()
            .map(|(l, v)| {
                choice(
                    l,
                    SettingValue::Delay(v),
                    self.settings.screensaver_seconds == v,
                )
            })
            .collect(),
            Some(Intent::SaverDim) => [
                ("Do Not Dim", 0),
                ("5%", 5),
                ("15%", 15),
                ("30%", 30),
                ("50%", 50),
            ]
            .into_iter()
            .map(|(l, v)| choice(l, SettingValue::Dim(v), self.settings.screensaver_dim == v))
            .collect(),
            Some(Intent::SaverStyle) => [("Cover Mosaic", "cover"), ("Visualizer", "viz")]
                .into_iter()
                .map(|(l, v)| {
                    choice(
                        l,
                        SettingValue::Style(v),
                        self.settings.screensaver_style == v,
                    )
                })
                .collect(),
            Some(Intent::SaverEffect) => (0..saver::effect_count(&self.settings.screensaver_style))
                .map(|v| {
                    choice(
                        saver::effect_name(&self.settings.screensaver_style, v),
                        SettingValue::Effect(v),
                        self.settings.screensaver_effect == v,
                    )
                })
                .collect(),
            Some(Intent::SaverColors) => (0..saver::palette_count())
                .map(|v| {
                    choice(
                        saver::palette_name(v),
                        SettingValue::Palette(v),
                        self.settings.screensaver_colors == v,
                    )
                })
                .collect(),
            other => {
                let lines = match other {
                    Some(Intent::Volume) => vec![
                        format!("{}%", (self.settings.volume * 100.0).round() as u32),
                        "LEFT / RIGHT to adjust".into(),
                        "Audio keeps priority".into(),
                    ],
                    Some(Intent::Rescan) => vec![
                        format!("{} songs", self.library.tracks.len()),
                        format!("{} albums", self.library.albums.len()),
                        format!("{} artists", self.library.artists.len()),
                        "Scan music in background".into(),
                    ],
                    Some(Intent::Maintenance(MaintenanceMode::Music)) => vec![
                        self.music_dir.display().to_string(),
                        "Choose, copy or move".into(),
                        "Files verified before removal".into(),
                    ],
                    Some(Intent::Maintenance(MaintenanceMode::Database)) => vec![
                        self.data_dir.display().to_string(),
                        "Private Rust app data".into(),
                    ],
                    Some(Intent::Maintenance(MaintenanceMode::Update)) => vec![
                        "Independent Rust channel".into(),
                        format!("Version {}", env!("CARGO_PKG_VERSION")),
                        "Local experiment".into(),
                    ],
                    Some(Intent::Extra(ExtraPage::Bluetooth)) => {
                        vec!["Paired and new devices".into(), "Connect headphones".into()]
                    }
                    Some(Intent::Open(Screen::About)) => vec![
                        "betterPod Rust".into(),
                        format!("Version {}", env!("CARGO_PKG_VERSION")),
                        "Independent app".into(),
                    ],
                    _ => vec!["A or RIGHT to open".into(), "B or LEFT to return".into()],
                };
                lines
                    .into_iter()
                    .map(|l| {
                        let mut row = Row::new(l, Intent::CloseOptions);
                        row.arrow = false;
                        row
                    })
                    .collect()
            }
        };
        self.preview_selected = self
            .preview_rows
            .iter()
            .position(|r| r.detail == "Selected")
            .unwrap_or(0);
    }
    pub(super) fn handle_setting_preview(&mut self, action: Action) -> bool {
        if self.screen() != Screen::Settings {
            return false;
        }
        let choices = self
            .preview_rows
            .first()
            .is_some_and(|r| matches!(r.intent, Intent::Setting(_)));
        if self.preview_focus {
            match action {
                Action::Left | Action::Back => self.preview_focus = false,
                Action::Up => self.preview_selected = self.preview_selected.saturating_sub(1),
                Action::Down => {
                    self.preview_selected =
                        (self.preview_selected + 1).min(self.preview_rows.len().saturating_sub(1))
                }
                Action::Select | Action::Right => {
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
        if choices && matches!(action, Action::Select | Action::Right) {
            self.preview_focus = true;
            return true;
        }
        if action == Action::Left {
            if self.pages.len() > 1 {
                self.pages.pop();
            }
            return true;
        }
        false
    }
    pub(super) fn draw_setting_preview(&self, r: &mut Renderer, fg: Color, muted: Color) {
        let title = self
            .pages
            .last()
            .and_then(|p| p.rows.get(p.selected))
            .map_or("Settings", |row| {
                row.label.split(':').next().unwrap_or("Settings")
            });
        r.text(title, 337.0, 55.0, 24, fg, 285.0);
        r.rect(333.0, 89.0, 295.0, 1.0, [0.45, 0.47, 0.51, 0.25]);
        let first = if self.preview_focus {
            self.preview_selected.saturating_sub(5)
        } else {
            0
        };
        for (i, row) in self.preview_rows.iter().enumerate().skip(first).take(7) {
            let y = 105.0 + (i - first) as f32 * 40.0;
            let focused = self.preview_focus && i == self.preview_selected;
            if focused {
                r.rect(331.0, y - 3.0, 298.0, 37.0, BLUE);
            }
            r.text(
                &row.label,
                342.0,
                y + 4.0,
                20,
                if focused { WHITE } else { fg },
                if row.detail == "Selected" {
                    244.0
                } else {
                    276.0
                },
            );
            if row.detail == "Selected" {
                r.text(
                    "✓",
                    600.0,
                    y + 4.0,
                    20,
                    if focused { WHITE } else { BLUE },
                    25.0,
                );
            }
        }
        r.text(
            if self.preview_focus {
                "A: Choose   B: Return"
            } else {
                "A / RIGHT: Open"
            },
            337.0,
            378.0,
            15,
            muted,
            286.0,
        );
    }
}
