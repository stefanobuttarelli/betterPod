//! Bounded artwork decoding off the UI thread. GPU objects stay on the UI.
use crate::render::{Renderer, Texture};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

struct Pixels {
    generation: u64,
    path: PathBuf,
    result: Result<image::RgbaImage, String>,
    palette: Option<[[f32; 4]; 6]>,
}
pub struct Artwork {
    tx: Option<mpsc::SyncSender<(u64, PathBuf)>>,
    generation: u64,
    rx: mpsc::Receiver<Pixels>,
    pending: HashSet<PathBuf>,
    failed: HashSet<PathBuf>,
    textures: HashMap<PathBuf, (Texture, Instant)>,
    palettes: HashMap<PathBuf, [[f32; 4]; 6]>,
    pub placeholder: Texture,
}
impl Artwork {
    pub fn new(renderer: &Renderer, assets: &Path) -> Result<Self, String> {
        let fallback = assets.join("ipod/default_album_artwork.png");
        let image = decode(&fallback).unwrap_or_else(|_| {
            image::RgbaImage::from_pixel(64, 64, image::Rgba([128, 137, 151, 255]))
        });
        let placeholder = renderer.upload(image.width(), image.height(), image.as_raw())?;
        let (tx, jobs) = mpsc::sync_channel::<(u64, PathBuf)>(32);
        let (results, rx) = mpsc::sync_channel(8);
        let jobs = Arc::new(Mutex::new(jobs));
        for n in 0..2 {
            let jobs = jobs.clone();
            let results = results.clone();
            thread::Builder::new()
                .name(format!("artwork-{n}"))
                .spawn(move || loop {
                    let (generation, path) = match jobs.lock() {
                        Ok(r) => match r.recv() {
                            Ok(p) => p,
                            Err(_) => break,
                        },
                        Err(_) => break,
                    };
                    let result = decode(&path);
                    let palette = result.as_ref().ok().map(colors);
                    if results
                        .send(Pixels {
                            generation,
                            path,
                            result,
                            palette,
                        })
                        .is_err()
                    {
                        break;
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            tx: Some(tx),
            generation: 0,
            rx,
            pending: HashSet::new(),
            failed: HashSet::new(),
            textures: HashMap::new(),
            palettes: HashMap::new(),
            placeholder,
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
            if let Some(tx) = &self.tx {
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
            let Ok(result) = self.rx.try_recv() else {
                break;
            };
            if result.generation != self.generation {
                continue;
            }
            self.pending.remove(&result.path);
            match result
                .result
                .and_then(|p| renderer.upload(p.width(), p.height(), p.as_raw()))
            {
                Ok(texture) => {
                    if let Some(palette) = result.palette {
                        self.palettes.insert(result.path.clone(), palette);
                    }
                    self.textures.insert(result.path, (texture, Instant::now()));
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
        for (_, (texture, _)) in self.textures.drain() {
            renderer.delete_texture(texture);
        }
    }
    pub fn close(&mut self, renderer: &Renderer) {
        self.tx.take();
        for (_, (texture, _)) in self.textures.drain() {
            renderer.delete_texture(texture);
        }
        renderer.delete_texture(self.placeholder);
    }
    pub fn palette(&self, path: Option<&Path>) -> Option<[[f32; 4]; 6]> {
        path.and_then(|p| self.palettes.get(p).copied())
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
            if p[3] > 100 {
                colors.push([
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                    1.0,
                ]);
            }
        }
    }
    if colors.is_empty() {
        return [[0.4, 0.5, 0.8, 1.0]; 6];
    }
    colors.sort_by(|a, b| {
        let light = |p: &[f32; 4]| p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722;
        light(a).total_cmp(&light(b))
    });
    std::array::from_fn(|i| colors[i * (colors.len() - 1) / 5])
}
fn decode(path: &Path) -> Result<image::RgbaImage, String> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    Ok(image
        .resize(512, 512, image::imageops::FilterType::Triangle)
        .to_rgba8())
}
