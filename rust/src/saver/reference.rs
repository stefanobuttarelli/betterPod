//! Original Go 1.1 screensaver state, timing and GPU composition.
use super::*;
use crate::player_geometry as geometry;
use crate::{
    library::Track,
    motion::{Anim, Ease},
    status::BatteryIcon,
    visual_assets::VisualAssets,
};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
};
mod status;
const CELL: f64 = 110.;
const EFFECTS: [&str; 5] = ["Bars", "Waves", "Tunnel", "Stars", "Spectrogram"];
const PALETTES: [&str; 8] = [
    "Classic", "Album", "Magma", "Aurora", "Sunset", "Ocean", "Phosphor", "Mono",
];
pub fn effect_count(style: &str) -> u32 {
    if style == "viz" {
        5
    } else {
        1
    }
}
pub fn effect_name(style: &str, index: u32) -> &'static str {
    if style == "viz" {
        EFFECTS[index as usize % 5]
    } else {
        "Cover Flip"
    }
}
pub fn palette_count() -> u32 {
    8
}
pub fn palette_name(index: u32) -> &'static str {
    PALETTES[index as usize % 8]
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaverAction {
    OpenAlbum(usize),
    AlbumOptions(usize),
    SongOptions(usize),
    PlayTrack(usize),
    Exit,
}
#[derive(Clone, Copy)]
pub struct SaverStatus {
    pub battery: Option<u8>,
    pub charging: bool,
    pub wifi: i8,
    pub bluetooth: bool,
    pub headphones: bool,
    pub held: bool,
}
impl Default for SaverStatus {
    fn default() -> Self {
        Self {
            battery: None,
            charging: false,
            wifi: -1,
            bluetooth: false,
            headphones: false,
            held: false,
        }
    }
}
#[derive(Clone, Copy)]
struct Tile {
    album: Option<usize>,
    next: Option<usize>,
    flip: Anim,
    arrival: Anim,
}
impl Default for Tile {
    fn default() -> Self {
        Self {
            album: None,
            next: None,
            flip: Anim::new(1.),
            arrival: Anim::new(0.),
        }
    }
}
struct Card {
    album: usize,
    tracks: Vec<usize>,
    index: usize,
    cell: isize,
    open: Anim,
    home: Anim,
    playing: bool,
    scroll: f64,
    since: f64,
    moved: Option<f64>,
    marquee: f64,
}
pub struct Saver {
    style: String,
    effect: u32,
    palette: u32,
    elapsed: f64,
    time_supplied: bool,
    started: f64,
    initialized: bool,
    ready: bool,
    ready_by: f64,
    library_identity: (usize, usize, usize),
    tiles: [Tile; 24],
    big: Option<usize>,
    big_old: Option<usize>,
    big_flip: Anim,
    big_on: Anim,
    fade: Anim,
    exiting: bool,
    events: VecDeque<SaverAction>,
    next_flip: f64,
    rng: u64,
    selected: Option<(usize, usize)>,
    selected_at: f64,
    grow: Anim,
    card: Option<Card>,
    current: Option<usize>,
    song_old: Option<usize>,
    song_change: Anim,
    song_in: Anim,
    sub_album: Anim,
    sub_next: f64,
    skip_direction: f64,
    clock_a: Anim,
    low_a: Anim,
    low_pin: bool,
    low_off: Option<f64>,
    low_armed: bool,
    playing: bool,
    paused: bool,
    volume: f32,
    status: SaverStatus,
    dark: bool,
    assets: HashMap<String, Texture>,
    ink: HashMap<String, (f32, f32)>,
    volume_at: Option<f64>,
    brightness: Option<(i32, f64)>,
    date: Option<String>,
    name_at: f64,
    name_palette: bool,
    fly: Anim,
    fly_from: Option<[f64; 4]>,
    fly_tilt: bool,
    from_np: bool,
    np_tilt: f64,
    shared_mini: bool,
    album_palette: Vec<Color>,
    viz: super::viz::Visualizer,
    clock: Option<super::clock::FrostClock>,
    pub analysis: Analysis,
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
            elapsed: 0.,
            time_supplied: false,
            started: 0.,
            initialized: false,
            ready: false,
            ready_by: 0.,
            library_identity: (0, 0, 0),
            tiles: [Tile::default(); 24],
            big: None,
            big_old: None,
            big_flip: Anim::new(1.),
            big_on: Anim::new(0.),
            fade: Anim::new(0.),
            exiting: false,
            events: VecDeque::new(),
            next_flip: 3.,
            rng: 0xa93f72bcd6751831,
            selected: None,
            selected_at: 0.,
            grow: Anim::new(0.),
            card: None,
            current: None,
            song_old: None,
            song_change: Anim::new(1.),
            song_in: Anim::new(0.),
            sub_album: Anim::new(0.),
            sub_next: 0.,
            skip_direction: 1.,
            clock_a: Anim::new(0.),
            low_a: Anim::new(0.),
            low_pin: false,
            low_off: None,
            low_armed: false,
            playing: true,
            paused: false,
            volume: 0.5,
            status: SaverStatus::default(),
            dark: false,
            assets: HashMap::new(),
            ink: HashMap::new(),
            volume_at: None,
            brightness: None,
            date: None,
            name_at: 0.,
            name_palette: false,
            fly: Anim::new(1.),
            fly_from: None,
            fly_tilt: false,
            from_np: false,
            np_tilt: 0.,
            shared_mini: false,
            album_palette: Vec::new(),
            viz: super::viz::Visualizer::new(),
            clock: None,
            analysis: Analysis::new(),
        }
    }
    /// Parse immutable font resources before entering the UI loop.
    pub fn configure_assets(&mut self, path: &Path) -> Result<(), String> {
        self.clock = Some(super::clock::FrostClock::load(path)?);
        Ok(())
    }
    pub fn set_assets(&mut self, assets: &VisualAssets) {
        for name in [
            "battery_shell",
            "bluetooth",
            "headphones",
            "volume_full_16",
            "wifi_0",
            "wifi_1",
            "wifi_2",
            "wifi_3",
            "wifi_4",
            "wifi",
            "play",
            "pause",
            "volume_full_bar",
            "volume_mute_bar",
        ] {
            for suffix in ["", "__white"] {
                let name = format!("{name}{suffix}");
                if let Some(t) = assets.get(&name) {
                    if let Some(ink) = assets.ink(&name) {
                        self.ink.insert(name.clone(), ink);
                    }
                    self.assets.insert(name, t);
                }
            }
        }
    }
    pub fn set_status(&mut self, status: SaverStatus) {
        self.status = status;
    }
    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
    }
    pub fn set_playback(&mut self, playing: bool, paused: bool, volume: f32) {
        self.playing = playing;
        self.paused = paused;
        self.volume = volume;
    }
    pub fn set_time(&mut self, now: f64) {
        self.elapsed = now;
        self.time_supplied = true;
    }
    pub fn set_clock_date(&mut self, date: &str) {
        self.date = Some(date.into());
    }
    pub fn use_shared_mini(&mut self, enabled: bool) {
        self.shared_mini = enabled;
    }
    pub fn low_overlay(&mut self) -> Option<(f32, f32)> {
        let a = self.low_a.value(self.elapsed);
        if self.ready && self.style == "viz" && a > 0.002 {
            Some((
                356. + 60. * (1. - a) as f32,
                (a * self.fade.value(self.elapsed)) as f32,
            ))
        } else {
            None
        }
    }
    pub fn set_track_direction(&mut self, direction: i32) {
        self.skip_direction = if direction < 0 { -1. } else { 1. };
    }
    pub fn volume_changed(&mut self, now: f64) {
        self.volume_at = Some(now);
    }
    pub fn brightness_changed(&mut self, level: i32, now: f64) {
        self.brightness = Some((level, now + 1.3));
    }
    pub fn begin_from_np(&mut self, tilt: f64) {
        self.from_np = true;
        self.np_tilt = tilt;
        self.fly_from = Some([51., 125., 203., 203.]);
        self.fly.snap(0.);
    }
    pub fn fly_hides_np(&self) -> bool {
        self.from_np
            && self.ready
            && (self.fly.target() == 0.
                || self.elapsed >= self.started + if self.np_tilt > 0.01 { 0.28 } else { 0. })
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn exiting(&self) -> bool {
        self.exiting
    }
    pub fn poll(&mut self) -> Option<SaverAction> {
        self.events.pop_front()
    }
    pub fn request_exit(&mut self, duration: f64) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        self.fade.set(0., duration, Ease::Css, 0., self.elapsed);
        if self.from_np && self.fly.target() == 1. && self.fly.value(self.elapsed) < 1. {
            self.fly.set(0., 0.4, Ease::InOut, 0., self.elapsed);
            self.fly_tilt = self.playing && !self.paused;
        }
    }
    pub fn return_to_now_playing(&mut self) {
        self.from_np = true;
        self.fly_from = Some([51., 125., 203., 203.]);
        self.fly.snap(1.);
        self.fly.set(0., 0.65, Ease::InOut, 0., self.elapsed);
        self.fly_tilt = self.playing && !self.paused;
        self.request_exit(0.45);
    }
    pub fn restart(&mut self) {
        self.viz.reset();
        self.analysis = Analysis::new();
        self.initialized = false;
        self.ready = false;
        self.exiting = false;
        self.events.clear();
        self.from_np = false;
        self.fly_from = None;
        self.fly.snap(1.);
        self.clock_a.snap(0.);
        self.low_a.snap(0.);
        self.low_pin = false;
        self.low_off = None;
        self.low_armed = false;
        self.selected = None;
        self.card = None;
        self.fade.snap(0.);
        self.name_at = self.elapsed;
        self.name_palette = false;
    }
    pub fn set_style(&mut self, style: &str) {
        let style = if style == "viz" { "viz" } else { "cover" };
        if self.style != style {
            self.style = style.into();
            self.restart();
        }
    }
    pub fn set_effect(&mut self, effect: u32) {
        self.effect = effect % 5;
        self.name_at = self.elapsed;
        self.name_palette = false;
    }
    pub fn set_palette(&mut self, palette: u32) {
        self.palette = palette % 8;
        self.name_at = self.elapsed;
        self.name_palette = true;
    }
    pub fn effect(&self) -> u32 {
        self.effect
    }
    pub fn palette(&self) -> u32 {
        self.palette
    }
    pub fn set_album_palette(&mut self, stops: &[Color]) {
        if self.album_palette != stops {
            self.album_palette.clear();
            self.album_palette.extend(stops.iter().copied());
        }
    }
    pub fn samples(&mut self, samples: &[f32], rate: u32) {
        if self.style == "viz" {
            if self.playing && !self.paused {
                self.analysis.samples(samples, rate)
            } else {
                self.analysis.samples(&[], rate)
            }
        }
    }
    pub fn close(&mut self, r: &Renderer) {
        self.viz.close(r);
        if let Some(clock) = self.clock.as_mut() {
            clock.close(r)
        }
    }
    fn begin(&mut self) {
        let now = self.elapsed;
        self.ready = true;
        self.started = now;
        self.name_at = now;
        for i in 0..24 {
            self.tiles[i]
                .arrival
                .set(1., 0.38, Ease::Css, (i / 6 + i % 6) as f64 * 0.045, now);
        }
        if self.from_np && self.big.is_some() {
            let delay = if self.np_tilt > 0.01 { 0.28 } else { 0. };
            self.fly.set(1., 0.65, Ease::InOut, delay, now);
            self.fade.set(1., 0.45, Ease::Css, delay, now);
        } else {
            self.fade.set(
                1.,
                if self.style == "viz" { 0.4 } else { 0.3 },
                Ease::Css,
                0.,
                now,
            );
        }
        self.next_flip = now + 3.;
    }
    pub fn update(&mut self, dt: f32, library: &Library, current: Option<usize>) {
        if !self.time_supplied {
            self.elapsed += dt.max(0.) as f64;
        }
        self.time_supplied = false;
        let now = self.elapsed;
        let identity = (
            library.albums.as_ptr() as usize,
            library.albums.len(),
            library.tracks.len(),
        );
        if identity != self.library_identity {
            self.library_identity = identity;
            if self.initialized {
                self.restart();
            }
        }
        let current = current.filter(|i| *i < library.tracks.len());
        if !self.initialized {
            self.initialized = true;
            self.started = now;
            self.ready_by = now + 1.5;
            self.tiles = [Tile::default(); 24];
            self.big = current.and_then(|i| {
                library
                    .albums
                    .iter()
                    .position(|a| a.track_indices.contains(&i))
            });
            self.big_old = None;
            self.big_flip.snap(1.);
            self.big_on.snap(if self.big.is_some() { 1. } else { 0. });
            self.current = current;
            self.song_in
                .snap(if self.playing && !self.paused { 1. } else { 0. });
            for i in 0..24 {
                self.tiles[i].album = self.pick_album(library.albums.len(), Some(i));
            }
            self.sub_next = now + 6.;
            if self.style == "viz" {
                self.begin();
            }
        }
        if self.exiting && !self.ready {
            if !self.events.contains(&SaverAction::Exit) {
                self.events.push_back(SaverAction::Exit);
            }
            return;
        }
        if !self.ready {
            return;
        }
        if self.exiting && !self.fade.running(now) && !self.fly.running(now) {
            if !self.events.contains(&SaverAction::Exit) {
                self.events.push_back(SaverAction::Exit);
            }
            return;
        }
        if self.current != current {
            if self.low_armed {
                self.low_auto();
            }
            self.song_old = self.current;
            if self.song_old.is_some() && current.is_some() {
                self.song_change.snap(0.);
                self.song_change.set(1., 0.42, Ease::InOut, 0., now);
            }
            self.current = current;
            self.sub_album.snap(0.);
            self.sub_next = now + 6.;
            let album = current.and_then(|i| {
                library
                    .albums
                    .iter()
                    .position(|a| a.track_indices.contains(&i))
            });
            if self.card.as_ref().is_none_or(|c| !c.home.running(now)) {
                self.follow(album, library.albums.len());
            }
        }
        let want = if self.playing && !self.paused { 1. } else { 0. };
        if self.song_in.target() != want {
            if want == 1. && self.low_armed {
                self.low_auto();
            }
            self.song_in.set(want, 0.38, Ease::InOut, 0., now);
        }
        if now > self.sub_next {
            let target = if self.sub_album.target() == 0. {
                1.
            } else {
                0.
            };
            self.sub_album.set(target, 0.45, Ease::InOut, 0., now);
            self.sub_next = now + if target == 1. { 4. } else { 6. };
        }
        if self.style == "viz" {
            self.low_armed = true;
            if self.low_off.is_some_and(|at| now > at) {
                self.low_off = None;
                self.low_a.set(0., 0.45, Ease::InOut, 0., now);
            }
            return;
        }
        for tile in &mut self.tiles {
            if tile.next.is_some() && !tile.flip.running(now) && tile.flip.value(now) >= 1. {
                tile.album = tile.next.take();
            }
        }
        if self.big_old.is_some() && !self.big_flip.running(now) && !self.big_on.running(now) {
            self.big_old = None;
        }
        let mut card_done = false;
        if let Some(card) = self.card.as_mut() {
            if card.tracks.is_empty() {
                if let Some(album) = library.albums.get(card.album) {
                    card.tracks = album.track_indices.clone();
                    card.index = current
                        .and_then(|i| card.tracks.iter().position(|j| *j == i))
                        .unwrap_or(0);
                }
            }
            if (card.playing && !card.home.running(now) && card.home.value(now) >= 1.)
                || (!card.playing && card.open.target() == 0. && !card.open.running(now))
            {
                card_done = true;
            }
            let h = 400. - info_height() - 4.;
            if card.index as f64 * 50. < card.scroll {
                card.scroll = card.index as f64 * 50.;
            }
            if (card.index + 1) as f64 * 50. > card.scroll + h {
                card.scroll = (card.index + 1) as f64 * 50. - h;
            }
            card.scroll = card.scroll.max(0.);
        }
        if card_done {
            self.card_done(library.albums.len());
        }
        if now > self.next_flip {
            self.next_flip = now + 5.;
            let n = 1 + self.random() as usize % 3;
            for j in 0..n {
                let i = self.random() as usize % 24;
                if self.covered(i / 6, i % 6)
                    || self.selected == Some((i / 6, i % 6))
                    || self.tiles[i].next.is_some()
                    || self.card.as_ref().is_some_and(|c| c.cell == i as isize)
                {
                    continue;
                }
                let next = self.pick_album(library.albums.len(), Some(i));
                self.turn(i, next, j as f64 * 0.25);
            }
        }
        if self.selected.is_some() && self.card.is_none() && now - self.selected_at > 6. {
            self.selected = None;
        }
    }
    fn low_auto(&mut self) {
        if self.style != "viz" || self.low_pin {
            return;
        }
        self.low_a.set(1., 0.45, Ease::InOut, 0., self.elapsed);
        self.low_off = Some(self.elapsed + 5.);
    }
    fn turn(&mut self, i: usize, next: Option<usize>, delay: f64) {
        if next.is_none() || next == self.tiles[i].album {
            return;
        }
        let tile = &mut self.tiles[i];
        tile.next = next;
        tile.flip.snap(0.);
        tile.flip.set(1., 0.65, Ease::InOut, delay, self.elapsed);
    }
    fn follow(&mut self, album: Option<usize>, count: usize) {
        if album == self.big {
            return;
        }
        let old = self.big;
        match (old, album) {
            (_, None) => {
                self.big_on.set(0., 0.35, Ease::Css, 0., self.elapsed);
                self.big_old = old;
            }
            (None, Some(_)) => {
                self.big = album;
                self.big_old = None;
                self.big_flip.snap(1.);
                self.big_on.set(1., 0.35, Ease::Css, 0., self.elapsed);
                for i in 0..24 {
                    if (self.tiles[i].album == album || self.tiles[i].next == album)
                        && !self.covered(i / 6, i % 6)
                    {
                        self.tiles[i].album = None;
                        self.tiles[i].next = None;
                        let next = self.pick_album(count, Some(i));
                        self.turn(i, next, 0.);
                    }
                }
            }
            (Some(_), Some(_)) => {
                self.big = album;
                self.big_old = old;
                self.big_flip.snap(0.);
                self.big_flip.set(1., 0.7, Ease::InOut, 0., self.elapsed);
                for i in 0..24 {
                    if (self.tiles[i].album == album || self.tiles[i].next == album)
                        && !self.covered(i / 6, i % 6)
                    {
                        self.turn(i, old, 0.15);
                    }
                }
            }
        }
        self.big = album;
    }
    fn card_done(&mut self, count: usize) {
        let Some(card) = self.card.take() else { return };
        if card.playing {
            let old = self.big;
            self.big = Some(card.album);
            self.big_old = None;
            self.big_flip.snap(1.);
            self.big_on.snap(1.);
            if card.cell >= 0 && !self.covered(card.cell as usize / 6, card.cell as usize % 6) {
                let i = card.cell as usize;
                self.tiles[i].album = if old.is_some() && old != self.big {
                    old
                } else {
                    self.pick_album(count, Some(i))
                };
                self.tiles[i].next = None;
                self.tiles[i].flip.snap(1.);
            }
            self.selected = Some((0, 0));
            self.selected_at = self.elapsed;
        }
    }
    pub fn input(&mut self, action: Action) -> Option<SaverAction> {
        let now = self.elapsed;
        if self.exiting {
            return None;
        }
        if !self.ready {
            self.request_exit(0.);
            return None;
        }
        if self.style == "viz" {
            match action {
                Action::Left => self.set_effect((self.effect + 4) % 5),
                Action::Right => self.set_effect(self.effect + 1),
                Action::Select => self.set_palette(self.palette + 1),
                Action::Start => {
                    let target = if self.clock_a.target() > 0.5 { 0. } else { 1. };
                    self.clock_a.set(
                        target,
                        if target == 0. { 0.35 } else { 0.45 },
                        Ease::InOut,
                        0.,
                        now,
                    );
                }
                Action::Up | Action::Down => {
                    self.low_off = None;
                    self.low_pin = self.low_a.target() != 1.;
                    let target = if self.low_pin { 1. } else { 0. };
                    self.low_a.set(
                        target,
                        if target == 0. { 0.35 } else { 0.45 },
                        Ease::InOut,
                        0.,
                        now,
                    );
                }
                Action::Back | Action::MenuDown => self.request_exit(0.35),
                _ => {}
            }
            return None;
        }
        if let Some(card) = self.card.as_mut() {
            if card.home.target() == 1. {
                return None;
            }
            match action {
                Action::Up => {
                    if card.index > 0 {
                        card.index -= 1;
                        card.moved = Some(now);
                        card.marquee = now;
                    }
                }
                Action::Down => {
                    if card.index + 1 < card.tracks.len() {
                        card.index += 1;
                        card.moved = Some(now);
                        card.marquee = now;
                    }
                }
                Action::Back | Action::MenuDown => card.open.set(0., 0.45, Ease::InOut, 0., now),
                Action::Select if card.open.target() == 1. => {
                    if let Some(&track) = card.tracks.get(card.index) {
                        card.playing = true;
                        card.home.snap(0.);
                        card.home.set(1., 0.65, Ease::InOut, 0., now);
                        return Some(SaverAction::PlayTrack(track));
                    }
                }
                Action::Options | Action::SelectHeld => {
                    return card
                        .tracks
                        .get(card.index)
                        .copied()
                        .map(SaverAction::SongOptions);
                }
                _ => {}
            }
            return None;
        }
        match action {
            Action::Up | Action::Down | Action::Left | Action::Right => self.move_selection(action),
            Action::Select => {
                if self.selected.is_none() {
                    self.move_selection(action);
                } else {
                    self.open_card();
                }
            }
            Action::Options | Action::SelectHeld => {
                if self.selected.is_none() {
                    self.move_selection(Action::Select);
                } else {
                    return self.selected_album().map(SaverAction::AlbumOptions);
                }
            }
            Action::Back | Action::MenuDown => {
                if self.selected.take().is_none() {
                    self.request_exit(0.25)
                } else {
                    self.grow.snap(0.);
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
            let t = self.tiles[r * 6 + c];
            if t.flip.running(self.elapsed) {
                t.next.or(t.album)
            } else {
                t.album
            }
        }
    }
    fn open_card(&mut self) {
        let Some(album) = self.selected_album() else {
            return;
        };
        let (r, c) = self.selected.unwrap();
        let mut open = Anim::new(0.);
        open.set(1., 0.55, Ease::InOut, 0., self.elapsed);
        self.card = Some(Card {
            album,
            tracks: Vec::new(),
            index: 0,
            cell: if self.covered(r, c) {
                -1
            } else {
                (r * 6 + c) as isize
            },
            open,
            home: Anim::new(0.),
            playing: false,
            scroll: 0.,
            since: self.elapsed + 0.6,
            moved: None,
            marquee: self.elapsed + 0.6,
        });
    }
    fn move_selection(&mut self, action: Action) {
        let (mut r, mut c) = self.selected.unwrap_or((0, 0));
        if self.selected.is_some() {
            let (dr, dc) = match action {
                Action::Up => (-1, 0),
                Action::Down => (1, 0),
                Action::Left => (0, -1),
                Action::Right => (0, 1),
                _ => (0, 0),
            };
            if self.covered(r, c) {
                if dc > 0 {
                    c = 1
                }
                if dr > 0 {
                    r = 1
                }
            }
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if !(0..4).contains(&nr) || !(0..6).contains(&nc) {
                return;
            }
            r = nr as usize;
            c = nc as usize;
        }
        if self.covered(r, c) {
            r = 0;
            c = 0;
        }
        self.selected = Some((r, c));
        self.selected_at = self.elapsed;
        self.grow.snap(0.);
        self.grow.set(1., 0.18, Ease::Css, 0., self.elapsed);
    }
    fn pick_album(&mut self, count: usize, exclude: Option<usize>) -> Option<usize> {
        if count == 0 {
            return None;
        }
        let start = self.random() as usize % count;
        (0..count).map(|i| (i + start) % count).find(|a| {
            Some(*a) != self.big
                && !self
                    .tiles
                    .iter()
                    .enumerate()
                    .any(|(i, t)| Some(i) != exclude && (t.album == Some(*a) || t.next == Some(*a)))
        })
    }
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    pub fn draw(
        &mut self,
        r: &mut Renderer,
        art: &mut Artwork,
        library: &Library,
        current: Option<usize>,
        clock: &str,
    ) -> Result<(), String> {
        if !self.initialized {
            return Ok(());
        }
        if let Some(card) = self.card.as_mut() {
            if card.tracks.is_empty() {
                if let Some(album) = library.albums.get(card.album) {
                    card.tracks = album.track_indices.clone();
                    card.index = current
                        .and_then(|i| card.tracks.iter().position(|j| *j == i))
                        .unwrap_or(0);
                }
            }
        }
        if !self.ready {
            let mut all = true;
            for t in &self.tiles {
                if let Some(a) = t.album.and_then(|i| library.albums.get(i)) {
                    let tex = art.get(a.cover.as_deref());
                    if art.missing(a.cover.as_deref()) && tex.id == art.placeholder.id {
                        all = false;
                    }
                }
            }
            if all || self.elapsed >= self.ready_by {
                self.begin();
            } else {
                return Ok(());
            }
        }
        let alpha = self.fade.value(self.elapsed);
        r.push_transform(0., 0., 1., 1., alpha as f32);
        r.rect(0., 0., 640., 480., BLACK);
        if self.style == "viz" {
            self.viz.render(
                r,
                &mut self.analysis,
                self.effect,
                self.palette,
                &self.album_palette,
                self.elapsed,
            )?;
            self.draw_viz_overlay(r, art, library, current, clock)?;
        } else {
            self.draw_mosaic(r, art, library, clock);
        }
        r.pop_transform();
        if self.from_np
            && self.fly.running(self.elapsed)
            && self.big.is_some()
            && (self.exiting
                || self.elapsed >= self.started + if self.np_tilt > 0.01 { 0.28 } else { 0. })
        {
            self.draw_flying(r, art, library);
        }
        Ok(())
    }
    fn draw_mosaic(&mut self, r: &mut Renderer, art: &mut Artwork, library: &Library, clock: &str) {
        let now = self.elapsed;
        let big_on = self.big_on.value(now);
        let big = self.big.or(self.big_old);
        r.push_clip(0., 40., 640., 440.);
        for i in 0..24 {
            let row = i / 6;
            let col = i % 6;
            if self.card.as_ref().is_some_and(|c| c.cell == i as isize) {
                continue;
            }
            if row < 2 && col < 2 && big.is_some() && big_on >= 1. {
                continue;
            }
            if self.selected == Some((row, col)) && self.card.is_none() {
                continue;
            }
            draw_tile(
                r,
                art,
                library,
                &mut self.tiles[i],
                cell_rect(row, col, 1),
                now,
                1.,
            );
        }
        if let Some(album) = big {
            if big_on > 0.
                && !self.card.as_ref().is_some_and(|c| c.cell < 0)
                && !(self.from_np && self.fly.running(now))
            {
                let rect = cell_rect(0, 0, 2);
                let phase = self.big_flip.value(now);
                let shown = if phase < 0.5 {
                    self.big_old.or(Some(album))
                } else {
                    self.big.or(Some(album))
                };
                draw_square(
                    r,
                    art,
                    library,
                    shown,
                    rect,
                    if self.big_old.is_some() { phase } else { 0. },
                    big_on,
                );
            }
        }
        if let Some((row, col)) = self.selected.filter(|_| self.card.is_none()) {
            let grow = self.grow.value(now);
            let big = self.covered(row, col);
            let mut rect = cell_rect(
                if big { 0 } else { row },
                if big { 0 } else { col },
                if big { 2 } else { 1 },
            );
            let inset = (rect[2] * 0.07 * grow) as i32 as f64;
            rect[0] -= inset;
            rect[1] -= inset;
            rect[2] += 2. * inset;
            rect[3] += 2. * inset;
            rect[0] = rect[0].max(0.).min(640. - rect[2]);
            rect[1] = rect[1].max(40.).min(480. - rect[3]);
            if big {
                draw_square(r, art, library, self.big, rect, 0., 1.)
            } else {
                draw_tile(
                    r,
                    art,
                    library,
                    &mut self.tiles[row * 6 + col],
                    rect,
                    now,
                    1.,
                );
            }
            stroke_rect(
                r,
                [rect[0] - 1., rect[1] - 1., rect[2] + 2., rect[3] + 2.],
                3.,
                [1., 1., 1., grow as f32],
            );
        }
        if let Some(card) = self.card.as_mut() {
            draw_card(
                r,
                art,
                library,
                card,
                now,
                self.dark,
                self.current,
                self.paused,
            );
        }
        r.pop_clip();
        self.draw_bar(r, library, clock);
    }
    fn draw_flying(&mut self, r: &mut Renderer, art: &mut Artwork, library: &Library) {
        let f = self.fly.value(self.elapsed);
        let Some(album) = self.big.and_then(|i| library.albums.get(i)) else {
            return;
        };
        let tex = art.get(album.cover.as_deref());
        if self.exiting && self.fly_tilt {
            let flat = geometry::quad(geometry::now_playing_pose(0.), 0., 0., 128., 128.);
            let fx0 = flat
                .iter()
                .map(|p| p[0] as f64 / 2.)
                .fold(f64::INFINITY, f64::min);
            let fy0 = flat
                .iter()
                .map(|p| (p[1] as f64 - 40.) / 2.)
                .fold(f64::INFINITY, f64::min);
            let fx1 = flat
                .iter()
                .map(|p| p[0] as f64 / 2.)
                .fold(f64::NEG_INFINITY, f64::max);
            let ox = 51. / 2. - fx0;
            let oy = 125. / 2. - fy0;
            let k1 = 110. / (fx1 - fx0);
            let tx1 = -k1 * (fx0 + ox);
            let ty1 = 20. - k1 * (fy0 + oy);
            let k = 1. + (k1 - 1.) * f;
            let tx = tx1 * f + k * ox;
            let ty = ty1 * f + k * oy;
            // quad() includes the ordinary header offset; this flight is a
            // complete screen plane, as ssFlyTilted's destination is.
            r.push_transform(
                (2. * tx) as f32,
                (2. * ty - 40. * k) as f32,
                k as f32,
                k as f32,
                1.,
            );
            let p = geometry::now_playing_pose(1. - f);
            r.projected_image(
                tex,
                geometry::quad(p, 0., 0., 128., 128.),
                [0., 0., 1., 1.],
                [WHITE; 4],
            );
            let a = (0.2 * (1. - f)) as f32;
            if a > 0.001 {
                r.projected_image(
                    tex,
                    geometry::quad(p, 0., 128., 128., 166.4),
                    [0., 1., 1., 0.7],
                    [
                        [1., 1., 1., a],
                        [1., 1., 1., a],
                        [1., 1., 1., 0.],
                        [1., 1., 1., 0.],
                    ],
                );
            }
            r.pop_transform();
        } else {
            let from = self.fly_from.unwrap_or([51., 125., 203., 203.]);
            let rect = lerp_rect(from, cell_rect(0, 0, 2), f);
            r.image_uv(tex, rect.map(|x| x as f32), [0., 0., 1., 1.], 1.);
        }
    }
    fn draw_viz_overlay(
        &mut self,
        r: &mut Renderer,
        art: &mut Artwork,
        library: &Library,
        current: Option<usize>,
        clock: &str,
    ) -> Result<(), String> {
        let now = self.elapsed;
        let energy = self.analysis.peaks.iter().sum::<f64>() / 40.;
        if !self.analysis.fresh && energy < 0.01 {
            let text = "Play a song to see it move";
            let (_, a) = crate::render::line_box(28.);
            let tw = r.text_width_weighted(text, 28., 600);
            r.text_baseline(
                text,
                320. - tw * 0.5,
                220. + a * 0.5,
                28.,
                600,
                [1., 1., 1., 0.45],
                620.,
            );
        }
        let elapsed = now - self.name_at;
        if elapsed < 1.8 {
            let a = ((1.8 - elapsed) / 0.5).clamp(0., 1.);
            let text = if self.name_palette {
                format!("Colors · {}", palette_name(self.palette))
            } else {
                format!("‹  {}  ›", effect_name("viz", self.effect))
            };
            let tw = r.text_width_weighted(&text, 30., 700);
            r.rounded_rect(
                320. - tw * 0.5 - 28.,
                36.,
                tw + 56.,
                48.,
                24.,
                [0., 0., 0., (0.55 * a) as f32],
            );
            let (_, asc) = crate::render::line_box(30.);
            r.text_baseline(
                &text,
                320. - tw * 0.5,
                59. + asc * 0.5,
                30.,
                700,
                [1., 1., 1., a as f32],
                620.,
            );
        }
        let a = self.clock_a.value(now);
        if a > 0.002 {
            let cy = 196. + 12. * (1. - a);
            if let Some(frost) = self.clock.as_mut() {
                let viz = &self.viz;
                let analysis = &self.analysis;
                frost.draw(r, clock, cy as i32, a, |x, y| viz.pixel(analysis, x, y))?;
            } else {
                let tw = r.text_width_weighted(clock, 150., 300);
                r.text_baseline(
                    clock,
                    320. - tw * 0.5,
                    cy as f32 + 150. * 0.727 * 0.5,
                    150.,
                    300,
                    [1., 1., 1., a as f32],
                    620.,
                );
            }
            let date = self.date.clone().unwrap_or_else(local_date);
            let (_, asc) = crate::render::line_box(30.);
            let tw = r.text_width_weighted(&date, 30., 500);
            let baseline = (cy + 92.) as f32 + asc * 0.5;
            for (dx, dy) in [(-1., 0.), (1., 0.), (0., -1.), (0., 2.)] {
                r.text_baseline(
                    &date,
                    320. - tw * 0.5 + dx,
                    baseline + dy,
                    30.,
                    500,
                    [0., 0., 0., (0.45 * a) as f32],
                    620.,
                );
            }
            r.text_baseline(
                &date,
                320. - tw * 0.5,
                baseline,
                30.,
                500,
                [1., 1., 1., (0.9 * a) as f32],
                620.,
            );
        }
        let a = self.low_a.value(now);
        if a > 0.002 {
            if let Some(track) = current.and_then(|i| library.tracks.get(i)) {
                let top = 330. + 150. * (1. - a) as f32;
                for row in 0..150 {
                    let k = row as f32 / 150.;
                    r.rect(
                        0.,
                        top + row as f32,
                        640.,
                        1.,
                        [0., 0., 0., 0.82 * k * k * (3. - 2. * k) * a as f32],
                    );
                }
                let y = 356. + 60. * (1. - a) as f32;
                if !self.shared_mini {
                    draw_mini(r, art, track, y, a, self.paused);
                }
            }
        }
        if self.brightness.is_some_and(|(_, until)| now < until)
            || self.volume_at.is_some_and(|at| now - at < 1.4)
            || self.status.held
        {
            self.draw_bar(r, library, clock);
        }
        Ok(())
    }
    fn draw_bar(&mut self, r: &mut Renderer, library: &Library, clock: &str) {
        let now = self.elapsed;
        r.rect(0., 0., 640., 40., BLACK);
        r.push_clip(0., 0., 640., 40.);
        let x0 = 576.;
        let y0 = 7.;
        let shell = self.assets.get("battery_shell").copied();
        let (bw, bh) = shell.map_or((52., 26.), |t| (t.width as f32, t.height as f32));
        stroke_rect(r, [x0, y0, bw as f64 - 5., bh as f64], 2., WHITE);
        r.rect(
            x0 as f32 + bw - 4.,
            y0 as f32 + bh * 0.5 - 4.,
            3.,
            8.,
            WHITE,
        );
        // The same level steps as the header; the saver bar has no plug glyph.
        let fill = BatteryIcon::new(self.status.battery, self.status.charging, false).fill;
        r.rect(
            x0 as f32 + 4.,
            y0 as f32 + 4.,
            ((bw - 13.) * fill + 0.5).floor(),
            bh - 8.,
            WHITE,
        );
        if self.status.charging {
            r.triangle(
                [
                    [x0 as f32 + bw * 0.5 + 3., y0 as f32 + 3.],
                    [x0 as f32 + bw * 0.5 - 6., y0 as f32 + bh * 0.5 + 1.],
                    [x0 as f32 + bw * 0.5, y0 as f32 + bh * 0.5 + 1.],
                ],
                BLACK,
            );
            r.triangle(
                [
                    [x0 as f32 + bw * 0.5 - 3., y0 as f32 + bh - 3.],
                    [x0 as f32 + bw * 0.5 + 6., y0 as f32 + bh * 0.5 - 1.],
                    [x0 as f32 + bw * 0.5, y0 as f32 + bh * 0.5 - 1.],
                ],
                BLACK,
            );
        }
        let mut left = 568.;
        if self.status.bluetooth || self.status.headphones {
            status::draw_headset(
                r,
                left + 3. - 30.,
                7.,
                self.status.wifi,
                self.status.bluetooth,
                WHITE,
            );
            left -= 34.;
        } else if self.status.wifi >= 0 {
            status::draw_wifi(
                r,
                left + 3. - 36.,
                7.,
                ((self.status.wifi as i32 * 3 + 4) / 5) as usize,
                WHITE,
            );
            left -= 40.;
        }
        if self.current.is_some() {
            let name = if self.paused { "pause" } else { "play" };
            if let Some(t) = self
                .assets
                .get(&format!("{name}__white"))
                .or_else(|| self.assets.get(name))
                .copied()
            {
                let (ink_left, ink_right) = self.ink.get(name).copied().unwrap_or((0., 0.));
                let x = left + ink_right - t.width as f32;
                r.image_tinted(
                    t,
                    [x, 7., t.width as f32, t.height as f32],
                    [0., 0., 1., 1.],
                    WHITE,
                );
                left = x + ink_left - 10.;
            }
        }
        if self.status.held {
            left -= 25.;
            stroke_rect(r, [left as f64 + 4., 7., 14., 12.], 2., WHITE);
            r.rounded_rect(left, 16., 22., 17., 2., WHITE);
            left -= 8.;
        }
        if let Some((level, until)) = self.brightness.filter(|(_, until)| now < *until) {
            let _ = until;
            draw_brightness(r, level);
            r.pop_clip();
            return;
        }
        let volume_a = self
            .volume_at
            .map_or(0., |at| {
                let d = now - at;
                if d < 0.18 {
                    Ease::InOut.sample(d / 0.18)
                } else if d < 1.1 {
                    1.
                } else {
                    1. - Ease::InOut.sample((d - 1.1) / 0.3)
                }
            })
            .clamp(0., 1.);
        let off = -40. * Ease::InOut.sample(volume_a);
        let (_, asc) = crate::render::line_box(26.);
        let baseline = 3. + asc + off as f32;
        r.text_baseline(clock, 12., baseline, 26., 700, WHITE, 120.);
        if self.style != "viz" {
            if let Some(track) = self.current.and_then(|i| library.tracks.get(i)) {
                let a = self.song_in.value(now);
                if a > 0.001 {
                    let tx = 12. + r.text_width_weighted(clock, 26., 700) + 24.;
                    let room = left - 20. - tx;
                    let base = baseline + 40. * (1. - a) as f32;
                    let change = self.song_change.value(now);
                    if change < 1. {
                        if let Some(old) = self.song_old.and_then(|i| library.tracks.get(i)) {
                            draw_song(
                                r,
                                old,
                                tx,
                                base - 40. * self.skip_direction as f32 * change as f32,
                                room,
                                0.,
                            );
                        }
                        draw_song(
                            r,
                            track,
                            tx,
                            base + 40. * self.skip_direction as f32 * (1. - change) as f32,
                            room,
                            0.,
                        );
                    } else {
                        draw_song(r, track, tx, base, room, self.sub_album.value(now));
                    }
                }
            }
        }
        if volume_a > 0.001 {
            let y = 6. + 40. + off as f32;
            let name = if self.status.headphones {
                "headphones"
            } else {
                "volume_full_16"
            };
            if let Some(t) = self
                .assets
                .get(&format!("{name}__white"))
                .or_else(|| self.assets.get(name))
                .copied()
            {
                r.image_tinted(
                    t,
                    [12., y, t.width as f32, t.height as f32],
                    [0., 0., 1., 1.],
                    WHITE,
                );
            }
            let w = (left - 20. - 12. - 42. - 12.).min(300.);
            r.rounded_rect(54., 15. + 40. + off as f32, w, 10., 5., [1., 1., 1., 0.28]);
            r.rounded_rect(
                54.,
                15. + 40. + off as f32,
                (w * self.volume).max(10.),
                10.,
                5.,
                WHITE,
            );
        }
        r.pop_clip();
    }
}
fn cell_rect(row: usize, col: usize, span: usize) -> [f64; 4] {
    [
        col as f64 * CELL,
        40. + row as f64 * CELL,
        span as f64 * CELL,
        span as f64 * CELL,
    ]
}
fn lerp_rect(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    std::array::from_fn(|i| (a[i] + (b[i] - a[i]) * t) as i32 as f64)
}
fn draw_square(
    r: &mut Renderer,
    art: &mut Artwork,
    library: &Library,
    album: Option<usize>,
    rect: [f64; 4],
    turn: f64,
    alpha: f64,
) {
    let Some(album) = album.and_then(|i| library.albums.get(i)) else {
        return;
    };
    let k = (turn * std::f64::consts::PI).cos().abs();
    let w = (rect[2] * k + 0.5) as i32 as f32;
    if w < 1. {
        return;
    }
    let shade = (1. - k) * 0.55;
    let texture = art.get(album.cover.as_deref());
    r.image_tinted(
        texture,
        [
            (rect[0] + (rect[2] - w as f64) * 0.5) as f32,
            rect[1] as f32,
            w,
            rect[3] as f32,
        ],
        [0., 0., 1., 1.],
        [
            (1. - shade) as f32,
            (1. - shade) as f32,
            (1. - shade) as f32,
            alpha as f32,
        ],
    );
}
fn draw_tile(
    r: &mut Renderer,
    art: &mut Artwork,
    library: &Library,
    tile: &mut Tile,
    mut rect: [f64; 4],
    now: f64,
    mut alpha: f64,
) {
    let arrival = tile.arrival.value(now);
    if arrival <= 0. {
        return;
    }
    if arrival < 1. {
        let inset = (rect[2] * 0.12 * (1. - arrival)) as i32 as f64;
        rect[0] += inset;
        rect[1] += inset;
        rect[2] -= inset * 2.;
        rect[3] -= inset * 2.;
        alpha *= arrival;
    }
    let phase = tile.flip.value(now);
    let album = if tile.next.is_some() && phase >= 0.5 {
        tile.next
    } else {
        tile.album
    };
    draw_square(
        r,
        art,
        library,
        album,
        rect,
        if tile.next.is_some() { phase } else { 0. },
        alpha,
    );
}
fn info_height() -> f64 {
    crate::render::line_box(32.).0 as f64 + 8. + crate::render::line_box(28.).0 as f64 + 8. + 2.
}
fn stroke_rect(r: &mut Renderer, rect: [f64; 4], w: f32, color: Color) {
    let [x, y, width, height] = rect.map(|v| v as f32);
    r.rect(x, y, width, w, color);
    r.rect(x, y + height - w, width, w, color);
    r.rect(x, y, w, height, color);
    r.rect(x + width - w, y, w, height, color);
}
fn fit(r: &mut Renderer, text: &str, size: f32, weight: u16, width: f32) -> String {
    if r.text_width_weighted(text, size, weight) <= width {
        return text.into();
    }
    let ell = r.text_width_weighted("…", size, weight);
    if ell > width {
        return String::new();
    }
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if r.text_width_weighted(&out, size, weight) + ell > width {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}
fn marquee(
    r: &mut Renderer,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    weight: u16,
    width: f32,
    color: Color,
    since: f64,
    now: f64,
    cycle: f64,
) {
    let tw = r.text_width_weighted(text, size, weight);
    if tw <= width {
        r.text_baseline(text, x, baseline, size, weight, color, width);
        return;
    }
    let own = (tw as f64 + 80.) / 64. + 1.5;
    let t = (now - since).max(0.) % cycle.max(own);
    let offset = if t > 1.5 {
        ((t - 1.5) * 64.).min(tw as f64 + 80.)
    } else {
        0.
    };
    r.push_clip(x, baseline - size * 1.3, width, size * 1.7);
    r.text_baseline(
        text,
        x - offset as f32,
        baseline,
        size,
        weight,
        color,
        tw + 1.,
    );
    r.text_baseline(
        text,
        x - offset as f32 + tw + 80.,
        baseline,
        size,
        weight,
        color,
        tw + 1.,
    );
    r.pop_clip();
}
fn draw_card(
    r: &mut Renderer,
    art: &mut Artwork,
    library: &Library,
    card: &mut Card,
    now: f64,
    dark: bool,
    current: Option<usize>,
    paused: bool,
) {
    let open = card.open.value(now);
    let home = card.home.value(now);
    let from = if card.cell < 0 {
        cell_rect(0, 0, 2)
    } else {
        cell_rect(card.cell as usize / 6, card.cell as usize % 6, 1)
    };
    let mid = [64., 82., 512., 400.];
    let (rect, turn, dim) = if card.playing {
        (
            lerp_rect(mid, cell_rect(0, 0, 2), home),
            1. - home,
            1. - home,
        )
    } else {
        (lerp_rect(from, mid, open), open, open)
    };
    r.rect(0., 40., 640., 440., [0., 0., 0., (0.6 * dim) as f32]);
    if turn < 0.5 {
        let side = rect[2].min(rect[3]);
        let square = [
            rect[0] + (rect[2] - side) * 0.5,
            rect[1] + (rect[3] - side) * 0.5,
            side,
            side,
        ];
        draw_square(r, art, library, Some(card.album), square, turn, 1.);
        return;
    }
    let k = (turn * std::f64::consts::PI).cos().abs();
    let w = (rect[2] * k + 0.5) as i32 as f32;
    if w < 2. {
        return;
    }
    let x = rect[0] as f32 + (rect[2] as f32 - w) * 0.5;
    r.push_transform(x, rect[1] as f32, w / 512., rect[3] as f32 / 400., 1.);
    let Some(album) = library.albums.get(card.album) else {
        r.pop_transform();
        return;
    };
    let bg = if dark { rgb(22, 23, 25) } else { WHITE };
    let fg = if dark { rgb(238, 239, 241) } else { BLACK };
    let stripe = if dark {
        rgb(30, 31, 34)
    } else {
        rgb(242, 243, 245)
    };
    let border = if dark {
        rgb(60, 62, 66)
    } else {
        rgb(211, 211, 211)
    };
    r.rect(0., 0., 512., 400., bg);
    let info = info_height() as f32;
    r.gradient(
        2.,
        2.,
        508.,
        info - 2.,
        rgb(101, 133, 173),
        rgb(120, 154, 179),
    );
    r.rect(2., info, 508., 2., rgb(109, 135, 163));
    let (th, ta) = crate::render::line_box(32.);
    let (_, aa) = crate::render::line_box(28.);
    let cycle = ((r
        .text_width_weighted(&album.name, 32., 700)
        .max(r.text_width_weighted(&album.artist, 28., 700))
        + 80.)
        / 64.) as f64
        + 1.5;
    marquee(
        r,
        &album.name,
        18.,
        10. + ta,
        32.,
        700,
        476.,
        WHITE,
        card.since,
        now,
        cycle,
    );
    marquee(
        r,
        &album.artist,
        18.,
        10. + th + aa,
        28.,
        700,
        476.,
        WHITE,
        card.since,
        now,
        cycle,
    );
    let list_y = info + 2.;
    let list_h = 400. - list_y - 2.;
    let scrollable = card.tracks.len() as f32 * 50. > list_h;
    let (lh, asc) = crate::render::line_box(28.);
    r.push_transform(2., list_y, 1., 1., 1.);
    r.push_clip(0., 0., 508., list_h);
    for n in (card.scroll / 50.) as usize..card.tracks.len() {
        let y = n as f32 * 50. - card.scroll as f32;
        if y > list_h {
            break;
        }
        let Some(track) = library.tracks.get(card.tracks[n]) else {
            continue;
        };
        let selected = n == card.index;
        let color = if selected { WHITE } else { fg };
        if selected {
            r.gradient(0., y, 508., 50., rgb(60, 184, 255), rgb(52, 122, 181))
        } else if n % 2 == 1 {
            r.rect(0., y, 508., 50., stripe)
        }
        let mark = current == Some(card.tracks[n]);
        let x = 8. + if mark { 28. } else { 0. };
        let duration = track_time(track.duration);
        let dw = r.text_width_weighted(&duration, 28., 600) + 20.;
        let right = 508. - if scrollable { 18. } else { 0. } - dw;
        let baseline = y + (50. - (16. + lh)) * 0.5 + 8. + asc;
        let width = (right - x - 16.).max(0.);
        if selected {
            marquee(
                r,
                &track.title,
                x + 8.,
                baseline,
                28.,
                700,
                width,
                color,
                card.marquee,
                now,
                0.,
            )
        } else {
            let label = fit(r, &track.title, 28., 700, width);
            r.text_baseline(&label, x + 8., baseline, 28., 700, color, width)
        }
        if mark {
            let cy = baseline - 28. * 0.727 * 0.5;
            if paused {
                r.rect(18., cy - 9., 5.2, 18., color);
                r.rect(28., cy - 9., 5.2, 18., color)
            } else {
                r.triangle([[18., cy - 9.], [18., cy + 9.], [33., cy]], color)
            }
        }
        let tw = r.text_width_weighted(&duration, 28., 600);
        r.text_baseline(
            &duration,
            right + dw - 16. - tw,
            baseline,
            28.,
            600,
            color,
            tw + 1.,
        );
    }
    if scrollable {
        let a = card.moved.map_or(0., |at| {
            let d = now - at;
            if d <= 0.9 {
                1.
            } else if d >= 1.25 {
                0.
            } else {
                1. - Ease::Css.sample((d - 0.9) / 0.35)
            }
        });
        if a > 0. {
            scrollbar(
                r,
                490.,
                list_h,
                card.scroll as f32,
                card.tracks.len() as f32 * 50.,
                a,
                dark,
            );
        }
    }
    r.pop_clip();
    r.pop_transform();
    stroke_rect(r, [0., 0., 512., 400.], 2., border);
    if k < 1. {
        r.rect(0., 0., 512., 400., [0., 0., 0., ((1. - k) * 0.5) as f32]);
    }
    r.pop_transform();
}
fn track_time(seconds: f64) -> String {
    if seconds <= 0. {
        return String::new();
    }
    let s = (seconds + 0.5) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn scrollbar(r: &mut Renderer, x: f32, h: f32, scroll: f32, total: f32, alpha: f64, dark: bool) {
    let x = crate::render::scrollbar_track_x(x, 18.);
    let y = 6.;
    let height = h - 12.;
    let border = if dark {
        rgb(69, 72, 78)
    } else {
        rgb(180, 180, 180)
    };
    r.rounded_rect(
        x,
        y,
        12.,
        height,
        6.,
        [border[0], border[1], border[2], alpha as f32],
    );
    let top = if dark {
        rgb(38, 40, 44)
    } else {
        rgb(228, 228, 228)
    };
    let bottom = if dark {
        rgb(48, 50, 55)
    } else {
        rgb(247, 247, 247)
    };
    r.push_clip(x + 1., y + 1., 10., height - 2.);
    r.gradient(
        x + 1.,
        y + 1.,
        10.,
        height - 2.,
        [top[0], top[1], top[2], alpha as f32],
        [bottom[0], bottom[1], bottom[2], alpha as f32],
    );
    r.pop_clip();
    let thumb = (height * h / total).max(28.);
    let ty = y + (height - thumb) * (scroll / (total - h)).clamp(0., 1.);
    r.rounded_rect(x + 1., ty, 10., thumb, 5., [0.54, 0.54, 0.54, alpha as f32]);
}
fn draw_song(r: &mut Renderer, track: &Track, x: f32, baseline: f32, width: f32, album: f64) {
    if width < 80. {
        return;
    }
    let tw = r.text_width_weighted(&track.title, 26., 700);
    let title = fit(r, &track.title, 26., 700, width);
    r.text_baseline(&title, x, baseline, 26., 700, WHITE, width);
    if tw >= width || width - tw <= 60. {
        return;
    }
    let room = width - tw;
    let grey = [1., 1., 1., 0.55];
    let show_album = !track.album.is_empty() && track.album != track.artist;
    let a = if show_album { album } else { 0. };
    for (text, off) in [(&track.artist, -40. * a), (&track.album, 40. * (1. - a))] {
        if text.is_empty()
            || std::ptr::eq(text, &track.artist) && a >= 0.999
            || std::ptr::eq(text, &track.album) && a <= 0.001
        {
            continue;
        }
        let text = fit(r, &format!("  {text}"), 26., 500, room);
        r.text_baseline(&text, x + tw, baseline + off as f32, 26., 500, grey, room);
    }
}
fn draw_mini(
    r: &mut Renderer,
    art: &mut Artwork,
    track: &Track,
    top: f32,
    alpha: f64,
    paused: bool,
) {
    let a = alpha as f32;
    let pz = if paused { 1. } else { 0. };
    let size = 80. * (1. - 0.14 * pz);
    let ax = 14. + (80. - size) * 0.5;
    let ay = top + (116. - size) * 0.5 + 1.;
    r.rect(
        ax - 2.,
        ay - 2.,
        size + 4.,
        size + 4.,
        [1., 1., 1., 0.85 * a],
    );
    r.image_uv(
        art.get(track.cover.as_deref()),
        [ax, ay, size, size],
        [0., 0., 1., 1.],
        a,
    );
    if paused {
        r.rect(ax, ay, size, size, [0., 0., 0., 0.42 * a]);
        let cx = ax + size * 0.5;
        let cy = ay + size * 0.5;
        for bx in [cx - 3. - 7., cx + 3.] {
            r.rounded_rect(bx, cy - 13., 7., 26., 2.4, [1., 1., 1., a]);
        }
    }
    let lines = [
        (&track.title, 24., 700, 1.),
        (&track.artist, 22., 600, 0.86),
        (&track.album, 22., 500, 0.86),
    ];
    let total = lines
        .iter()
        .map(|l| crate::render::line_box(l.1).0)
        .sum::<f32>();
    let mut y = top + (116. - total) * 0.5 + 1.;
    r.push_clip(102., 0., 538., 480.);
    for (text, size, weight, opacity) in lines {
        let (height, asc) = crate::render::line_box(size);
        if !text.is_empty() {
            let label = fit(r, text, size, weight, 516.);
            r.text_baseline(
                &label,
                108.,
                y + asc + 1.,
                size,
                weight,
                [0., 0., 0., 0.35 * a],
                516.,
            );
            r.text_baseline(
                &label,
                108.,
                y + asc,
                size,
                weight,
                [1., 1., 1., opacity * a],
                516.,
            );
        }
        y += height;
    }
    r.pop_clip();
}
fn draw_brightness(r: &mut Renderer, level: i32) {
    r.circle(26., 20., 5.2, WHITE);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        r.circle(26. + a.cos() * 9.2, 20. + a.sin() * 9.2, 1.5, WHITE);
    }
    r.rounded_rect(54., 15., 300., 10., 5., [1., 1., 1., 0.28]);
    r.rounded_rect(
        54.,
        15.,
        (300. * level as f32 / 100.).max(10.),
        10.,
        5.,
        WHITE,
    );
}
fn local_date() -> String {
    #[cfg(unix)]
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut t: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&now, &mut t).is_null() {
            let days = [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ];
            let months = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];
            return format!(
                "{} {} {}",
                days[t.tm_wday as usize % 7],
                t.tm_mday,
                months[t.tm_mon as usize % 12]
            );
        }
    }
    String::new()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mosaic_uses_exact_cell_and_card_geometry() {
        assert_eq!(cell_rect(0, 0, 2), [0., 40., 220., 220.]);
        assert_eq!(cell_rect(3, 5, 1), [550., 370., 110., 110.]);
        assert_eq!(info_height(), 92.);
    }
    #[test]
    fn source_selection_grows_for_180ms_and_expires_after_six_seconds() {
        let mut s = Saver::new();
        s.ready = true;
        s.big = Some(0);
        s.move_selection(Action::Select);
        assert_eq!(s.selected, Some((0, 0)));
        s.move_selection(Action::Right);
        assert_eq!(s.selected, Some((0, 2)));
        assert!(s.grow.running(0.179));
        assert!(!s.grow.running(0.18));
    }
    #[test]
    fn visualizer_controls_keep_clock_and_song_independent() {
        let mut s = Saver::new();
        s.style = "viz".into();
        s.ready = true;
        s.input(Action::Start);
        assert_eq!(s.clock_a.target(), 1.);
        assert_eq!(s.low_a.target(), 0.);
        s.input(Action::Up);
        assert_eq!(s.low_a.target(), 1.);
        s.input(Action::Select);
        assert_eq!(s.palette(), 1);
        s.input(Action::Left);
        assert_eq!(s.effect(), 4);
    }
}
