//! Bounded artwork decoding off the UI thread. GPU objects stay on the UI.
use crate::render::{Renderer, Texture};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::{Instant, SystemTime};
pub type CoverSignatures = HashMap<PathBuf, (u64, Option<SystemTime>)>;

/// Called by the library worker, never by frame lookups.
pub fn cover_signatures<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> CoverSignatures {
    let mut signatures = HashMap::new();
    for path in paths {
        signatures.entry(path.clone()).or_insert_with(|| {
            std::fs::metadata(path)
                .map(|m| (m.len(), m.modified().ok()))
                .unwrap_or((0, None))
        });
    }
    signatures
}
mod decoder;

struct Pixels {
    generation: u64,
    path: PathBuf,
    result: Result<image::RgbaImage, String>,
    palette: Option<[[f32; 4]; 6]>,
    mipmaps: Vec<image::RgbaImage>,
}
struct Decoders {
    tx: Option<mpsc::SyncSender<(u64, PathBuf)>>,
    rx: mpsc::Receiver<Pixels>,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Decoders {
    fn new() -> Result<Self, String> {
        Self::with_decoder(decode)
    }
    fn with_decoder<F>(decode: F) -> Result<Self, String>
    where
        F: Fn(&Path) -> Result<image::RgbaImage, String> + Send + Sync + 'static,
    {
        let (tx, jobs) = mpsc::sync_channel::<(u64, PathBuf)>(32);
        let (results, rx) = mpsc::sync_channel(8);
        let jobs = Arc::new(Mutex::new(jobs));
        let decode = Arc::new(decode);
        let mut decoders = Self {
            tx: Some(tx),
            rx,
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::with_capacity(2),
        };
        for n in 0..2 {
            let jobs = jobs.clone();
            let results = results.clone();
            let decode = decode.clone();
            let stop = decoders.stop.clone();
            match thread::Builder::new()
                .name(format!("artwork-{n}"))
                .spawn(move || {
                    background_priority();
                    while !stop.load(Ordering::Acquire) {
                        let (generation, path) = match jobs.lock() {
                            Ok(r) => match r.recv() {
                                Ok(job) => job,
                                Err(_) => break,
                            },
                            Err(_) => break,
                        };
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let result = decode(&path);
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let palette = result.as_ref().ok().map(colors);
                        let mipmaps = result.as_ref().ok().map(mipmap_levels).unwrap_or_default();
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        if results
                            .send(Pixels {
                                generation,
                                path,
                                result,
                                palette,
                                mipmaps,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }) {
                Ok(worker) => decoders.threads.push(worker),
                Err(error) => {
                    decoders.shutdown();
                    return Err(error.to_string());
                }
            }
        }
        Ok(decoders)
    }
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        // A completed decode can be blocked on the bounded result channel.
        // Drain it while joining, and stop before taking any queued SD reads.
        while self.threads.iter().any(|worker| !worker.is_finished()) {
            let _ = self.rx.recv_timeout(std::time::Duration::from_millis(10));
        }
        for worker in self.threads.drain(..) {
            let _ = worker.join();
        }
        while self.rx.try_recv().is_ok() {}
    }
}

fn background_priority() {
    // On Linux niceness belongs to this worker thread. JPEG's short-lived
    // per-component helpers inherit it; UI/audio keep their existing priority.
    #[cfg(target_os = "linux")]
    unsafe {
        if libc::getpriority(libc::PRIO_PROCESS, 0) < 10
            && libc::setpriority(libc::PRIO_PROCESS, 0, 10) != 0
        {
            eprintln!(
                "Artwork background priority: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

impl Drop for Decoders {
    fn drop(&mut self) {
        self.shutdown();
    }
}
pub struct Artwork {
    decoders: Decoders,
    generation: u64,
    pending: HashSet<PathBuf>,
    failed: HashSet<PathBuf>,
    textures: HashMap<PathBuf, (Texture, Instant)>,
    palettes: HashMap<PathBuf, [[f32; 4]; 6]>,
    pub placeholder: Texture,
    closed: bool,
    signatures: CoverSignatures,
}
impl Artwork {
    pub fn new(renderer: &Renderer, _assets: &Path) -> Result<Self, String> {
        let image = raster_default_artwork(512)?;
        let decoders = Decoders::new()?;
        let placeholder =
            renderer.upload_artwork(image.width(), image.height(), image.as_raw(), &[])?;
        Ok(Self {
            decoders,
            generation: 0,
            pending: HashSet::new(),
            failed: HashSet::new(),
            textures: HashMap::new(),
            palettes: HashMap::new(),
            placeholder,
            closed: false,
            signatures: HashMap::new(),
        })
    }
    pub fn peek(&mut self, path: Option<&Path>) -> Texture {
        path.and_then(|p| {
            self.textures.get_mut(p).map(|(texture, used)| {
                *used = Instant::now();
                *texture
            })
        })
        .unwrap_or(self.placeholder)
    }
    /// A failed picture already has its final fallback. It must not consume
    /// Cover Flow's bounded request budget again on every frame.
    pub fn missing(&self, path: Option<&Path>) -> bool {
        path.is_some_and(|p| {
            !self.textures.contains_key(p) && !self.failed.contains(p) && !self.pending.contains(p)
        })
    }
    pub fn get(&mut self, path: Option<&Path>) -> Texture {
        let Some(path) = path else {
            return self.placeholder;
        };
        if let Some((texture, used)) = self.textures.get_mut(path) {
            *used = Instant::now();
            return *texture;
        }
        if !self.pending.contains(path) && !self.failed.contains(path) {
            if let Some(tx) = &self.decoders.tx {
                if tx.try_send((self.generation, path.to_path_buf())).is_ok() {
                    self.pending.insert(path.to_path_buf());
                }
            }
        }
        self.placeholder
    }
    pub fn poll(&mut self, renderer: &Renderer) -> bool {
        let mut changed = false;
        // At most two uploads per frame; decoding/SD never happens here.
        for _ in 0..2 {
            let Ok(result) = self.decoders.rx.try_recv() else {
                break;
            };
            if result.generation != self.generation {
                continue;
            }
            self.pending.remove(&result.path);
            match result.result.and_then(|p| {
                renderer.upload_artwork(p.width(), p.height(), p.as_raw(), &result.mipmaps)
            }) {
                Ok(texture) => {
                    if let Some(palette) = result.palette {
                        self.palettes.insert(result.path.clone(), palette);
                    }
                    if let Some((old, _)) =
                        self.textures.insert(result.path, (texture, Instant::now()))
                    {
                        renderer.delete_texture(old);
                    }
                    changed = true;
                }
                Err(e) => {
                    eprintln!("Artwork {}: {e}", result.path.display());
                    self.failed.insert(result.path);
                }
            }
        }
        while self.textures.len() > 40 {
            if let Some(old) = self
                .textures
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(p, _)| p.clone())
            {
                if let Some((texture, _)) = self.textures.remove(&old) {
                    self.palettes.remove(&old);
                    renderer.delete_texture(texture);
                }
            }
        }
        changed
    }
    pub fn invalidate(&mut self, renderer: &Renderer) {
        self.generation = self.generation.wrapping_add(1);
        self.pending.clear();
        self.failed.clear();
        self.palettes.clear();
        self.signatures.clear();
        for (_, (texture, _)) in self.textures.drain() {
            renderer.delete_texture(texture);
        }
    }
    pub fn reconcile(&mut self, signatures: CoverSignatures, renderer: &Renderer) {
        let changed: Vec<_> = self
            .signatures
            .iter()
            .filter(|(path, value)| signatures.get(*path) != Some(*value))
            .map(|(path, _)| path.clone())
            .collect();
        if !changed.is_empty() {
            self.generation = self.generation.wrapping_add(1);
            self.pending.clear();
            for path in changed {
                self.failed.remove(&path);
                self.palettes.remove(&path);
                if let Some((texture, _)) = self.textures.remove(&path) {
                    renderer.delete_texture(texture);
                }
            }
        }
        self.signatures = signatures;
    }
    pub fn close(&mut self, renderer: &Renderer) {
        self.decoders.shutdown();
        if self.closed {
            return;
        }
        self.closed = true;
        self.pending.clear();
        self.failed.clear();
        self.palettes.clear();
        for (_, (texture, _)) in self.textures.drain() {
            renderer.delete_texture(texture);
        }
        renderer.delete_texture(self.placeholder);
    }
    pub fn palette(&self, path: Option<&Path>) -> Option<[[f32; 4]; 6]> {
        path.and_then(|p| self.palettes.get(p).copied())
    }
}
/// A trace of the original 339px default_album_artwork.png, rendered at the
/// largest album-art display size. The paired stems, sloped beam and muted
/// horizontal grey gradient keep the original picture; only its raster
/// resolution changes. This runs once at startup, before the frame loop.
fn raster_default_artwork(size: u32) -> Result<image::RgbaImage, String> {
    use tiny_skia as sk;
    if !(16..=2048).contains(&size) {
        return Err("Invalid fallback artwork resolution".into());
    }
    let mut pixmap = sk::Pixmap::new(size, size).ok_or("Cannot allocate fallback artwork")?;
    let scale = sk::Transform::from_scale(size as f32 / 339.0, size as f32 / 339.0);
    let background =
        sk::PathBuilder::from_rect(sk::Rect::from_xywh(0.0, 0.0, 339.0, 339.0).unwrap());
    let shader = sk::LinearGradient::new(
        sk::Point::from_xy(0.0, 0.0),
        sk::Point::from_xy(339.0, 0.0),
        vec![
            sk::GradientStop::new(0.0, sk::Color::from_rgba8(226, 225, 225, 255)),
            sk::GradientStop::new(1.0, sk::Color::from_rgba8(223, 221, 222, 255)),
        ],
        sk::SpreadMode::Pad,
        sk::Transform::identity(),
    )
    .unwrap();
    pixmap.fill_path(
        &background,
        &sk::Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        },
        sk::FillRule::Winding,
        scale,
        None,
    );
    let mut note = sk::PathBuilder::new();
    note.move_to(127.0, 107.0);
    note.quad_to(127.0, 105.0, 131.0, 104.0);
    note.line_to(217.0, 91.0);
    note.cubic_to(220.0, 90.0, 223.0, 91.0, 223.0, 95.0);
    note.line_to(223.0, 214.0);
    note.cubic_to(223.0, 226.0, 212.0, 234.0, 199.0, 234.0);
    note.cubic_to(187.0, 234.0, 179.0, 228.0, 179.0, 219.0);
    note.cubic_to(179.0, 207.0, 191.0, 197.0, 205.0, 197.0);
    note.line_to(209.0, 197.0);
    note.quad_to(215.0, 197.0, 215.0, 191.0);
    note.line_to(215.0, 139.0);
    note.cubic_to(215.0, 130.0, 212.0, 127.0, 202.0, 129.0);
    note.line_to(146.0, 137.0);
    note.cubic_to(138.0, 138.0, 135.0, 141.0, 135.0, 149.0);
    note.line_to(135.0, 228.0);
    note.cubic_to(135.0, 239.0, 124.0, 247.0, 111.0, 247.0);
    note.cubic_to(100.0, 247.0, 91.0, 241.0, 91.0, 231.0);
    note.cubic_to(91.0, 220.0, 103.0, 210.0, 117.0, 210.0);
    note.line_to(120.0, 210.0);
    note.quad_to(127.0, 210.0, 127.0, 203.0);
    note.close();
    let mut paint = sk::Paint::default();
    paint.set_color_rgba8(182, 181, 182, 255);
    paint.anti_alias = true;
    pixmap.fill_path(
        &note.finish().unwrap(),
        &paint,
        sk::FillRule::Winding,
        scale,
        None,
    );
    image::RgbaImage::from_raw(size, size, pixmap.take())
        .ok_or_else(|| "Invalid fallback artwork pixels".into())
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    #[test]
    fn native_fallback_retains_original_note_silhouette() {
        let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("../port/betterpod/assets");
        let root = std::env::var_os("BETTERPOD_RUST_ASSETS_DIR")
            .map(PathBuf::from)
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
        let original = image::open(root.join("ipod/default_album_artwork.png"))
            .unwrap()
            .to_rgba8();
        let traced = raster_default_artwork(339).unwrap();
        let (mut intersection, mut union) = (0, 0);
        for (a, b) in original.pixels().zip(traced.pixels()) {
            let a = a[0] < 205;
            let b = b[0] < 205;
            intersection += usize::from(a && b);
            union += usize::from(a || b);
        }
        assert!(
            intersection as f64 / union as f64 > 0.98,
            "traced shape IoU {}",
            intersection as f64 / union as f64
        );
        let native = raster_default_artwork(512).unwrap();
        assert_eq!(native.dimensions(), (512, 512));
        assert!(native.pixels().all(|p| p[3] == 255));
        assert!(native.get_pixel(0, 0)[0] > native.get_pixel(511, 0)[0]);
        assert!(raster_default_artwork(0).is_err());
    }
    #[test]
    #[ignore = "writes local native-resolution fallback artwork fixture"]
    fn dump_native_fallback() -> Result<(), String> {
        raster_default_artwork(512)?
            .save(std::env::var("BETTERPOD_FALLBACK_OUT").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
}
fn colors(image: &image::RgbaImage) -> [[f32; 4]; 6] {
    let mut colors = Vec::with_capacity(196);
    for y in 0..14 {
        for x in 0..14 {
            let p = image.get_pixel(
                ((x * image.width() / 14) + image.width() / 28).min(image.width() - 1),
                ((y * image.height() / 14) + image.height() / 28).min(image.height() - 1),
            );
            colors.push([p[0] as f64, p[1] as f64, p[2] as f64]);
        }
    }
    colors.sort_by(|a, b| {
        let light = |p: &[f64; 3]| p[0] * 0.3 + p[1] * 0.59 + p[2] * 0.11;
        light(a).total_cmp(&light(b))
    });
    // ij_vizcolors.go's exact 14×14 quantile neighbourhoods, saturation
    // and luminance floor; calculation stays on the artwork worker.
    let pick = |q: f64| {
        let center = (q * (colors.len() - 1) as f64) as usize;
        let start = center.saturating_sub(6);
        let end = (center + 6).min(colors.len() - 1);
        let mut c = [0.; 3];
        for sample in &colors[start..=end] {
            for k in 0..3 {
                c[k] += sample[k];
            }
        }
        for v in &mut c {
            *v /= (end - start + 1) as f64;
        }
        let mean = (c[0] + c[1] + c[2]) / 3.;
        for v in &mut c {
            *v = (mean + (*v - mean) * 1.45).clamp(0., 255.);
        }
        let luminance = c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
        if luminance < 60. {
            let f = 60. / luminance.max(1.);
            for v in &mut c {
                *v = (*v * f).min(255.);
            }
        }
        c
    };
    let first = pick(0.3);
    let last = pick(0.93);
    let stops = [
        first.map(|v| v * 0.12),
        first,
        pick(0.55),
        pick(0.75),
        last,
        last.map(|v| v + (255. - v) * 0.6),
    ];
    stops.map(|c| {
        [
            c[0] as f32 / 255.,
            c[1] as f32 / 255.,
            c[2] as f32 / 255.,
            1.,
        ]
    })
}
// Driver-generated mipmaps can alias odd-sized images. Prepare their area
// filtered levels on the bounded decoder workers, never in the UI loop.
pub(crate) fn mipmap_levels(image: &image::RgbaImage) -> Vec<image::RgbaImage> {
    if image.width().is_power_of_two() && image.height().is_power_of_two() {
        return Vec::new();
    }
    let mut levels: Vec<image::RgbaImage> = Vec::new();
    let (mut width, mut height) = (image.width(), image.height());
    while width > 1 || height > 1 {
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        let previous = levels.last().unwrap_or(image);
        levels.push(image::imageops::resize(
            previous,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ));
    }
    levels
}
fn decode(path: &Path) -> Result<image::RgbaImage, String> {
    decoder::decode(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_joins_inflight_decoders_without_reading_the_queued_backlog() {
        use std::sync::{atomic::AtomicUsize, Barrier};
        let entered = Arc::new(Barrier::new(3));
        let release = Arc::new(Barrier::new(3));
        let calls = Arc::new(AtomicUsize::new(0));
        let (started, finish, count) = (entered.clone(), release.clone(), calls.clone());
        let mut decoders = Decoders::with_decoder(move |_| {
            count.fetch_add(1, Ordering::AcqRel);
            started.wait();
            finish.wait();
            Ok(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([1, 2, 3, 255]),
            ))
        })
        .unwrap();
        for i in 0..2 {
            decoders
                .tx
                .as_ref()
                .unwrap()
                .send((0, PathBuf::from(format!("inflight-{i}"))))
                .unwrap();
        }
        entered.wait();
        for i in 0..32 {
            decoders
                .tx
                .as_ref()
                .unwrap()
                .try_send((0, PathBuf::from(format!("queued-{i}"))))
                .unwrap();
        }
        let stop = decoders.stop.clone();
        let shutdown = thread::spawn(move || {
            decoders.shutdown();
            assert!(decoders.threads.is_empty());
            assert!(decoders.tx.is_none());
        });
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while !stop.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        release.wait();
        shutdown.join().unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 2);
    }
    #[test]
    fn shutdown_drains_a_full_result_channel_before_joining() {
        use std::sync::atomic::AtomicUsize;
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut decoders = Decoders::with_decoder(move |_| {
            count.fetch_add(1, Ordering::AcqRel);
            Ok(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([50, 50, 50, 255]),
            ))
        })
        .unwrap();
        for i in 0..24 {
            decoders
                .tx
                .as_ref()
                .unwrap()
                .send((0, PathBuf::from(format!("fixture-{i}"))))
                .unwrap();
        }
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while calls.load(Ordering::Acquire) < 10 {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        // Eight completed results plus two blocked senders, without UI polling.
        decoders.shutdown();
        assert!(decoders.threads.is_empty());
        assert!(calls.load(Ordering::Acquire) <= 10);
        decoders.shutdown(); // closing an already closed app is harmless
    }
    #[test]
    fn album_scheme_uses_original_luminance_floor_and_tints() {
        let image = image::RgbaImage::from_pixel(28, 28, image::Rgba([50, 50, 50, 255]));
        let palette = colors(&image);
        assert!((palette[0][0] - 7.2 / 255.).abs() < 1e-6);
        assert!((palette[1][0] - 60. / 255.).abs() < 1e-6);
        assert!((palette[5][0] - 177. / 255.).abs() < 1e-6);
        assert!(palette
            .iter()
            .all(|c| c[0] == c[1] && c[1] == c[2] && c[3] == 1.));
    }
}
