//! Bounded fallback glyph preparation. Keep Noto's original font data and
//! rasterize just the requested outline, rather than expanding 35k outlines
//! across both weights before any text is visible. Inter stays in fontdue.
use fontdue::{Metrics, OutlineBounds};
use std::{cell::RefCell, collections::HashMap};
use tiny_skia as sk;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

const MAX_METRICS: usize = 8192;
#[derive(Clone, Copy)]
struct GlyphInfo {
    id: u16,
    advance_width: f32,
    advance_height: f32,
    bounds: OutlineBounds,
}
pub(super) struct OutlineFont {
    data: Vec<u8>,
    units: f32,
    glyphs: RefCell<HashMap<char, GlyphInfo>>,
}
impl OutlineFont {
    pub(super) fn new(data: Vec<u8>) -> Result<Self, String> {
        let units = Face::parse(&data, 0)
            .map_err(|e| e.to_string())?
            .units_per_em() as f32;
        Ok(Self {
            data,
            units,
            glyphs: RefCell::new(HashMap::new()),
        })
    }
    pub(super) fn source_bytes(&self) -> usize {
        self.data.len()
    }
    pub(super) fn cached_metrics(&self) -> usize {
        self.glyphs.borrow().len()
    }
    fn info(&self, c: char) -> GlyphInfo {
        if let Some(info) = self.glyphs.borrow().get(&c) {
            return *info;
        }
        let face = Face::parse(&self.data, 0).expect("Validated fallback font");
        let id = face.glyph_index(c).unwrap_or(GlyphId(0));
        let bounds = outline(&face, id)
            .and_then(|path| path.compute_tight_bounds())
            .map_or(OutlineBounds::default(), |r| OutlineBounds {
                xmin: r.left(),
                ymin: r.top(),
                width: r.width(),
                height: r.height(),
            });
        let info = GlyphInfo {
            id: id.0,
            advance_width: face.glyph_hor_advance(id).unwrap_or(0) as f32,
            advance_height: face.glyph_ver_advance(id).unwrap_or(0) as f32,
            bounds,
        };
        if self.glyphs.borrow().len() < MAX_METRICS {
            self.glyphs.borrow_mut().insert(c, info);
        }
        info
    }
    pub(super) fn lookup_glyph_index(&self, c: char) -> u16 {
        self.info(c).id
    }
    fn scaled(&self, info: GlyphInfo, size: f32) -> Metrics {
        if size <= 0. {
            return Metrics::default();
        }
        let scale = size / self.units;
        let bounds = info.bounds.scale(scale);
        let xmin = bounds.xmin.floor() as i32;
        let ymin = bounds.ymin.floor() as i32;
        let width = if bounds.width > 0. {
            ((bounds.xmin + bounds.width).ceil() as i32 - xmin).max(0) as usize
        } else {
            0
        };
        let height = if bounds.height > 0. {
            ((bounds.ymin + bounds.height).ceil() as i32 - ymin).max(0) as usize
        } else {
            0
        };
        Metrics {
            xmin,
            ymin,
            width,
            height,
            advance_width: info.advance_width * scale,
            advance_height: info.advance_height * scale,
            bounds,
        }
    }
    pub(super) fn metrics(&self, c: char, size: f32) -> Metrics {
        self.scaled(self.info(c), size)
    }
    pub(super) fn rasterize(&self, c: char, size: f32) -> (Metrics, Vec<u8>) {
        let info = self.info(c);
        let metrics = self.scaled(info, size);
        if metrics.width == 0 || metrics.height == 0 {
            return (metrics, Vec::new());
        }
        let face = Face::parse(&self.data, 0).expect("Validated fallback font");
        let Some(path) = outline(&face, GlyphId(info.id)) else {
            return (metrics, vec![0; metrics.width * metrics.height]);
        };
        // Text-size fallback strokes need finer coverage than a single Skia
        // sample grid. Supersample only this new glyph, then average coverage
        // into its existing native-size atlas cell. Bound normal scratch to
        // 512-square; larger glyphs use the native antialiased rasterizer.
        let longest = metrics.width.max(metrics.height);
        let samples = if longest <= 128 {
            4
        } else if longest <= 256 {
            2
        } else {
            1
        };
        let mut image = sk::Pixmap::new(
            (metrics.width * samples) as u32,
            (metrics.height * samples) as u32,
        )
        .expect("Bounded fallback glyph");
        let scale = size / self.units * samples as f32;
        image.fill_path(
            &path,
            &sk::Paint {
                shader: sk::Shader::SolidColor(sk::Color::WHITE),
                anti_alias: true,
                ..Default::default()
            },
            sk::FillRule::Winding,
            sk::Transform::from_row(
                scale,
                0.,
                0.,
                -scale,
                -metrics.xmin as f32 * samples as f32,
                (metrics.ymin + metrics.height as i32) as f32 * samples as f32,
            ),
            None,
        );
        let mut pixels = vec![0; metrics.width * metrics.height];
        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let mut coverage = 0_u32;
                for yy in 0..samples {
                    for xx in 0..samples {
                        coverage += image.data()[((y * samples + yy) * metrics.width * samples
                            + x * samples
                            + xx)
                            * 4
                            + 3] as u32;
                    }
                }
                let count = (samples * samples) as u32;
                pixels[y * metrics.width + x] = ((coverage + count / 2) / count) as u8;
            }
        }
        (metrics, pixels)
    }
}
struct PathBuilder(sk::PathBuilder);
impl OutlineBuilder for PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}
fn outline(face: &Face<'_>, id: GlyphId) -> Option<sk::Path> {
    let mut builder = PathBuilder(sk::PathBuilder::new());
    face.outline_glyph(id, &mut builder)?;
    builder.0.finish()
}
pub(super) enum RasterFont<'a> {
    Primary(&'a fontdue::Font),
    Fallback(&'a OutlineFont),
}
impl RasterFont<'_> {
    pub(super) fn metrics(&self, c: char, size: f32) -> Metrics {
        match self {
            Self::Primary(f) => f.metrics(c, size),
            Self::Fallback(f) => f.metrics(c, size),
        }
    }
    pub(super) fn rasterize(&self, c: char, size: f32) -> (Metrics, Vec<u8>) {
        match self {
            Self::Primary(f) => f.rasterize(c, size),
            Self::Fallback(f) => f.rasterize(c, size),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn font_bytes(name: &str) -> Vec<u8> {
        let root = std::env::var_os("BETTERPOD_RUST_ASSETS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let native = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../port/betterpod/assets");
                if native.is_dir() {
                    native
                } else {
                    std::path::PathBuf::from("/repo/port/betterpod/assets")
                }
            });
        std::fs::read(root.join("fonts").join(name)).unwrap()
    }
    #[test]
    fn fallback_retains_source_data_and_only_requested_glyph_metrics() {
        for name in ["NotoSansJP-Regular.otf", "NotoSansJP-Bold.otf"] {
            let bytes = font_bytes(name);
            let expected = bytes.len();
            let font = OutlineFont::new(bytes).unwrap();
            assert_eq!(font.source_bytes(), expected);
            assert_eq!(font.cached_metrics(), 0);
            let face = Face::parse(&font.data, 0).unwrap();
            for c in "漢字あア語龍鷹。ｶ一 Ελληνικά Русский".chars() {
                let expected = face.glyph_index(c).unwrap_or(GlyphId(0));
                assert_eq!(font.lookup_glyph_index(c), expected.0);
                for size in [16., 20., 28., 40., 64.] {
                    let metrics = font.metrics(c, size);
                    let advance = face.glyph_hor_advance(expected).unwrap_or(0) as f32
                        * (size / face.units_per_em() as f32);
                    assert_eq!(metrics.advance_width, advance);
                    let (raster_metrics, bitmap) = font.rasterize(c, size);
                    assert_eq!(raster_metrics, metrics);
                    assert_eq!(bitmap.len(), metrics.width * metrics.height);
                }
            }
            assert!(
                font.cached_metrics() < 40,
                "opening one label must not expand all 17,936 outlines"
            );
        }
    }
    fn coverage_at(metrics: &Metrics, pixels: &[u8], x: i32, y: i32) -> u8 {
        let bx = x - metrics.xmin;
        let by = (metrics.ymin + metrics.height as i32 - 1) - y;
        if bx < 0 || by < 0 || bx >= metrics.width as i32 || by >= metrics.height as i32 {
            0
        } else {
            pixels[by as usize * metrics.width + bx as usize]
        }
    }
    #[test]
    #[ignore = "loads the original large fontdue fallback for cross-rasterizer comparison"]
    fn fallback_layout_and_coverage_match_original_noto() {
        let mut worst_error = 0_f64;
        let mut worst_iou = 1_f64;
        let mut worst_weighted = 1_f64;
        for name in ["NotoSansJP-Regular.otf", "NotoSansJP-Bold.otf"] {
            let bytes = font_bytes(name);
            let reference =
                fontdue::Font::from_bytes(bytes.clone(), fontdue::FontSettings::default()).unwrap();
            let font = OutlineFont::new(bytes).unwrap();
            let mut sheets = HashMap::<u32, image::RgbaImage>::new();
            for (index, c) in "漢字あア語龍鷹。ｶ一".chars().enumerate() {
                assert_eq!(font.lookup_glyph_index(c), reference.lookup_glyph_index(c));
                for size in [16., 20., 28., 40., 64.] {
                    let (a, ap) = reference.rasterize(c, size);
                    let (b, bp) = font.rasterize(c, size);
                    assert!((a.advance_width - b.advance_width).abs() <= 1. / 64.);
                    assert!((a.xmin - b.xmin).abs() <= 1 && (a.ymin - b.ymin).abs() <= 1);
                    assert!(a.width.abs_diff(b.width) <= 1 && a.height.abs_diff(b.height) <= 1);
                    let (
                        mut error,
                        mut used,
                        mut intersection,
                        mut union,
                        mut alpha_min,
                        mut alpha_max,
                    ) = (0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u64);
                    for y in
                        a.ymin.min(b.ymin)..(a.ymin + a.height as i32).max(b.ymin + b.height as i32)
                    {
                        for x in a.xmin.min(b.xmin)
                            ..(a.xmin + a.width as i32).max(b.xmin + b.width as i32)
                        {
                            let aa = coverage_at(&a, &ap, x, y);
                            let bb = coverage_at(&b, &bp, x, y);
                            if aa > 0 || bb > 0 {
                                error += aa.abs_diff(bb) as u64;
                                used += 1;
                            }
                            alpha_min += aa.min(bb) as u64;
                            alpha_max += aa.max(bb) as u64;
                            intersection += u64::from(aa >= 128 && bb >= 128);
                            union += u64::from(aa >= 128 || bb >= 128);
                        }
                    }
                    let error = error as f64 / (used.max(1) * 255) as f64;
                    let iou = intersection as f64 / union.max(1) as f64;
                    let weighted = alpha_min as f64 / alpha_max.max(1) as f64;
                    worst_weighted = worst_weighted.min(weighted);
                    worst_error = worst_error.max(error);
                    worst_iou = worst_iou.min(iou);
                    assert!(
                        error < 0.04 && weighted > 0.94,
                        "{name} {c} {size}px: alpha error {error}, weighted silhouette IoU {weighted}, thresholded IoU {iou}"
                    );
                    let sz = size as u32;
                    let sheet = sheets.entry(sz).or_insert_with(|| {
                        image::RgbaImage::from_pixel(
                            (sz + 12) * 10,
                            (sz + 32) * 2,
                            image::Rgba([255; 4]),
                        )
                    });
                    for (metrics, bitmap, baseline) in
                        [(&a, &ap, sz as i32 + 12), (&b, &bp, sz as i32 * 2 + 44)]
                    {
                        for y in 0..metrics.height {
                            for x in 0..metrics.width {
                                let xx =
                                    index as i32 * (sz as i32 + 12) + 6 + metrics.xmin + x as i32;
                                let yy = baseline - metrics.ymin - metrics.height as i32 + y as i32;
                                if xx >= 0
                                    && yy >= 0
                                    && (xx as u32) < sheet.width()
                                    && (yy as u32) < sheet.height()
                                {
                                    let color = 255 - bitmap[y * metrics.width + x];
                                    sheet.put_pixel(
                                        xx as u32,
                                        yy as u32,
                                        image::Rgba([color, color, color, 255]),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let Some(out) = std::env::var_os("BETTERPOD_RUST_QA_OUTPUT") {
                for (size, sheet) in sheets {
                    sheet
                        .save(
                            std::path::PathBuf::from(&out)
                                .join(format!("{name}-{size}-before-top-after-bottom.png")),
                        )
                        .unwrap();
                }
            }
        }
        eprintln!(
            "Noto fallback worst alpha error {worst_error:.6}, worst weighted silhouette IoU {worst_weighted:.6}, worst thresholded silhouette IoU {worst_iou:.6}"
        );
    }
}
