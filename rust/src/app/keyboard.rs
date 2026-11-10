//! The Go 1.1 four-row QWERTY keyboard and rune-based text editor.
//! A caller owns the query and reacts to typed effects; drawing has no I/O.
use super::{Action, Color, Renderer, VisualAssets};
use betterpod_rust::motion::{Anim, Ease};

pub(super) const PANEL_HEIGHT: f32 = 242.0;
const KEY_HEIGHT: f32 = 50.0;
const GAP: f32 = 6.0;
const PAD: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyboardEffect {
    None,
    Changed,
    Submit,
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyAction {
    Character(char),
    Space,
    Delete,
    Layer,
    Enter,
    Shift,
}
#[derive(Clone, Copy, Debug)]
struct Key {
    action: KeyAction,
    width: f32,
}
#[derive(Clone)]
pub(super) struct Keyboard {
    pub query: String,
    pub search: bool,
    /// Position in Unicode scalar values, exactly as Go's []rune editor.
    pub caret: usize,
    layer: usize,
    shift: u8,
    row: usize,
    col: usize,
    animation: Anim,
    started: bool,
    exiting: bool,
    now: f64,
}
impl Keyboard {
    pub fn new(query: impl Into<String>) -> Self {
        let query = query.into();
        let caret = query.chars().count();
        Self {
            query,
            search: true,
            caret,
            layer: 0,
            shift: 0,
            row: 1,
            col: 4,
            animation: Anim::new(0.0),
            started: false,
            exiting: false,
            now: 0.0,
        }
    }
    pub fn show(&mut self, now: f64) {
        self.now = now;
        self.started = true;
        self.exiting = false;
        self.animation.set(1.0, 0.260, Ease::Css, 0.0, now);
    }
    pub fn hide(&mut self, now: f64) {
        if !self.exiting {
            self.now = now;
            self.exiting = true;
            self.animation.set(0.0, 0.220, Ease::InOut, 0.0, now);
        }
    }
    pub fn is_exiting(&self) -> bool {
        self.exiting
    }
    pub fn finished(&self, now: f64) -> bool {
        self.exiting && !self.animation.running(now)
    }
    pub fn snap_visible(&mut self, now: f64) {
        self.now = now;
        self.started = true;
        self.exiting = false;
        self.animation.snap(1.0);
    }
    pub fn height(&mut self, now: f64) -> f32 {
        PANEL_HEIGHT * self.animation.value(now) as f32
    }
    pub fn with_caret(&self) -> String {
        if self.exiting {
            return self.query.clone();
        }
        let byte = rune_byte(&self.query, self.caret);
        let mut text = String::with_capacity(self.query.len() + 1);
        text.push_str(&self.query[..byte]);
        text.push('|');
        text.push_str(&self.query[byte..]);
        text
    }
    pub fn fit_with_caret(
        &self,
        r: &mut Renderer,
        size: f32,
        weight: u16,
        max_width: f32,
    ) -> String {
        let text = self.with_caret();
        if r.text_width_weighted(&text, size, weight) <= max_width {
            return text;
        }
        let Some(caret) = text.rfind('|') else {
            return super::visual_menu::ellipsize(r, &text, size, weight, max_width);
        };
        if r.text_width_weighted(&format!("{}…", &text[..caret + 1]), size, weight) <= max_width {
            return super::visual_menu::ellipsize(r, &text, size, weight, max_width);
        }
        let tail = if caret + 1 < text.len() { "…" } else { "" };
        let mut from = 0;
        while from < caret
            && r.text_width_weighted(
                &format!("…{}{}", &text[from..caret + 1], tail),
                size,
                weight,
            ) > max_width
        {
            from += text[from..].chars().next().unwrap().len_utf8();
        }
        super::visual_menu::ellipsize(r, &format!("…{}", &text[from..]), size, weight, max_width)
    }

    pub fn handle(&mut self, action: Action) -> KeyboardEffect {
        if self.exiting {
            return KeyboardEffect::None;
        }
        match action {
            Action::Left => self.move_key(-1, 0),
            Action::Right => self.move_key(1, 0),
            Action::Up => self.move_key(0, -1),
            Action::Down => self.move_key(0, 1),
            Action::PlayPause => return self.insert(" "),
            Action::Options => self.toggle_layer(),
            Action::Previous => return self.move_caret(-1),
            Action::Next => return self.move_caret(1),
            Action::Back => return self.backspace(),
            Action::BackHeld => {
                self.hide(self.now);
                return KeyboardEffect::Close;
            }
            Action::Start => {
                self.hide(self.now);
                return KeyboardEffect::Submit;
            }
            Action::Select => return self.press(),
            _ => {}
        }
        KeyboardEffect::None
    }
    /// Optional click-wheel routing, separate from L/R caret movement.
    pub fn step(&mut self, direction: i32) {
        let rows = key_rows(self.layer);
        let next = self.col as i32 + direction;
        if next >= rows[self.row].len() as i32 {
            self.row = (self.row + 1) % rows.len();
            self.col = 0;
        } else if next < 0 {
            self.row = (self.row + rows.len() - 1) % rows.len();
            self.col = rows[self.row].len() - 1;
        } else {
            self.col = next as usize;
        }
    }
    fn move_key(&mut self, dx: i32, dy: i32) {
        let rows = key_rows(self.layer);
        if dx != 0 {
            self.col = (self.col as i32 + dx).rem_euclid(rows[self.row].len() as i32) as usize;
            return;
        }
        let next_row = self.row as i32 + dy;
        if !(0..rows.len() as i32).contains(&next_row) {
            return;
        }
        let xs = key_positions(&rows[self.row]);
        let center = xs[self.col] + rows[self.row][self.col].width * 0.5;
        let next_row = next_row as usize;
        let next_x = key_positions(&rows[next_row]);
        self.col = rows[next_row]
            .iter()
            .enumerate()
            .min_by(|(a, ka), (b, kb)| {
                (next_x[*a] + ka.width * 0.5 - center)
                    .abs()
                    .total_cmp(&(next_x[*b] + kb.width * 0.5 - center).abs())
            })
            .map_or(0, |(i, _)| i);
        self.row = next_row;
    }
    fn toggle_layer(&mut self) {
        self.layer = 1 - self.layer;
        self.col = self.col.min(key_rows(self.layer)[self.row].len() - 1);
    }
    fn insert(&mut self, text: &str) -> KeyboardEffect {
        self.caret = self.caret.min(self.query.chars().count());
        let byte = rune_byte(&self.query, self.caret);
        self.query.insert_str(byte, text);
        self.caret += text.chars().count();
        KeyboardEffect::Changed
    }
    fn backspace(&mut self) -> KeyboardEffect {
        self.caret = self.caret.min(self.query.chars().count());
        if self.caret == 0 {
            return KeyboardEffect::None;
        }
        let start = rune_byte(&self.query, self.caret - 1);
        let end = rune_byte(&self.query, self.caret);
        self.query.replace_range(start..end, "");
        self.caret -= 1;
        KeyboardEffect::Changed
    }
    fn move_caret(&mut self, direction: i32) -> KeyboardEffect {
        self.caret = (self.caret as i64 + direction as i64)
            .clamp(0, self.query.chars().count() as i64) as usize;
        KeyboardEffect::Changed
    }
    fn press(&mut self) -> KeyboardEffect {
        let key = key_rows(self.layer)[self.row][self.col];
        match key.action {
            KeyAction::Character(mut c) => {
                if self.layer == 0 && self.shift > 0 {
                    c = c.to_ascii_uppercase();
                    if self.shift == 1 {
                        self.shift = 0;
                    }
                }
                self.insert(&c.to_string())
            }
            KeyAction::Space => self.insert(" "),
            KeyAction::Delete => self.backspace(),
            KeyAction::Layer => {
                self.toggle_layer();
                KeyboardEffect::None
            }
            KeyAction::Shift => {
                self.shift = (self.shift + 1) % 3;
                KeyboardEffect::None
            }
            KeyAction::Enter => {
                self.hide(self.now);
                KeyboardEffect::Submit
            }
        }
    }

    pub fn draw(&mut self, r: &mut Renderer, _assets: &VisualAssets, _dark: bool, now: f64) {
        if !self.started {
            self.show(now);
        }
        self.now = now;
        let shown = self.animation.value(now) as f32;
        if shown <= 0.001 {
            return;
        }
        let y = 480.0 - PANEL_HEIGHT * shown;
        // The original sigma=4 px profile is 12 pixels long, drawn upwards.
        for i in 0..12 {
            let a = 0.5 * erfc((i as f32 + 0.5) / (std::f32::consts::SQRT_2 * 4.0));
            r.rect(
                0.0,
                y.round() - 1.0 - i as f32,
                640.0,
                1.0,
                [0.0, 0.0, 0.0, a * 0.6 * shown],
            );
        }
        r.push_clip(0.0, y, 640.0, PANEL_HEIGHT);
        r.gradient(
            0.0,
            y,
            640.0,
            PANEL_HEIGHT,
            rgb(74, 78, 85),
            rgb(28, 30, 34),
        );
        r.rect(0.0, y, 640.0, 2.0, rgb(18, 19, 22));
        r.rect(0.0, y + 2.0, 640.0, 1.0, [1.0, 1.0, 1.0, 0.1]);
        for (row_index, row) in key_rows(self.layer).iter().enumerate() {
            let xs = key_positions(row);
            let key_y = y + PAD + row_index as f32 * (KEY_HEIGHT + GAP);
            for (column, key) in row.iter().enumerate() {
                let x = xs[column];
                let selected = row_index == self.row && column == self.col;
                let character = matches!(key.action, KeyAction::Character(_));
                let active_shift = key.action == KeyAction::Shift && self.shift > 0;
                let (top, bottom, border, text) = if selected {
                    (
                        rgb(60, 184, 255),
                        rgb(52, 122, 181),
                        rgb(36, 98, 156),
                        rgb(255, 255, 255),
                    )
                } else if character || active_shift {
                    (
                        rgb(255, 255, 255),
                        rgb(212, 214, 218),
                        rgb(118, 122, 128),
                        rgb(20, 21, 24),
                    )
                } else {
                    (
                        rgb(150, 155, 162),
                        rgb(104, 109, 117),
                        rgb(70, 74, 80),
                        rgb(255, 255, 255),
                    )
                };
                r.rounded_rect(
                    x,
                    key_y + 2.0,
                    key.width,
                    KEY_HEIGHT,
                    10.0,
                    [0.0, 0.0, 0.0, 0.35],
                );
                rounded_gradient(r, x, key_y, key.width, KEY_HEIGHT, 10.0, top, bottom);
                stroke_rounded(r, x, key_y, key.width, KEY_HEIGHT, 10.0, 1.5, border);
                let center_y = key_y + KEY_HEIGHT * 0.5;
                if key.action == KeyAction::Shift {
                    draw_shift(r, x + key.width * 0.5, center_y, self.shift, text);
                    continue;
                }
                let label = match key.action {
                    KeyAction::Character(c) if self.shift > 0 || self.layer != 0 => {
                        c.to_ascii_uppercase().to_string()
                    }
                    KeyAction::Character(c) => c.to_string(),
                    KeyAction::Space => "space".into(),
                    KeyAction::Delete => "⌫".into(),
                    KeyAction::Layer => if self.layer == 0 { "123" } else { "ABC" }.into(),
                    KeyAction::Enter if !self.search => "Enter".into(),
                    _ => String::new(),
                };
                let size = if character || key.action == KeyAction::Delete {
                    28.0
                } else {
                    22.0
                };
                let label_width = if key.action == KeyAction::Enter && self.search {
                    26.0
                } else {
                    r.text_width_weighted(&label, size, 600)
                };
                let badge = match key.action {
                    KeyAction::Space => "X",
                    KeyAction::Delete => "B",
                    KeyAction::Layer => "Y",
                    KeyAction::Enter => "START",
                    _ => "",
                };
                let badge_width = if badge.is_empty() {
                    0.0
                } else {
                    badge_size(r, badge) + 8.0
                };
                let label_x = x + (key.width - label_width - badge_width) * 0.5;
                if key.action == KeyAction::Enter && self.search {
                    draw_magnifier(r, label_x, center_y, 26.0, text);
                } else {
                    r.text_baseline(
                        &label,
                        label_x,
                        center_y + size * 0.727 * 0.5,
                        size,
                        600,
                        text,
                        key.width,
                    );
                }
                if !badge.is_empty() {
                    draw_badge(r, badge, label_x + label_width + 8.0, center_y, selected);
                }
            }
        }
        r.pop_clip();
    }
}

fn rune_byte(text: &str, rune: usize) -> usize {
    text.char_indices()
        .nth(rune)
        .map_or(text.len(), |(byte, _)| byte)
}
fn character_row(text: &str) -> Vec<Key> {
    text.chars()
        .map(|c| Key {
            action: KeyAction::Character(c),
            width: 56.0,
        })
        .collect()
}
fn key_rows(layer: usize) -> [Vec<Key>; 4] {
    let bottom = vec![
        Key {
            action: KeyAction::Layer,
            width: 96.0,
        },
        Key {
            action: KeyAction::Space,
            width: 280.0,
        },
        Key {
            action: KeyAction::Delete,
            width: 92.0,
        },
        Key {
            action: KeyAction::Enter,
            width: 128.0,
        },
    ];
    if layer == 0 {
        let mut third = vec![Key {
            action: KeyAction::Shift,
            width: 76.0,
        }];
        third.extend(character_row("zxcvbnm"));
        [
            character_row("qwertyuiop"),
            character_row("asdfghjkl"),
            third,
            bottom,
        ]
    } else {
        [
            character_row("1234567890"),
            character_row("-._'&!?()"),
            character_row("@#:,/+="),
            bottom,
        ]
    }
}
fn key_positions(row: &[Key]) -> Vec<f32> {
    let total: f32 = row.iter().map(|key| key.width + GAP).sum::<f32>() - GAP;
    let mut x = (640.0 - total) * 0.5;
    row.iter()
        .map(|key| {
            let at = x;
            x += key.width + GAP;
            at
        })
        .collect()
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
fn color_mix(a: Color, b: Color, t: f32) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn rounded_inset(y: f32, height: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    let dy = if y < radius {
        radius - y
    } else if y > height - radius {
        y - (height - radius)
    } else {
        0.0
    };
    if dy == 0.0 {
        0.0
    } else {
        radius - (radius * radius - dy * dy).max(0.0).sqrt()
    }
}
/// GPU scanline primitives retain the original rounded gradient profile. This
/// produces vertices, never a CPU framebuffer or an uploaded frame texture.
#[allow(clippy::too_many_arguments)]
fn rounded_gradient(
    r: &mut Renderer,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    top: Color,
    bottom: Color,
) {
    r.rounded_gradient(x, y, width, height, radius, &[(0.0, top), (1.0, bottom)]);
}

#[allow(clippy::too_many_arguments)]
fn stroke_rounded(
    r: &mut Renderer,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    border: f32,
    color: Color,
) {
    r.stroke_rounded(
        x + border * 0.5,
        y + border * 0.5,
        width - border,
        height - border,
        (radius - border * 0.5).max(0.0),
        border,
        color,
    );
}

fn badge_size(r: &mut Renderer, badge: &str) -> f32 {
    if badge.len() == 1 {
        24.0
    } else {
        r.text_width_weighted(badge, 14.0, 700) + 14.0
    }
}
fn draw_badge(r: &mut Renderer, badge: &str, x: f32, cy: f32, selected: bool) {
    let width = badge_size(r, badge);
    let radius = if badge.len() == 1 { 6.0 } else { 12.0 };
    r.rounded_rect(x, cy - 12.0, width, 24.0, radius, [1.0, 1.0, 1.0, 0.92]);
    let size = if badge.len() == 1 { 18.0 } else { 14.0 };
    let tw = r.text_width_weighted(badge, size, 700);
    r.text_baseline(
        badge,
        x + (width - tw) * 0.5,
        cy + size * 0.727 * 0.5,
        size,
        700,
        if selected {
            rgb(36, 98, 156)
        } else {
            rgb(70, 74, 80)
        },
        width,
    );
}
pub(super) fn draw_magnifier(r: &mut Renderer, x: f32, cy: f32, size: f32, color: Color) {
    let radius = size * 0.34;
    let cx = x + radius + 2.0;
    let center_y = cy - size * 0.12;
    let mut previous = None;
    for i in 0..=48 {
        let angle = i as f32 / 48.0 * std::f32::consts::TAU;
        let p = (cx + radius * angle.cos(), center_y + radius * angle.sin());
        if let Some((px, py)) = previous {
            r.line(px, py, p.0, p.1, 3.8, color);
        }
        previous = Some(p);
    }
    let angle = std::f32::consts::FRAC_PI_4;
    r.line(
        cx + radius * angle.cos() + 1.2,
        center_y + radius * angle.sin() + 1.2,
        x + size - 2.0,
        cy + size * 0.5 - 2.0,
        4.8,
        color,
    );
}
fn draw_shift(r: &mut Renderer, cx: f32, cy: f32, state: u8, color: Color) {
    let top = cy - 13.0 - if state == 2 { 3.0 } else { 0.0 };
    let points = [
        [cx, top],
        [cx + 12.0, top + 12.0],
        [cx + 6.0, top + 12.0],
        [cx + 6.0, top + 22.0],
        [cx - 6.0, top + 22.0],
        [cx - 6.0, top + 12.0],
        [cx - 12.0, top + 12.0],
    ];
    if state == 0 {
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            r.line(a[0], a[1], b[0], b[1], 1.6, color);
        }
    } else {
        r.triangle([points[0], points[1], points[6]], color);
        r.rect(cx - 6.0, top + 12.0, 12.0, 10.0, color);
    }
    if state == 2 {
        r.rounded_rect(cx - 6.0, top + 26.0, 12.0, 3.2, 1.6, color);
    }
}
fn erfc(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let erf = 1.0
        - (((((1.0614054 * t - 1.4531521) * t + 1.4214138) * t - 0.28449672) * t + 0.2548296) * t)
            * (-x * x).exp();
    1.0 - erf
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caret_edits_unicode_without_removing_the_wrong_character() {
        let mut keyboard = Keyboard::new("a東京z");
        assert_eq!(keyboard.handle(Action::Previous), KeyboardEffect::Changed);
        assert_eq!(keyboard.with_caret(), "a東京|z");
        keyboard.handle(Action::Back);
        assert_eq!(keyboard.query, "a東z");
        keyboard.handle(Action::PlayPause);
        assert_eq!(keyboard.query, "a東 z");
        assert_eq!(keyboard.with_caret(), "a東 |z");
    }
    #[test]
    fn rows_wrap_and_up_down_choose_nearest_key_centers() {
        let mut keyboard = Keyboard::new("");
        keyboard.row = 0;
        keyboard.col = 0;
        keyboard.handle(Action::Left);
        assert_eq!((keyboard.row, keyboard.col), (0, 9));
        keyboard.handle(Action::Down);
        assert_eq!((keyboard.row, keyboard.col), (1, 8));
        keyboard.row = 2;
        keyboard.col = 0;
        keyboard.handle(Action::Select);
        assert_eq!(keyboard.shift, 1);
        keyboard.row = 0;
        keyboard.col = 0;
        keyboard.handle(Action::Select);
        assert_eq!(keyboard.query, "Q");
        assert_eq!(keyboard.shift, 0);
        keyboard.handle(Action::Options);
        keyboard.handle(Action::Select);
        assert_eq!(keyboard.query, "Q1");
    }
    #[test]
    fn b_at_start_does_not_close_but_held_b_does() {
        let mut keyboard = Keyboard::new("");
        keyboard.snap_visible(1.0);
        assert_eq!(keyboard.handle(Action::Back), KeyboardEffect::None);
        assert!(!keyboard.is_exiting());
        assert_eq!(keyboard.handle(Action::BackHeld), KeyboardEffect::Close);
        assert!(keyboard.is_exiting());
        assert!(!keyboard.finished(1.1));
        assert!(keyboard.finished(1.221));
    }
    #[test]
    fn original_keyboard_geometry_and_animation_endpoints() {
        assert_eq!(PANEL_HEIGHT, 242.0);
        let rows = key_rows(0);
        assert_eq!(
            rows.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![10, 9, 8, 4]
        );
        assert_eq!(key_positions(&rows[0])[0], 13.0);
        assert_eq!(key_positions(&rows[3])[0], 13.0);
        let mut keyboard = Keyboard::new("");
        keyboard.show(2.0);
        assert_eq!(keyboard.height(2.0), 0.0);
        assert!((keyboard.height(2.26) - 242.0).abs() < 0.001);
    }
}
