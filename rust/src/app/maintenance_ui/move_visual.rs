//! Go 1.1 Moving Music composition. File inventory and album projection are
//! prepared on a worker; the frame loop only samples the published model.
use super::{Renderer, VisualAssets};
use crate::{
    art::Artwork,
    library::Library,
    render::{line_box, Color},
};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
    time::Instant,
};
#[derive(Clone, Debug)]
struct Unit {
    title: String,
    artist: String,
    cover: Option<PathBuf>,
    bytes: u64,
    files: Vec<PathBuf>,
}
struct FileProgress {
    unit: usize,
    unit_before: u64,
    unit_end: u64,
    global_before: u64,
    index: usize,
}
struct Plan {
    source: PathBuf,
    units: Vec<Unit>,
    lookup: HashMap<PathBuf, FileProgress>,
    all: u64,
    files: usize,
    songs: usize,
    from: String,
    to: String,
    moving: bool,
}
pub(super) struct MoveView {
    plan: Option<Plan>,
    pending: Option<mpsc::Receiver<Plan>>,
    worker: Option<thread::JoinHandle<()>>,
    phase: String,
    path: PathBuf,
    done: u64,
    total: u64,
    cur: usize,
    checked: usize,
    shown_unit: usize,
    shown_frac: f64,
    last: Option<Instant>,
    started: Instant,
    speed: f64,
    error: String,
    file_progress: Option<u64>,
}
impl MoveView {
    #[cfg(test)]
    pub fn visual_fixture(source: PathBuf, library: &Library, phase: &str) -> Self {
        let files = library
            .tracks
            .iter()
            .filter_map(|track| {
                track
                    .path
                    .strip_prefix(&source)
                    .ok()
                    .map(|p| (p.to_path_buf(), 10 * 1024 * 1024))
            })
            .collect();
        let mut plan = project(source, "/fixture-sd2/music".into(), true, files, library);
        plan.from = "SD 1".into();
        plan.to = "SD 2".into();
        let cur = 1.min(plan.units.len().saturating_sub(1));
        let unit = &plan.units[cur];
        let done = plan.units.iter().take(cur).map(|u| u.bytes).sum::<u64>() + unit.bytes / 2;
        let path = unit.files.first().cloned().unwrap_or_default();
        let checked = plan
            .units
            .iter()
            .take(cur)
            .map(|u| u.files.len())
            .sum::<usize>()
            + unit.files.len() / 2;
        let total = plan.all;
        let mut view = Self::new();
        view.plan = Some(plan);
        view.phase = phase.into();
        view.cur = cur;
        view.shown_unit = cur;
        view.shown_frac = 0.5;
        view.path = path;
        view.done = done;
        view.total = total;
        view.checked = checked;
        view.speed = 8. * 1024. * 1024.;
        view.last = Some(Instant::now() + std::time::Duration::from_secs(3600));
        view
    }
    pub fn new() -> Self {
        Self {
            plan: None,
            pending: None,
            worker: None,
            phase: "count".into(),
            path: PathBuf::new(),
            done: 0,
            total: 0,
            cur: 0,
            checked: 0,
            shown_unit: 0,
            shown_frac: 0.,
            last: None,
            started: Instant::now(),
            speed: 0.,
            error: String::new(),
            file_progress: None,
        }
    }
    pub fn prepare(
        &mut self,
        source: PathBuf,
        destination: PathBuf,
        moving: bool,
        files: Vec<(PathBuf, u64)>,
        library: Arc<Library>,
    ) {
        self.prepare_inner(source, destination, moving, files, library, false);
    }
    pub fn prepare_relocated(
        &mut self,
        source: PathBuf,
        destination: PathBuf,
        moving: bool,
        files: Vec<(PathBuf, u64)>,
        library: Arc<Library>,
    ) {
        self.prepare_inner(source, destination, moving, files, library, true);
    }
    fn prepare_inner(
        &mut self,
        source: PathBuf,
        destination: PathBuf,
        moving: bool,
        files: Vec<(PathBuf, u64)>,
        library: Arc<Library>,
        relocated: bool,
    ) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(rx);
        // An older projection can be pending when a job is resumed; disconnect
        // before joining so its bounded send cannot keep shutdown waiting.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.worker = thread::Builder::new()
            .name("music-transfer-view".into())
            .spawn(move || {
                let mut plan =
                    project(source.clone(), destination.clone(), moving, files, &library);
                if relocated {
                    for unit in &mut plan.units {
                        if let Some(cover) = unit.cover.as_mut() {
                            if let Ok(relative) = cover.strip_prefix(&source) {
                                *cover = destination.join(relative);
                            }
                        }
                    }
                }
                let _ = tx.send(plan);
            })
            .ok();
    }
    pub fn progress(&mut self, phase: &str, done: u64, total: u64, path: PathBuf) {
        self.phase = match phase {
            "scan" => "count",
            "copy-file" => "copy",
            other => other,
        }
        .into();
        self.done = done;
        self.total = total;
        self.file_progress = (phase == "copy-file").then_some(done);
        self.path = path;
        self.sync_progress();
        if self.phase == "copy" {
            self.speed = self.done as f64 / self.started.elapsed().as_secs_f64().max(0.001);
        }
    }
    fn sync_progress(&mut self) {
        if let Some(p) = &self.plan {
            if let Ok(relative) = self.path.strip_prefix(&p.source) {
                self.path = relative.to_path_buf();
            }
            if let Some(file) = p.lookup.get(&self.path) {
                self.cur = file.unit;
                if let Some(done) = self.file_progress {
                    self.done = file.global_before + done.min(file.unit_end - file.unit_before);
                    self.total = p.all;
                }
                if self.phase == "verify" {
                    self.checked = file.index + 1;
                }
            }
            if self.phase == "delete" {
                self.checked = self.done as usize;
            }
        }
    }
    pub fn done(&mut self) {
        self.phase = "done".into();
    }
    pub fn fail(&mut self, error: String, stopped: bool) {
        self.phase = if stopped { "stopped" } else { "failed" }.into();
        self.error = error;
    }
    pub fn finished(&self) -> bool {
        matches!(self.phase.as_str(), "done" | "failed" | "stopped")
    }
    pub fn deleting(&self) -> bool {
        self.phase == "delete"
    }
    fn poll(&mut self) {
        if let Some(plan) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.plan = Some(plan);
            self.pending = None;
            self.sync_progress();
        }
    }
    pub fn draw(&mut self, r: &mut Renderer, art: &mut Artwork, assets: &VisualAssets, dark: bool) {
        self.poll();
        let theme = Theme::new(dark);
        r.rect(0., 40., 640., 440., theme.bg);
        let a12 = line_box(24.).1;
        let a11 = line_box(22.).1;
        if (self.phase == "count" || self.plan.is_none()) && !self.finished() {
            center(
                r,
                "Counting the files…",
                320.,
                220. + a12,
                28.,
                700,
                theme.text,
            );
            center(
                r,
                "Then they're copied, checked, and only then deleted here",
                320.,
                260. + a11,
                22.,
                500,
                theme.sub,
            );
            return;
        }
        if self.phase == "failed" || self.phase == "stopped" {
            let (title, subtitle) = if self.phase == "stopped" {
                (
                    "Stopped",
                    format!(
                        "What's copied stays on {}; nothing was deleted. Move again to go on.",
                        self.plan
                            .as_ref()
                            .map_or("the destination card", |p| p.to.as_str())
                    ),
                )
            } else {
                ("Not moved", self.error.clone())
            };
            center(r, title, 320., 200. + a12, 30., 700, rgb(212, 42, 30));
            let lines = wrap(r, &subtitle, 540., 24., 500, 3);
            for (i, line) in lines.iter().enumerate() {
                center(
                    r,
                    line,
                    320.,
                    244. + i as f32 * 32. + a12,
                    24.,
                    500,
                    theme.sub,
                );
            }
            center(
                r,
                "The music is still where it was",
                320.,
                352. + a11,
                22.,
                500,
                theme.sub,
            );
            button(r, "Close", false, &theme);
            return;
        }
        let Some(p) = self.plan.as_ref() else { return };
        let n = p.units.len();
        if self.phase == "done" {
            for k in (0..=3).rev() {
                for sign in [-1_i32, 1] {
                    if (k == 0 && sign == 1) || n == 0 {
                        continue;
                    }
                    let i = ((n as i32 / 2 + sign * k).rem_euclid(n as i32)) as usize;
                    let sz = 168. - k as f32 * 24.;
                    let x = 320. + (sign * k) as f32 * 92. - sz / 2.;
                    let y = 84. + (168. - sz) * 0.5;
                    let opacity = 1. - 0.18 * k as f32;
                    cover(r, art, &p.units[i], x, y, sz, opacity, theme.border);
                    let t = art.get(p.units[i].cover.as_deref());
                    let rh = (sz * 0.25) as usize;
                    for row in 0..rh {
                        let source = 1. - (row as f32 + 1.) / sz;
                        r.image_uv(
                            t,
                            [x, y + sz + 2. + row as f32, sz, 1.],
                            [0., source, 1., source + 1. / sz],
                            opacity * 0.22 * (1. - row as f32 / rh as f32),
                        );
                    }
                }
            }
            let count = if p.songs > 0 {
                format!("{} songs", commas(p.songs))
            } else {
                format!("{} files", commas(p.files))
            };
            let verb = if p.moving { "moved" } else { "copied" };
            center(
                r,
                &format!("All {count} {verb} to {}", p.to),
                320.,
                320. + a12,
                30.,
                700,
                theme.text,
            );
            center(
                r,
                &format!(
                    "{:.1} GB · {} {} · checked",
                    p.all as f64 / 1e9,
                    n,
                    if n == 1 { "folder" } else { "folders" }
                ),
                320.,
                356. + a12,
                24.,
                500,
                theme.sub,
            );
            button(r, "Done", true, &theme);
            return;
        }
        let now = Instant::now();
        let dt = self.last.map_or(0.2, |last| {
            now.saturating_duration_since(last).as_secs_f64().min(0.2)
        });
        self.last = Some(now);
        if self.phase == "copy" || self.phase == "paused" {
            if self.cur.saturating_sub(self.shown_unit) > 3 {
                self.shown_unit = self.cur - 1;
                self.shown_frac = 0.5;
            }
            let target = if self.shown_unit == self.cur {
                p.lookup.get(&self.path).map_or(0., |file| {
                    let file_done = self
                        .file_progress
                        .unwrap_or(file.unit_end - file.unit_before)
                        .min(file.unit_end - file.unit_before);
                    (file.unit_before + file_done) as f64 / p.units[self.cur].bytes.max(1) as f64
                })
            } else {
                1.
            };
            if self.shown_frac < target {
                self.shown_frac = target
                    .min(self.shown_frac + ((target - self.shown_frac) * dt * 6.).max(dt * 0.6));
                if self.shown_unit < self.cur {
                    self.shown_frac = 1_f64.min(self.shown_frac + dt * 2.);
                }
            }
            if self.shown_unit < self.cur && self.shown_frac >= 1. {
                self.shown_unit += 1;
                self.shown_frac = 0.;
            }
        } else {
            self.shown_unit = self.cur;
        }
        let cur = self.shown_unit.min(n.saturating_sub(1));
        match self.phase.as_str() {
            "verify" => {
                pile(r, art, p, 68., cur + 1, n, false, &theme);
                pile(r, art, p, 460., 0, cur, true, &theme);
                label(
                    r,
                    124.,
                    &p.to,
                    &format!("{} to check", p.files.saturating_sub(self.checked)),
                    &theme,
                );
                label(r, 516., &p.to, &format!("{} checked", self.checked), &theme);
                if let Some(unit) = p.units.get(cur) {
                    r.rect(262., 110., 128., 128., [0., 0., 0., 0.16]);
                    cover(r, art, unit, 256., 100., 128., 1., theme.border);
                    r.stroke_rounded(253., 97., 134., 134., 4., 3., rgb(58, 160, 255));
                }
            }
            "delete" => {
                pile(r, art, p, 460., 0, n, true, &theme);
                label(r, 124., &p.from, "removing the originals", &theme);
                label(
                    r,
                    516.,
                    &p.to,
                    &format!("{:.1} GB arrived", p.all as f64 / 1e9),
                    &theme,
                );
            }
            _ => {
                pile(r, art, p, 68., cur + 1, n, false, &theme);
                pile(r, art, p, 460., 0, cur, false, &theme);
                label(
                    r,
                    124.,
                    &p.from,
                    &format!(
                        "{:.1} GB to move",
                        p.all.saturating_sub(self.done) as f64 / 1e9
                    ),
                    &theme,
                );
                label(
                    r,
                    516.,
                    &p.to,
                    &format!("{:.1} GB arrived", self.done as f64 / 1e9),
                    &theme,
                );
                if let Some(unit) = p.units.get(cur) {
                    let e = self.shown_frac * self.shown_frac * (3. - 2. * self.shown_frac);
                    let size = 112. * (1. + 0.25 * (std::f64::consts::PI * e).sin());
                    let x = 68. + 392. * e + 56. - size / 2.;
                    let y = 108. - 32. * (std::f64::consts::PI * e).sin() + 56. - size / 2.;
                    r.rect(
                        x as f32 + 6.,
                        y as f32 + 10.,
                        size as f32,
                        size as f32,
                        [0., 0., 0., 0.16],
                    );
                    cover(
                        r,
                        art,
                        unit,
                        x as f32,
                        y as f32,
                        size as f32,
                        1.,
                        theme.border,
                    );
                }
            }
        }
        let unit = p.units.get(cur);
        let (title, subtitle) = match self.phase.as_str() {
            "verify" => (
                format!(
                    "Checking {} · {} of {} files",
                    unit.map_or("Other files", |u| u.title.as_str()),
                    self.checked,
                    p.files
                ),
                format!("Each file is read back from {} and compared", p.to),
            ),
            "delete" => (
                format!("Removing the originals · {} of {}", self.checked, p.files),
                format!("All checked: the copies on {} are good", p.to),
            ),
            "paused" => (
                "Paused · put the card back".into(),
                "It goes on from here; nothing is lost".into(),
            ),
            _ => (
                unit.map_or_else(
                    || "Other files".into(),
                    |u| {
                        if u.artist.is_empty() {
                            u.title.clone()
                        } else {
                            format!("{} · {}", u.title, u.artist)
                        }
                    },
                ),
                self.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        center(
            r,
            &title,
            320.,
            332. + a12,
            26.,
            700,
            if self.phase == "paused" {
                rgb(212, 42, 30)
            } else {
                theme.text
            },
        );
        center(r, &subtitle, 320., 366. + a11, 22., 500, theme.sub);
        let pct = if self.total > 0 {
            self.done as f64 / self.total as f64
        } else {
            0.
        };
        super::update_visual::progress(r, assets, 80., 400., 480., pct as f32, dark);
        let mut line = format!("{}%", (pct * 100. + 0.5) as u64);
        if self.phase == "copy" && self.speed > 0. {
            line.push_str(&format!(
                " · {} · {:.0} MB/s",
                eta(p.all.saturating_sub(self.done) as f64 / self.speed),
                self.speed / 1e6
            ));
        }
        center(r, &line, 320., 444. + a12, 24., 700, theme.sub);
    }
}
impl Drop for MoveView {
    fn drop(&mut self) {
        self.pending.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn project(
    source: PathBuf,
    destination: PathBuf,
    moving: bool,
    files: Vec<(PathBuf, u64)>,
    library: &Library,
) -> Plan {
    let mut by_dir = HashMap::new();
    for album in &library.albums {
        if let Some(track) = album
            .track_indices
            .first()
            .and_then(|&i| library.tracks.get(i))
        {
            if let Some(dir) = track.path.parent() {
                if let Ok(relative) = dir.strip_prefix(&source) {
                    by_dir.insert(relative.to_path_buf(), album);
                }
            }
        }
    }
    let mut grouped = BTreeMap::<PathBuf, Unit>::new();
    let mut other = Unit {
        title: "Other files".into(),
        artist: String::new(),
        cover: None,
        bytes: 0,
        files: Vec::new(),
    };
    let count = files.len();
    let mut songs = 0;
    let mut all = 0;
    let mut sizes = HashMap::new();
    let mut global = HashMap::new();
    for (index, (path, size)) in files.into_iter().enumerate() {
        global.insert(path.clone(), (all, index));
        sizes.insert(path.clone(), size);
        all += size;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "mp3" | "flac" | "m4a" | "aac" | "wav" | "ogg" | "opus" | "aiff" | "aif" | "wv"
        ) {
            songs += 1;
        }
        let directory = path.parent().unwrap_or(Path::new(""));
        let unit = if let Some(album) = by_dir.get(directory) {
            grouped
                .entry(directory.to_path_buf())
                .or_insert_with(|| Unit {
                    title: album.name.clone(),
                    artist: album.artist.clone(),
                    cover: album.cover.clone(),
                    bytes: 0,
                    files: Vec::new(),
                })
        } else {
            &mut other
        };
        unit.bytes += size;
        unit.files.push(path);
    }
    let mut units = grouped.into_values().collect::<Vec<_>>();
    if !other.files.is_empty() {
        units.push(other);
    }
    let mut lookup = HashMap::new();
    for (i, unit) in units.iter().enumerate() {
        let mut bytes = 0;
        for file in &unit.files {
            let size = sizes.get(file).copied().unwrap_or(0);
            let before = bytes;
            bytes += size;
            let (global_before, index) = global[file];
            lookup.insert(
                file.clone(),
                FileProgress {
                    unit: i,
                    unit_before: before,
                    unit_end: bytes,
                    global_before,
                    index,
                },
            );
        }
    }
    let from = card_name(source.parent().unwrap_or(&source));
    let to = card_name(destination.parent().unwrap_or(&destination));
    Plan {
        source,
        units,
        lookup,
        all,
        files: count,
        songs,
        from,
        to,
        moving,
    }
}
fn card_name(path: &Path) -> String {
    super::discovery::card_name(path)
}
struct Theme {
    bg: Color,
    text: Color,
    sub: Color,
    border: Color,
    dark: bool,
}
impl Theme {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: rgb(22, 23, 25),
                text: rgb(238, 239, 241),
                sub: rgb(150, 153, 158),
                border: rgb(48, 50, 54),
                dark,
            }
        } else {
            Self {
                bg: [1.; 4],
                text: rgb(0, 0, 0),
                sub: rgb(100, 100, 100),
                border: rgb(243, 243, 243),
                dark,
            }
        }
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn center(r: &mut Renderer, text: &str, x: f32, y: f32, size: f32, weight: u16, color: Color) {
    let text = super::update_visual::ellipsize(r, text, 600., size, weight);
    let width = r.text_width_weighted(&text, size, weight);
    r.text_baseline(&text, x - width / 2., y, size, weight, color, 600.);
}
fn cover(
    r: &mut Renderer,
    art: &mut Artwork,
    unit: &Unit,
    x: f32,
    y: f32,
    size: f32,
    alpha: f32,
    border: Color,
) {
    r.rect(x - 1., y - 1., size + 2., size + 2., border);
    r.image(art.get(unit.cover.as_deref()), x, y, size, size, alpha);
}
fn pile(
    r: &mut Renderer,
    art: &mut Artwork,
    p: &Plan,
    x: f32,
    from: usize,
    to: usize,
    badge: bool,
    theme: &Theme,
) {
    let show = to.saturating_sub(from).min(4);
    for j in 0..show {
        let i = to - show + j;
        if let Some(unit) = p.units.get(i) {
            let y = 108. + (show - 1 - j) as f32 * 6.;
            r.rect(x + 4., y + 6., 112., 112., [0., 0., 0., 0.12]);
            cover(r, art, unit, x, y, 112., 1., theme.border);
            if badge && j == show - 1 {
                r.rounded_gradient(
                    x + 82.,
                    y + 82.,
                    36.,
                    36.,
                    18.,
                    &[(0., rgb(95, 211, 106)), (1., rgb(44, 159, 58))],
                );
                let width = r.text_width_weighted("✓", 22., 700);
                r.text_baseline(
                    "✓",
                    x + 100. - width / 2.,
                    y + 100. + line_box(22.).1 / 2.,
                    22.,
                    700,
                    [1.; 4],
                    30.,
                );
            }
        }
    }
}
fn label(r: &mut Renderer, cx: f32, card: &str, line: &str, theme: &Theme) {
    let width = r.text_width_weighted(card, 24., 700);
    let h = 24. * 0.727;
    let iw = h * 0.78;
    let x = cx - (width + iw + 8.) / 2.;
    let baseline = 244. + line_box(24.).1;
    sd_icon(r, x, baseline, h, theme.text, theme.bg);
    r.text_baseline(card, x + iw + 8., baseline, 24., 700, theme.text, 250.);
    center(r, line, cx, 280. + line_box(22.).1, 22., 500, theme.sub);
}
fn sd_icon(r: &mut Renderer, x: f32, y: f32, h: f32, color: Color, bg: Color) {
    let w = h * 0.78;
    let top = y - h;
    r.rounded_rect(x, top, w, h, 2.4, color);
    let cut = w * 0.34;
    r.triangle(
        [
            [x + w - cut, top - 1.],
            [x + w + 1., top - 1.],
            [x + w + 1., top + cut],
        ],
        bg,
    );
    for i in 0..3 {
        r.rect(
            x + w * 0.16 + i as f32 * w * 0.2,
            top + h * 0.14,
            (w * 0.11).max(1.),
            h * 0.26,
            bg,
        );
    }
}
fn button(r: &mut Renderer, label: &str, primary: bool, theme: &Theme) {
    let stops = if primary {
        vec![(0., rgb(60, 184, 255)), (1., rgb(52, 122, 181))]
    } else if theme.dark {
        vec![
            (0., rgb(85, 88, 95)),
            (0.4844, rgb(52, 55, 61)),
            (1., rgb(60, 63, 69)),
        ]
    } else {
        vec![
            (0., [1.; 4]),
            (0.4844, rgb(203, 204, 206)),
            (1., rgb(223, 223, 223)),
        ]
    };
    r.rounded_gradient(80., 408., 480., 56., 18., &stops);
    r.stroke_rounded(
        80.,
        408.,
        480.,
        56.,
        18.,
        2.,
        if primary {
            rgb(58, 120, 173)
        } else if theme.dark {
            rgb(21, 23, 26)
        } else {
            rgb(62, 66, 73)
        },
    );
    center(
        r,
        label,
        320.,
        436. + 28. * 0.727 / 2.,
        28.,
        700,
        if primary { [1.; 4] } else { theme.text },
    );
}
fn wrap(
    r: &mut Renderer,
    text: &str,
    width: f32,
    size: f32,
    weight: u16,
    max: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.into()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && r.text_width_weighted(&candidate, size, weight) > width {
            out.push(std::mem::take(&mut line));
            if out.len() == max - 1 {
                line = word.into();
                break;
            }
        }
        if !line.is_empty() {
            line.push(' ')
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(super::update_visual::ellipsize(
            r, &line, width, size, weight,
        ));
    }
    out
}
fn eta(seconds: f64) -> String {
    if seconds < 60. {
        "less than a minute left".into()
    } else if seconds < 3600. {
        format!("{} min left", (seconds / 60. + 0.5) as u64)
    } else {
        format!(
            "{} h {} min left",
            (seconds / 3600.) as u64,
            (seconds % 3600. / 60.) as u64
        )
    }
}
fn commas(value: usize) -> String {
    let value = value.to_string();
    let mut out = String::new();
    for (i, c) in value.chars().enumerate() {
        if i > 0 && (value.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c)
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_unit_projection_preserves_album_groups_and_other_files() {
        let lib = Library::from_tracks(vec![crate::library::Track {
            path: "/music/Album/song.flac".into(),
            album: "Album".into(),
            artist: "Artist".into(),
            ..Default::default()
        }]);
        let plan = project(
            "/music".into(),
            "/second/music".into(),
            false,
            vec![
                ("Album/song.flac".into(), 100),
                ("Album/cover.jpg".into(), 20),
                ("readme.txt".into(), 5),
            ],
            &lib,
        );
        assert_eq!(plan.units.len(), 2);
        assert_eq!(
            (plan.units[0].title.as_str(), plan.units[0].bytes),
            ("Album", 120)
        );
        assert_eq!(plan.units[1].title, "Other files");
        assert_eq!((plan.files, plan.songs, plan.all), (3, 1, 125));
    }
    #[test]
    fn remaining_time_uses_original_thresholds() {
        assert_eq!(eta(59.), "less than a minute left");
        assert_eq!(eta(150.), "3 min left");
        assert_eq!(eta(4500.), "1 h 15 min left");
        assert_eq!(commas(1234567), "1,234,567");
    }
}
