//! Original ij_keycaps.go inline buttons. Measurements and drawing share the
//! same token stream, so hints wrap/ellipsize with the physical button widths.
use crate::render::{Color, Renderer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Round,
    Pill,
    Square,
    Dpad,
}
fn shape(token: &str) -> Option<Shape> {
    Some(match token {
        "A" | "B" | "X" | "Y" => Shape::Round,
        "L" | "R" | "L2" | "R2" | "START" | "SELECT" => Shape::Pill,
        "↑" | "↓" | "←" | "→" | "◀" | "▶" | "▲" | "▼" => Shape::Square,
        "DPAD" => Shape::Dpad,
        _ => return None,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Part<'a> {
    text: &'a str,
    key: bool,
}
/// Allocation-free renderer guard. An unknown bracket sequence is ordinary
/// text; advance only past its opening bracket so nested valid tokens retain
/// the original Go keyParts behavior.
pub fn has_tokens(text: &str) -> bool {
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            return false;
        };
        if shape(&rest[open + 1..open + close]).is_some() {
            return true;
        }
        rest = &rest[open + 1..];
    }
    false
}
fn parts(text: &str) -> Option<Vec<Part<'_>>> {
    if !text.contains('[') {
        return None;
    }
    let (mut rest, mut found) = (text, false);
    let mut parts = Vec::new();
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        let token = &rest[open + 1..open + close];
        if shape(token).is_none() {
            parts.push(Part {
                text: &rest[..open + 1],
                key: false,
            });
            rest = &rest[open + 1..];
            continue;
        }
        if open > 0 {
            parts.push(Part {
                text: &rest[..open],
                key: false,
            });
        }
        parts.push(Part {
            text: token,
            key: true,
        });
        found = true;
        rest = &rest[open + close + 1..];
    }
    if !found {
        return None;
    }
    if !rest.is_empty() {
        parts.push(Part {
            text: rest,
            key: false,
        });
    }
    Some(parts)
}
fn metrics(size: f32) -> (f32, f32) {
    // Go recovers the quantized physical face size before keyMetrics.
    let size = (size * 2.0).round() * 0.5;
    (size * 0.727 * 1.85, size * 0.62)
}
fn key_width(r: &mut Renderer, token: &str, size: f32) -> f32 {
    let (height, label) = metrics(size);
    if shape(token) == Some(Shape::Pill) {
        r.text_width_weighted(token, label, 700) + height * 0.7
    } else {
        height
    }
}
pub fn width(r: &mut Renderer, text: &str, size: f32, weight: u16) -> f32 {
    let Some(parts) = parts(text) else {
        return r.text_width_weighted(text, size, weight);
    };
    parts
        .into_iter()
        .map(|part| {
            if part.key {
                key_width(r, part.text, size)
            } else {
                r.text_width_weighted(part.text, size, weight)
            }
        })
        .sum()
}
#[allow(clippy::too_many_arguments)]
pub fn draw(
    r: &mut Renderer,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    weight: u16,
    color: Color,
    max_width: f32,
) {
    let Some(parts) = parts(text) else {
        r.text_baseline(text, x, baseline, size, weight, color, max_width);
        return;
    };
    let (height, label) = metrics(size);
    let cy = baseline - height / 1.85 / 2.0;
    let mut cursor = x;
    for part in parts {
        if part.key {
            let w = key_width(r, part.text, size);
            if cursor + w > x + max_width + 0.01 {
                return;
            }
            draw_cap(r, part.text, cursor, cy, w, height, label, color);
            cursor += w;
        } else {
            let w = r.text_width_weighted(part.text, size, weight);
            r.text_baseline(
                part.text,
                cursor,
                baseline,
                size,
                weight,
                color,
                (x + max_width - cursor).max(0.0),
            );
            cursor += w;
            if cursor > x + max_width {
                return;
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn draw_cap(
    r: &mut Renderer,
    token: &str,
    x: f32,
    cy: f32,
    w: f32,
    h: f32,
    label: f32,
    color: Color,
) {
    let y = cy - h / 2.0;
    let mut fill = color;
    fill[3] *= 0.14;
    let mut edge = color;
    edge[3] *= 0.85;
    let line = (h / 12.0).max(1.0);
    let radius = match shape(token) {
        Some(Shape::Round) => h / 2.0,
        Some(Shape::Pill) => h * 0.32,
        Some(Shape::Square) => h * 0.24,
        Some(Shape::Dpad) => {
            let t = h * 0.36;
            r.rounded_rect(x + w / 2.0 - t / 2.0, y, t, h, t * 0.25, edge);
            r.rounded_rect(x, cy - t / 2.0, w, t, t * 0.25, edge);
            return;
        }
        None => return,
    };
    r.rounded_rect(x, y, w, h, radius, fill);
    // Source strokeRounded draws its edge entirely inside the bounds.
    r.stroke_rounded(
        x + line / 2.0,
        y + line / 2.0,
        w - line,
        h - line,
        (radius - line / 2.0).max(0.0),
        line,
        edge,
    );
    let requested = if matches!(token, "START" | "SELECT") || token.len() == 2 {
        label * 0.86
    } else {
        label
    };
    let label_size = (requested * 2.0).round() * 0.5;
    let text_width = r.text_width_weighted(token, label_size, 700);
    r.text_baseline(
        token,
        x + (w - text_width) / 2.0,
        cy + label_size * 0.727 / 2.0,
        label_size,
        700,
        edge,
        text_width + 1.0,
    );
}
pub fn ellipsize(r: &mut Renderer, text: &str, size: f32, weight: u16, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if width(r, text, size, weight) <= max_width {
        return text.to_owned();
    }
    let ellipsis = width(r, "…", size, weight);
    let mut ends = text
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    ends.push(text.len());
    let (mut low, mut high) = (0, ends.len() - 1);
    while low < high {
        let mid: usize = (low + high).div_ceil(2);
        if width(r, &text[..ends[mid]], size, weight) + ellipsis <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    while low > 0 && text[..ends[low]].ends_with(' ') {
        low -= 1;
    }
    format!("{}…", &text[..ends[low]])
}
pub fn caret_fit(r: &mut Renderer, text: &str, size: f32, weight: u16, max_width: f32) -> String {
    if width(r, text, size, weight) <= max_width {
        return text.to_owned();
    }
    let Some(caret) = text.rfind('|') else {
        return ellipsize(r, text, size, weight, max_width);
    };
    if width(r, &format!("{}…", &text[..caret + 1]), size, weight) <= max_width {
        return ellipsize(r, text, size, weight, max_width);
    }
    let tail = if caret + 1 < text.len() { "…" } else { "" };
    let mut from = 0;
    while from < caret
        && width(
            r,
            &format!("…{}{tail}", &text[from..caret + 1]),
            size,
            weight,
        ) > max_width
    {
        from += text[from..].chars().next().unwrap().len_utf8();
    }
    ellipsize(r, &format!("…{}", &text[from..]), size, weight, max_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_guard_recognizes_only_complete_source_tokens_including_nested_text() {
        for plain in [
            "plain text",
            "[unknown]",
            "[]",
            "[AA]",
            "[a]",
            "[START",
            "[[no]]",
            "日本語 [not a key]",
        ] {
            assert!(!has_tokens(plain), "{plain}");
            assert!(parts(plain).is_none());
        }
        for keyed in [
            "[A]",
            "[] [SELECT]",
            "[unknown] [Y]",
            "[[A]]",
            "[broken [L2] 日本語",
            "[→][DPAD]",
            "[unknown [START] tail",
        ] {
            assert!(has_tokens(keyed), "{keyed}");
            let parsed = parts(keyed).unwrap();
            // Renderer width/draw may delegate these literal spans back to
            // themselves: none can recurse into another keycap dispatch.
            for plain in parsed.iter().filter(|part| !part.key) {
                assert!(
                    !has_tokens(plain.text),
                    "nested dispatch in {:?}",
                    plain.text
                );
            }
        }
    }
    #[test]
    fn source_tokens_preserve_unknown_text_and_unicode_boundaries() {
        assert!(parts("No buttons here").is_none());
        assert!(parts("[unknown] or [A").is_none());
        let parsed = parts("Caffè [SELECT] · [Y] · [L2] 日本語").unwrap();
        assert_eq!(
            parsed
                .iter()
                .filter(|p| p.key)
                .map(|p| p.text)
                .collect::<Vec<_>>(),
            vec!["SELECT", "Y", "L2"]
        );
        let parsed = parts("[unknown] [[A] [→]").unwrap();
        assert_eq!(
            parsed
                .iter()
                .filter(|p| p.key)
                .map(|p| p.text)
                .collect::<Vec<_>>(),
            vec!["A", "→"]
        );
        let rebuilt = parsed
            .iter()
            .map(|p| {
                if p.key {
                    format!("[{}]", p.text)
                } else {
                    p.text.to_owned()
                }
            })
            .collect::<String>();
        assert_eq!(rebuilt, "[unknown] [[A] [→]");
    }
    #[test]
    fn every_original_button_uses_the_original_shape_and_physical_metrics() {
        for token in ["A", "B", "X", "Y"] {
            assert_eq!(shape(token), Some(Shape::Round));
        }
        for token in ["L", "R", "L2", "R2", "START", "SELECT"] {
            assert_eq!(shape(token), Some(Shape::Pill));
        }
        for token in ["↑", "↓", "←", "→", "◀", "▶", "▲", "▼"] {
            assert_eq!(shape(token), Some(Shape::Square));
        }
        assert_eq!(shape("DPAD"), Some(Shape::Dpad));
        assert_eq!(shape("select"), None);
        let (h, label) = metrics(24.0);
        assert!((h - 32.2788).abs() < 1e-4);
        assert!((label - 14.88).abs() < 1e-4);
    }
}
