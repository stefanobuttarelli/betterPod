//! Bounded artwork decoding off the UI thread. GPU objects stay on the UI.
use crate::render::{Renderer, Texture};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
pub type CoverSignatures = HashMap<PathBuf, (u64, Option<SystemTime>)>;

/// Scaled covers kept on disk in the database folder, as Go's `.ipod_art`:
/// a cover is decoded from its original once, then from this copy, until the
/// original changes. Each name comes from the original's path, size and
/// modification time, so an edited cover gets a new entry; scans prune the
/// entries no album uses.
pub const SCALED_DIR: &str = "artwork-scaled";

fn scaled_name(path: &Path, len: u64, modified: Option<SystemTime>) -> String {
    use sha2::{Digest, Sha256};
    let nanos = modified
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_nanos());
    let mut hash = Sha256::new();
    hash.update(path.as_os_str().as_encoded_bytes());
    hash.update(format!("\0{len}\0{nanos}"));
    hash.finalize()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The cover at display size: from the scaled copy when the original is
/// unchanged, else decoded from the original and saved for next time.
fn decode_cached(dir: &Path, path: &Path) -> Result<image::RgbaImage, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let name = scaled_name(path, meta.len(), meta.modified().ok());
    for extension in ["jpg", "png"] {
        let copy = dir.join(format!("{name}.{extension}"));
        if copy.is_file() {
            if let Ok(image) = decoder::decode(&copy) {
                return Ok(image);
            }
        }
    }
    let image = decoder::decode(path)?;
    if let Err(error) = store_scaled(dir, &name, &image) {
        eprintln!("Artwork cache {}: {error}", path.display());
    }
    Ok(image)
}

/// The size of a cover in list rows (drawn at 96 x 96).
pub const THUMB: u32 = 96;
/// How many list thumbnails stay on the GPU: about 11 MB at 96 px, enough
/// for a whole album list, so scrolling back never loads them again.
const THUMBS_KEPT: usize = 400;

fn shrink(image: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = image.dimensions();
    if w <= THUMB && h <= THUMB {
        return image;
    }
    let scale = THUMB as f32 / w.max(h) as f32;
    image::imageops::resize(
        &image,
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
        image::imageops::FilterType::Triangle,
    )
}

/// A list thumbnail: its own small copy on disk, else made from the cover
/// at display size (which is cached too).
fn decode_thumb_cached(dir: &Path, path: &Path) -> Result<image::RgbaImage, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let name = format!("{}-t{THUMB}", scaled_name(path, meta.len(), meta.modified().ok()));
    for extension in ["jpg", "png"] {
        let copy = dir.join(format!("{name}.{extension}"));
        if copy.is_file() {
            if let Ok(image) = decoder::decode(&copy) {
                return Ok(image);
            }
        }
    }
    let image = shrink(decode_cached(dir, path)?);
    if let Err(error) = store_scaled(dir, &name, &image) {
        eprintln!("Artwork cache {}: {error}", path.display());
    }
    Ok(image)
}

/// JPEG for opaque covers (as Go); PNG keeps a transparent cover's alpha.
/// Written beside the target and renamed, so readers see whole files only.
fn store_scaled(dir: &Path, name: &str, image: &image::RgbaImage) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let opaque = image.pixels().all(|pixel| pixel[3] == 255);
    let mut bytes = Vec::new();
    let extension = if opaque {
        let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .encode_image(&rgb)
            .map_err(|e| e.to_string())?;
        "jpg"
    } else {
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(|e| e.to_string())?;
        "png"
    };
    let target = dir.join(format!("{name}.{extension}"));
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    let written = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| std::fs::rename(&temporary, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.map_err(|e| e.to_string())
}

/// Deletes scaled copies no album uses any more. Only files older than ten
/// minutes go: not one just written for an album a scan is still adding.
fn prune_scaled(dir: &Path, keep: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let old = SystemTime::now() - Duration::from_secs(600);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stem = name.split('.').next().unwrap_or_default();
        // a list thumbnail goes with its cover
        let stem = stem.strip_suffix(&format!("-t{THUMB}")).unwrap_or(stem);
        let hex = stem.len() == 32 && stem.bytes().all(|b| b.is_ascii_hexdigit());
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|modified| modified < old);
        let leftover = name.starts_with('.') && name.ends_with(".tmp");
        if stale && ((hex && !keep.contains(stem)) || leftover) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

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
    fn new(scaled: Option<PathBuf>) -> Result<Self, String> {
        match scaled {
            Some(dir) => Self::with_decoder(move |path| decode_cached(&dir, path)),
            None => Self::with_decoder(decode),
        }
    }
    fn thumbs(scaled: Option<PathBuf>) -> Result<Self, String> {
        match scaled {
            Some(dir) => Self::with_decoder(move |path| decode_thumb_cached(&dir, path)),
            None => Self::with_decoder(|path| decode(path).map(shrink)),
        }
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
/// Small covers for list rows, apart from the full ones (Cover Flow, Now
/// Playing), so a long list neither evicts those nor reloads on scrolling.
struct Thumbs {
    decoders: Decoders,
    pending: HashSet<PathBuf>,
    failed: HashSet<PathBuf>,
    textures: HashMap<PathBuf, (Texture, Instant)>,
}
pub struct Artwork {
    decoders: Decoders,
    thumbs: Thumbs,
    generation: u64,
    pending: HashSet<PathBuf>,
    failed: HashSet<PathBuf>,
    textures: HashMap<PathBuf, (Texture, Instant)>,
    palettes: HashMap<PathBuf, [[f32; 4]; 6]>,
    pub placeholder: Texture,
    closed: bool,
    signatures: CoverSignatures,
    scaled: Option<PathBuf>,
}
impl Artwork {
    /// `scaled` is the folder for scaled copies (`SCALED_DIR` in the
    /// database folder); None decodes every cover from its original.
    pub fn new(
        renderer: &Renderer,
        _assets: &Path,
        scaled: Option<PathBuf>,
    ) -> Result<Self, String> {
        let image = raster_default_artwork(512)?;
        let decoders = Decoders::new(scaled.clone())?;
        let thumbs = Thumbs {
            decoders: Decoders::thumbs(scaled.clone())?,
            pending: HashSet::new(),
            failed: HashSet::new(),
            textures: HashMap::new(),
        };
        let placeholder =
            renderer.upload_artwork(image.width(), image.height(), image.as_raw(), &[])?;
        Ok(Self {
            decoders,
            thumbs,
            generation: 0,
            pending: HashSet::new(),
            failed: HashSet::new(),
            textures: HashMap::new(),
            palettes: HashMap::new(),
            placeholder,
            closed: false,
            signatures: HashMap::new(),
            scaled,
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
    /// A list row's cover: the small copy, or the full one when it is
    /// already loaded; asks for the small one otherwise.
    pub fn thumb(&mut self, path: Option<&Path>) -> Texture {
        let Some(path) = path else {
            return self.placeholder;
        };
        if let Some((texture, used)) = self.thumbs.textures.get_mut(path) {
            *used = Instant::now();
            return *texture;
        }
        self.request_thumb(path);
        if let Some((texture, _)) = self.textures.get(path) {
            return *texture;
        }
        self.placeholder
    }
    fn request_thumb(&mut self, path: &Path) -> bool {
        if self.thumbs.textures.contains_key(path)
            || self.thumbs.pending.contains(path)
            || self.thumbs.failed.contains(path)
        {
            return true;
        }
        let Some(tx) = &self.thumbs.decoders.tx else {
            return false;
        };
        if tx.try_send((self.generation, path.to_path_buf())).is_ok() {
            self.thumbs.pending.insert(path.to_path_buf());
            true
        } else {
            false // queue full: asked again on a later frame
        }
    }
    /// Loads the covers of the rows around the visible ones ahead of
    /// scrolling, nearest first, while the decode queue has room.
    pub fn prefetch_thumbs<'a>(&mut self, paths: impl Iterator<Item = &'a Path>) {
        for path in paths {
            if !self.request_thumb(path) {
                break;
            }
        }
    }
    pub fn poll(&mut self, renderer: &Renderer) -> bool {
        let mut changed = false;
        // Small uploads: a few per frame keep a long list filling quickly.
        for _ in 0..6 {
            let Ok(result) = self.thumbs.decoders.rx.try_recv() else {
                break;
            };
            if result.generation != self.generation {
                continue;
            }
            self.thumbs.pending.remove(&result.path);
            match result.result.and_then(|p| {
                renderer.upload_artwork(p.width(), p.height(), p.as_raw(), &result.mipmaps)
            }) {
                Ok(texture) => {
                    if let Some((old, _)) = self
                        .thumbs
                        .textures
                        .insert(result.path, (texture, Instant::now()))
                    {
                        renderer.delete_texture(old);
                    }
                    changed = true;
                }
                Err(e) => {
                    eprintln!("Artwork {}: {e}", result.path.display());
                    self.thumbs.failed.insert(result.path);
                }
            }
        }
        while self.thumbs.textures.len() > THUMBS_KEPT {
            if let Some(old) = self
                .thumbs
                .textures
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(p, _)| p.clone())
            {
                if let Some((texture, _)) = self.thumbs.textures.remove(&old) {
                    renderer.delete_texture(texture);
                }
            }
        }
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
        self.thumbs.pending.clear();
        self.thumbs.failed.clear();
        for (_, (texture, _)) in self.thumbs.textures.drain() {
            renderer.delete_texture(texture);
        }
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
            self.thumbs.pending.clear();
            for path in changed {
                self.thumbs.failed.remove(&path);
                if let Some((texture, _)) = self.thumbs.textures.remove(&path) {
                    renderer.delete_texture(texture);
                }
                self.failed.remove(&path);
                self.palettes.remove(&path);
                if let Some((texture, _)) = self.textures.remove(&path) {
                    renderer.delete_texture(texture);
                }
            }
        }
        if let Some(dir) = self.scaled.clone() {
            let keep: HashSet<String> = signatures
                .iter()
                .map(|(path, (len, modified))| scaled_name(path, *len, *modified))
                .collect();
            // File I/O stays off the UI thread.
            let _ = thread::Builder::new()
                .name("artwork-prune".into())
                .spawn(move || {
                    background_priority();
                    prune_scaled(&dir, &keep);
                });
        }
        self.signatures = signatures;
    }
    pub fn close(&mut self, renderer: &Renderer) {
        self.decoders.shutdown();
        self.thumbs.decoders.shutdown();
        if self.closed {
            return;
        }
        self.closed = true;
        self.thumbs.pending.clear();
        self.thumbs.failed.clear();
        for (_, (texture, _)) in self.thumbs.textures.drain() {
            renderer.delete_texture(texture);
        }
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
    fn scaled_covers_are_kept_until_the_original_changes() {
        let root = std::env::temp_dir().join(format!("bp-scaled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cache = root.join(SCALED_DIR);
        std::fs::create_dir_all(&root).unwrap();
        let cover = root.join("cover.png");
        let opaque = image::RgbaImage::from_fn(1000, 800, |x, y| {
            image::Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255])
        });
        opaque.save(&cover).unwrap();
        let first = decode_cached(&cache, &cover).unwrap();
        assert_eq!(first.dimensions(), (512, 410));
        let names = |dir: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        let saved = names(&cache);
        assert_eq!(saved.len(), 1);
        assert!(saved[0].ends_with(".jpg"), "{saved:?}");
        // The copy is read back instead of the original: same size, close pixels.
        let again = decode_cached(&cache, &cover).unwrap();
        assert_eq!(again.dimensions(), first.dimensions());
        let error = first
            .as_raw()
            .iter()
            .zip(again.as_raw())
            .map(|(a, b)| f64::from(a.abs_diff(*b)))
            .sum::<f64>()
            / first.as_raw().len() as f64;
        eprintln!("scaled copy mean channel error: {error:.2}");
        assert!(
            error < 4.0,
            "a q90 copy stays close to the decoded cover: {error}"
        );
        assert_eq!(names(&cache), saved, "a hit writes nothing new");

        // A transparent cover keeps its alpha in a PNG copy.
        let clear = root.join("clear.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 128]))
            .save(&clear)
            .unwrap();
        let alpha = decode_cached(&cache, &clear).unwrap();
        assert_eq!(alpha.get_pixel(0, 0)[3], 128);
        assert_eq!(
            decode_cached(&cache, &clear).unwrap().get_pixel(0, 0)[3],
            128
        );
        assert!(names(&cache).iter().any(|n| n.ends_with(".png")));

        // An edited cover (new size or time) gets its own entry.
        let edited = image::RgbaImage::from_pixel(700, 700, image::Rgba([200, 0, 0, 255]));
        edited.save(&cover).unwrap();
        let file = std::fs::File::options().write(true).open(&cover).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        drop(file);
        let red = decode_cached(&cache, &cover).unwrap();
        assert_eq!(red.dimensions(), (512, 512));
        assert!(red.get_pixel(10, 10)[0] > 180);
        assert_eq!(names(&cache).len(), 3);

        // Pruning keeps what albums use and anything written in the last ten
        // minutes; an old unused copy goes.
        let meta = std::fs::metadata(&cover).unwrap();
        let keep: HashSet<String> = [scaled_name(&cover, meta.len(), meta.modified().ok())].into();
        prune_scaled(&cache, &keep);
        assert_eq!(names(&cache).len(), 3, "recent copies stay");
        let old = SystemTime::now() - Duration::from_secs(3600);
        for name in names(&cache) {
            let file = std::fs::File::options()
                .write(true)
                .open(cache.join(&name))
                .unwrap();
            file.set_modified(old).unwrap();
        }
        prune_scaled(&cache, &keep);
        let left = names(&cache);
        assert_eq!(left.len(), 1);
        assert!(left[0].starts_with(keep.iter().next().unwrap().as_str()));
        std::fs::remove_dir_all(&root).unwrap();
    }
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
