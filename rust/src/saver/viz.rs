//! Go 1.1's original 320×240 feedback effects. These are deliberately the
//! original equations and integer colour operations, rather than replacement
//! geometry with a similar name. Only this half-size effects layer is pixels;
//! the player, menus, text, mosaic and cards remain GPU compositions.
use super::{Analysis, Color, Renderer, Texture};
const W: usize = 320;
const H: usize = 240;
const BANDS: usize = 40;
#[derive(Clone, Copy)]
struct Spark {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    life: f64,
    hue: f64,
}
pub(super) struct Visualizer {
    pixels: Vec<u8>,
    scratch: Vec<u8>,
    maps: [Vec<usize>; 3],
    zoom: Vec<usize>,
    map: usize,
    map_at: f64,
    sparks: Vec<Spark>,
    palette: [[u8; 3]; 1024],
    heat: [[u8; 3]; 256],
    bar: Vec<u8>,
    reflection: Vec<u8>,
    cap: [u8; 3],
    texture: Option<Texture>,
    bar_texture: Option<Texture>,
    reflection_texture: Option<Texture>,
    last: Option<f64>,
    effect: u32,
    colors: u32,
    album: Vec<Color>,
    rng: u64,
    dirty: bool,
    rows: [(f64, f64); H],
    row_rate: u32,
    previous: [f64; H],
}
impl Visualizer {
    pub(super) fn new() -> Self {
        let mut s = Self {
            pixels: vec![0; W * H * 4],
            scratch: vec![0; W * H * 4],
            maps: [
                make_map(1.035, 0.025),
                make_map(1.035, -0.025),
                make_map(0.975, 0.012),
            ],
            zoom: make_map(0.985, 0.),
            map: 0,
            map_at: 0.,
            sparks: Vec::with_capacity(700),
            palette: [[0; 3]; 1024],
            heat: [[0; 3]; 256],
            bar: vec![0; 11 * 280 * 4],
            reflection: vec![0; 11 * 150 * 4],
            cap: [230, 245, 255],
            texture: None,
            bar_texture: None,
            reflection_texture: None,
            last: None,
            effect: u32::MAX,
            colors: u32::MAX,
            album: Vec::new(),
            rng: 0x836a2f7b4c921de5,
            dirty: true,
            rows: [(0., 0.); H],
            row_rate: 0,
            previous: [0.; H],
        };
        s.clear();
        s
    }
    fn clear(&mut self) {
        self.pixels.fill(0);
        self.scratch.fill(0);
        for p in self.pixels.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        for p in self.scratch.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        self.sparks.clear();
        self.previous.fill(0.);
    }
    pub(super) fn reset(&mut self) {
        self.clear();
        self.effect = u32::MAX;
        self.last = None;
        self.map = 0;
        self.map_at = 0.;
        self.dirty = true;
    }
    pub(super) fn close(&mut self, r: &Renderer) {
        for t in [
            &mut self.texture,
            &mut self.bar_texture,
            &mut self.reflection_texture,
        ] {
            if let Some(t) = t.take() {
                r.delete_texture(t)
            }
        }
    }
    pub(super) fn render(
        &mut self,
        r: &mut Renderer,
        a: &mut Analysis,
        effect: u32,
        colors: u32,
        album: &[Color],
        now: f64,
    ) -> Result<(), String> {
        let mut changed = false;
        if self.effect != effect {
            self.effect = effect;
            self.clear();
            self.dirty = true;
            changed = true;
        }
        if self.colors != colors || self.album != album {
            self.colors = colors;
            self.album = album.to_vec();
            self.apply_colors(colors, album);
            for t in [&mut self.bar_texture, &mut self.reflection_texture] {
                if let Some(t) = t.take() {
                    r.delete_texture(t)
                }
            }
            changed = true;
        }
        let due = self.last.is_none_or(|last| now - last >= 1. / 30. - 1e-7);
        if due {
            let dt = self.last.map_or(1. / 30., |last| {
                let d = now - last;
                if d > 0.2 || d <= 0. {
                    1. / 30.
                } else {
                    d
                }
            });
            self.last = Some(now);
            a.update(dt);
            match effect {
                1 => self.waves(a),
                2 => self.tunnel(a, now),
                3 => self.stars(a),
                4 => self.spectrogram(a),
                _ => {}
            }
            self.dirty = true;
        }
        if effect == 0 {
            self.draw_bars(r, a)?;
        } else {
            match self.texture {
                None => {
                    let t = r.upload(W as u32, H as u32, &self.pixels)?;
                    r.set_texture_nearest(t);
                    self.texture = Some(t);
                    self.dirty = false;
                }
                Some(t) if self.dirty || changed => {
                    r.update_texture(t, &self.pixels)?;
                    self.dirty = false;
                }
                Some(_) => {}
            }
            r.image_uv(
                self.texture.unwrap(),
                [0., 0., 640., 480.],
                [0., 0., 1., 1.],
                1.,
            );
        }
        Ok(())
    }
    pub(super) fn pixel(&self, a: &Analysis, x: usize, y: usize) -> [u8; 3] {
        if self.effect != 0 {
            let o = ((y / 2).min(H - 1) * W + (x / 2).min(W - 1)) * 4;
            return self.pixels[o..o + 3].try_into().unwrap();
        }
        let total = BANDS * 11 + (BANDS - 1) * 4;
        let x0 = (640 - total) / 2;
        if x < x0 {
            return [0; 3];
        }
        let slot = (x - x0) / 15;
        if slot >= BANDS || (x - x0) % 15 >= 11 {
            return [0; 3];
        }
        let bx = (x - x0) % 15;
        let h = (a.bands[slot] * 280.) as usize;
        let py = 330_i32 - (a.peaks[slot] * 280.) as i32 - 5;
        if y as i32 >= py && (y as i32) < py + 3 {
            return self.cap;
        }
        if h > 0 && y >= 330 - h && y < 330 {
            let o = ((y - (330 - h) + 280 - h) * 11 + bx) * 4;
            return self.bar[o..o + 3].try_into().unwrap();
        }
        if h > 0 && y >= 332 && y < 332 + h.min(150) {
            let o = ((y - 332) * 11 + bx) * 4;
            return self.reflection[o..o + 3].try_into().unwrap();
        }
        [0; 3]
    }
    fn draw_bars(&mut self, r: &mut Renderer, a: &Analysis) -> Result<(), String> {
        if self.bar_texture.is_none() {
            self.bar_texture = Some(r.upload(11, 280, &self.bar)?);
            self.reflection_texture = Some(r.upload(11, 150, &self.reflection)?);
        }
        let total = BANDS * 11 + (BANDS - 1) * 4;
        let x0 = (640 - total) / 2;
        for b in 0..BANDS {
            let x = (x0 + b * 15) as f32;
            let height = (a.bands[b] * 280.) as i32;
            if height > 0 {
                r.image_uv(
                    self.bar_texture.unwrap(),
                    [x, (330 - height) as f32, 11., height as f32],
                    [0., (280 - height) as f32 / 280., 1., 1.],
                    1.,
                );
                let rh = height.min(150);
                r.image_uv(
                    self.reflection_texture.unwrap(),
                    [x, 332., 11., rh as f32],
                    [0., 0., 1., rh as f32 / 150.],
                    1.,
                );
            }
            let py = 330 - (a.peaks[b] * 280.) as i32 - 5;
            r.rect(
                x,
                py as f32,
                11.,
                3.,
                [
                    self.cap[0] as f32 / 255.,
                    self.cap[1] as f32 / 255.,
                    self.cap[2] as f32 / 255.,
                    1.,
                ],
            );
        }
        Ok(())
    }
    fn apply_colors(&mut self, index: u32, album: &[Color]) {
        let stop = scheme(index, album);
        let heat = stop
            .as_ref()
            .cloned()
            .unwrap_or_else(|| scheme(2, &[]).unwrap());
        for i in 0..256 {
            self.heat[i] = at(&heat, i as f64 / 255.);
        }
        for i in 0..1024 {
            let t = i as f64 / 1024.;
            self.palette[i] = if let Some(stop) = stop.as_ref() {
                at(stop, 0.35 + 0.65 * (1. - (2. * t - 1.).abs()))
            } else {
                hsv(t, 0.85, 1.)
            };
        }
        let (bottom, top, cap) = if let Some(stop) = stop.as_ref() {
            (
                at_float(stop, 0.4),
                at_float(stop, 0.92),
                lerp(at(stop, 1.), [255; 3], 0.4),
            )
        } else {
            ([30., 110., 230.], [150., 230., 255.], [230, 245, 255])
        };
        self.cap = cap;
        for y in 0..280 {
            let t = 1. - y as f64 / 280.;
            let c: [f64; 3] = std::array::from_fn(|k| bottom[k] + (top[k] - bottom[k]) * t * t);
            for x in 0..11 {
                let e = 1. - 0.25 * (x as f64 - 5.).abs() / 5.5;
                let o = (y * 11 + x) * 4;
                for k in 0..3 {
                    self.bar[o + k] = (c[k] * e) as u8;
                }
                self.bar[o + 3] = 255;
            }
        }
        for y in 0..150 {
            let fade = 0.28 * (1. - y as f64 / 150.);
            for x in 0..11 {
                let src = ((279 - y) * 11 + x) * 4;
                let dst = (y * 11 + x) * 4;
                for k in 0..3 {
                    self.reflection[dst + k] = (self.bar[src + k] as f64 * fade) as u8;
                }
                self.reflection[dst + 3] = 255;
            }
        }
    }
    fn remap(&mut self, map: usize, fade: u32) {
        let indices = if map == 3 {
            &self.zoom
        } else {
            &self.maps[map]
        };
        for (dst, &src) in indices.iter().enumerate() {
            for c in 0..3 {
                self.scratch[dst * 4 + c] = ((self.pixels[src * 4 + c] as u32 * fade) >> 8) as u8;
            }
            self.scratch[dst * 4 + 3] = 255;
        }
        std::mem::swap(&mut self.pixels, &mut self.scratch);
    }
    fn fade(&mut self, f: u32) {
        for p in self.pixels.as_chunks_mut::<4>().0 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * f) >> 8) as u8;
            }
        }
    }
    fn color(&self, h: f64) -> [u8; 3] {
        self.palette[((((h + 1.) % 1.) * 1023.) as usize) & 1023]
    }
    fn dot(&mut self, x: i32, y: i32, color: [u8; 3]) {
        for dy in -1_i32..=1 {
            let yy = y + dy;
            if !(0..H as i32).contains(&yy) {
                continue;
            }
            for dx in -1_i32..=1 {
                let xx = x + dx;
                if !(0..W as i32).contains(&xx) {
                    continue;
                }
                let shift = if dx != 0 && dy != 0 {
                    2
                } else if dx != 0 || dy != 0 {
                    1
                } else {
                    0
                };
                let o = (yy as usize * W + xx as usize) * 4;
                for c in 0..3 {
                    self.pixels[o + c] = self.pixels[o + c].saturating_add(color[c] >> shift);
                }
            }
        }
    }
    fn line(&mut self, mut x: i32, mut y: i32, x1: i32, y1: i32, c: [u8; 3]) {
        let dx = (x1 - x).abs();
        let dy = -(y1 - y).abs();
        let sx = if x > x1 { -1 } else { 1 };
        let sy = if y > y1 { -1 } else { 1 };
        let mut e = dx + dy;
        for _ in 0..400 {
            self.dot(x, y, c);
            if x == x1 && y == y1 {
                return;
            }
            let e2 = 2 * e;
            if e2 >= dy {
                e += dy;
                x += sx;
            }
            if e2 <= dx {
                e += dx;
                y += sy;
            }
        }
    }
    fn waves(&mut self, a: &Analysis) {
        self.remap(3, 226);
        if !a.fresh {
            return;
        }
        let gain = a
            .samples
            .iter()
            .map(|s| (*s as f64).abs())
            .fold(0., f64::max);
        let amp = 80. / gain.max(0.08);
        let c = self.color(a.hue);
        let mut c2 = self.color(a.hue + 0.5);
        for c in &mut c2 {
            *c /= 2;
        }
        let (mut px, mut py, mut qy) = (0, 120, 120);
        for x in 0..320 {
            let s = a.samples[x * 3] as f64 * amp;
            let y = 120 + s as i32;
            let y2 = 120 - (s * 0.6) as i32;
            if x > 0 {
                self.line(px, py, x as i32, y, c);
                self.line(px, qy, x as i32, y2, c2);
            }
            px = x as i32;
            py = y;
            qy = y2;
        }
    }
    fn tunnel(&mut self, a: &Analysis, now: f64) {
        if a.beat && now - self.map_at > 3. {
            self.map = (self.map + 1 + (self.random() * 2.) as usize) % 3;
            self.map_at = now;
        }
        self.remap(self.map, 236);
        let base = 26. + 24. * a.bass;
        let (mut px, mut py) = (0, 0);
        for i in 0..=80 {
            let mut b = i % 80;
            if b >= 40 {
                b = 79 - b;
            }
            let angle = i as f64 / 80. * std::f64::consts::TAU + a.hue * std::f64::consts::TAU;
            let radius = base + a.bands[b] * 70.;
            let c = self.color(a.hue + b as f64 / 80.);
            let x = 160 + (angle.cos() * radius) as i32;
            let y = 120 + (angle.sin() * radius) as i32;
            if i > 0 {
                self.line(px, py, x, y, c)
            }
            px = x;
            py = y;
        }
    }
    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1_u64 << 53) as f64
    }
    fn emit(&mut self, n: usize, speed: f64, hue: f64) {
        for _ in 0..n {
            if self.sparks.len() >= 700 {
                break;
            }
            let angle = self.random() * std::f64::consts::TAU;
            let speed = speed * (0.4 + self.random());
            let hue = hue + self.random() * 0.25;
            self.sparks.push(Spark {
                x: 160.,
                y: 120.,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: 1.,
                hue,
            });
        }
    }
    fn stars(&mut self, a: &Analysis) {
        self.fade(200);
        let treble = a.bands[20..].iter().sum::<f64>() / 20.;
        if a.fresh {
            self.emit(
                (2. + treble * 14. + a.bass * 6.) as usize,
                1.2 + 2.5 * treble,
                a.hue,
            )
        }
        if a.beat {
            self.emit(90, 3. + 3. * a.bass, a.hue)
        }
        let mut k = 0;
        for i in 0..self.sparks.len() {
            let mut p = self.sparks[i];
            p.x += p.vx;
            p.y += p.vy;
            p.vx *= 1.02;
            p.vy *= 1.02;
            p.life -= 0.012;
            if p.life <= 0. || p.x < 0. || p.y < 0. || p.x >= W as f64 || p.y >= H as f64 {
                continue;
            }
            let mut c = self.color(p.hue);
            for ch in &mut c {
                *ch = (*ch as f64 * p.life) as u8;
            }
            self.dot(p.x as i32, p.y as i32, c);
            self.sparks[k] = p;
            k += 1;
        }
        self.sparks.truncate(k);
    }
    fn spectrogram(&mut self, a: &Analysis) {
        if self.row_rate != a.rate {
            let lo = 40_f64;
            let hi = 16000_f64.min(a.rate as f64 * 0.475);
            let hz = a.rate as f64 / 1024.;
            for y in 0..H {
                self.rows[y] = (
                    lo * (hi / lo).powf(1. - (y + 1) as f64 / H as f64) / hz,
                    lo * (hi / lo).powf(1. - y as f64 / H as f64) / hz,
                );
            }
            self.row_rate = a.rate;
        }
        for y in 0..H {
            let begin = y * W * 4;
            self.pixels.copy_within(begin + 8..begin + W * 4, begin);
        }
        let power = |bin: f64| {
            let i = bin as usize;
            if !(1..511).contains(&i) {
                return 0.;
            }
            let f = bin - i as f64;
            let first = a.spectrum[i];
            let second = a.spectrum[i + 1];
            let pa = first.re * first.re + first.im * first.im;
            let pb = second.re * second.re + second.im * second.im;
            pa + (pb - pa) * f
        };
        for y in 0..H {
            let mut x = 0.;
            if a.fresh {
                let (lo, hi) = self.rows[y];
                let mut p = power((lo + hi) * 0.5);
                let mut b = lo as usize + 1;
                while (b as f64) < hi {
                    p = p.max(power(b as f64));
                    b += 1;
                }
                let db = 10. * (p + 1e-12).log10() + 9. * (1. - y as f64 / H as f64);
                x = (1. + (db - a.norm - 4.) / 60.).clamp(0., 1.);
            }
            let previous = self.previous[y];
            self.previous[y] = x;
            for k in 0..2 {
                let value = previous + (x - previous) * (k + 1) as f64 / 2.;
                let c = self.heat[(value * 255. + 0.5) as usize];
                let o = (y * W + W - 2 + k) * 4;
                self.pixels[o..o + 3].copy_from_slice(&c);
                self.pixels[o + 3] = 255;
            }
        }
    }
}
fn make_map(zoom: f64, rotation: f64) -> Vec<usize> {
    let mut out = Vec::with_capacity(W * H);
    let (sn, cs) = (rotation.sin(), rotation.cos());
    for y in 0..H {
        for x in 0..W {
            let dx = x as f64 - 160.;
            let dy = y as f64 - 120.;
            let sx = 160. + (dx * cs - dy * sn) * zoom;
            let sy = 120. + (dx * sn + dy * cs) * zoom;
            let ix = ((sx + 0.5) as i32).clamp(0, 319) as usize;
            let iy = ((sy + 0.5) as i32).clamp(0, 239) as usize;
            out.push(iy * W + ix)
        }
    }
    out
}
fn lerp(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    std::array::from_fn(|c| ((b[c] as f64 - a[c] as f64).mul_add(t, a[c] as f64)) as u8)
}
fn at(stops: &[[u8; 3]], t: f64) -> [u8; 3] {
    let t = t.clamp(0., 1.) * (stops.len() - 1) as f64;
    let i = (t as usize).min(stops.len() - 2);
    lerp(stops[i], stops[i + 1], t - i as f64)
}
fn at_float(stops: &[[u8; 3]], t: f64) -> [f64; 3] {
    let t = t.clamp(0., 1.) * (stops.len() - 1) as f64;
    let i = (t as usize).min(stops.len() - 2);
    std::array::from_fn(|k| {
        stops[i][k] as f64 + (stops[i + 1][k] as f64 - stops[i][k] as f64) * (t - i as f64)
    })
}
fn scheme(index: u32, album: &[Color]) -> Option<Vec<[u8; 3]>> {
    match index {
        1 if album.len() >= 2 => Some(
            album
                .iter()
                .map(|c| {
                    [
                        (c[0] * 255.) as u8,
                        (c[1] * 255.) as u8,
                        (c[2] * 255.) as u8,
                    ]
                })
                .collect(),
        ),
        2 => Some(vec![
            [0, 0, 4],
            [40, 11, 84],
            [101, 21, 110],
            [159, 42, 99],
            [212, 72, 66],
            [245, 125, 21],
            [250, 193, 39],
            [252, 255, 164],
        ]),
        3 => Some(vec![
            [2, 8, 20],
            [0, 70, 90],
            [0, 170, 140],
            [90, 230, 160],
            [150, 120, 255],
            [230, 205, 255],
        ]),
        4 => Some(vec![
            [10, 4, 30],
            [90, 20, 110],
            [220, 50, 120],
            [255, 120, 80],
            [255, 200, 90],
            [255, 245, 205],
        ]),
        5 => Some(vec![
            [2, 6, 30],
            [10, 40, 120],
            [30, 110, 230],
            [110, 200, 255],
            [220, 245, 255],
        ]),
        6 => Some(vec![
            [0, 8, 2],
            [0, 60, 20],
            [20, 160, 60],
            [110, 255, 140],
            [225, 255, 225],
        ]),
        7 => Some(vec![
            [0, 0, 0],
            [55, 56, 60],
            [150, 151, 156],
            [245, 245, 248],
        ]),
        _ => None,
    }
}
fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let h = (h % 1.) * 6.;
    let i = h.floor();
    let f = h - i;
    let p = v * (1. - s);
    let q = v * (1. - s * f);
    let t = v * (1. - s * (1. - f));
    let c = match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    c.map(|v| (v * 255.) as u8)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "writes local visual reference fixtures; no device or performance run"]
    fn dump_reference_frames() -> Result<(), String> {
        let out = std::path::PathBuf::from(
            std::env::var("BETTERPOD_SAVER_REFERENCE_OUT").map_err(|e| e.to_string())?,
        );
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        for effect in 0..5 {
            let mut v = Visualizer::new();
            v.effect = effect;
            v.apply_colors(0, &[]);
            let mut a = Analysis::new();
            if effect == 3 {
                v.sparks = vec![
                    Spark {
                        x: 160.,
                        y: 120.,
                        vx: 1.3,
                        vy: -0.9,
                        life: 1.,
                        hue: 0.15,
                    },
                    Spark {
                        x: 160.,
                        y: 120.,
                        vx: -0.8,
                        vy: 1.7,
                        life: 1.,
                        hue: 0.7,
                    },
                ];
            }
            for frame in 0..30 {
                let mut samples = [0_f32; 1024];
                for (i, s) in samples.iter_mut().enumerate() {
                    *s = ((i * 37 + frame * 17) % 257) as f32 / 512. - 0.25;
                }
                a.samples(&samples, 44100);
                a.update(1. / 30.);
                match effect {
                    1 => v.waves(&a),
                    2 => v.tunnel(&a, frame as f64 / 30.),
                    3 => {
                        a.fresh = false;
                        a.beat = false;
                        v.stars(&a)
                    }
                    4 => v.spectrogram(&a),
                    _ => {}
                }
            }
            let pixels = if effect == 0 {
                let mut pixels = Vec::with_capacity(640 * 480 * 4);
                for y in 0..480 {
                    for x in 0..640 {
                        pixels.extend_from_slice(&v.pixel(&a, x, y));
                        pixels.push(255);
                    }
                }
                pixels
            } else {
                v.pixels
            };
            std::fs::write(out.join(format!("rust-{effect}.rgba")), pixels)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[test]
    fn source_maps_are_bounded_and_centred() {
        for map in [make_map(1.035, 0.025), make_map(0.985, 0.)] {
            assert_eq!(map.len(), W * H);
            assert!(map.iter().all(|i| *i < W * H));
            assert_eq!(map[120 * W + 160], 120 * W + 160);
        }
    }
    #[test]
    fn classic_and_original_named_scheme_endpoints() {
        assert_eq!(at(&scheme(2, &[]).unwrap(), 0.), [0, 0, 4]);
        assert_eq!(at(&scheme(2, &[]).unwrap(), 1.), [252, 255, 164]);
        assert_eq!(hsv(0., 0.85, 1.), [255, 38, 38]);
    }
    #[test]
    fn source_dot_uses_saturating_soft_edges() {
        let mut v = Visualizer::new();
        v.dot(10, 10, [200, 100, 20]);
        v.dot(10, 10, [200, 100, 20]);
        assert_eq!(
            &v.pixels[(10 * W + 10) * 4..(10 * W + 10) * 4 + 3],
            &[255, 200, 40]
        );
        assert_eq!(v.pixels[(9 * W + 9) * 4], 100);
    }
}
