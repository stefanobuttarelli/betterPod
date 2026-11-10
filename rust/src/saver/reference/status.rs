//! The same original status geometry used by the Go 1.1 header.
use crate::render::{Color, Renderer};
fn alpha(mut c: Color, a: f32) -> Color {
    c[3] *= a;
    c
}
pub(super) fn draw_wifi(r: &mut Renderer, x: f32, y: f32, arcs: usize, color: Color) {
    r.circle(x + 18.0, y + 20.3, 2.8, color);
    for (i, radius) in [7.7, 13.2, 18.7].iter().copied().enumerate() {
        let color = if i < arcs { color } else { alpha(color, 0.3) };
        let mut previous = None;
        for sample in 0..=24 {
            let angle = (-135.0 + 90.0 * sample as f32 / 24.0).to_radians();
            let point = (
                x + 18.0 + radius * angle.cos(),
                y + 20.3 + radius * angle.sin(),
            );
            if let Some((px, py)) = previous {
                r.line(px, py, point.0, point.1, 3.2, color);
            }
            previous = Some(point);
        }
    }
}
pub(super) fn draw_headset(
    r: &mut Renderer,
    x: f32,
    y: f32,
    wifi: i8,
    bluetooth: bool,
    color: Color,
) {
    let cy = if wifi >= 0 { 9.5 } else { 11.8 };
    let (cx, s) = (15.0, 19.0);
    let w = s * 1.05;
    let mut previous = None;
    for sample in 0..=24 {
        let angle = (180.0 + 180.0 * sample as f32 / 24.0).to_radians();
        let p = (
            x + cx + (w * 0.5 - s * 0.07) * angle.cos(),
            y + cy + s * 0.05 + (s * 0.525 - s * 0.07) * angle.sin(),
        );
        if let Some((px, py)) = previous {
            r.line(px, py, p.0, p.1, s * 0.14, color);
        }
        previous = Some(p);
    }
    let cw = s * 0.30;
    let ch = s * 0.46;
    let yb = y + cy + s * 0.05;
    for cup_x in [x + cx - w * 0.5 - cw * 0.15, x + cx + w * 0.5 - cw * 0.85] {
        r.rounded_rect(cup_x, yb - ch * 0.1, cw, ch, cw * 0.35, color);
    }
    if bluetooth {
        let bw = s * 0.16;
        let bh = s * 0.56;
        let by = y + cy + s * 0.1;
        let pts = [
            (cx - bw, by - bh * 0.27),
            (cx + bw, by + bh * 0.25),
            (cx, by + bh * 0.5),
            (cx, by - bh * 0.5),
            (cx + bw, by - bh * 0.25),
            (cx - bw, by + bh * 0.27),
        ];
        for pair in pts.windows(2) {
            r.line(
                x + pair[0].0,
                pair[0].1,
                x + pair[1].0,
                pair[1].1,
                s * 0.085,
                color,
            );
        }
    }
    if wifi >= 0 {
        for i in 0..5 {
            r.circle(
                x + cx + (i as f32 - 2.0) * 5.2,
                y + 21.4,
                1.6,
                if i < wifi as usize {
                    color
                } else {
                    alpha(color, 0.25)
                },
            );
        }
    }
}
