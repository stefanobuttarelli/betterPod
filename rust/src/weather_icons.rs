//! Original ij_weather_icons.go shapes at the requested physical resolution.
//! The bounded worker rasterizes only icons. The UI uploads completed pixels
//! and draws cached textures; audio and the frame loop never render paths.
use crate::render::{Color, Renderer, Texture};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Instant,
};
use tiny_skia as sk;
const FRAMES: i16 = 60;
const MAX_BYTES: usize = 32 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Kind {
    Sun,
    Moon,
    PartlySun,
    PartlyMoon,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Sleet,
    Thunder,
}
pub fn kind(code: i32, day: bool) -> Kind {
    match code {
        0 | 1 => {
            if day {
                Kind::Sun
            } else {
                Kind::Moon
            }
        }
        2 => {
            if day {
                Kind::PartlySun
            } else {
                Kind::PartlyMoon
            }
        }
        3 => Kind::Cloudy,
        45 | 48 => Kind::Fog,
        51..=55 => Kind::Drizzle,
        56 | 57 | 66 | 67 => Kind::Sleet,
        61..=65 | 80..=82 => Kind::Rain,
        71..=77 | 85 | 86 => Kind::Snow,
        95..=i32::MAX => Kind::Thunder,
        _ => Kind::Cloudy,
    }
}
pub fn sky(code: i32, day: bool) -> [Color; 3] {
    let k = kind(code, day);
    let colors = if !day {
        match k {
            Kind::Moon | Kind::Sun | Kind::PartlyMoon | Kind::PartlySun => {
                [0x06112b, 0x13306a, 0x2b4f8e]
            }
            Kind::Snow | Kind::Sleet => [0x1a2740, 0x3a4f70, 0x62789a],
            _ => [0x141b2b, 0x2c3850, 0x4a5a76],
        }
    } else {
        match k {
            Kind::Sun => [0x1f6fd0, 0x58a8ec, 0xbfe4fb],
            Kind::PartlySun => [0x3a82d2, 0x7ab6ea, 0xcfe7f8],
            Kind::Cloudy | Kind::Fog => [0x5c7593, 0x8ea4bd, 0xc9d5e2],
            Kind::Drizzle | Kind::Rain => [0x3d506b, 0x66809e, 0x9fb2c7],
            Kind::Thunder => [0x232c3d, 0x46546d, 0x77869e],
            Kind::Snow | Kind::Sleet => [0x7f9fc0, 0xb4cbe0, 0xeef4fa],
            _ => [0x3a82d2, 0x7ab6ea, 0xcfe7f8],
        }
    };
    colors.map(|c| {
        [
            ((c >> 16) & 255) as f32 / 255.,
            ((c >> 8) & 255) as f32 / 255.,
            (c & 255) as f32 / 255.,
            1.,
        ]
    })
}
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Key {
    kind: Kind,
    size: u16,
    frame: i16,
}
struct Pixels {
    key: Key,
    pixels: Vec<u8>,
}
struct Cache {
    tx: Option<mpsc::SyncSender<Key>>,
    rx: mpsc::Receiver<Pixels>,
    worker: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pending: HashSet<Key>,
    textures: HashMap<Key, (Texture, Instant)>,
    bytes: usize,
}
impl Cache {
    fn new() -> Self {
        let (tx, jobs) = mpsc::sync_channel::<Key>(48);
        let (results, rx) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::Builder::new()
            .name("weather-icons".into())
            .spawn(move || {
                while let Ok(key) = jobs.recv() {
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let pixels = raster(
                        key.kind,
                        key.size as u32,
                        if key.frame < 0 {
                            0.3
                        } else {
                            key.frame as f64 / FRAMES as f64
                        },
                    );
                    if results.send(Pixels { key, pixels }).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self {
            tx: Some(tx),
            rx,
            worker,
            stop,
            pending: HashSet::new(),
            textures: HashMap::new(),
            bytes: 0,
        }
    }
    fn request(&mut self, key: Key) {
        if self.textures.contains_key(&key) || self.pending.contains(&key) {
            return;
        }
        if self.tx.as_ref().is_some_and(|tx| tx.try_send(key).is_ok()) {
            self.pending.insert(key);
        }
    }
    fn poll(&mut self, r: &Renderer) {
        for _ in 0..2 {
            let Ok(result) = self.rx.try_recv() else {
                break;
            };
            self.pending.remove(&result.key);
            if let Ok(texture) = r.upload(
                result.key.size as u32,
                result.key.size as u32,
                &result.pixels,
            ) {
                if let Some((old, _)) = self.textures.insert(result.key, (texture, Instant::now()))
                {
                    r.delete_texture(old);
                } else {
                    self.bytes += result.pixels.len();
                }
            }
        }
        while self.bytes > MAX_BYTES || self.textures.len() > 144 {
            let Some(key) = self
                .textures
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some((texture, _)) = self.textures.remove(&key) {
                self.bytes -= texture.width as usize * texture.height as usize * 4;
                r.delete_texture(texture);
            }
        }
    }
    fn texture(&mut self, key: Key) -> Option<Texture> {
        self.textures.get_mut(&key).map(|(t, used)| {
            *used = Instant::now();
            *t
        })
    }
    fn close(&mut self, r: &Renderer) {
        self.close_with(|texture| r.delete_texture(texture));
    }
    fn close_with(&mut self, mut delete: impl FnMut(Texture)) {
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            while !worker.is_finished() {
                while self.rx.try_recv().is_ok() {}
                thread::yield_now();
            }
            let _ = worker.join();
        }
        for (_, (texture, _)) in self.textures.drain() {
            delete(texture);
        }
        self.pending.clear();
        self.bytes = 0;
    }
}
thread_local! {static CACHE:RefCell<Option<Cache>>=const{RefCell::new(None)};}
pub fn warm_icons(sizes: &[u16]) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let c = cache.get_or_insert_with(Cache::new);
        for &size in sizes.iter().take(3) {
            for kind in [
                Kind::Sun,
                Kind::Moon,
                Kind::PartlySun,
                Kind::PartlyMoon,
                Kind::Cloudy,
                Kind::Fog,
                Kind::Drizzle,
                Kind::Rain,
                Kind::Snow,
                Kind::Sleet,
                Kind::Thunder,
            ] {
                c.request(Key {
                    kind,
                    size: size.clamp(16, 512),
                    frame: -1,
                });
            }
        }
    });
}
pub fn close_icons(r: &Renderer) {
    CACHE.with(|cache| {
        if let Some(mut cache) = cache.borrow_mut().take() {
            cache.close(r)
        }
    });
}
/// Bounded GPU uploads can also run while another page is visible, so a
/// prewarmed icon is already available when its weather panel opens.
pub fn poll_icons(r: &Renderer) {
    CACHE.with(|cache| {
        if let Some(c) = cache.borrow_mut().as_mut() {
            c.poll(r)
        }
    });
}
/// `phase` is normalized 0..1 for the original 3-second, 60-frame loop.
/// A negative phase selects the original still phase (0.3).
pub fn draw_icon(r: &mut Renderer, code: i32, is_day: bool, x: f32, y: f32, size: f32, phase: f64) {
    if !size.is_finite() || size <= 0. {
        return;
    }
    let key = Key {
        kind: kind(code, is_day),
        size: size.ceil().clamp(16., 512.) as u16,
        frame: if phase < 0. {
            -1
        } else {
            ((phase.rem_euclid(1.) * FRAMES as f64).floor() as i16).min(FRAMES - 1)
        },
    };
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let c = cache.get_or_insert_with(Cache::new);
        c.poll(r);
        let still = Key { frame: -1, ..key };
        c.request(still);
        if key.frame >= 0 {
            c.request(key);
        }
        if let Some(t) = c.texture(key).or_else(|| c.texture(still)) {
            r.image_uv(t, [x, y, size, size], [0., 0., 1., 1.], 1.);
        }
    });
}

struct Pen {
    image: sk::Pixmap,
    scale: sk::Transform,
    mask: Option<sk::Mask>,
}
type Stop = (f32, sk::Color);
impl Pen {
    fn new(size: u32) -> Self {
        Self {
            image: sk::Pixmap::new(size, size).unwrap(),
            scale: sk::Transform::from_scale(size as f32 / 100., size as f32 / 100.),
            mask: None,
        }
    }
    fn fill(&mut self, path: &sk::Path, shader: sk::Shader<'static>) {
        let paint = sk::Paint {
            shader,
            anti_alias: true,
            ..sk::Paint::default()
        };
        self.image.fill_path(
            path,
            &paint,
            sk::FillRule::Winding,
            self.scale,
            self.mask.as_ref(),
        );
    }
    fn stroke(&mut self, path: &sk::Path, color: sk::Color, width: f64, round: bool) {
        let paint = sk::Paint {
            shader: sk::Shader::SolidColor(color),
            anti_alias: true,
            ..sk::Paint::default()
        };
        let stroke = sk::Stroke {
            width: width as f32,
            line_cap: if round {
                sk::LineCap::Round
            } else {
                sk::LineCap::Butt
            },
            line_join: if round {
                sk::LineJoin::Round
            } else {
                sk::LineJoin::Miter
            },
            ..sk::Stroke::default()
        };
        self.image
            .stroke_path(path, &paint, &stroke, self.scale, self.mask.as_ref());
    }
    fn clip(&mut self, path: &sk::Path, invert: bool) {
        let mut mask = sk::Mask::new(self.image.width(), self.image.height()).unwrap();
        mask.fill_path(path, sk::FillRule::Winding, true, self.scale);
        if invert {
            mask.invert()
        }
        self.mask = Some(mask);
    }
    fn reset_clip(&mut self) {
        self.mask = None;
    }
    fn gloss(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, a: f64) {
        let path = ellipse(cx, cy, rx, ry);
        self.fill(
            &path,
            linear(
                0.,
                cy - ry,
                0.,
                cy + ry,
                &[(0., rgba(255, 255, 255, a)), (1., rgba(255, 255, 255, 0.))],
            ),
        );
    }
    fn sun(&mut self, cx: f64, cy: f64, r: f64, ray: f64, angle: f64) {
        self.fill(
            &circle(cx, cy, r + ray + 3.),
            radial(
                cx,
                cy,
                r,
                cx,
                cy,
                r + ray + 3.,
                &[(0., rgba(255, 214, 70, 0.45)), (1., rgba(255, 200, 60, 0.))],
            ),
        );
        let mut b = sk::PathBuilder::new();
        for i in 0..12 {
            let a = angle + i as f64 * std::f64::consts::PI / 6.;
            let w = 0.17;
            let r0 = r + 1.5;
            let r1 = r + if i % 2 == 1 { ray * 0.72 } else { ray };
            b.move_to(
                (cx + r0 * (a - w).cos()) as f32,
                (cy + r0 * (a - w).sin()) as f32,
            );
            b.quad_to(
                (cx + (r1 + 1.5) * a.cos()) as f32,
                (cy + (r1 + 1.5) * a.sin()) as f32,
                (cx + r0 * (a + w).cos()) as f32,
                (cy + r0 * (a + w).sin()) as f32,
            );
            b.close();
        }
        let path = b.finish().unwrap();
        self.fill(
            &path,
            radial(
                cx,
                cy,
                r,
                cx,
                cy,
                r + ray,
                &[(0., hex(0xffc93a)), (1., hex(0xff8a12))],
            ),
        );
        self.stroke(&path, rgba(214, 110, 0, 0.75), 0.9, false);
        self.fill(&circle(cx, cy + 1.2, r), solid(rgba(150, 70, 0, 0.25)));
        let path = circle(cx, cy, r);
        self.fill(
            &path,
            radial(
                cx - r * 0.3,
                cy - r * 0.35,
                0.,
                cx,
                cy,
                r * 1.05,
                &[
                    (0., hex(0xfffbe0)),
                    (0.35, hex(0xffe35a)),
                    (0.8, hex(0xffb21e)),
                    (1., hex(0xff9612)),
                ],
            ),
        );
        self.stroke(&path, hex(0xe07800), 1.3, false);
        self.gloss(cx, cy - r * 0.42, r * 0.66, r * 0.42, 0.85);
    }
    fn moon(&mut self, cx: f64, cy: f64, r: f64) {
        let (ix, iy, ir) = (cx + r * 0.5, cy - r * 0.32, r * 0.86);
        let inner = circle(ix, iy, ir);
        self.clip(&inner, true);
        self.fill(
            &circle(cx, cy, r + 6.),
            radial(
                cx,
                cy,
                r * 0.9,
                cx,
                cy,
                r + 6.,
                &[
                    (0., rgba(255, 248, 200, 0.4)),
                    (1., rgba(255, 248, 200, 0.)),
                ],
            ),
        );
        self.fill(&circle(cx, cy + 1.2, r), solid(rgba(60, 50, 0, 0.25)));
        let path = circle(cx, cy, r);
        self.fill(
            &path,
            radial(
                cx - r * 0.4,
                cy - r * 0.2,
                0.,
                cx,
                cy,
                r,
                &[
                    (0., hex(0xfffdf0)),
                    (0.5, hex(0xfbeaa0)),
                    (1., hex(0xe2c25a)),
                ],
            ),
        );
        self.stroke(&path, hex(0xb89530), 1.3, false);
        self.gloss(cx - r * 0.35, cy - r * 0.25, r * 0.32, r * 0.5, 0.7);
        self.reset_clip();
        self.clip(&path, false);
        self.stroke(&inner, hex(0xc9a640), 1.1, false);
        self.reset_clip();
    }
    fn star(&mut self, x: f64, y: f64, r: f64, a: f64) {
        if a <= 0.02 {
            return;
        }
        let mut b = sk::PathBuilder::new();
        b.move_to(x as f32, (y - r) as f32);
        for (ex, ey) in [(x + r, y), (x, y + r), (x - r, y), (x, y - r)] {
            b.quad_to(x as f32, y as f32, ex as f32, ey as f32);
        }
        b.close();
        self.fill(&b.finish().unwrap(), solid(rgba(255, 250, 215, a)));
    }
    fn cloud(&mut self, x: f64, y: f64, w: f64, tone: u8) {
        let h = w * 0.62;
        let shape = |grow: f64, dy: f64| {
            let mut b = sk::PathBuilder::new();
            for (c0, c1, c2) in [(0.30, 0.56, 0.22), (0.53, 0.42, 0.29), (0.76, 0.60, 0.19)] {
                b.push_circle(
                    (x + c0 * w) as f32,
                    (y + c1 * h + dy) as f32,
                    (c2 * w + grow) as f32,
                );
            }
            let x0 = x + 0.08 * w - grow;
            let x1 = x + 0.94 * w + grow;
            let y0 = y + 0.52 * h - grow + dy;
            let y1 = y + 0.92 * h + grow + dy;
            rounded(&mut b, x0, y0, x1 - x0, y1 - y0, (y1 - y0) * 0.5);
            b.finish().unwrap()
        };
        let (top, bottom, line) = match tone {
            1 => (hex(0xf2f4f7), hex(0x98a5b6), hex(0x66738a)),
            2 => (hex(0xd4d9e1), hex(0x5f6a7c), hex(0x454f60)),
            _ => (hex(0xffffff), hex(0xc7d6e8), hex(0x7f97b4)),
        };
        self.fill(&shape(1.2, 2.2), solid(rgba(10, 30, 60, 0.22)));
        self.fill(&shape(1.3, 0.), solid(line));
        self.fill(
            &shape(0., 0.),
            linear(
                0.,
                y,
                0.,
                y + h * 0.95,
                &[(0., top), (0.55, mix(top, bottom, 0.35)), (1., bottom)],
            ),
        );
        self.gloss(
            x + 0.53 * w - w * 0.03,
            y + 0.42 * h - 0.29 * w * 0.45,
            0.29 * w * 0.72,
            0.29 * w * 0.5,
            0.9,
        );
    }
    fn drop(&mut self, x: f64, y: f64, s: f64, a: f64) {
        if a <= 0.02 {
            return;
        }
        let radius = s * 0.32;
        let cy = y - radius;
        let mut b = sk::PathBuilder::new();
        b.move_to(x as f32, (y - s) as f32);
        b.quad_to(
            (x + radius * 1.05) as f32,
            (cy - radius * 0.9) as f32,
            (x + radius) as f32,
            cy as f32,
        );
        arc(
            &mut b,
            x,
            cy,
            radius,
            radius,
            0.,
            std::f64::consts::PI,
            false,
        );
        b.quad_to(
            (x - radius * 1.05) as f32,
            (cy - radius * 0.9) as f32,
            x as f32,
            (y - s) as f32,
        );
        b.close();
        let path = b.finish().unwrap();
        self.fill(
            &path,
            linear(
                x - radius,
                y - s,
                x + radius,
                y,
                &[
                    (0., rgba(196, 238, 255, a)),
                    (0.5, rgba(64, 160, 240, a)),
                    (1., rgba(20, 96, 205, a)),
                ],
            ),
        );
        self.stroke(&path, rgba(18, 80, 170, a), 0.8, false);
        self.fill(
            &ellipse(
                x - radius * 0.38,
                cy - radius * 0.2,
                radius * 0.22,
                radius * 0.42,
            ),
            solid(rgba(255, 255, 255, 0.8 * a)),
        );
    }
    fn flake(&mut self, x: f64, y: f64, r: f64, rotation: f64, a: f64) {
        if a <= 0.02 {
            return;
        }
        let mut b = sk::PathBuilder::new();
        for i in 0..6 {
            let angle = rotation + i as f64 * std::f64::consts::PI / 3.;
            b.move_to(x as f32, y as f32);
            b.line_to((x + r * angle.cos()) as f32, (y + r * angle.sin()) as f32);
            let (mx, my) = (x + r * 0.55 * angle.cos(), y + r * 0.55 * angle.sin());
            for d in [-0.7, 0.7] {
                b.move_to(mx as f32, my as f32);
                b.line_to(
                    (mx + r * 0.32 * (angle + d).cos()) as f32,
                    (my + r * 0.32 * (angle + d).sin()) as f32,
                );
            }
        }
        let path = b.finish().unwrap();
        self.stroke(&path, rgba(70, 130, 200, a), 3.4, true);
        self.stroke(&path, rgba(255, 255, 255, a), 1.7, true);
    }
    fn bolt(&mut self, x: f64, y: f64, s: f64, a: f64) {
        let path = |dx: f64, dy: f64| {
            let mut b = sk::PathBuilder::new();
            for (i, (px, py)) in [
                (0.52, 0.),
                (0.08, 0.56),
                (0.40, 0.56),
                (0.22, 1.),
                (0.86, 0.38),
                (0.52, 0.38),
                (0.74, 0.),
            ]
            .into_iter()
            .enumerate()
            {
                let x = (x + px * s * 0.7 + dx) as f32;
                let y = (y + py * s + dy) as f32;
                if i == 0 {
                    b.move_to(x, y)
                } else {
                    b.line_to(x, y)
                }
            }
            b.close();
            b.finish().unwrap()
        };
        if a > 0.6 {
            self.fill(
                &circle(x + s * 0.33, y + s * 0.5, s * 0.75),
                radial(
                    x + s * 0.33,
                    y + s * 0.5,
                    0.,
                    x + s * 0.33,
                    y + s * 0.5,
                    s * 0.75,
                    &[
                        (0., rgba(255, 250, 190, 0.55 * (a - 0.6) / 0.4)),
                        (1., rgba(255, 250, 190, 0.)),
                    ],
                ),
            );
        }
        self.fill(&path(0.8, 1.5), solid(rgba(90, 50, 0, 0.25)));
        let path = path(0., 0.);
        self.fill(
            &path,
            linear(
                x,
                y,
                x + s * 0.5,
                y + s,
                &[
                    (0., hex(0xfffbb0)),
                    (0.5, hex(0xffd52a)),
                    (1., hex(0xff9e0d)),
                ],
            ),
        );
        self.stroke(&path, hex(0xc97800), 1.2, true);
    }
    fn fog(&mut self, y: f64, t: f64) {
        for (i, w) in [62., 72., 54.].into_iter().enumerate() {
            let dx = (std::f64::consts::TAU * (t + i as f64 * 0.33)).sin() * 3.;
            let x = 50. - w / 2. + dx + (i % 2) as f64 * 4. - 2.;
            let y = y + i as f64 * 9.5;
            let mut b = sk::PathBuilder::new();
            rounded(&mut b, x, y + 1.2, w, 6., 3.);
            self.fill(&b.finish().unwrap(), solid(rgba(20, 40, 70, 0.18)));
            let mut b = sk::PathBuilder::new();
            rounded(&mut b, x, y, w, 6., 3.);
            let path = b.finish().unwrap();
            self.fill(
                &path,
                linear(
                    0.,
                    y,
                    0.,
                    y + 6.,
                    &[(0., hex(0xffffff)), (1., hex(0xb9c6d6))],
                ),
            );
            self.stroke(&path, hex(0x8796ab), 0.9, false);
        }
    }
    fn draw(&mut self, kind: Kind, t: f64) {
        let drift = (std::f64::consts::TAU * t).sin() * 2.2;
        match kind {
            Kind::Sun => self.sun(50., 50., 22., 17., t * std::f64::consts::PI / 6.),
            Kind::Moon => {
                self.moon(48., 52., 27.);
                for (i, (x, y, r)) in [(80., 22., 5.), (86., 48., 3.5), (66., 12., 3.)]
                    .into_iter()
                    .enumerate()
                {
                    self.star(
                        x,
                        y,
                        r,
                        0.55 + 0.45 * (std::f64::consts::TAU * (t + i as f64 * 0.37)).sin(),
                    );
                }
            }
            Kind::PartlySun => {
                self.sun(37., 36., 17., 12., t * std::f64::consts::PI / 6.);
                self.cloud(20. + drift, 38., 74., 0);
            }
            Kind::PartlyMoon => {
                self.moon(38., 36., 20.);
                self.star(
                    78.,
                    18.,
                    4.,
                    0.55 + 0.45 * (std::f64::consts::TAU * t).sin(),
                );
                self.cloud(20. + drift, 40., 72., 0);
            }
            Kind::Cloudy => {
                self.cloud(40. - drift * 0.6, 18., 54., 1);
                self.cloud(6. + drift, 36., 80., 0);
            }
            Kind::Fog => {
                self.cloud(14. + drift * 0.5, 14., 72., 1);
                self.fog(64., t);
            }
            Kind::Drizzle | Kind::Rain | Kind::Thunder | Kind::Sleet => {
                let (n, size) = if kind == Kind::Drizzle {
                    (3, 11.)
                } else {
                    (4, 16.)
                };
                for i in 0..n {
                    let x = if n == 3 {
                        [34., 52., 70.][i]
                    } else {
                        [30., 47., 64., 79.][i]
                    };
                    let (y, a) = falling(t, [0., 0.55, 0.25, 0.8][i], 2., 68., 99.);
                    if kind == Kind::Sleet && i % 2 == 1 {
                        self.flake(x, y - 6., 6.5, t * std::f64::consts::PI, a)
                    } else {
                        self.drop(x - drift * 0.3, y, size, a);
                    }
                }
                self.cloud(
                    8. + drift,
                    12.,
                    84.,
                    if kind == Kind::Thunder { 2 } else { 1 },
                );
                if kind == Kind::Thunder {
                    self.bolt(42., 50., 40., if (t * 2.) % 1. < 0.12 { 1. } else { 0.55 });
                }
            }
            Kind::Snow => {
                for (i, x) in [30., 50., 70.].into_iter().enumerate() {
                    let offset = [0., 0.4, 0.75][i];
                    let (y, a) = falling(t, offset, 1., 66., 100.);
                    self.flake(
                        x + (std::f64::consts::TAU * (t + offset)).sin() * 3.,
                        y - 6.,
                        7.5,
                        t * std::f64::consts::TAU / 3. + i as f64,
                        a,
                    );
                }
                self.cloud(8. + drift, 12., 84., 0);
            }
        }
    }
}
fn rgba(r: u8, g: u8, b: u8, a: f64) -> sk::Color {
    sk::Color::from_rgba(
        r as f32 / 255.,
        g as f32 / 255.,
        b as f32 / 255.,
        a.clamp(0., 1.) as f32,
    )
    .unwrap()
}
fn hex(c: u32) -> sk::Color {
    rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 1.)
}
fn mix(a: sk::Color, b: sk::Color, t: f32) -> sk::Color {
    sk::Color::from_rgba(
        a.red() + (b.red() - a.red()) * t,
        a.green() + (b.green() - a.green()) * t,
        a.blue() + (b.blue() - a.blue()) * t,
        a.alpha() + (b.alpha() - a.alpha()) * t,
    )
    .unwrap()
}
fn solid(c: sk::Color) -> sk::Shader<'static> {
    sk::Shader::SolidColor(c)
}
fn stops(colors: &[Stop]) -> Vec<sk::GradientStop> {
    colors
        .iter()
        .map(|(p, c)| sk::GradientStop::new(*p, *c))
        .collect()
}
fn linear(x0: f64, y0: f64, x1: f64, y1: f64, colors: &[Stop]) -> sk::Shader<'static> {
    sk::LinearGradient::new(
        sk::Point::from_xy(x0 as f32, y0 as f32),
        sk::Point::from_xy(x1 as f32, y1 as f32),
        stops(colors),
        sk::SpreadMode::Pad,
        sk::Transform::identity(),
    )
    .unwrap_or_else(|| solid(colors[0].1))
}
fn radial(
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
    colors: &[Stop],
) -> sk::Shader<'static> {
    sk::RadialGradient::new(
        sk::Point::from_xy(x0 as f32, y0 as f32),
        r0 as f32,
        sk::Point::from_xy(x1 as f32, y1 as f32),
        r1 as f32,
        stops(colors),
        sk::SpreadMode::Pad,
        sk::Transform::identity(),
    )
    .unwrap_or_else(|| solid(colors[0].1))
}
fn circle(x: f64, y: f64, r: f64) -> sk::Path {
    let mut b = sk::PathBuilder::new();
    b.push_circle(x as f32, y as f32, r as f32);
    b.finish().unwrap()
}
fn ellipse(x: f64, y: f64, rx: f64, ry: f64) -> sk::Path {
    let mut b = sk::PathBuilder::new();
    b.push_oval(
        sk::Rect::from_xywh(
            (x - rx) as f32,
            (y - ry) as f32,
            (2. * rx) as f32,
            (2. * ry) as f32,
        )
        .unwrap(),
    );
    b.finish().unwrap()
}
fn arc(b: &mut sk::PathBuilder, x: f64, y: f64, rx: f64, ry: f64, start: f64, end: f64, mov: bool) {
    let n = ((end - start).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.) as usize;
    let step = (end - start) / n as f64;
    if mov {
        b.move_to((x + rx * start.cos()) as f32, (y + ry * start.sin()) as f32)
    }
    for i in 0..n {
        let a = start + i as f64 * step;
        let z = a + step;
        let k = 4. / 3. * ((z - a) / 4.).tan();
        b.cubic_to(
            (x + rx * (a.cos() - k * a.sin())) as f32,
            (y + ry * (a.sin() + k * a.cos())) as f32,
            (x + rx * (z.cos() + k * z.sin())) as f32,
            (y + ry * (z.sin() - k * z.cos())) as f32,
            (x + rx * z.cos()) as f32,
            (y + ry * z.sin()) as f32,
        );
    }
}
fn rounded(b: &mut sk::PathBuilder, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.).min(h / 2.);
    b.move_to((x + r) as f32, y as f32);
    b.line_to((x + w - r) as f32, y as f32);
    arc(
        b,
        x + w - r,
        y + r,
        r,
        r,
        -std::f64::consts::FRAC_PI_2,
        0.,
        false,
    );
    b.line_to((x + w) as f32, (y + h - r) as f32);
    arc(
        b,
        x + w - r,
        y + h - r,
        r,
        r,
        0.,
        std::f64::consts::FRAC_PI_2,
        false,
    );
    b.line_to((x + r) as f32, (y + h) as f32);
    arc(
        b,
        x + r,
        y + h - r,
        r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
        false,
    );
    b.line_to(x as f32, (y + r) as f32);
    arc(
        b,
        x + r,
        y + r,
        r,
        r,
        std::f64::consts::PI,
        3. * std::f64::consts::FRAC_PI_2,
        false,
    );
    b.close();
}
fn falling(t: f64, offset: f64, speed: f64, y0: f64, y1: f64) -> (f64, f64) {
    let phase = (t * speed + offset) % 1.;
    (
        y0 + (y1 - y0) * phase,
        1_f64.min(phase * 5.).min((1. - phase) * 4.),
    )
}
fn raster(kind: Kind, size: u32, phase: f64) -> Vec<u8> {
    let mut pen = Pen::new(size);
    pen.draw(kind, phase);
    let mut pixels = pen.image.take();
    for p in pixels.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    pixels
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_three_native_forecast_sizes_are_prewarmed() {
        warm_icons(&[168, 76, 256]);
        CACHE.with(|slot| {
            let cache = slot.borrow();
            let cache = cache.as_ref().unwrap();
            assert_eq!(cache.pending.len(), 33);
            assert!(cache.pending.iter().any(|key| key.size == 256));
        });
        CACHE.with(|slot| {
            slot.borrow_mut()
                .take()
                .unwrap()
                .close_with(|_| panic!("this test never uploads GPU textures"));
        });
    }
    #[test]
    fn native_icon_worker_restarts_after_in_process_app_shutdown() {
        warm_icons(&[84]);
        CACHE.with(|slot| {
            let mut old = slot.borrow_mut().take().unwrap();
            old.close_with(|_| panic!("this test never uploads GPU textures"));
            drop(old);
            assert!(slot.borrow().is_none());
        });
        warm_icons(&[84]);
        CACHE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let new = slot.as_mut().unwrap();
            let result = new
                .rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("replacement icon worker runs");
            assert_eq!(result.key.size, 84);
            assert_eq!(result.pixels.len(), 84 * 84 * 4);
        });
        CACHE.with(|slot| {
            slot.borrow_mut()
                .take()
                .unwrap()
                .close_with(|_| panic!("no GPU textures"));
        });
    }
    #[test]
    #[ignore = "writes local native-resolution original-weather visual fixtures"]
    fn dump_reference_icons() -> Result<(), String> {
        let out = std::path::PathBuf::from(
            std::env::var("BETTERPOD_WEATHER_ICONS_OUT").map_err(|e| e.to_string())?,
        );
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        for (i, kind) in [
            Kind::Sun,
            Kind::Moon,
            Kind::PartlySun,
            Kind::PartlyMoon,
            Kind::Cloudy,
            Kind::Fog,
            Kind::Drizzle,
            Kind::Rain,
            Kind::Snow,
            Kind::Sleet,
            Kind::Thunder,
        ]
        .into_iter()
        .enumerate()
        {
            for size in [84, 216] {
                let mut pixels = raster(kind, size, 0.3);
                for p in pixels.chunks_exact_mut(4) {
                    let a = p[3] as u32;
                    for (ch, bg) in [80_u32, 112, 144].into_iter().enumerate() {
                        p[ch] = ((p[ch] as u32 * a + bg * (255 - a) + 127) / 255) as u8;
                    }
                    p[3] = 255;
                }
                image::save_buffer(
                    out.join(format!("rust-{i}-{size}.png")),
                    &pixels,
                    size,
                    size,
                    image::ColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    #[test]
    fn source_weather_codes_and_sky_stops() {
        assert_eq!(kind(1, false), Kind::Moon);
        assert_eq!(kind(67, true), Kind::Sleet);
        assert_eq!(kind(96, true), Kind::Thunder);
        assert_eq!(sky(0, true)[0], [31. / 255., 111. / 255., 208. / 255., 1.]);
    }
    #[test]
    fn every_original_shape_renders_at_native_size() {
        for kind in [
            Kind::Sun,
            Kind::Moon,
            Kind::PartlySun,
            Kind::PartlyMoon,
            Kind::Cloudy,
            Kind::Fog,
            Kind::Drizzle,
            Kind::Rain,
            Kind::Snow,
            Kind::Sleet,
            Kind::Thunder,
        ] {
            let p = raster(kind, 216, 0.3);
            assert_eq!(p.len(), 216 * 216 * 4);
            assert!(p.chunks_exact(4).any(|c| c[3] > 200));
            assert!(p.chunks_exact(4).any(|c| c[3] == 0));
        }
    }
}
