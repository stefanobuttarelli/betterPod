//! GPU screensavers. Analysis uses a fixed 1024-sample FFT; rendering submits
//! bounded geometry, never a CPU-rendered framebuffer or filesystem operation.
use crate::{
    art::Artwork,
    input::Action,
    library::Library,
    render::{Color, Renderer, BLACK, WHITE},
};
use std::f32::consts::{PI, TAU};

const N: usize = 1024;
const BANDS: usize = 40;
const ROWS: usize = 4;
const COLS: usize = 6;
const CELL: f32 = 110.0;
const HISTORY: usize = 96;
const EFFECTS: [&str; 5] = ["Bars", "Waves", "Tunnel", "Stars", "Spectrogram"];
const PALETTES: [&str; 8] = [
    "Classic", "Album", "Magma", "Aurora", "Sunset", "Ocean", "Phosphor", "Mono",
];
pub fn effect_count(style: &str) -> u32 {
    if style == "viz" {
        EFFECTS.len() as u32
    } else {
        1
    }
}
pub fn effect_name(style: &str, index: u32) -> &'static str {
    if style == "viz" {
        EFFECTS[index as usize % EFFECTS.len()]
    } else {
        "Cover Flip"
    }
}
pub fn palette_count() -> u32 {
    PALETTES.len() as u32
}
pub fn palette_name(index: u32) -> &'static str {
    PALETTES[index as usize % PALETTES.len()]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaverAction {
    OpenAlbum(usize),
    PlayTrack(usize),
    Exit,
}
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}
#[derive(Clone, Copy)]
struct Tile {
    album: Option<usize>,
    next: Option<usize>,
    phase: f32,
}
impl Default for Tile {
    fn default() -> Self {
        Self {
            album: None,
            next: None,
            phase: 1.0,
        }
    }
}
struct Card {
    album: usize,
    tracks: Vec<usize>,
    index: usize,
    open: f32,
    closing: bool,
    from: [f32; 3],
}
#[derive(Clone, Copy, Default)]
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    hue: f32,
}

pub struct Saver {
    style: String,
    effect: u32,
    palette: u32,
    elapsed: f32,
    next_flip: f32,
    name_at: f32,
    name_palette: bool,
    rng: u64,
    library_identity: (usize, usize, usize),
    tiles: [Tile; ROWS * COLS],
    big: Option<usize>,
    big_old: Option<usize>,
    big_phase: f32,
    selected: Option<(usize, usize)>,
    selected_at: f32,
    card: Option<Card>,
    clock_on: bool,
    clock_alpha: f32,
    song_on: bool,
    song_alpha: f32,
    song_until: f32,
    current: Option<usize>,
    pub analysis: Analysis,
    waves: [[f32; 128]; 8],
    wave_cursor: usize,
    history: [[u8; BANDS]; HISTORY],
    history_cursor: usize,
    history_accumulator: f32,
    particles: [Particle; 192],
    particle_cursor: usize,
    album_palette: Vec<Color>,
}
impl Default for Saver {
    fn default() -> Self {
        Self::new()
    }
}
impl Saver {
    pub fn new() -> Self {
        Self {
            style: "cover".into(),
            effect: 0,
            palette: 0,
            elapsed: 0.0,
            next_flip: 5.0,
            name_at: 0.0,
            name_palette: false,
            rng: 0xa93f_72bc_d675_1831,
            library_identity: (0, 0, 0),
            tiles: [Tile::default(); ROWS * COLS],
            big: None,
            big_old: None,
            big_phase: 1.0,
            selected: None,
            selected_at: 0.0,
            card: None,
            clock_on: false,
            clock_alpha: 0.0,
            song_on: false,
            song_alpha: 0.0,
            song_until: 5.0,
            current: None,
            analysis: Analysis::new(),
            waves: [[0.0; 128]; 8],
            wave_cursor: 0,
            history: [[0; BANDS]; HISTORY],
            history_cursor: 0,
            history_accumulator: 0.0,
            particles: [Particle::default(); 192],
            particle_cursor: 0,
            album_palette: Vec::new(),
        }
    }
    pub fn set_style(&mut self, style: &str) {
        self.style = if style == "viz" { "viz" } else { "cover" }.into();
    }
    pub fn set_effect(&mut self, effect: u32) {
        self.effect = effect % effect_count("viz");
        self.name_at = self.elapsed;
        self.name_palette = false;
    }
    pub fn set_palette(&mut self, palette: u32) {
        self.palette = palette % palette_count();
        self.name_at = self.elapsed;
        self.name_palette = true;
    }
    pub fn effect(&self) -> u32 {
        self.effect
    }
    pub fn palette(&self) -> u32 {
        self.palette
    }
    /// Cached cover colours can be supplied by Artwork's decode worker.
    pub fn set_album_palette(&mut self, stops: &[Color]) {
        self.album_palette.clear();
        self.album_palette.extend(stops.iter().take(8).copied());
    }
    pub fn samples(&mut self, samples: &[f32], sample_rate: u32) {
        if self.style == "viz" {
            self.analysis.samples(samples, sample_rate);
        }
    }
    pub fn update(&mut self, dt: f32, library: &Library, current: Option<usize>) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.elapsed += dt;
        let identity = (
            library.albums.as_ptr() as usize,
            library.albums.len(),
            library.tracks.len(),
        );
        if self.library_identity != identity {
            self.library_identity = identity;
            self.tiles.fill(Tile::default());
            self.card = None;
            self.selected = None;
            self.big = None;
            self.big_old = None;
            self.current = None;
            for i in 0..self.tiles.len() {
                self.tiles[i].album = self.pick_album(library.albums.len(), None);
            }
        }
        let current = current.filter(|&i| i < library.tracks.len());
        if current != self.current {
            self.current = current;
            self.song_until = self.elapsed + 5.0;
            let album = current.and_then(|i| {
                library
                    .albums
                    .iter()
                    .position(|a| a.track_indices.contains(&i))
            });
            if album != self.big {
                self.big_old = self.big;
                self.big = album;
                self.big_phase = 0.0;
                for i in 0..self.tiles.len() {
                    if self.tiles[i].album == self.big && self.big.is_some() {
                        self.tiles[i].album = self.pick_album(library.albums.len(), Some(i));
                    }
                }
                self.album_palette.clear();
            }
        }
        self.big_phase = (self.big_phase + dt / 0.7).min(1.0);
        if self.big_phase == 1.0 {
            self.big_old = None;
        }
        if self.selected.is_some() && self.elapsed - self.selected_at > 8.0 && self.card.is_none() {
            self.selected = None;
        }
        for tile in &mut self.tiles {
            if tile.next.is_some() {
                tile.phase = (tile.phase + dt / 0.65).min(1.0);
                if tile.phase == 1.0 {
                    tile.album = tile.next.take();
                }
            }
        }
        if self.style == "cover" && self.card.is_none() && self.elapsed >= self.next_flip {
            self.next_flip = self.elapsed + 5.0;
            for offset in 0..3 {
                let i = (self.random() as usize + offset) % self.tiles.len();
                if self.covered(i / COLS, i % COLS) || self.selected == Some((i / COLS, i % COLS)) {
                    continue;
                }
                let next = self.pick_album(library.albums.len(), None);
                if next.is_some() {
                    self.tiles[i].next = next;
                    self.tiles[i].phase = -(offset as f32) * 0.15;
                }
            }
        }
        if let Some(card) = &mut self.card {
            if let Some(album) = library.albums.get(card.album) {
                if card.tracks.is_empty() {
                    card.tracks = album.track_indices.clone();
                    card.index = current
                        .and_then(|i| card.tracks.iter().position(|&t| t == i))
                        .unwrap_or(0);
                }
            } else {
                card.closing = true;
            }
            card.open =
                (card.open + dt / 0.45 * if card.closing { -1.0 } else { 1.0 }).clamp(0.0, 1.0);
            if card.closing && card.open <= 0.0 {
                self.card = None;
            }
        }
        self.clock_alpha = approach(self.clock_alpha, self.clock_on as u8 as f32, dt * 5.0);
        self.song_alpha = approach(
            self.song_alpha,
            (self.song_on || self.elapsed < self.song_until) as u8 as f32,
            dt * 5.0,
        );
        if self.style != "viz" {
            return;
        }
        self.analysis.update(dt);
        self.history_accumulator += dt;
        if self.history_accumulator >= 1.0 / 30.0 {
            self.history_accumulator %= 1.0 / 30.0;
            for b in 0..BANDS {
                self.history[self.history_cursor][b] =
                    (self.analysis.bands[b] * 255.0).round() as u8;
            }
            self.history_cursor = (self.history_cursor + 1) % HISTORY;
            for i in 0..128 {
                self.waves[self.wave_cursor][i] = self.analysis.samples[i * (N / 128)];
            }
            self.wave_cursor = (self.wave_cursor + 1) % self.waves.len();
        }
        if self.analysis.fresh && (self.analysis.beat || self.analysis.level > 0.025) {
            let count = if self.analysis.beat { 32 } else { 2 };
            for _ in 0..count {
                let a = self.random_unit() * TAU;
                let speed = 50.0 + self.random_unit() * 180.0 + self.analysis.bass * 100.0;
                let p = Particle {
                    x: 320.0,
                    y: 240.0,
                    vx: a.cos() * speed,
                    vy: a.sin() * speed,
                    life: 1.0,
                    hue: self.random_unit(),
                };
                self.particles[self.particle_cursor] = p;
                self.particle_cursor = (self.particle_cursor + 1) % self.particles.len();
            }
        }
        for p in &mut self.particles {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life = (p.life - dt * 0.55).max(0.0);
        }
    }
    pub fn input(&mut self, action: Action) -> Option<SaverAction> {
        if self.style == "viz" {
            match action {
                Action::Left => {
                    self.set_effect((self.effect + EFFECTS.len() as u32 - 1) % EFFECTS.len() as u32)
                }
                Action::Right => self.set_effect(self.effect + 1),
                Action::Select => self.set_palette(self.palette + 1),
                Action::Up | Action::Down => self.song_on = !self.song_on,
                Action::Options | Action::Start => self.clock_on = !self.clock_on,
                Action::Back | Action::MenuDown => return Some(SaverAction::Exit),
                _ => {}
            }
            return None;
        }
        if let Some(card) = &mut self.card {
            match action {
                Action::Up => card.index = card.index.saturating_sub(1),
                Action::Down => {
                    card.index = (card.index + 1).min(card.tracks.len().saturating_sub(1))
                }
                Action::Back | Action::MenuDown => card.closing = true,
                Action::Select if !card.closing && card.open > 0.9 => {
                    let track = card.tracks.get(card.index).copied();
                    card.closing = true;
                    return track.map(SaverAction::PlayTrack);
                }
                Action::Options => return Some(SaverAction::OpenAlbum(card.album)),
                _ => {}
            }
            return None;
        }
        match action {
            Action::Up | Action::Down | Action::Left | Action::Right => self.move_selection(action),
            Action::Select => {
                if self.selected.is_none() {
                    self.move_selection(Action::Select);
                } else if let Some(album) = self.selected_album() {
                    let (r, c) = self.selected.unwrap();
                    let size = if self.covered(r, c) { CELL * 2.0 } else { CELL };
                    self.card = Some(Card {
                        album,
                        tracks: Vec::new(),
                        index: 0,
                        open: 0.0,
                        closing: false,
                        from: [
                            c as f32 * CELL + size * 0.5,
                            40.0 + r as f32 * CELL + size * 0.5,
                            size,
                        ],
                    });
                }
            }
            Action::Options => return self.selected_album().map(SaverAction::OpenAlbum),
            Action::Back | Action::MenuDown => {
                if self.selected.take().is_none() {
                    return Some(SaverAction::Exit);
                }
            }
            _ => {}
        }
        None
    }
    fn covered(&self, r: usize, c: usize) -> bool {
        self.big.is_some() && r < 2 && c < 2
    }
    fn selected_album(&self) -> Option<usize> {
        let (r, c) = self.selected?;
        if self.covered(r, c) {
            self.big
        } else {
            self.tiles[r * COLS + c].album
        }
    }
    fn move_selection(&mut self, key: Action) {
        let Some((mut r, mut c)) = self.selected else {
            self.selected = Some((0, 0));
            self.selected_at = self.elapsed;
            return;
        };
        if self.covered(r, c) {
            if key == Action::Right {
                c = 1;
            }
            if key == Action::Down {
                r = 1;
            }
        }
        match key {
            Action::Up => r = r.saturating_sub(1),
            Action::Down => r = (r + 1).min(ROWS - 1),
            Action::Left => c = c.saturating_sub(1),
            Action::Right => c = (c + 1).min(COLS - 1),
            _ => {}
        }
        if self.covered(r, c) {
            r = 0;
            c = 0;
        }
        self.selected = Some((r, c));
        self.selected_at = self.elapsed;
    }
    fn pick_album(&mut self, count: usize, exclude: Option<usize>) -> Option<usize> {
        if count == 0 {
            return None;
        }
        let start = self.random() as usize % count;
        (0..count).map(|n| (start + n) % count).find(|&a| {
            Some(a) != self.big
                && !self
                    .tiles
                    .iter()
                    .enumerate()
                    .any(|(i, t)| Some(i) != exclude && (t.album == Some(a) || t.next == Some(a)))
        })
    }
    fn random(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
    fn random_unit(&mut self) -> f32 {
        (self.random() >> 40) as f32 / 16_777_216.0
    }
    pub fn draw(
        &mut self,
        r: &mut Renderer,
        art: &mut Artwork,
        library: &Library,
        current: Option<usize>,
        clock: &str,
    ) -> Result<(), String> {
        r.rect(0.0, 0.0, 640.0, 480.0, BLACK);
        if self.style == "viz" {
            self.draw_viz(r);
            self.draw_overlays(r, art, library, current, clock);
        } else {
            self.draw_mosaic(r, art, library, current, clock);
        }
        Ok(())
    }
    fn draw_mosaic(
        &mut self,
        r: &mut Renderer,
        art: &mut Artwork,
        library: &Library,
        current: Option<usize>,
        clock: &str,
    ) {
        r.set_clip(0.0, 40.0, 640.0, 440.0);
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.covered(row, col) {
                    continue;
                }
                let tile = self.tiles[row * COLS + col];
                let selected = self.selected == Some((row, col));
                let cx = (col as f32 * CELL + CELL * 0.5).min(if selected {
                    640.0 - CELL * 0.5 - 4.0
                } else {
                    700.0
                });
                let cy = 40.0 + row as f32 * CELL + CELL * 0.5;
                if selected {
                    r.rect(cx - 57.0, cy - 57.0, 114.0, 114.0, WHITE);
                }
                draw_flip(
                    r,
                    art,
                    library,
                    tile.album,
                    tile.next,
                    tile.phase,
                    cx,
                    cy,
                    if selected { 108.0 } else { 109.0 },
                );
            }
        }
        if self.big.is_some() || self.big_old.is_some() {
            let selected = self.selected == Some((0, 0));
            if selected {
                r.rect(0.0, 40.0, 222.0, 222.0, WHITE);
            }
            draw_flip(
                r,
                art,
                library,
                self.big_old.or(self.big),
                self.big,
                self.big_phase,
                110.0,
                150.0,
                if selected { 216.0 } else { 219.0 },
            );
        }
        r.reset_clip();
        r.rect(0.0, 0.0, 640.0, 40.0, BLACK);
        r.text(clock, 12.0, 9.0, 19, WHITE, 120.0);
        if let Some(track) = current.and_then(|i| library.tracks.get(i)) {
            r.text(&track.title, 125.0, 10.0, 17, WHITE, 500.0);
        }
        if let Some(card) = &self.card {
            let Some(album) = library.albums.get(card.album) else {
                return;
            };
            let t = smooth(card.open);
            let cx = lerp(card.from[0], 320.0, t);
            let cy = lerp(card.from[1], 251.0, t);
            let size = lerp(card.from[2], 376.0, t);
            r.rect(0.0, 40.0, 640.0, 440.0, [0.0, 0.0, 0.0, 0.7 * t]);
            if card.open < 0.5 {
                r.cover(
                    art.get(album.cover.as_deref()),
                    cx,
                    cy,
                    size,
                    card.open * PI,
                    1.0,
                    false,
                );
            } else {
                let w = size * ((1.0 - card.open) * PI).cos().abs().max(0.01);
                let x = cx - w * 0.5;
                let y = cy - size * 0.5;
                r.gradient(
                    x,
                    y,
                    w,
                    size,
                    [0.94, 0.95, 0.98, 1.0],
                    [0.82, 0.85, 0.90, 1.0],
                );
                if card.open > 0.86 {
                    r.text(&album.name, x + 18.0, y + 15.0, 24, BLACK, w - 36.0);
                    r.text(
                        &album.artist,
                        x + 18.0,
                        y + 47.0,
                        17,
                        [0.25, 0.28, 0.34, 1.0],
                        w - 36.0,
                    );
                    let start = card
                        .index
                        .saturating_sub(3)
                        .min(card.tracks.len().saturating_sub(7));
                    for (row, &index) in card.tracks.iter().enumerate().skip(start).take(7) {
                        let Some(track) = library.tracks.get(index) else {
                            continue;
                        };
                        let ry = y + 80.0 + (row - start) as f32 * 39.0;
                        if row == card.index {
                            r.rect(x + 7.0, ry - 4.0, w - 14.0, 37.0, [0.12, 0.43, 0.89, 1.0]);
                        }
                        let color = if row == card.index { WHITE } else { BLACK };
                        r.text(
                            &track.track.to_string(),
                            x + 16.0,
                            ry + 3.0,
                            16,
                            color,
                            30.0,
                        );
                        r.text(&track.title, x + 48.0, ry + 3.0, 17, color, w - 64.0);
                    }
                }
            }
        }
    }
    fn color(&self, t: f32, alpha: f32, heat: bool) -> Color {
        let value = t.clamp(0.0, 1.0);
        let palette = if self.palette == 1 && self.album_palette.len() >= 2 {
            Some(self.album_palette.as_slice())
        } else {
            scheme(self.palette)
        };
        let mut c = if let Some(stops) = palette {
            interpolate(stops, value)
        } else if heat {
            interpolate(MAGMA, value)
        } else {
            hsv((value + self.analysis.hue).fract(), 0.85, 1.0)
        };
        c[3] = alpha;
        c
    }
    fn draw_viz(&self, r: &mut Renderer) {
        match self.effect {
            0 => {
                for b in 0..BANDS {
                    let x = 22.0 + b as f32 * 15.0;
                    let height = self.analysis.bands[b] * 280.0;
                    let (mut top, mut bottom) = if self.palette == 0
                        || self.palette == 1 && self.album_palette.is_empty()
                    {
                        ([0.59, 0.90, 1.0, 1.0], [0.12, 0.43, 0.90, 1.0])
                    } else {
                        (self.color(0.92, 1.0, false), self.color(0.40, 1.0, false))
                    };
                    r.gradient(x, 330.0 - height, 11.0, height, top, bottom);
                    bottom[3] = 0.28;
                    top[3] = 0.0;
                    r.gradient(x, 333.0, 11.0, (height * 0.5).min(135.0), bottom, top);
                    r.rect(
                        x,
                        325.0 - self.analysis.peaks[b] * 280.0,
                        11.0,
                        3.0,
                        self.color(1.0, 0.95, false),
                    );
                }
            }
            1 => {
                for age in (0..self.waves.len()).rev() {
                    let wave = &self.waves
                        [(self.wave_cursor + self.waves.len() - 1 - age) % self.waves.len()];
                    let alpha = (1.0 - age as f32 / self.waves.len() as f32).powi(2);
                    let expansion = 1.0 + age as f32 * 0.026;
                    for i in 1..wave.len() {
                        let x0 = 320.0 + ((i - 1) as f32 / 127.0 * 600.0 - 300.0) * expansion;
                        let x1 = 320.0 + (i as f32 / 127.0 * 600.0 - 300.0) * expansion;
                        let gain = (0.12 / self.analysis.level.max(0.03)).min(4.0);
                        let y0 = 240.0 + wave[i - 1] * gain * 240.0 * expansion;
                        let y1 = 240.0 + wave[i] * gain * 240.0 * expansion;
                        let c = self.color(i as f32 / 127.0, alpha, false);
                        if age == 0 {
                            let mut glow = c;
                            glow[3] *= 0.14;
                            r.line(x0, y0, x1, y1, 9.0, glow);
                        }
                        r.line(x0, y0, x1, y1, if age == 0 { 2.5 } else { 1.5 }, c);
                    }
                }
            }
            2 => {
                for ring in 0..18 {
                    let depth = ((ring as f32 / 18.0 + self.elapsed * 0.16).fract()).max(0.005);
                    let radius = 15.0 + depth.powf(1.8) * 355.0;
                    let rotate = self.elapsed * 0.18 + (1.0 - depth) * 1.8;
                    for b in 0..BANDS {
                        let a = b as f32 / BANDS as f32 * TAU + rotate;
                        let a1 = (b + 1) as f32 / BANDS as f32 * TAU + rotate;
                        let radius0 = radius * (1.0 + self.analysis.bands[b] * 0.24);
                        let radius1 = radius * (1.0 + self.analysis.bands[(b + 1) % BANDS] * 0.24);
                        r.line(
                            320.0 + a.cos() * radius0,
                            240.0 + a.sin() * radius0 * 0.75,
                            320.0 + a1.cos() * radius1,
                            240.0 + a1.sin() * radius1 * 0.75,
                            1.0 + depth * 2.0,
                            self.color((b + ring) as f32 / BANDS as f32 % 1.0, depth * 0.85, false),
                        );
                    }
                }
            }
            3 => {
                for p in &self.particles {
                    if p.life <= 0.0 {
                        continue;
                    }
                    let c = self.color(p.hue, p.life * p.life, false);
                    let mut glow = c;
                    glow[3] *= 0.10;
                    r.circle(p.x, p.y, 5.0 * p.life + 1.0, glow);
                    r.line(
                        p.x,
                        p.y,
                        p.x - p.vx * 0.035,
                        p.y - p.vy * 0.035,
                        1.0 + p.life * 1.7,
                        c,
                    );
                }
            }
            _ => {
                let width = 640.0 / HISTORY as f32;
                for x in 0..HISTORY {
                    let column = &self.history[(self.history_cursor + x) % HISTORY];
                    for (b, &value) in column.iter().enumerate() {
                        if value < 2 {
                            continue;
                        }
                        r.rect(
                            x as f32 * width,
                            480.0 - (b + 1) as f32 * 12.0,
                            width + 0.1,
                            12.1,
                            self.color(value as f32 / 255.0, 1.0, true),
                        );
                    }
                }
                for (hz, text) in [(100.0_f32, "100"), (1000.0, "1k"), (10000.0, "10k")] {
                    let y = 480.0 - (hz / 40.0).ln() / (16000.0_f32 / 40.0).ln() * 480.0;
                    r.text(text, 8.0, y, 13, [1.0, 1.0, 1.0, 0.6], 50.0);
                }
            }
        }
        if !self.analysis.fresh && self.analysis.peaks.iter().sum::<f32>() / (BANDS as f32) < 0.01 {
            r.centered(
                "Play a song to see it move",
                10.0,
                226.0,
                620.0,
                21,
                [1.0, 1.0, 1.0, 0.5],
            );
        }
        let opacity = ((1.8 - (self.elapsed - self.name_at)) / 0.5).clamp(0.0, 1.0);
        if opacity > 0.0 {
            let name = if self.name_palette {
                palette_name(self.palette)
            } else {
                effect_name("viz", self.effect)
            };
            r.rect(220.0, 14.0, 200.0, 36.0, [0.0, 0.0, 0.0, 0.65 * opacity]);
            r.centered(name, 225.0, 21.0, 190.0, 20, [1.0, 1.0, 1.0, opacity]);
        }
    }
    fn draw_overlays(
        &self,
        r: &mut Renderer,
        art: &mut Artwork,
        library: &Library,
        current: Option<usize>,
        clock: &str,
    ) {
        if self.clock_alpha > 0.01 {
            let a = self.clock_alpha;
            r.rect(150.0, 164.0, 340.0, 124.0, [0.0, 0.0, 0.0, 0.35 * a]);
            r.centered(clock, 100.0, 181.0, 440.0, 76, [1.0, 1.0, 1.0, 0.92 * a]);
        }
        if self.song_alpha > 0.01 {
            if let Some(track) = current.and_then(|i| library.tracks.get(i)) {
                let a = self.song_alpha;
                let y = 390.0 + 90.0 * (1.0 - a);
                r.gradient(
                    0.0,
                    y - 60.0,
                    640.0,
                    150.0,
                    [0.0; 4],
                    [0.0, 0.0, 0.0, 0.85 * a],
                );
                r.image(art.get(track.cover.as_deref()), 16.0, y, 72.0, 72.0, a);
                r.text(&track.title, 104.0, y + 5.0, 24, [1.0, 1.0, 1.0, a], 520.0);
                r.text(
                    &track.artist,
                    104.0,
                    y + 40.0,
                    18,
                    [0.8, 0.84, 0.9, a],
                    520.0,
                );
            }
        }
    }
}

fn draw_flip(
    r: &mut Renderer,
    art: &mut Artwork,
    library: &Library,
    old: Option<usize>,
    next: Option<usize>,
    phase: f32,
    cx: f32,
    cy: f32,
    size: f32,
) {
    let phase = smooth(phase.clamp(0.0, 1.0));
    let (album, angle) = if next.is_some() && phase < 1.0 {
        if phase < 0.5 {
            (old, phase * PI)
        } else {
            (next, (phase - 1.0) * PI)
        }
    } else {
        (next.or(old), 0.0)
    };
    let Some(album) = album.and_then(|i| library.albums.get(i)) else {
        r.rect(
            cx - size * 0.5,
            cy - size * 0.5,
            size,
            size,
            [0.035, 0.04, 0.045, 1.0],
        );
        return;
    };
    r.cover(
        art.get(album.cover.as_deref()),
        cx,
        cy,
        size,
        angle,
        1.0,
        false,
    );
}
fn approach(value: f32, target: f32, speed: f32) -> f32 {
    value + (target - value) * speed.clamp(0.0, 1.0)
}
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn interpolate(stops: &[Color], t: f32) -> Color {
    let f = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (f as usize).min(stops.len() - 2);
    let k = f - i as f32;
    [
        lerp(stops[i][0], stops[i + 1][0], k),
        lerp(stops[i][1], stops[i + 1][1], k),
        lerp(stops[i][2], stops[i + 1][2], k),
        1.0,
    ]
}
fn hsv(h: f32, s: f32, v: f32) -> Color {
    let h = h.fract() * 6.0;
    let i = h.floor() as u32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b, 1.0]
}
const MAGMA: &[Color] = &[
    [0.0, 0.0, 0.016, 1.0],
    [0.16, 0.04, 0.33, 1.0],
    [0.40, 0.08, 0.43, 1.0],
    [0.62, 0.16, 0.39, 1.0],
    [0.83, 0.28, 0.26, 1.0],
    [0.96, 0.49, 0.08, 1.0],
    [0.98, 0.76, 0.15, 1.0],
    [0.99, 1.0, 0.64, 1.0],
];
fn scheme(index: u32) -> Option<&'static [Color]> {
    match index {
        2 => Some(MAGMA),
        3 => Some(&[
            [0.008, 0.03, 0.08, 1.0],
            [0.0, 0.27, 0.35, 1.0],
            [0.0, 0.67, 0.55, 1.0],
            [0.35, 0.9, 0.63, 1.0],
            [0.59, 0.47, 1.0, 1.0],
            [0.9, 0.8, 1.0, 1.0],
        ]),
        4 => Some(&[
            [0.04, 0.016, 0.12, 1.0],
            [0.35, 0.08, 0.43, 1.0],
            [0.86, 0.2, 0.47, 1.0],
            [1.0, 0.47, 0.31, 1.0],
            [1.0, 0.78, 0.35, 1.0],
            [1.0, 0.96, 0.8, 1.0],
        ]),
        5 => Some(&[
            [0.008, 0.024, 0.12, 1.0],
            [0.04, 0.16, 0.47, 1.0],
            [0.12, 0.43, 0.90, 1.0],
            [0.43, 0.78, 1.0, 1.0],
            [0.86, 0.96, 1.0, 1.0],
        ]),
        6 => Some(&[
            [0.0, 0.03, 0.008, 1.0],
            [0.0, 0.24, 0.08, 1.0],
            [0.08, 0.63, 0.24, 1.0],
            [0.43, 1.0, 0.55, 1.0],
            [0.88, 1.0, 0.88, 1.0],
        ]),
        7 => Some(&[
            [0.0, 0.0, 0.0, 1.0],
            [0.22, 0.22, 0.24, 1.0],
            [0.59, 0.59, 0.61, 1.0],
            [0.96, 0.96, 0.97, 1.0],
        ]),
        _ => None,
    }
}

pub struct Analysis {
    samples: [f32; N],
    window: [f32; N],
    reversed: [usize; N],
    twiddles: [Complex; N / 2],
    spectrum: [Complex; N],
    rate: u32,
    ranges: [(usize, usize); BANDS],
    norm: f32,
    pub bands: [f32; BANDS],
    pub peaks: [f32; BANDS],
    fall: [f32; BANDS],
    pub level: f32,
    pub bass: f32,
    bass_average: f32,
    pub beat: bool,
    beat_elapsed: f32,
    pub fresh: bool,
    hue: f32,
}
impl Default for Analysis {
    fn default() -> Self {
        Self::new()
    }
}
impl Analysis {
    pub fn new() -> Self {
        let mut out = Self {
            samples: [0.0; N],
            window: [0.0; N],
            reversed: [0; N],
            twiddles: [Complex::default(); N / 2],
            spectrum: [Complex::default(); N],
            rate: 0,
            ranges: [(0, 0); BANDS],
            norm: -60.0,
            bands: [0.0; BANDS],
            peaks: [0.0; BANDS],
            fall: [0.0; BANDS],
            level: 0.0,
            bass: 0.0,
            bass_average: 0.0,
            beat: false,
            beat_elapsed: 1.0,
            fresh: false,
            hue: 0.0,
        };
        for i in 0..N {
            out.window[i] = 0.5 - 0.5 * (TAU * i as f32 / (N - 1) as f32).cos();
            out.reversed[i] = i.reverse_bits() >> (usize::BITS - N.trailing_zeros());
        }
        for i in 0..N / 2 {
            let a = -TAU * i as f32 / N as f32;
            out.twiddles[i] = Complex {
                re: a.cos(),
                im: a.sin(),
            };
        }
        out.set_rate(44100);
        out
    }
    fn set_rate(&mut self, rate: u32) {
        self.rate = if rate >= 8000 { rate } else { 44100 };
        let lo = 40.0_f32;
        let hi = 16000.0_f32.min(self.rate as f32 * 0.475);
        let hz = self.rate as f32 / N as f32;
        for b in 0..BANDS {
            let f0 = lo * (hi / lo).powf(b as f32 / BANDS as f32);
            let f1 = lo * (hi / lo).powf((b + 1) as f32 / BANDS as f32);
            let start = (f0 / hz).floor().max(1.0) as usize;
            let end = (f1 / hz).ceil() as usize;
            self.ranges[b] = (start.min(N / 2 - 1), end.max(start + 1).min(N / 2));
        }
    }
    pub fn samples(&mut self, input: &[f32], rate: u32) {
        if rate != self.rate {
            self.set_rate(rate);
        }
        self.samples.fill(0.0);
        let count = input.len().min(N);
        let start = input.len() - count;
        for (to, &from) in self.samples[N - count..].iter_mut().zip(&input[start..]) {
            *to = if from.is_finite() {
                from.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
    pub fn update(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let k = dt * 30.0;
        let mut power = 0.0;
        for i in 0..N {
            let x = self.samples[i];
            power += x * x;
            self.spectrum[self.reversed[i]] = Complex {
                re: x * self.window[i],
                im: 0.0,
            };
        }
        self.level = (power / N as f32).sqrt();
        self.fresh = self.level >= 1e-4;
        let mut size = 2;
        while size <= N {
            let half = size / 2;
            let step = N / size;
            for i in (0..N).step_by(size) {
                for j in 0..half {
                    let w = self.twiddles[j * step];
                    let p = self.spectrum[i + j + half];
                    let u = self.spectrum[i + j];
                    let tr = w.re * p.re - w.im * p.im;
                    let ti = w.re * p.im + w.im * p.re;
                    self.spectrum[i + j] = Complex {
                        re: u.re + tr,
                        im: u.im + ti,
                    };
                    self.spectrum[i + j + half] = Complex {
                        re: u.re - tr,
                        im: u.im - ti,
                    };
                }
            }
            size <<= 1;
        }
        let mut raw = [0.0; BANDS];
        let mut top = -200.0_f32;
        for (b, out) in raw.iter_mut().enumerate() {
            let (start, end) = self.ranges[b];
            let mut max = 0.0_f32;
            for bin in &self.spectrum[start..end] {
                max = max.max(bin.re * bin.re + bin.im * bin.im);
            }
            *out = 10.0 * (max + 1e-12).log10() + 9.0 * b as f32 / BANDS as f32;
            top = top.max(*out);
        }
        if top > self.norm {
            self.norm += (top - self.norm) * 0.5;
        } else {
            self.norm = (-60.0_f32).max(self.norm - (self.norm - top) * 0.12 * dt);
        }
        for b in 0..BANDS {
            let x = if self.fresh {
                (1.0 + (raw[b] - self.norm - 2.0) / 42.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if x > self.bands[b] {
                self.bands[b] += (x - self.bands[b]) * (0.65 * k).min(1.0);
                self.fall[b] = 0.0;
            } else {
                self.fall[b] += 0.004 * k;
                self.bands[b] = x.max(self.bands[b] - self.fall[b] * k);
            }
            self.peaks[b] = self.bands[b].max(self.peaks[b] - 0.012 * k);
        }
        self.bass = self.bands[..BANDS / 4].iter().sum::<f32>() / (BANDS / 4) as f32;
        self.beat_elapsed += dt;
        self.beat = self.fresh
            && self.bass > 0.3
            && self.bass > self.bass_average * 1.35
            && self.beat_elapsed > 0.22;
        if self.beat {
            self.beat_elapsed = 0.0;
        }
        self.bass_average += (self.bass - self.bass_average) * (0.08 * k).min(1.0);
        self.hue = (self.hue + (0.0025 + 0.01 * self.bass) * k).fract();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fft_places_tone_in_log_frequency_band_and_decays_silence() {
        let mut a = Analysis::new();
        let rate = 48000;
        let frequency = 3000.0;
        let input: Vec<_> = (0..N)
            .map(|i| (TAU * frequency * i as f32 / rate as f32).sin() * 0.4)
            .collect();
        a.samples(&input, rate);
        for _ in 0..20 {
            a.update(1.0 / 60.0);
        }
        let strongest = a
            .bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let center =
            (a.ranges[strongest].0 + a.ranges[strongest].1) as f32 * 0.5 * rate as f32 / N as f32;
        assert!((center - frequency).abs() < 600.0, "{center}");
        assert!(a.level > 0.2);
        assert!(a.bands[strongest] > 0.8);
        a.samples(&[], rate);
        for _ in 0..300 {
            a.update(1.0 / 60.0);
        }
        assert!(!a.fresh);
        assert!(a.peaks.iter().all(|v| *v < 0.001));
    }
    #[test]
    fn analysis_sanitizes_invalid_samples_and_supports_high_rate() {
        let mut a = Analysis::new();
        a.samples(&[f32::NAN, f32::INFINITY, 1.0], 192000);
        a.update(0.016);
        assert!(a
            .bands
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        assert!(a.ranges.iter().all(|&(lo, hi)| lo < hi && hi <= N / 2));
    }
    #[test]
    fn mosaic_keeps_albums_unique_and_card_emits_track_index() {
        use crate::library::Track;
        let tracks = (0..30)
            .map(|i| Track {
                path: format!("/music/{i}/song.flac").into(),
                title: format!("Song{i}"),
                album: format!("Album{i}"),
                ..Default::default()
            })
            .collect();
        let library = Library::from_tracks(tracks);
        let mut saver = Saver::new();
        saver.update(0.016, &library, Some(0));
        let mut shown = std::collections::HashSet::new();
        for tile in &saver.tiles {
            if let Some(album) = tile.album {
                assert!(shown.insert(album));
                assert_ne!(Some(album), saver.big);
            }
        }
        saver.input(Action::Select);
        saver.input(Action::Select);
        saver.update(0.1, &library, Some(0));
        for _ in 0..5 {
            saver.update(0.1, &library, Some(0));
        }
        assert_eq!(saver.input(Action::Select), Some(SaverAction::PlayTrack(0)));
    }
}
