//! The Go 1.1 Software Update composition and wheel behavior. The updater
//! remains a separate actor; this state only describes what is visible.
use super::{u, Action, Renderer, VisualAssets};
use betterpod_rust::render::{line_box, Color};

#[derive(Clone, Debug, PartialEq)]
enum State {
    Idle,
    Checking,
    LocalBuild,
    UpToDate,
    Available,
    Downloading,
    Installing,
    Done,
    Failed(String),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Button {
    Check,
    Install,
    Exit,
}
pub(super) struct UpdateView {
    state: State,
    selected: usize,
    note_top: usize,
    note_total: usize,
    note_fit: usize,
    notes_source: String,
    notes: Vec<(bool, String)>,
    checked: Option<std::time::SystemTime>,
    target: f64,
    shown: f64,
    last: Option<std::time::Instant>,
}
impl UpdateView {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            selected: 0,
            note_top: 0,
            note_total: 0,
            note_fit: 1,
            notes_source: String::new(),
            notes: Vec::new(),
            checked: None,
            target: 0.,
            shown: 0.,
            last: None,
        }
    }
    pub fn open(&mut self, available: Option<&u::Manifest>) {
        self.selected = 0;
        self.note_top = 0;
        if available.is_some() {
            self.state = State::Available;
        }
    }
    pub fn checking(&mut self) {
        self.state = State::Checking;
        self.selected = 0;
    }
    pub fn clear_available(&mut self) {
        self.state = State::Idle;
        self.notes.clear();
        self.notes_source.clear();
        self.note_total = 0;
        self.note_top = 0;
        self.selected = 0;
    }
    pub fn local_build(&mut self) {
        self.state = State::LocalBuild;
        self.selected = 0;
    }
    pub fn up_to_date(&mut self) {
        self.state = State::UpToDate;
        self.checked = Some(std::time::SystemTime::now());
        self.selected = 0;
    }
    pub fn available(&mut self) {
        self.state = State::Available;
        self.selected = 0;
        self.note_top = 0;
    }
    pub fn fail(&mut self, message: String) {
        self.state = State::Failed(message);
        self.selected = 0;
    }
    pub fn download(&mut self) {
        self.state = State::Downloading;
        self.target = 0.;
        self.shown = 0.;
        self.last = None;
    }
    pub fn progress(&mut self, phase: &str, done: u64, total: u64) {
        if phase == "download" {
            self.state = State::Downloading;
            self.target = if total > 0 {
                done as f64 / total as f64
            } else {
                0.
            };
        } else {
            self.state = State::Installing;
            self.target = 1.;
        }
    }
    pub fn done(&mut self) {
        self.state = State::Done;
        self.target = 1.;
    }
    fn buttons(&self) -> &'static [(&'static str, Button)] {
        match self.state {
            State::Idle | State::LocalBuild => &[("Check for Updates", Button::Check)],
            State::UpToDate => &[("Check Again", Button::Check)],
            State::Failed(_) => &[("Try Again", Button::Check)],
            State::Available => &[
                ("Download and Install", Button::Install),
                ("Not Now", Button::Exit),
            ],
            State::Checking | State::Downloading | State::Installing | State::Done => &[],
        }
    }
    pub fn handle(&mut self, action: Action) -> Option<Button> {
        if matches!(action, Action::Back) {
            return Some(Button::Exit);
        }
        let n = self.buttons().len();
        if n == 0 {
            return None;
        }
        self.selected = self.selected.min(n - 1);
        let more = self.note_total > self.note_fit;
        match action {
            Action::Up | Action::Left => {
                if more && self.selected == 0 && self.note_top > 0 {
                    self.note_top -= 1;
                } else if !(more && self.selected == 0) {
                    self.selected = (self.selected + n - 1) % n;
                }
            }
            Action::Down | Action::Right => {
                if more && self.selected == 0 && self.note_top + self.note_fit < self.note_total {
                    self.note_top += 1;
                } else if !(more && self.selected == n - 1) {
                    self.selected = (self.selected + 1) % n;
                }
            }
            Action::Select => {
                let button = self.buttons()[self.selected].1;
                self.selected = 0;
                return Some(button);
            }
            _ => {}
        }
        None
    }
    pub fn draw(
        &mut self,
        r: &mut Renderer,
        dark: bool,
        assets: &VisualAssets,
        version: &str,
        beta: bool,
        manifest: Option<&u::Manifest>,
    ) {
        let now = std::time::Instant::now();
        let dt = self.last.map_or(0.5, |last| {
            now.saturating_duration_since(last).as_secs_f64().min(0.5)
        });
        self.last = Some(now);
        if self.target < self.shown {
            self.shown = self.target;
        } else {
            self.shown += (self.target - self.shown) * (dt * 10.).min(1.);
        }
        let bg = if dark {
            rgb(22, 23, 25)
        } else {
            rgb(255, 255, 255)
        };
        let fg = if dark {
            rgb(238, 239, 241)
        } else {
            rgb(0, 0, 0)
        };
        let sub = if dark {
            rgb(150, 153, 158)
        } else {
            rgb(100, 100, 100)
        };
        r.rect(0., 40., 640., 440., bg);
        if let Some(logo) = assets.get("betterpod_logo") {
            r.image(
                logo,
                (640. - logo.width as f32) * 0.5,
                68.,
                logo.width as f32,
                logo.height as f32,
                1.,
            );
        }
        let (_, asc16) = line_box(32.);
        let (_, asc13) = line_box(26.);
        let word_width =
            r.text_width_weighted("better", 32., 900) + r.text_width_weighted("Pod", 32., 900);
        let x = (640. - word_width) * 0.5;
        let y = 40. + 108. + asc16;
        r.text_baseline("better", x, y, 32., 900, fg, 640.);
        let better = r.text_width_weighted("better", 32., 900);
        r.text_baseline(
            "Pod",
            x + better,
            y,
            32.,
            900,
            if dark {
                rgb(88, 186, 255)
            } else {
                rgb(47, 134, 208)
            },
            640.,
        );
        let (mut status, mut detail) = (format!("Version {version}"), String::new());
        match &self.state {
            State::Idle => {
                detail = if beta {
                    "Beta versions are on: they are offered too".into()
                } else {
                    "Connect to Wi-Fi to check for a new version".into()
                }
            }
            State::Checking => status = "Checking for updates…".into(),
            State::LocalBuild => {
                detail = "Local experiment: no published Rust update channel".into()
            }
            State::UpToDate => {
                detail = "betterPod is up to date".into();
                if beta {
                    detail.push_str(", betas included")
                }
                if let Some(checked) = self.checked {
                    detail.push_str(" · checked ");
                    detail.push_str(&checked_when(checked));
                }
            }
            State::Available => {
                if let Some(m) = manifest {
                    status = format!(
                        "{} {} is available · {:.1} MB",
                        if super::is_beta(&m.version) {
                            "Beta"
                        } else {
                            "Version"
                        },
                        m.version,
                        m.size as f64 / 1e6
                    );
                }
            }
            State::Downloading => {
                if let Some(m) = manifest {
                    status = format!("Downloading version {}…", m.version);
                    detail = format!(
                        "{:.1} of {:.1} MB",
                        self.target * m.size as f64 / 1e6,
                        m.size as f64 / 1e6
                    );
                }
            }
            State::Installing => {
                status = "Installing…".into();
                detail = "Don't turn off the console".into();
            }
            State::Done => {
                status = manifest.map_or_else(
                    || "Updated".into(),
                    |m| format!("Updated to version {}", m.version),
                );
                detail = "Restarting…".into();
            }
            State::Failed(message) => {
                status = "Update failed".into();
                detail = message.clone();
            }
        }
        centered(r, &status, 40. + 148. + asc13, 26., 600, fg);
        if !detail.is_empty() {
            centered(r, &detail, 40. + 184. + asc13, 24., 500, sub);
        }
        if self.state == State::Available {
            let source = manifest.map_or("", |m| m.notes.as_str());
            if self.notes_source != source {
                self.notes_source = source.into();
                self.notes = wrap_notes(r, source);
            }
            let lh = line_box(24.).0 + 2.;
            let asc = line_box(24.).1;
            let mut y = 232.;
            let bottom = 40. + (220. - 8. - self.buttons().len() as f32 * 34. + 6. - 4.) * 2.;
            self.note_total = self.notes.len();
            self.note_fit = ((bottom - y) / lh).max(1.) as usize;
            self.note_top = self
                .note_top
                .min(self.note_total.saturating_sub(self.note_fit));
            let bullet = r.text_width_weighted("• ", 24., 500);
            for (first, line) in self.notes.iter().skip(self.note_top).take(self.note_fit) {
                if *first {
                    r.text_baseline("•", 84., y + asc, 24., 500, sub, 24.);
                }
                r.text_baseline(line, 84. + bullet, y + asc, 24., 500, sub, 472. - bullet);
                y += lh;
            }
            if self.note_total > self.note_fit {
                let top = 232.;
                let h = self.note_fit as f32 * lh;
                let mut track = sub;
                track[3] = 0.18;
                r.rounded_gradient(568., top, 6., h, 3., &[(0., track), (1., track)]);
                let ty = top + h * self.note_top as f32 / self.note_total as f32;
                let thumb = (h * self.note_fit as f32 / self.note_total as f32)
                    .max(20.)
                    .min(top + h - ty);
                track[3] = 0.7;
                r.rounded_gradient(568., ty, 6., thumb, 3., &[(0., track), (1., track)]);
            }
        } else {
            progress(r, assets, 80., 268., 480., self.shown as f32, dark);
            self.note_total = 0;
            if matches!(
                self.state,
                State::Downloading | State::Installing | State::Done
            ) {
                centered(
                    r,
                    &format!("{}%", (self.shown * 100. + 0.5) as i32),
                    40. + 288. + line_box(24.).1,
                    24.,
                    700,
                    sub,
                );
            }
        }
        let buttons = self.buttons();
        let mut by = 40. + (220. - 8. - buttons.len() as f32 * 34. + 6.) * 2.;
        for (i, (label, _)) in buttons.iter().enumerate() {
            let selected = i == self.selected;
            let stops = if selected {
                [
                    (0., rgb(60, 184, 255)),
                    (0.4844, mix(rgb(60, 184, 255), rgb(52, 122, 181), 0.4844)),
                    (1., rgb(52, 122, 181)),
                ]
            } else if dark {
                [
                    (0., rgb(85, 88, 95)),
                    (0.4844, rgb(52, 55, 61)),
                    (1., rgb(60, 63, 69)),
                ]
            } else {
                [
                    (0., rgb(255, 255, 255)),
                    (0.4844, rgb(203, 204, 206)),
                    (1., rgb(223, 223, 223)),
                ]
            };
            r.rounded_gradient(80., by, 480., 56., 18., &stops);
            let border = if selected {
                rgb(58, 120, 173)
            } else if dark {
                rgb(21, 23, 26)
            } else {
                rgb(62, 66, 73)
            };
            r.stroke_rounded(80., by, 480., 56., 18., 2., border);
            let color = if selected {
                [1.; 4]
            } else if dark {
                rgb(240, 241, 243)
            } else {
                rgb(0, 0, 0)
            };
            centered(r, label, by + 28. + 28. * 0.727 * 0.5, 28., 700, color);
            by += 68.;
        }
    }
}
fn centered(r: &mut Renderer, text: &str, baseline: f32, size: f32, weight: u16, color: Color) {
    let text = ellipsize(r, text, 560., size, weight);
    let width = r.text_width_weighted(&text, size, weight);
    r.text_baseline(
        &text,
        (640. - width) * 0.5,
        baseline,
        size,
        weight,
        color,
        560.,
    );
}
pub(super) fn ellipsize(
    r: &mut Renderer,
    text: &str,
    width: f32,
    size: f32,
    weight: u16,
) -> String {
    if r.text_width_weighted(text, size, weight) <= width {
        return text.into();
    }
    let mut text = text.chars().take(4096).collect::<String>();
    while !text.is_empty() {
        text.pop();
        if r.text_width_weighted(&format!("{text}…"), size, weight) <= width {
            break;
        }
    }
    format!("{text}…")
}
fn wrap_notes(r: &mut Renderer, source: &str) -> Vec<(bool, String)> {
    let width = 472. - r.text_width_weighted("• ", 24., 500);
    let mut lines = Vec::new();
    for note in source
        .lines()
        .map(|s| s.trim().trim_start_matches(['-', '*', '•']).trim())
        .filter(|s| !s.is_empty())
        .take(100)
    {
        let mut line = String::new();
        let mut first = true;
        for word in note.split_whitespace() {
            let candidate = if line.is_empty() {
                word.into()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && r.text_width_weighted(&candidate, 24., 500) > width {
                lines.push((first, std::mem::take(&mut line)));
                first = false;
            }
            if line.is_empty() {
                line.push_str(word)
            } else {
                line.push(' ');
                line.push_str(word)
            }
            if lines.len() >= 100 {
                return lines;
            }
        }
        if !line.is_empty() {
            lines.push((first, ellipsize(r, &line, width, 24., 500)));
        }
        if lines.len() >= 100 {
            break;
        }
    }
    lines
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn mix(a: Color, b: Color, t: f32) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
pub(super) fn progress(
    r: &mut Renderer,
    assets: &VisualAssets,
    x: f32,
    y: f32,
    width: f32,
    fill: f32,
    dark: bool,
) {
    let track = if dark {
        assets
            .get("gloss_track__dark")
            .or_else(|| assets.get("gloss_track"))
    } else {
        assets.get("gloss_track")
    };
    let mut parts = Vec::with_capacity(2);
    if let Some(t) = track {
        parts.push((t, width.min(t.width as f32)));
    }
    if fill > 0. {
        if let Some(t) = assets.get("gloss_blue") {
            parts.push((t, (width * fill).min(t.width as f32)));
        }
    }
    for &(t, w) in &parts {
        r.image_uv(
            t,
            [x, y, w, t.height as f32],
            [0., 0., w / t.width as f32, 1.],
            1.,
        );
    }
    for row in 0..24 {
        let alpha = (1. - (row as f32 + 0.5) / 24.) * 0.35;
        for &(t, w) in &parts {
            let source = 31 - row;
            r.image_uv(
                t,
                [x, y + 32. + row as f32, w, 1.],
                [
                    0.,
                    source as f32 / 32.,
                    w / t.width as f32,
                    (source + 1) as f32 / 32.,
                ],
                alpha,
            );
        }
    }
}
fn checked_when(time: std::time::SystemTime) -> String {
    #[cfg(unix)]
    {
        let seconds = time
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as libc::time_t;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as libc::time_t;
        let mut checked = unsafe { std::mem::zeroed::<libc::tm>() };
        let mut today = unsafe { std::mem::zeroed::<libc::tm>() };
        if !unsafe { libc::localtime_r(&seconds, &mut checked) }.is_null()
            && !unsafe { libc::localtime_r(&now, &mut today) }.is_null()
        {
            return if checked.tm_year == today.tm_year && checked.tm_yday == today.tm_yday {
                format!("at {:02}:{:02}", checked.tm_hour, checked.tm_min)
            } else {
                let months = [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ];
                format!(
                    "on {} {}",
                    checked.tm_mday,
                    months[checked.tm_mon.clamp(0, 11) as usize]
                )
            };
        }
    }
    "just now".into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notes_and_buttons_form_one_bounded_scroll_path() {
        let mut v = UpdateView::new();
        v.available();
        v.note_total = 5;
        v.note_fit = 2;
        for _ in 0..3 {
            v.handle(Action::Down);
        }
        assert_eq!((v.note_top, v.selected), (3, 0));
        v.handle(Action::Down);
        assert_eq!(v.selected, 1);
        v.handle(Action::Down);
        assert_eq!(v.selected, 1);
        v.handle(Action::Up);
        assert_eq!((v.note_top, v.selected), (3, 0));
        v.handle(Action::Up);
        assert_eq!(v.note_top, 2);
        assert_eq!(v.handle(Action::Select), Some(Button::Install));
    }
    #[test]
    fn pending_checks_have_no_install_button() {
        let mut v = UpdateView::new();
        v.available();
        v.checking();
        assert_eq!(v.handle(Action::Select), None);
        assert_eq!(v.handle(Action::Back), Some(Button::Exit));
        v.local_build();
        assert_eq!(v.handle(Action::Select), Some(Button::Check));
    }
}
