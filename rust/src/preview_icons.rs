//! Go 1.1's generated preview pictures, rasterized once at twice their display
//! resolution. Geometry, colours and gloss come from ij_gameicons.go and each
//! original view's icon function; no raster work is performed while drawing.
use std::path::Path;
use tiny_skia as sk;
#[path = "bluetooth_icons.rs"]
mod bluetooth_icons;
pub use bluetooth_icons::{
    device_icon_name as bluetooth_device_icon_name, generate as generate_bluetooth,
};

pub struct GeneratedIcon {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// Straight RGBA, matching Renderer::upload.
    pub pixels: Vec<u8>,
    pub display_size: u16,
}

pub fn generate(assets: &Path) -> Result<Vec<GeneratedIcon>, String> {
    let bytes = std::fs::read(assets.join("fonts/Inter-Bold.otf")).map_err(|e| e.to_string())?;
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| e.to_string())?;
    let mut icons = Vec::with_capacity(7);
    let mut pen = Pen::new(224, 100.0)?;
    brick(&mut pen);
    icons.push(pen.finish("preview_brick", 224));
    let mut pen = Pen::new(224, 100.0)?;
    solitaire(&mut pen, &font, assets)?;
    icons.push(pen.finish("preview_solitaire", 224));
    for (name, draw) in [
        ("preview_2048", tiles as fn(&mut Pen, &fontdue::Font)),
        ("preview_words", words),
        ("preview_quiz", quiz),
        ("preview_puzzles", puzzles),
        ("preview_videos", videos),
    ] {
        let mut pen = Pen::new(192, 192.0)?;
        draw(&mut pen, &font);
        icons.push(pen.finish(name, 192));
    }
    Ok(icons)
}

type Stop = (f32, sk::Color);
fn rgb(value: u32) -> sk::Color {
    sk::Color::from_rgba8((value >> 16) as u8, (value >> 8) as u8, value as u8, 255)
}
fn alpha(value: u32, a: f32) -> sk::Color {
    let mut color = rgb(value);
    color.set_alpha(a);
    color
}
fn gradient(y: f32, h: f32, colors: &[Stop]) -> sk::Shader<'static> {
    sk::LinearGradient::new(
        sk::Point::from_xy(0.0, y),
        sk::Point::from_xy(0.0, y + h),
        colors
            .iter()
            .map(|(at, c)| sk::GradientStop::new(*at, *c))
            .collect(),
        sk::SpreadMode::Pad,
        sk::Transform::identity(),
    )
    .unwrap_or(sk::Shader::SolidColor(colors[0].1))
}
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> sk::Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8;
    let mut b = sk::PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.cubic_to(x + w - r + k * r, y, x + w, y + r - k * r, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.cubic_to(
        x + w,
        y + h - r + k * r,
        x + w - r + k * r,
        y + h,
        x + w - r,
        y + h,
    );
    b.line_to(x + r, y + h);
    b.cubic_to(x + r - k * r, y + h, x, y + h - r + k * r, x, y + h - r);
    b.line_to(x, y + r);
    b.cubic_to(x, y + r - k * r, x + r - k * r, y, x + r, y);
    b.close();
    b.finish().unwrap()
}
struct Pen {
    image: sk::Pixmap,
    transform: sk::Transform,
    raster_scale: f32,
}
impl Pen {
    fn new(display: u32, base: f32) -> Result<Self, String> {
        let size = display * 2;
        let raster_scale = size as f32 / base;
        Ok(Self {
            image: sk::Pixmap::new(size, size).ok_or("Cannot allocate preview icon")?,
            transform: sk::Transform::from_scale(raster_scale, raster_scale),
            raster_scale,
        })
    }
    fn fill(&mut self, path: &sk::Path, shader: sk::Shader<'static>) {
        self.image.fill_path(
            path,
            &sk::Paint {
                shader,
                anti_alias: true,
                ..Default::default()
            },
            sk::FillRule::Winding,
            self.transform,
            None,
        );
    }
    fn stroke(&mut self, path: &sk::Path, color: sk::Color, width: f32) {
        self.image.stroke_path(
            path,
            &sk::Paint {
                shader: sk::Shader::SolidColor(color),
                anti_alias: true,
                ..Default::default()
            },
            &sk::Stroke {
                width,
                ..Default::default()
            },
            self.transform,
            None,
        );
    }
    fn round(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, colors: &[Stop]) {
        self.fill(&rounded(x, y, w, h, r), gradient(y, h, colors));
    }
    fn solid_round(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: sk::Color) {
        self.fill(&rounded(x, y, w, h, r), sk::Shader::SolidColor(color));
    }
    fn text(
        &mut self,
        font: &fontdue::Font,
        text: &str,
        cx: f32,
        baseline: f32,
        size: f32,
        color: sk::Color,
    ) {
        // Rasterize at the final physical resolution before applying the fan's
        // rotation; glyph masks are never enlarged from a smaller bitmap.
        let scale = self.raster_scale;
        let px = size * scale;
        let width = text
            .chars()
            .map(|c| font.metrics(c, px).advance_width)
            .sum::<f32>();
        let mut x = cx * scale - width / 2.0;
        for ch in text.chars() {
            let (m, coverage) = font.rasterize(ch, px);
            if m.width > 0 && m.height > 0 {
                if let Some(mut image) = sk::Pixmap::new(m.width as u32, m.height as u32) {
                    for (rgba, a) in image.data_mut().chunks_exact_mut(4).zip(coverage) {
                        let a = (a as f32 * color.alpha()).round() as u8;
                        rgba[0] = (color.red() * a as f32).round() as u8;
                        rgba[1] = (color.green() * a as f32).round() as u8;
                        rgba[2] = (color.blue() * a as f32).round() as u8;
                        rgba[3] = a;
                    }
                    let transform = self.transform.pre_scale(1.0 / scale, 1.0 / scale);
                    self.image.draw_pixmap(
                        (x + m.xmin as f32).round() as i32,
                        (baseline * scale - m.ymin as f32 - m.height as f32).round() as i32,
                        image.as_ref(),
                        &sk::PixmapPaint {
                            quality: sk::FilterQuality::Bicubic,
                            ..Default::default()
                        },
                        transform,
                        None,
                    );
                }
            }
            x += m.advance_width;
        }
    }
    fn picture(&mut self, image: &sk::Pixmap, cx: f32, cy: f32, width: f32) {
        let scale = width / image.width() as f32;
        let height = image.height() as f32 * scale;
        let transform = self
            .transform
            .pre_translate(cx - width / 2.0, cy - height / 2.0)
            .pre_scale(scale, scale);
        self.image.draw_pixmap(
            0,
            0,
            image.as_ref(),
            &sk::PixmapPaint {
                quality: sk::FilterQuality::Bicubic,
                ..Default::default()
            },
            transform,
            None,
        );
    }
    fn finish(self, name: &'static str, display_size: u16) -> GeneratedIcon {
        let (width, height) = (self.image.width(), self.image.height());
        let mut pixels = self.image.take();
        for p in pixels.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        GeneratedIcon {
            name,
            width,
            height,
            pixels,
            display_size,
        }
    }
}
fn circle(x: f32, y: f32, r: f32) -> sk::Path {
    let mut b = sk::PathBuilder::new();
    b.push_circle(x, y, r);
    b.finish().unwrap()
}
fn brick(p: &mut Pen) {
    let rows = [
        (0xff6b5e, 0xd8281c),
        (0xffb648, 0xe57a0c),
        (0xffe25a, 0xe3b30e),
        (0x7fe06a, 0x2f9f2a),
    ];
    let (bw, bh, gap, x0) = (22.0, 9.5, 2.5, 2.25);
    for (row, (top, bot)) in rows.iter().enumerate() {
        for i in 0..4 {
            let y = 10.0 + row as f32 * (bh + gap);
            if row % 2 == 1 && i == 0 {
                draw_brick(p, x0, y, bw / 2.0 - gap / 2.0, bh, *top, *bot);
                draw_brick(
                    p,
                    x0 + 3.0 * (bw + gap) + bw / 2.0 + gap / 2.0,
                    y,
                    bw / 2.0 - gap / 2.0,
                    bh,
                    *top,
                    *bot,
                );
            } else {
                let off = if row % 2 == 1 { -(bw + gap) / 2.0 } else { 0.0 };
                draw_brick(p, x0 + off + i as f32 * (bw + gap), y, bw, bh, *top, *bot);
            }
        }
    }
    p.fill(
        &circle(58.8, 71.5, 6.0),
        sk::Shader::SolidColor(alpha(0, 0.25)),
    );
    let ball = circle(58.0, 70.0, 6.0);
    let shader = sk::RadialGradient::new(
        sk::Point::from_xy(55.6, 67.6),
        0.0,
        sk::Point::from_xy(58.0, 70.0),
        6.0,
        vec![
            sk::GradientStop::new(0.0, rgb(0xffffff)),
            sk::GradientStop::new(0.6, rgb(0xc9d3de)),
            sk::GradientStop::new(1.0, rgb(0x7d8a99)),
        ],
        sk::SpreadMode::Pad,
        sk::Transform::identity(),
    )
    .unwrap();
    p.fill(&ball, shader);
    p.stroke(&ball, rgb(0x5c6774), 0.8);
    p.solid_round(26.0, 85.5, 40.0, 7.0, 3.5, alpha(0, 0.25));
    p.round(
        26.0,
        84.0,
        40.0,
        7.0,
        3.5,
        &[
            (0.0, rgb(0x9fd2ff)),
            (0.5, rgb(0x2f8ee8)),
            (1.0, rgb(0x1660b8)),
        ],
    );
    p.stroke(&rounded(26.0, 84.0, 40.0, 7.0, 3.5), rgb(0x0f4c94), 0.8);
    let oval =
        sk::PathBuilder::from_oval(sk::Rect::from_xywh(28.0, 84.0, 36.0, 3.6).unwrap()).unwrap();
    p.fill(
        &oval,
        gradient(
            84.0,
            3.6,
            &[(0.0, alpha(0xffffff, 0.8)), (1.0, alpha(0xffffff, 0.0))],
        ),
    );
}
fn draw_brick(p: &mut Pen, x: f32, y: f32, w: f32, h: f32, top: u32, bot: u32) {
    p.solid_round(x, y + 1.2, w, h, 2.0, alpha(0, 0.22));
    let shape = rounded(x, y, w, h, 2.0);
    p.fill(&shape, gradient(y, h, &[(0.0, rgb(top)), (1.0, rgb(bot))]));
    p.stroke(&shape, alpha(0, 0.35), 0.6);
    p.fill(
        &rounded(x + 1.0, y + 0.8, w - 2.0, h * 0.42, 1.5),
        gradient(
            y,
            h * 0.45,
            &[(0.0, alpha(0xffffff, 0.75)), (1.0, alpha(0xffffff, 0.15))],
        ),
    );
}
fn read_picture(path: &Path) -> Result<sk::Pixmap, String> {
    let image = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = image.dimensions();
    let mut pixels = image.into_raw();
    for p in pixels.chunks_exact_mut(4) {
        let a = p[3] as u32;
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    sk::Pixmap::from_vec(
        pixels,
        sk::IntSize::from_wh(w, h).ok_or("Invalid original suit size")?,
    )
    .ok_or_else(|| "Invalid original suit pixels".into())
}
fn solitaire(p: &mut Pen, font: &fontdue::Font, assets: &Path) -> Result<(), String> {
    let saved = p.transform;
    p.transform = p
        .transform
        .pre_translate(50.0, 53.0)
        .pre_scale(0.84, 0.84)
        .pre_translate(-50.0, -53.0);
    let fan = p.transform;
    for (i, (rank, suit, red, angle)) in [
        ("Q", "sol_diamond", true, -18.0),
        ("K", "sol_heart", true, 0.0),
        ("A", "sol_spade", false, 18.0),
    ]
    .iter()
    .enumerate()
    {
        let cx = 50.0 + (i as f32 - 1.0) * 14.0;
        let cy = 54.0 + (i as f32 - 1.0).abs() * 3.0;
        p.transform = fan.pre_rotate_at(*angle, cx, cy + 30.0);
        let (x, y) = (cx - 23.0, cy - 32.0);
        p.solid_round(x + 1.0, y + 2.0, 46.0, 64.0, 5.0, alpha(0, 0.25));
        p.round(
            x,
            y,
            46.0,
            64.0,
            5.0,
            &[(0.0, rgb(0xffffff)), (1.0, rgb(0xe9ebee))],
        );
        p.stroke(&rounded(x, y, 46.0, 64.0, 5.0), rgb(0x969ba2), 0.9);
        // gg's DrawStringAnchored(.5,.5) uses face height, not ink bounds.
        let height = font
            .horizontal_line_metrics(13.0)
            .map(|m| m.new_line_size)
            .unwrap_or(15.73);
        p.text(
            font,
            rank,
            x + 7.0,
            y + 9.0 + height / 2.0,
            13.0,
            if *red { rgb(0xd42a1e) } else { rgb(0x1e2024) },
        );
        let suit = read_picture(&assets.join(format!("ipod/{suit}.png")))?;
        p.picture(&suit, x + 23.0, y + 35.0, 22.0);
        p.picture(&suit, x + 7.0, y + 18.0, 7.0);
    }
    p.transform = saved;
    Ok(())
}
fn ascent(size: f32) -> f32 {
    (size * 0.96875).round()
}
fn tiles(p: &mut Pen, font: &fontdue::Font) {
    p.round(
        8.0,
        8.0,
        176.0,
        176.0,
        26.0,
        &[(0.0, rgb(0x7a8494)), (1.0, rgb(0x5c6575))],
    );
    for (i, (value, top, bot, dark)) in [
        ("2", 0xfafbfd, 0xdadfe7, true),
        ("4", 0xfaf4e2, 0xe4d6b0, true),
        ("8", 0xffbe78, 0xf07a1e, false),
        ("2048", 0xc8a0ff, 0x7a3fe0, false),
    ]
    .iter()
    .enumerate()
    {
        let x = 18.0 + (i % 2) as f32 * 83.0;
        let y = 18.0 + (i / 2) as f32 * 83.0;
        p.round(
            x,
            y,
            73.0,
            73.0,
            14.0,
            &[(0.0, rgb(*top)), (1.0, rgb(*bot))],
        );
        p.round(
            x + 2.0,
            y + 2.0,
            69.0,
            34.5,
            12.0,
            &[(0.0, alpha(0xffffff, 0.45)), (1.0, alpha(0xffffff, 0.05))],
        );
        let size = if value.len() == 1 { 52.0 } else { 24.0 };
        p.text(
            font,
            value,
            x + 36.5,
            y + 36.5 + ascent(size) / 2.0 - 2.0,
            size,
            rgb(if *dark { 0x3a3e46 } else { 0xffffff }),
        );
    }
}
fn words(p: &mut Pen, font: &fontdue::Font) {
    p.round(
        12.0,
        12.0,
        168.0,
        168.0,
        36.0,
        &[(0.0, rgb(0x626c7e)), (1.0, rgb(0x2c3240))],
    );
    let states = [[0, 0, 1], [0, 2, 0], [2, 2, 2]];
    for (r, row) in ["TEA", "OAK", "PAW"].iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            let (top, bot) = match states[r][c] {
                1 => (0xffc45c, 0xe68410),
                2 => (0x76ce78, 0x28963e),
                _ => (0x969eac, 0x687080),
            };
            let x = 31.0 + c as f32 * 46.0;
            let y = 31.0 + r as f32 * 46.0;
            p.round(x, y, 38.0, 38.0, 7.0, &[(0.0, rgb(top)), (1.0, rgb(bot))]);
            p.round(
                x + 2.0,
                y + 2.0,
                34.0,
                15.0,
                6.0,
                &[(0.0, alpha(0xffffff, 0.42)), (1.0, alpha(0xffffff, 0.05))],
            );
            p.text(
                font,
                &ch.to_string(),
                x + 19.0,
                y + 19.0 + ascent(26.0) / 2.0 - 2.0,
                26.0,
                rgb(0xffffff),
            );
        }
    }
    p.round(
        15.0,
        15.0,
        162.0,
        81.0,
        32.0,
        &[(0.0, alpha(0xffffff, 0.3)), (1.0, alpha(0xffffff, 0.03))],
    );
}
fn quiz(p: &mut Pen, font: &fontdue::Font) {
    p.round(
        12.0,
        12.0,
        168.0,
        168.0,
        36.0,
        &[(0.0, rgb(0xff78aa)), (1.0, rgb(0xc8286e))],
    );
    p.round(
        15.0,
        15.0,
        162.0,
        81.0,
        32.0,
        &[(0.0, alpha(0xffffff, 0.45)), (1.0, alpha(0xffffff, 0.05))],
    );
    for (i, h) in [34.0, 56.0, 44.0, 62.0, 30.0].iter().enumerate() {
        p.round(
            46.0 + i as f32 * 22.0,
            152.0 - h,
            14.0,
            *h,
            5.0,
            &[(0.0, alpha(0xffffff, 0.55)), (1.0, alpha(0xffffff, 0.3))],
        );
    }
    p.text(
        font,
        "?",
        96.0,
        96.0 + ascent(100.0) / 2.0 - 14.0,
        100.0,
        rgb(0xffffff),
    );
}
fn puzzles(p: &mut Pen, _: &fontdue::Font) {
    p.round(
        12.0,
        12.0,
        168.0,
        168.0,
        36.0,
        &[(0.0, rgb(0xffc45c)), (1.0, rgb(0xe2780a))],
    );
    for (r, row) in [[1, 0, 1], [0, 1, 0], [1, 1, 0]].iter().enumerate() {
        for (c, on) in row.iter().enumerate() {
            let (top, bot, a) = if *on == 1 {
                (0x784614, 0x5a320a, 0.85)
            } else {
                (0xffffff, 0xfff4e2, 0.95)
            };
            p.round(
                40.0 + c as f32 * 40.0,
                40.0 + r as f32 * 40.0,
                34.0,
                34.0,
                7.0,
                &[(0.0, alpha(top, a)), (1.0, alpha(bot, a))],
            );
        }
    }
    p.round(
        15.0,
        15.0,
        162.0,
        81.0,
        32.0,
        &[(0.0, alpha(0xffffff, 0.4)), (1.0, alpha(0xffffff, 0.04))],
    );
}
fn videos(p: &mut Pen, _: &fontdue::Font) {
    p.round(
        12.0,
        12.0,
        168.0,
        168.0,
        36.0,
        &[(0.0, rgb(0x566276)), (1.0, rgb(0x1c212c))],
    );
    p.round(
        30.0,
        48.0,
        132.0,
        96.0,
        10.0,
        &[(0.0, rgb(0x2878dc)), (1.0, rgb(0x123478))],
    );
    for i in 0..6 {
        for y in [30.0, 152.0] {
            p.round(
                34.0 + i as f32 * 22.0,
                y,
                12.0,
                10.0,
                3.0,
                &[(0.0, alpha(0xffffff, 0.75)), (1.0, alpha(0xffffff, 0.55))],
            );
        }
    }
    let mut mark = sk::PathBuilder::new();
    mark.move_to(76.0, 68.0);
    mark.line_to(123.6, 96.0);
    mark.line_to(76.0, 124.0);
    mark.close();
    p.fill(
        &mark.finish().unwrap(),
        sk::Shader::SolidColor(alpha(0xffffff, 0.95)),
    );
    p.round(
        15.0,
        15.0,
        162.0,
        81.0,
        32.0,
        &[(0.0, alpha(0xffffff, 0.4)), (1.0, alpha(0xffffff, 0.04))],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assets() -> std::path::PathBuf {
        let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("../port/betterpod/assets");
        let root = std::env::var_os("BETTERPOD_RUST_ASSETS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                if host.is_dir() {
                    host
                } else {
                    "/repo/port/betterpod/assets".into()
                }
            });
        assert!(
            root.is_dir(),
            "Original asset fixture directory is required: {}",
            root.display()
        );
        root
    }
    #[test]
    fn original_shapes_have_double_resolution_and_transparent_margins() {
        let icons = generate(&assets()).unwrap();
        assert_eq!(icons.len(), 7);
        let mut names = std::collections::HashSet::new();
        for icon in icons {
            assert!(names.insert(icon.name));
            assert_eq!(icon.width, icon.display_size as u32 * 2);
            assert_eq!(icon.width, icon.height);
            assert_eq!(
                icon.pixels.len(),
                icon.width as usize * icon.height as usize * 4
            );
            assert_eq!(icon.pixels[3], 0);
            assert!(icon.pixels.chunks_exact(4).any(|p| p[3] == 255));
            assert!(icon.pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        }
    }
    #[test]
    #[ignore = "writes local source-faithful HD visual fixtures"]
    fn dump_icons() -> Result<(), String> {
        let out = std::path::PathBuf::from(
            std::env::var("BETTERPOD_PREVIEW_ICONS_OUT").map_err(|e| e.to_string())?,
        );
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        for icon in generate(&assets())? {
            image::save_buffer(
                out.join(format!("{}.png", icon.name)),
                &icon.pixels,
                icon.width,
                icon.height,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "compares executed Go source rasters with the higher-resolution Rust forms"]
    fn compare_original_rasters() -> Result<(), String> {
        let original = std::path::PathBuf::from(
            std::env::var("BETTERPOD_ORIGINAL_ICONS").map_err(|e| e.to_string())?,
        );
        let output = std::path::PathBuf::from(
            std::env::var("BETTERPOD_ICON_COMPARISON_OUT").map_err(|e| e.to_string())?,
        );
        let mut comparisons = Vec::new();
        for icon in generate(&assets())? {
            let reference = image::open(original.join(format!("{}.png", icon.name)))
                .map_err(|e| e.to_string())?
                .to_rgba8();
            let native = image::RgbaImage::from_raw(icon.width, icon.height, icon.pixels).unwrap();
            assert_eq!(native.width(), reference.width() * 2);
            assert_eq!(native.height(), reference.height() * 2);
            let sampled = image::imageops::resize(
                &native,
                reference.width(),
                reference.height(),
                image::imageops::FilterType::Lanczos3,
            );
            let (mut intersection, mut union, mut error, mut interior_error, mut interior) =
                (0usize, 0usize, 0u64, 0u64, 0usize);
            for (a, b) in reference.pixels().zip(sampled.pixels()) {
                let (inside_a, inside_b) = (a[3] >= 128, b[3] >= 128);
                intersection += usize::from(inside_a && inside_b);
                union += usize::from(inside_a || inside_b);
                for channel in 0..4 {
                    error += a[channel].abs_diff(b[channel]) as u64;
                }
                if a[3] == 255 && b[3] == 255 {
                    interior += 1;
                    for channel in 0..3 {
                        interior_error += a[channel].abs_diff(b[channel]) as u64;
                    }
                }
            }
            let iou = intersection as f64 / union as f64;
            assert!(iou > 0.98, "{} silhouette IoU {iou}", icon.name);
            comparisons.push(serde_json::json!({"name":icon.name,"source_size":reference.width(),"native_size":native.width(),"silhouette_iou":iou,"mean_absolute_rgba_difference":error as f64/(reference.width() as f64*reference.height() as f64*4.0),"opaque_rgb_difference":interior_error as f64/(interior.max(1)*3) as f64,"pixel_equal":false,"sampling":"Rust native2x reduced with Lanczos3 for comparison; resolution improvement intentionally changes edge coverage"}));
        }
        std::fs::write(
            output,
            serde_json::to_vec_pretty(&comparisons).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
