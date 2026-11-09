//! Share's code editor and transfer studio from ij_share.go / ij_share_draw.go.
//! Rendering consumes actor events; it never opens files or connections.
use super::super::app_feedback::rounded_gradient;
use super::*;
use crate::render::{Color, Texture};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Preparing,
    Waiting,
    Code,
    Connecting,
    Moving,
    Saving,
    Done,
    Failed,
}
pub(super) enum ShareAction {
    Receive(String),
    Exit,
    Play(Vec<PathBuf>),
}
#[derive(Clone)]
pub(super) struct ShareVisual {
    send: bool,
    phase: Phase,
    code: [u8; 8],
    cursor: usize,
    since: f64,
    info: ShareInfo,
    cover: Option<PathBuf>,
    paths: Vec<PathBuf>,
    button: usize,
    done: u64,
    total: u64,
    rate: f64,
    rate_time: f64,
    rate_bytes: u64,
    laser: f32,
    last_time: f64,
    error: String,
    egg: String,
    egg_since: f64,
    peer: String,
}
impl ShareVisual {
    pub(super) fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Code | Phase::Done | Phase::Failed)
    }
    pub(super) fn receive(now: f64) -> Self {
        Self {
            send: false,
            phase: Phase::Code,
            code: [0; 8],
            cursor: 0,
            since: now,
            info: ShareInfo::default(),
            cover: None,
            paths: Vec::new(),
            button: 0,
            done: 0,
            total: 0,
            rate: 0.,
            rate_time: now,
            rate_bytes: 0,
            laser: 0.,
            last_time: now,
            error: String::new(),
            egg: String::new(),
            egg_since: now,
            peer: String::new(),
        }
    }
    pub(super) fn send(info: ShareInfo, cover: Option<PathBuf>, now: f64) -> Self {
        let mut value = Self::receive(now);
        value.send = true;
        value.phase = Phase::Preparing;
        value.info = info;
        value.cover = cover;
        value
    }
    pub(super) fn event(&mut self, event: &share::Event, now: f64) {
        let phase = match event {
            share::Event::Preview { info, cover } => {
                if let Some(info) = info {
                    self.info = info.clone();
                }
                if let Some(cover) = cover {
                    self.cover = Some(cover.clone());
                }
                self.phase
            }
            share::Event::Code(code) => {
                for (i, digit) in code.bytes().filter(u8::is_ascii_digit).take(8).enumerate() {
                    self.code[i] = digit - b'0';
                }
                // Showing a locally generated code must not claim the relay
                // or archive is ready. Progress owns that state transition.
                self.phase
            }
            share::Event::Progress {
                done,
                total,
                phase,
                peer,
            } => {
                self.done = *done;
                self.total = *total;
                if let Some(peer) = peer {
                    self.peer = peer.clone();
                }
                match phase.as_str() {
                    "Preparing" => Phase::Preparing,
                    "Waiting for receiver" => Phase::Waiting,
                    "Finding sender" | "Connecting" => Phase::Connecting,
                    "Saving" => Phase::Saving,
                    _ => Phase::Moving,
                }
            }
            share::Event::Received { paths, info } => {
                self.paths = paths.clone();
                if let Some(info) = info {
                    self.info = info.clone();
                }
                Phase::Done
            }
            share::Event::Sent => Phase::Done,
            share::Event::Error(error) => {
                self.error = error.clone();
                Phase::Failed
            }
        };
        if phase != self.phase {
            self.phase = phase;
            self.since = now;
        }
    }
    pub(super) fn handle(&mut self, action: Action, now: f64) -> Option<ShareAction> {
        if action == Action::Back {
            return Some(ShareAction::Exit);
        }
        if !self.send && self.phase == Phase::Done {
            match action {
                Action::Left | Action::Right => self.button = 1 - self.button,
                Action::Select if self.button == 1 => *self = Self::receive(now),
                Action::Select if !self.paths.is_empty() => {
                    return Some(ShareAction::Play(self.paths.clone()))
                }
                _ => {}
            }
            return None;
        }
        if !self.send
            && self.phase == Phase::Failed
            && matches!(
                action,
                Action::Up | Action::Down | Action::Left | Action::Right
            )
        {
            self.phase = Phase::Code;
            self.error.clear();
            self.since = now;
        }
        if self.send || !matches!(self.phase, Phase::Code | Phase::Failed) {
            return None;
        }
        match action {
            Action::Up => self.code[self.cursor] = (self.code[self.cursor] + 1) % 10,
            Action::Down => self.code[self.cursor] = (self.code[self.cursor] + 9) % 10,
            Action::Left => self.cursor = self.cursor.saturating_sub(1),
            Action::Right => self.cursor = (self.cursor + 1).min(7),
            Action::Select | Action::Start => {
                let code = self
                    .code
                    .iter()
                    .map(|n| char::from(b'0' + *n))
                    .collect::<String>();
                if code == "00000000" || code == "31415926" {
                    self.egg = if code == "00000000" {
                        "Nothing comes from nowhere."
                    } else {
                        "3.14159265358979323846… and so on, forever."
                    }
                    .into();
                    self.egg_since = now;
                    return None;
                }
                self.egg.clear();
                self.error.clear();
                self.phase = Phase::Connecting;
                self.since = now;
                return Some(ShareAction::Receive(code));
            }
            _ => {}
        }
        if !matches!(action, Action::Select | Action::Start) {
            self.egg.clear();
        }
        None
    }
    pub(super) fn draw(
        &mut self,
        r: &mut Renderer,
        assets: &VisualAssets,
        art: &mut crate::art::Artwork,
        now: f64,
    ) {
        let dt = (now - self.last_time).clamp(0., 0.25);
        self.last_time = now;
        let progress = if self.total > 0 {
            (self.done as f64 / self.total as f64).clamp(0., 1.)
        } else {
            0.
        };
        if self.phase == Phase::Moving {
            self.laser += (progress as f32 - self.laser).max(0.) * (1. - (-dt / 0.6).exp()) as f32;
        } else {
            self.laser = if matches!(self.phase, Phase::Done | Phase::Saving) {
                1.
            } else {
                0.
            };
        }
        if now - self.rate_time >= 0.25 {
            let rate = self.done.saturating_sub(self.rate_bytes) as f64
                / (now - self.rate_time).max(0.001);
            self.rate = if self.rate == 0. {
                rate
            } else {
                self.rate * 0.75 + rate * 0.25
            };
            self.rate_bytes = self.done;
            self.rate_time = now;
        }
        r.push_transform(0., 40., 1., 1., 1.);
        r.push_clip(0., 0., 640., 440.);
        r.gradient(0., 0., 640., 332., rgb(250, 251, 252), rgb(214, 219, 226));
        r.gradient(0., 332., 640., 108., rgb(236, 239, 243), rgb(250, 251, 252));
        r.rect(0., 332., 640., 1., rgb(200, 205, 212));
        let cover = if self.send {
            Some(art.get(self.cover.as_deref()))
        } else {
            self.cover.as_deref().map(|path| art.get(Some(path)))
        };
        if !self.send
            && matches!(self.phase, Phase::Code | Phase::Connecting | Phase::Failed)
            && self.info.album.is_empty()
        {
            pedestal(
                r,
                assets,
                320.,
                "green",
                if self.phase == Phase::Connecting {
                    0.55 + 0.45 * (0.5 + 0.5 * (now * std::f64::consts::TAU / 0.9).sin()) as f32
                } else {
                    0.3
                },
            );
            text(
                r,
                "Type the code shown on your friend's betterPod",
                320.,
                40.,
                15.,
                500,
                rgb(70, 76, 88),
            );
            for (i, digit) in self.code.iter().enumerate() {
                let x = 70. + i as f32 * 60. + if i >= 4 { 20. } else { 0. };
                let selected = self.phase == Phase::Code && i == self.cursor;
                if selected {
                    text(
                        r,
                        &((digit + 1) % 10).to_string(),
                        x + 24.,
                        74.,
                        17.,
                        600,
                        rgb(170, 176, 186),
                    );
                    text(
                        r,
                        &((digit + 9) % 10).to_string(),
                        x + 24.,
                        186.,
                        17.,
                        600,
                        rgb(170, 176, 186),
                    );
                }
                digit_tile(r, x, 92., 48., 76., &digit.to_string(), selected);
            }
            let line = if !self.egg.is_empty() && now - self.egg_since < 4. {
                self.egg.as_str()
            } else if self.phase == Phase::Connecting {
                "Looking for your friend's betterPod…"
            } else if self.phase == Phase::Failed {
                self.error.as_str()
            } else {
                "[↑] [↓] digit      [←] [→] move      [A] receive"
            };
            text(
                r,
                line,
                320.,
                216.,
                if self.phase == Phase::Code { 14. } else { 15. },
                500,
                rgb(110, 118, 130),
            );
        } else if !self.send && self.phase == Phase::Done {
            self.draw_received(r, assets, cover, now);
        } else {
            let color = if self.send { "blue" } else { "green" };
            let beam = if matches!(self.phase, Phase::Waiting | Phase::Preparing) {
                0.55 + 0.45 * (0.5 + 0.5 * (now * std::f64::consts::TAU / 5.).sin()) as f32
            } else if self.send && self.phase == Phase::Done {
                0.5
            } else {
                1.
            };
            pedestal(r, assets, 180., color, beam);
            if let Some(texture) = cover {
                let (x, y, alpha) = self.cover_pose(now);
                let cut = if self.phase == Phase::Moving {
                    self.laser * 150.
                } else {
                    if self.send {
                        0.
                    } else {
                        150.
                    }
                };
                let (y0, y1) = if self.send { (cut, 150.) } else { (0., cut) };
                if y1 > y0 {
                    for i in (1..=6).rev() {
                        let i = i as f32;
                        let shadow = [40. / 255., 50. / 255., 70. / 255., 0.05 * alpha];
                        round(
                            r,
                            x + 4. - i * 1.5,
                            y + y0 + 6. - i,
                            142. + i * 3.,
                            y1 - y0 - 2. + i * 2.,
                            i * 2.,
                            shadow,
                            shadow,
                        );
                    }
                    r.image_uv(
                        texture,
                        [x, y + y0, 150., y1 - y0],
                        [0., y0 / 150., 1., y1 / 150.],
                        alpha,
                    );
                    if !matches!(self.phase, Phase::Moving | Phase::Done) {
                        let gloss = [1., 1., 1., 0.15 * alpha];
                        r.triangle([[x, y], [x + 150., y], [x + 150., y + 42.]], gloss);
                        r.triangle([[x, y], [x + 150., y + 42.], [x, y + 69.]], gloss);
                    }
                }
                if self.phase == Phase::Moving {
                    laser(
                        r,
                        x,
                        x + 150.,
                        y + cut,
                        if self.send {
                            rgb(40, 150, 255)
                        } else {
                            rgb(40, 200, 110)
                        },
                    );
                }
            }
            let title = if self.info.title.is_empty() {
                &self.info.album
            } else {
                &self.info.title
            };
            let sub = format!(
                "{}{}{}",
                self.info.artist,
                if self.info.songs > 1 { " · " } else { "" },
                if self.info.songs > 1 {
                    format!("{} songs", self.info.songs)
                } else {
                    String::new()
                }
            );
            if self.send {
                text(
                    r,
                    match self.phase {
                        Phase::Moving => "SHARING",
                        Phase::Done => "SENT",
                        Phase::Failed => "NOT SENT",
                        _ => "YOUR CODE",
                    },
                    468.,
                    52.,
                    15.,
                    600,
                    match self.phase {
                        Phase::Moving => rgb(40, 150, 255),
                        Phase::Done => rgb(40, 150, 70),
                        Phase::Failed => rgb(200, 60, 50),
                        _ => rgb(110, 118, 130),
                    },
                );
                for (i, digit) in self.code.iter().enumerate() {
                    digit_tile(
                        r,
                        332. + i as f32 * 33. + if i >= 4 { 10. } else { 0. },
                        66.,
                        30.,
                        46.,
                        &digit.to_string(),
                        false,
                    );
                }
                marquee(
                    r,
                    title,
                    468.,
                    150.,
                    20.,
                    700,
                    rgb(40, 44, 52),
                    270.,
                    self.since,
                    now,
                );
                marquee(
                    r,
                    &sub,
                    468.,
                    174.,
                    15.,
                    500,
                    rgb(110, 118, 130),
                    270.,
                    self.since,
                    now,
                );
                match self.phase {
                    Phase::Preparing => {
                        text(r, "Getting ready…", 468., 222., 15., 500, rgb(70, 76, 88))
                    }
                    Phase::Waiting => {
                        text(
                            r,
                            "Waiting for a friend…",
                            468.,
                            214.,
                            17.,
                            600,
                            rgb(70, 76, 88),
                        );
                        text(
                            r,
                            "On their betterPod: Music → Receive,",
                            468.,
                            240.,
                            14.,
                            500,
                            rgb(110, 118, 130),
                        );
                        text(
                            r,
                            "and these eight digits",
                            468.,
                            260.,
                            14.,
                            500,
                            rgb(110, 118, 130),
                        );
                    }
                    Phase::Moving => {
                        text(
                            r,
                            &if self.peer.is_empty() {
                                "Sending…".into()
                            } else {
                                format!("Sending to {}", self.peer)
                            },
                            468.,
                            222.,
                            15.,
                            500,
                            rgb(70, 76, 88),
                        );
                        aqua_bar(r, 360., 240., 216., progress as f32, false);
                        text(
                            r,
                            &format!(
                                "{}% · {}/s",
                                (progress * 100.) as u64,
                                visual::bytes_label(self.rate as u64)
                            ),
                            468.,
                            268.,
                            15.,
                            500,
                            rgb(110, 118, 130),
                        );
                    }
                    Phase::Done => text(
                        r,
                        "Sent! It's on their betterPod now.",
                        468.,
                        222.,
                        15.,
                        600,
                        rgb(70, 76, 88),
                    ),
                    Phase::Failed => text(r, &self.error, 468., 222., 14., 500, rgb(200, 60, 50)),
                    _ => {}
                }
            } else {
                text(
                    r,
                    if self.phase == Phase::Saving {
                        "SAVING"
                    } else if self.phase == Phase::Failed {
                        "STOPPED"
                    } else {
                        "RECEIVING"
                    },
                    468.,
                    62.,
                    15.,
                    600,
                    rgb(40, 150, 70),
                );
                marquee(
                    r,
                    title,
                    468.,
                    104.,
                    20.,
                    700,
                    rgb(40, 44, 52),
                    270.,
                    self.since,
                    now,
                );
                marquee(
                    r,
                    &sub,
                    468.,
                    128.,
                    15.,
                    500,
                    rgb(110, 118, 130),
                    270.,
                    self.since,
                    now,
                );
                text(
                    r,
                    if self.phase == Phase::Saving {
                        "Putting it in your Music…"
                    } else if self.phase == Phase::Failed {
                        &self.error
                    } else {
                        "Receiving from a friend's betterPod"
                    },
                    468.,
                    172.,
                    15.,
                    500,
                    rgb(70, 76, 88),
                );
                if self.phase == Phase::Moving {
                    aqua_bar(r, 360., 190., 216., progress as f32, true);
                    text(
                        r,
                        &format!(
                            "{}% · {}/s",
                            (progress * 100.) as u64,
                            visual::bytes_label(self.rate as u64)
                        ),
                        468.,
                        218.,
                        15.,
                        500,
                        rgb(110, 118, 130),
                    );
                }
            }
        }
        let hint = if self.send {
            if matches!(self.phase, Phase::Done | Phase::Failed) {
                "[B] back"
            } else {
                "[B] stop sharing"
            }
        } else {
            match self.phase {
                Phase::Code => "",
                Phase::Done => "[←] [→] choose     [A] select     [B] back",
                Phase::Failed => "[A] try again     [B] back",
                _ => "[B] cancel",
            }
        };
        if !hint.is_empty() {
            text(r, hint, 320., 418., 15., 600, rgb(110, 118, 130));
        }
        r.pop_clip();
        r.pop_transform();
    }
    fn cover_pose(&self, now: f64) -> (f32, f32, f32) {
        let elapsed = (now - self.since).max(0.);
        if self.send && self.phase == Phase::Done {
            // The sent cover returns to the same pedestal over Go's 0.6 s.
            let t = (elapsed / 0.6).clamp(0., 1.) as f32;
            let ease = super::super::app_visual_menu::go_ease_in_out(t);
            return (105., 118. - 20. * (1. - ease), ease);
        }
        if self.send && matches!(self.phase, Phase::Preparing | Phase::Waiting) {
            let phase = now * std::f64::consts::TAU / 5.;
            return (
                105. + (phase * 0.5).sin() as f32 * 2.,
                118. + phase.sin() as f32 * 4.,
                1.,
            );
        }
        (105., 118., 1.)
    }
    fn draw_received(
        &self,
        r: &mut Renderer,
        assets: &VisualAssets,
        cover: Option<Texture>,
        now: f64,
    ) {
        let ease = |a: f64, b: f64| {
            let k = ((now - self.since - a) / (b - a)).clamp(0., 1.);
            (k * k * (3. - 2. * k)) as f32
        };
        let pg = ease(0., 0.6);
        if pg < 1. {
            r.push_transform(0., pg * 40., 1., 1., 1. - pg);
            pedestal(r, assets, 180., "green", 1.);
            r.pop_transform();
        }
        let movement = ease(0.25, 1.25);
        let size = 150. + 50. * movement;
        let cx = 180. + 140. * movement;
        let cy = 193. - 31. * movement;
        let turn = (movement * std::f32::consts::TAU).cos();
        let width = (size * turn.abs()).max(2.);
        if let Some(cover) = cover {
            if turn > 0. {
                r.image(cover, cx - width / 2., cy - size / 2., width, size, 1.);
            } else {
                r.gradient(
                    cx - width / 2.,
                    cy - size / 2.,
                    width,
                    size,
                    rgb(250, 251, 253),
                    rgb(220, 225, 232),
                );
            }
        }
        let alpha = ease(1.1, 1.6);
        if alpha <= 0. {
            return;
        }
        r.push_transform(0., 0., 1., 1., alpha);
        text(r, "RECEIVED", 320., 40., 15., 600, rgb(40, 150, 70));
        marquee(
            r,
            if self.info.title.is_empty() {
                &self.info.album
            } else {
                &self.info.title
            },
            320.,
            288.,
            22.,
            700,
            rgb(40, 44, 52),
            580.,
            self.since,
            now,
        );
        marquee(
            r,
            &self.info.artist,
            320.,
            312.,
            15.,
            500,
            rgb(110, 118, 130),
            580.,
            self.since,
            now,
        );
        button(
            r,
            216.,
            366.,
            190.,
            if self.paths.is_empty() {
                "Adding…"
            } else {
                "Listen Now"
            },
            self.button == 0,
        );
        button(r, 424., 366., 190., "Receive Another", self.button == 1);
        r.pop_transform();
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn text(r: &mut Renderer, value: &str, cx: f32, cy: f32, size: f32, weight: u16, color: Color) {
    let max_width = if cx == 468. { 280. } else { 600. };
    let mut width = r.text_width_weighted(value, size, weight);
    let fitted;
    let value = if width > max_width {
        fitted = super::super::app_visual_menu::ellipsize(r, value, size, weight, max_width);
        width = r.text_width_weighted(&fitted, size, weight);
        fitted.as_str()
    } else {
        value
    };
    r.text_baseline(
        value,
        cx - width / 2.,
        cy + size * 0.727 / 2.,
        size,
        weight,
        color,
        width + 1.,
    );
}
fn marquee(
    r: &mut Renderer,
    value: &str,
    cx: f32,
    cy: f32,
    size: f32,
    weight: u16,
    color: Color,
    width: f32,
    since: f64,
    now: f64,
) {
    if r.text_width_weighted(value, size, weight) <= width {
        text(r, value, cx, cy, size, weight, color);
        return;
    }
    r.push_clip(cx - width / 2., cy - size, width, size * 2.);
    super::super::app_visual_menu::marquee(
        r,
        value,
        cx - width / 2.,
        cy + size * 0.727 / 2.,
        size,
        weight,
        color,
        width,
        since,
        now,
        0.,
    );
    r.pop_clip();
}
fn round(r: &mut Renderer, x: f32, y: f32, w: f32, h: f32, radius: f32, top: Color, bottom: Color) {
    rounded_gradient(r, x, y, w, h, radius, &[(0., top), (1., bottom)]);
}
fn pedestal(r: &mut Renderer, assets: &VisualAssets, cx: f32, color: &str, beam: f32) {
    if let Some(texture) = assets.get(&format!("share_beam_{color}")) {
        r.image(texture, cx - 130., 66., 260., texture.height as f32, beam);
    }
    if let Some(texture) = assets.get(&format!("share_ped_{color}")) {
        r.image(texture, cx - 130., 268., 260., texture.height as f32, 1.);
    }
}
fn digit_tile(r: &mut Renderer, x: f32, y: f32, w: f32, h: f32, value: &str, selected: bool) {
    let (top, bottom, color) = if selected {
        (rgb(120, 190, 255), rgb(28, 112, 218), WHITE)
    } else {
        (WHITE, rgb(222, 227, 234), rgb(50, 56, 66))
    };
    round(
        r,
        x,
        y + 2.,
        w,
        h,
        8.,
        [40. / 255., 50. / 255., 70. / 255., 0.12],
        [40. / 255., 50. / 255., 70. / 255., 0.12],
    );
    let edge = rgb(150, 158, 170);
    round(r, x, y, w, h, 7., edge, edge);
    round(r, x + 1., y + 1., w - 2., h - 2., 6., top, bottom);
    text(r, value, x + w / 2., y + h / 2., h * 0.6, 600, color);
    round(
        r,
        x + 1.,
        y + 1.,
        w - 2.,
        h / 2. - 1.,
        6.,
        [1., 1., 1., if selected { 0.35 } else { 0.45 }],
        [1., 1., 1., if selected { 0.21 } else { 0.27 }],
    );
    r.rect(x, y + h / 2., 3., 1., rgb(120, 128, 140));
    r.rect(x + w - 3., y + h / 2., 3., 1., rgb(120, 128, 140));
}
fn aqua_bar(r: &mut Renderer, x: f32, y: f32, w: f32, p: f32, green: bool) {
    round(r, x, y, w, 12., 6., rgb(196, 201, 208), rgb(232, 235, 240));
    let (top, bottom) = if green {
        (rgb(140, 230, 160), rgb(30, 150, 70))
    } else {
        (rgb(120, 200, 255), rgb(30, 110, 220))
    };
    let width = (w * p).max(12.);
    round(r, x, y, width, 12., 6., top, bottom);
    round(
        r,
        x + 1.,
        y + 1.,
        width - 2.,
        5.,
        3.,
        [1., 1., 1., 0.5],
        [1., 1., 1., 0.2],
    );
}
fn button(r: &mut Renderer, cx: f32, cy: f32, w: f32, value: &str, selected: bool) {
    let (top, bottom, color) = if selected {
        (rgb(120, 195, 255), rgb(26, 104, 214), WHITE)
    } else {
        (WHITE, rgb(218, 223, 230), rgb(40, 44, 52))
    };
    round(r, cx - w / 2., cy - 19., w, 38., 19., top, bottom);
    round(
        r,
        cx - w / 2. + 3.,
        cy - 17.,
        w - 6.,
        17.,
        10.,
        [1., 1., 1., 0.5],
        [1., 1., 1., 0.15],
    );
    text(r, value, cx, cy, 16., 700, color);
}
fn laser(r: &mut Renderer, x0: f32, x1: f32, y: f32, color: Color) {
    for (h, a) in [(9., 0.12), (5., 0.25), (3., 0.9)] {
        let mut c = color;
        c[3] = a;
        round(r, x0 - 12., y - h / 2., x1 - x0 + 24., h, h / 2., c, c);
    }
    r.rect(x0 - 6., y - 0.5, x1 - x0 + 12., 1., WHITE);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_digit_editor_wraps_and_avoids_void_network() {
        let mut s = ShareVisual::receive(0.);
        assert!(s.handle(Action::Select, 0.).is_none());
        assert_eq!(s.egg, "Nothing comes from nowhere.");
        s.handle(Action::Down, 0.);
        assert_eq!(s.code[0], 9);
        s.handle(Action::Right, 0.);
        s.handle(Action::Up, 0.);
        assert_eq!(s.code[1], 1);
        assert!(
            matches!(s.handle(Action::Start,0.),Some(ShareAction::Receive(code))if code=="91000000")
        );
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    #[test]
    fn sender_code_precedes_readiness_and_cover_returns_after_sending() {
        let mut view = ShareVisual::send(ShareInfo::default(), None, 0.);
        view.event(&share::Event::Code("12345678".into()), 0.01);
        assert!(matches!(view.phase, Phase::Preparing));
        assert_eq!(view.code, [1, 2, 3, 4, 5, 6, 7, 8]);
        view.event(
            &share::Event::Progress {
                done: 0,
                total: 0,
                phase: "Waiting for receiver".into(),
                peer: None,
            },
            1.,
        );
        let first = view.cover_pose(1.);
        let later = view.cover_pose(2.);
        assert_ne!(first, later);
        assert!((first.1 - later.1).abs() < 8.);
        view.event(&share::Event::Sent, 3.);
        assert_eq!(view.cover_pose(3.).2, 0.);
        let halfway = view.cover_pose(3.3);
        assert!(halfway.1 > 98. && halfway.1 < 118.);
        assert!(halfway.2 > 0. && halfway.2 < 1.);
        assert_eq!(view.cover_pose(3.7), (105., 118., 1.));
    }
}
