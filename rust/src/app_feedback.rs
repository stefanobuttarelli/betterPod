//! Original floating action sheets and toast lifecycle from ij_sheet.go.
use super::*;
use crate::render::BLACK;
use betterpod_rust::motion::{Anim, Ease};
pub(super) struct Feedback {
    sheet: Option<Page>,
    sheet_key: Option<usize>,
    sheet_y: Anim,
    toast_text: String,
    toast_lines: Vec<(String, f32)>,
    toast_key: Option<Instant>,
    toast_alpha: Anim,
    end_track: Option<Track>,
}
impl Feedback {
    pub fn new() -> Self {
        Self {
            sheet: None,
            sheet_key: None,
            sheet_y: Anim::new(1.0),
            toast_text: String::new(),
            toast_lines: Vec::new(),
            toast_key: None,
            toast_alpha: Anim::new(0.0),
            end_track: None,
        }
    }
}

/// Toasts stay inside the display. Only a changed message is laid out; drawing
/// reuses these short lines rather than measuring a potentially long error.
fn toast_lines(text: &str, width: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
    let normalized: String = text
        .chars()
        .take(1024)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let mut rest = normalized.trim();
    let mut lines = Vec::new();
    while !rest.is_empty() && lines.len() < 3 {
        let mut fit = 0;
        let mut space = None;
        for (start, character) in rest.char_indices() {
            let end = start + character.len_utf8();
            if measure(&rest[..end]) > width {
                break;
            }
            fit = end;
            if character == ' ' {
                space = Some(start);
            }
        }
        if fit == rest.len() {
            lines.push(rest.to_owned());
            rest = "";
        } else if fit == 0 {
            // A glyph wider than the available display cannot be shown whole.
            break;
        } else {
            let end = space.filter(|&i| i > 0).unwrap_or(fit);
            lines.push(rest[..end].trim_end().to_owned());
            rest = rest[end..].trim_start();
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    if !rest.is_empty() || text.chars().nth(1024).is_some() {
        let last = lines.last_mut().unwrap();
        while !last.is_empty() && measure(&format!("{last}…")) > width {
            last.pop();
        }
        if measure(&format!("{last}…")) <= width {
            last.push('…');
        }
    }
    lines
}

#[cfg(test)]
mod toast_tests {
    use super::toast_lines;
    fn width(text: &str) -> f32 {
        text.chars()
            .map(|c| if c.is_ascii() { 1. } else { 2. })
            .sum()
    }
    #[test]
    fn toast_errors_wrap_inside_the_screen_without_losing_short_message_words() {
        let message = "Choose a playlist name without path separators or control characters";
        let lines = toast_lines(message, 26., width);
        assert_eq!(lines.join(" "), message);
        assert!(lines.len() > 1 && lines.len() <= 3);
        assert!(lines.iter().all(|line| width(line) <= 26.));
        assert_eq!(
            toast_lines("Enter a playlist name", 26., width),
            ["Enter a playlist name"]
        );
    }
    #[test]
    fn toast_long_paths_and_unicode_remain_bounded_and_show_truncation() {
        let lines = toast_lines(&"長い名前/file".repeat(100), 20., width);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| width(line) <= 20.));
        assert!(lines.last().unwrap().ends_with('…'));
    }
}

impl App {
    pub(super) fn visual_feedback_draw(&mut self, r: &mut Renderer) {
        let now = self.visual_time;
        let light = self.settings.effects == "light";
        let active = self
            .options
            .clone()
            .or_else(|| {
                if self.screen() == Screen::Extra {
                    self.extras.visual_options()
                } else if self.screen() == Screen::Files {
                    self.files.visual_options()
                } else if self.screen() == Screen::Curation {
                    self.curation.visual_options()
                } else {
                    None
                }
            })
            .or_else(|| self.maintenance.as_ref().and_then(|m| m.visual_options()));
        let key = active.as_ref().map(|p| Arc::as_ptr(&p.rows) as usize);
        if key != self.feedback.sheet_key {
            self.feedback.sheet_key = key;
            if let Some(page) = &active {
                self.feedback.end_track = if page.title == "End of the Queue" {
                    self.now_track.clone()
                } else {
                    None
                };
                self.feedback.sheet = Some(page.clone());
                self.feedback.sheet_y.snap(1.0);
                self.feedback.sheet_y.set(
                    0.0,
                    if light { 0.0 } else { 0.3 },
                    Ease::InOut,
                    0.0,
                    now,
                );
            } else {
                self.feedback.sheet_y.set(
                    1.0,
                    if light { 0.0 } else { 0.3 },
                    Ease::InOut,
                    0.0,
                    now,
                );
            }
        } else if let Some(page) = &active {
            self.feedback.sheet = Some(page.clone());
        }
        let y = self.feedback.sheet_y.value(now) as f32;
        if let Some(page) = self.feedback.sheet.as_ref() {
            if y < 0.999 {
                if page.title == "End of the Queue" {
                    let dark = self.dark();
                    end_card(
                        r,
                        page,
                        dark,
                        y,
                        self.feedback.end_track.as_ref(),
                        &mut self.art,
                    );
                } else {
                    sheet(r, page, self.dark(), y, self.album_read_busy_row(page), now);
                }
            }
            if active.is_none() && !self.feedback.sheet_y.running(now) {
                self.feedback.sheet = None;
            }
        }
        let toast_key = self.toast.as_ref().map(|t| t.until);
        if toast_key != self.feedback.toast_key {
            self.feedback.toast_key = toast_key;
            if let Some(toast) = self.toast.as_ref() {
                self.feedback.toast_text = toast.text.clone();
                self.feedback.toast_lines = toast_lines(&toast.text, 552.0, |text| {
                    r.text_width_weighted(text, 26.0, 600)
                })
                .into_iter()
                .map(|text| {
                    let width = r.text_width_weighted(&text, 26.0, 600);
                    (text, width)
                })
                .collect();
                self.feedback.toast_alpha.set(
                    1.0,
                    if light { 0.0 } else { 0.2 },
                    Ease::Css,
                    0.0,
                    now,
                );
            } else {
                self.feedback.toast_alpha.set(
                    0.0,
                    if light { 0.0 } else { 0.25 },
                    Ease::Css,
                    0.0,
                    now,
                );
            }
        }
        let a = self.feedback.toast_alpha.value(now) as f32;
        if a > 0.001 {
            let lines = &self.feedback.toast_lines;
            let tw = lines.iter().map(|(_, width)| *width).fold(0.0, f32::max);
            let w = (tw + 56.0).min(608.0);
            let x = 320.0 - w * 0.5;
            let line_height = crate::render::line_box(26.0).0;
            let h = 56.0 + lines.len().saturating_sub(1) as f32 * line_height;
            let y = if matches!(self.feedback.toast_text.as_str(), "Hold On" | "Hold Off") {
                240.0 - h * 0.5
            } else {
                280.0 - (h - 56.0) * 0.5 + (1.0 - a) * 16.0
            };
            rounded_gradient(
                r,
                x,
                y,
                w,
                h,
                28.0,
                &[
                    (0.0, [48.0 / 255.0, 50.0 / 255.0, 54.0 / 255.0, 0.9 * a]),
                    (1.0, [12.0 / 255.0, 13.0 / 255.0, 15.0 / 255.0, 0.9 * a]),
                ],
            );
            stroke_rounded(r, x, y, w, h, 28.0, 1.5, [1.0, 1.0, 1.0, 0.18 * a]);
            for (index, (text, width)) in lines.iter().enumerate() {
                r.text_baseline(
                    text,
                    x + (w - width) * 0.5,
                    y + 28.0 + 26.0 * 0.727 * 0.5 + index as f32 * line_height,
                    26.0,
                    600,
                    [1.0, 1.0, 1.0, a],
                    *width + 1.0,
                );
            }
        }
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
pub(super) fn rounded_gradient(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    stops: &[(f32, Color)],
) {
    r.rounded_gradient(x, y, w, h, radius, stops);
}
pub(super) fn stroke_rounded(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    thickness: f32,
    color: Color,
) {
    r.stroke_rounded(x, y, w, h, radius, thickness, color);
}
fn card(r: &mut Renderer, x: f32, y: f32, w: f32, h: f32, dark: bool) {
    for (i, a) in [0.10, 0.07, 0.04].into_iter().enumerate() {
        let g = i as f32 * 2.0;
        r.rounded_rect(
            x - g,
            y + 2.0 - g,
            w + 2.0 * g,
            h + 2.0 + 2.0 * g,
            22.0 + g,
            [0.0, 0.0, 0.0, a],
        );
    }
    let (top, bottom, edge, hi) = if dark {
        (
            rgb(59, 62, 68),
            rgb(43, 46, 51),
            [0.0, 0.0, 0.0, 0.7],
            [1.0, 1.0, 1.0, 0.1],
        )
    } else {
        (
            WHITE,
            rgb(236, 238, 242),
            [0.0, 0.0, 0.0, 0.28],
            [1.0, 1.0, 1.0, 0.9],
        )
    };
    rounded_gradient(r, x, y, w, h, 22.0, &[(0.0, top), (1.0, bottom)]);
    stroke_rounded(r, x, y, w, h, 22.0, 1.5, edge);
    r.rect(x + 22.0, y + 2.0, w - 44.0, 1.0, hi);
}
fn sheet(
    r: &mut Renderer,
    page: &Page,
    dark: bool,
    hidden: f32,
    busy_row: Option<usize>,
    now: f64,
) {
    let cancel = page.rows.last().is_some_and(|row| row.label == "Cancel");
    let n = page.rows.len().saturating_sub(usize::from(cancel));
    let title = if page.title == "Options" {
        ""
    } else {
        page.title.as_str()
    };
    let title_h = if title.is_empty() { 0.0 } else { 52.0 };
    let height = n as f32 * 64.0 + title_h + 14.0 + 64.0 + 8.0;
    let rest = 480.0 - height - 24.0 + 8.0;
    let y = rest + ((480.0 - rest) * hidden + 0.5).floor();
    r.rect(
        0.0,
        0.0,
        640.0,
        480.0,
        [0.0, 0.0, 0.0, 0.32 * (1.0 - hidden)],
    );
    let text = if dark { rgb(238, 239, 241) } else { BLACK };
    let sub = if dark {
        rgb(150, 153, 158)
    } else {
        rgb(100, 100, 100)
    };
    let sep = if dark {
        [1.0, 1.0, 1.0, 0.07]
    } else {
        [0.0, 0.0, 0.0, 0.09]
    };
    if n > 0 || title_h > 0.0 {
        card(r, 24.0, y, 592.0, n as f32 * 64.0 + title_h, dark);
    }
    if title_h > 0.0 {
        let title = app_visual_menu::ellipsize(r, title, 22.0, 500, 544.0);
        let tw = r.text_width_weighted(&title, 22.0, 500);
        r.text_baseline(
            &title,
            320.0 - tw * 0.5,
            y + 26.0 + 22.0 * 0.727 * 0.5,
            22.0,
            500,
            sub,
            544.0,
        );
        r.rect(24.0, y + 51.0, 592.0, 1.0, sep);
    }
    for i in 0..=n {
        let yy = if i == n {
            y + title_h + n as f32 * 64.0 + 14.0
        } else {
            y + title_h + i as f32 * 64.0
        };
        if i == n {
            card(r, 24.0, yy, 592.0, 64.0, dark);
        }
        let row = if i < n {
            &page.rows[i]
        } else if cancel {
            &page.rows[n]
        } else {
            continue;
        };
        let danger = row.label == "Delete"
            || row.label.starts_with("Delete ")
            || row.label.starts_with("Remove ")
            || row.label.starts_with("Clear ");
        let selected = i == page.selected;
        let mut color = if danger { rgb(224, 53, 43) } else { text };
        if selected {
            let (top, bottom) = if danger {
                (rgb(255, 107, 94), rgb(212, 42, 30))
            } else {
                (rgb(60, 184, 255), rgb(52, 122, 181))
            };
            rounded_gradient(
                r,
                32.0,
                yy + 6.0,
                576.0,
                52.0,
                16.0,
                &[(0.0, top), (1.0, bottom)],
            );
            color = WHITE;
        }
        let weight = if i == n { 700 } else { 600 };
        let busy = row.visual.busy || busy_row == Some(i);
        let label = app_visual_menu::ellipsize(
            r,
            &row.label,
            30.0,
            weight,
            if busy { 512.0 } else { 544.0 },
        );
        let tw = r.text_width_weighted(&label, 30.0, weight);
        let mark = if row.visual.queue_mark.is_empty() {
            row.visual.icon.as_str()
        } else {
            row.visual.queue_mark.as_str()
        };
        let icon = matches!(mark, "queue_next" | "queue_last");
        let x = 320.0 - (tw + if icon { 52.0 } else { 0.0 }) * 0.5;
        if icon {
            queue_icon(r, x, yy + 32.0, mark == "queue_next", color);
        }
        r.text_baseline(
            &label,
            x + if icon { 52.0 } else { 0.0 },
            yy + 32.0 + 30.0 * 0.727 * 0.5,
            30.0,
            weight,
            color,
            544.0,
        );
        if busy {
            app_visual_menu::draw_spinner_in(r, 584.0, yy + 32.0, 12.0, now, color);
        }
        if i + 1 < n && i != page.selected && i + 1 != page.selected {
            r.rect(48.0, yy + 63.0, 544.0, 1.0, sep);
        }
    }
}
pub(super) fn queue_icon(r: &mut Renderer, x: f32, cy: f32, next: bool, c: Color) {
    use super::app_visual_menu::aa_polyline;
    for i in 0..3 {
        let y = cy - 11.0 + i as f32 * 11.0;
        aa_polyline(
            r,
            &[[x + 16.0, y], [x + 36.0, y]],
            if (next && i == 0) || (!next && i == 2) {
                5.6
            } else {
                2.8
            },
            c,
        );
    }
    let (from, to, dir) = if next {
        (cy + 11.0, cy - 11.0, 1.0)
    } else {
        (cy - 11.0, cy + 11.0, -1.0)
    };
    aa_polyline(r, &[[x + 5.0, from], [x + 5.0, to]], 3.2, c);
    aa_polyline(
        r,
        &[
            [x + 0.2, to + 5.2 * dir],
            [x + 5.0, to],
            [x + 9.8, to + 5.2 * dir],
        ],
        3.2,
        c,
    );
}

fn end_card(
    r: &mut Renderer,
    page: &Page,
    dark: bool,
    hidden: f32,
    track: Option<&Track>,
    art: &mut Artwork,
) {
    let a = 1.0 - hidden;
    r.rect(0.0, 40.0, 640.0, 440.0, [0.0, 0.0, 0.0, 0.45 * a]);
    r.push_transform(0.0, 20.0 * hidden, 1.0, 1.0, a);
    let x = 70.0;
    let y = 66.0;
    card(r, x, y, 500.0, 384.0, dark);
    let text = if dark { rgb(238, 239, 241) } else { BLACK };
    let sub = if dark {
        rgb(150, 153, 158)
    } else {
        rgb(100, 100, 100)
    };
    let texture = art.get(track.and_then(|t| t.cover.as_deref()));
    r.image(texture, 272.0, y + 26.0, 96.0, 96.0, 1.0);
    r.stroke_rounded(272.0, y + 26.0, 96.0, 96.0, 4.0, 1.0, [0.0, 0.0, 0.0, 0.18]);
    let centered = |r: &mut Renderer, s: &str, baseline: f32, size: f32, weight: u16, c: Color| {
        let s = app_visual_menu::ellipsize(r, s, size, weight, 440.0);
        let width = r.text_width_weighted(&s, size, weight);
        r.text_baseline(&s, 320.0 - width * 0.5, baseline, size, weight, c, 440.0);
    };
    centered(
        r,
        "End of the Queue",
        y + 150.0 + 30.0 * 0.727 * 0.5,
        30.0,
        700,
        text,
    );
    centered(
        r,
        "Nothing else is lined up after",
        y + 194.0,
        22.0,
        400,
        sub,
    );
    centered(
        r,
        &format!("“{}”.", track.map(|t| t.title.as_str()).unwrap_or("")),
        y + 220.0,
        22.0,
        600,
        text,
    );
    for (i, bx, by, bw, bh, label, subtitle) in [
        (
            0,
            x + 30.0,
            y + 238.0,
            440.0,
            68.0,
            "Genius",
            "50 songs from your library, at random",
        ),
        (1, x + 30.0, y + 318.0, 214.0, 48.0, "Go to Music", ""),
        (2, x + 256.0, y + 318.0, 214.0, 48.0, "Not Now", ""),
    ] {
        let selected = page.selected == i;
        let (tc, sc) = if selected {
            r.rounded_gradient(
                bx,
                by,
                bw,
                bh,
                18.0,
                &[(0.0, rgb(60, 184, 255)), (1.0, rgb(52, 122, 181))],
            );
            (WHITE, [1.0, 1.0, 1.0, 0.85])
        } else {
            let (top, bottom, edge) = if dark {
                (rgb(74, 77, 84), rgb(56, 59, 65), [0.0, 0.0, 0.0, 0.6])
            } else {
                (WHITE, rgb(233, 235, 239), [0.0, 0.0, 0.0, 0.22])
            };
            r.rounded_gradient(bx, by, bw, bh, 18.0, &[(0.0, top), (1.0, bottom)]);
            r.stroke_rounded(bx, by, bw, bh, 18.0, 1.5, edge);
            (text, sub)
        };
        let mid = by + bh * 0.5 - if subtitle.is_empty() { 0.0 } else { 12.0 };
        let tw = r.text_width_weighted(label, 28.0, 600);
        let lx = bx + bw * 0.5 - tw * 0.5 + if i == 0 { 14.0 } else { 0.0 };
        if i == 0 {
            sparkle(r, lx - 18.0, mid, tc);
        }
        r.text_baseline(label, lx, mid + 28.0 * 0.727 * 0.5, 28.0, 600, tc, bw - 8.0);
        if !subtitle.is_empty() {
            let tw = r.text_width_weighted(subtitle, 19.0, 500);
            r.text_baseline(
                subtitle,
                bx + bw * 0.5 - tw * 0.5,
                mid + 32.0,
                19.0,
                500,
                sc,
                bw - 8.0,
            );
        }
    }
    r.pop_transform();
}
fn sparkle(r: &mut Renderer, x: f32, y: f32, c: Color) {
    for (x, y, size) in [(x, y + 1.0, 11.0), (x + 10.0, y - 9.0, 4.8)] {
        let k = size * 0.3;
        let points = [
            [0.0, -size],
            [k, -k],
            [size, 0.0],
            [k, k],
            [0.0, size],
            [-k, k],
            [-size, 0.0],
            [-k, -k],
        ];
        for i in 0..8 {
            let p = points[i];
            let q = points[(i + 1) % 8];
            r.triangle([[x, y], [x + p[0], y + p[1]], [x + q[0], y + q[1]]], c);
        }
    }
}
