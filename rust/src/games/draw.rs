//! Original game drawing geometry expressed as GPU primitives.
use crate::render::{Color, Renderer};
pub(super) fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
pub(super) fn mix(a: Color, b: Color, t: f32) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn rounded(
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

pub(super) fn centered(
    r: &mut Renderer,
    s: &str,
    cx: f32,
    baseline: f32,
    size: f32,
    weight: u16,
    c: Color,
) {
    let width = r.text_width_weighted(s, size, weight);
    r.text_baseline(s, cx - width * 0.5, baseline, size, weight, c, width + 0.5);
}
pub(super) fn stroke_rect(r: &mut Renderer, x: f32, y: f32, w: f32, h: f32, line: f32, c: Color) {
    r.rect(x - line * 0.5, y - line * 0.5, w + line, line, c);
    r.rect(x - line * 0.5, y + h - line * 0.5, w + line, line, c);
    r.rect(x - line * 0.5, y + line * 0.5, line, h - line, c);
    r.rect(x + w - line * 0.5, y + line * 0.5, line, h - line, c);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn stroke_rounded(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    line: f32,
    color: Color,
) {
    r.stroke_rounded(
        x + line * 0.5,
        y + line * 0.5,
        (w - line).max(0.0),
        (h - line).max(0.0),
        (radius - line * 0.5).max(0.0),
        line,
        color,
    );
}

pub(super) fn soft_shadow(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    blur: f32,
    opacity: f32,
) {
    let steps = (blur * 0.5).max(1.0) as usize;
    for i in (1..=steps).rev() {
        let extra = i as f32 / steps as f32 * blur * 0.5;
        let color = [0.0, 0.0, 0.0, opacity / steps as f32 * 0.9];
        rounded(
            r,
            x - extra,
            y - extra,
            w + extra * 2.0,
            h + extra * 2.0,
            extra,
            &[(0.0, color), (1.0, color)],
        );
    }
}
pub(super) fn tile_colors(value: u32) -> (Color, Color, Color) {
    let dark = rgb(58, 62, 70);
    let white = [1.0; 4];
    match value {
        2 => (rgb(250, 251, 253), rgb(218, 223, 231), dark),
        4 => (rgb(250, 244, 226), rgb(228, 214, 176), dark),
        8 => (rgb(255, 190, 120), rgb(240, 122, 30), white),
        16 => (rgb(255, 158, 112), rgb(232, 80, 42), white),
        32 => (rgb(255, 124, 112), rgb(217, 48, 48), white),
        64 => (rgb(255, 92, 104), rgb(184, 20, 31), white),
        128 => (rgb(255, 232, 136), rgb(240, 191, 34), white),
        256 => (rgb(255, 220, 100), rgb(230, 168, 0), white),
        512 => (rgb(150, 232, 154), rgb(47, 174, 75), white),
        1024 => (rgb(130, 200, 255), rgb(47, 127, 224), white),
        2048 => (rgb(200, 160, 255), rgb(122, 63, 224), white),
        _ => (rgb(110, 120, 140), rgb(43, 50, 64), white),
    }
}
