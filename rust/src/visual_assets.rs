//! Original immutable UI sprites. Loading happens before the event loop.
use crate::render::{Color, Renderer, Texture};
use std::{collections::HashMap, path::Path};
pub struct VisualAssets {
    textures: HashMap<String, Texture>,
    ink: HashMap<String, (f32, f32)>,
    tint: HashMap<String, Color>,
}
impl VisualAssets {
    pub fn new(renderer: &Renderer, assets: &Path) -> Result<Self, String> {
        let mut textures = HashMap::new();
        let mut ink = HashMap::new();
        let mut tint = HashMap::new();
        for entry in std::fs::read_dir(assets.join("ipod")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("png") {
                continue;
            }
            let rgba = image::open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .to_rgba8();
            let texture = renderer.upload(rgba.width(), rgba.height(), rgba.as_raw())?;
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let left = (0..rgba.width())
                .find(|&x| (0..rgba.height()).any(|y| rgba.get_pixel(x, y)[3] > 40))
                .unwrap_or(0);
            let right = (0..rgba.width())
                .rev()
                .find(|&x| (0..rgba.height()).any(|y| rgba.get_pixel(x, y)[3] > 40))
                .map(|x| rgba.width() - 1 - x)
                .unwrap_or(0);
            ink.insert(name.clone(), (left as f32, right as f32));
            if let Some(pixel) = rgba.pixels().max_by_key(|p| p[3]) {
                tint.insert(
                    name.clone(),
                    [
                        pixel[0] as f32 / 255.0,
                        pixel[1] as f32 / 255.0,
                        pixel[2] as f32 / 255.0,
                        1.0,
                    ],
                );
            }
            textures.insert(name.clone(), texture);
            if matches!(name.as_str(), "share_ped_blue" | "share_ped_green") {
                // Prepare once at startup, as for the themed header sprites.
                // Neutral metal becomes graphite; the colored light retains
                // its hue and brightness. Keep the original silhouette/alpha.
                let mut themed = rgba.clone();
                for pixel in themed.pixels_mut() {
                    let high = *pixel.0[..3].iter().max().unwrap() as f32;
                    let low = *pixel.0[..3].iter().min().unwrap() as f32;
                    let color = ((high - low) / 80.).clamp(0., 1.);
                    let factor = 0.26 + 0.74 * color;
                    for channel in &mut pixel.0[..3] {
                        *channel = (*channel as f32 * factor).round() as u8;
                    }
                }
                textures.insert(
                    format!("{name}__dark"),
                    renderer.upload(themed.width(), themed.height(), themed.as_raw())?,
                );
            }
            let lighten = matches!(
                name.as_str(),
                "bluetooth"
                    | "hdr_shuffle"
                    | "hdr_repeat"
                    | "hdr_repeat_one"
                    | "volume_full_bar"
                    | "volume_mute_bar"
                    | "volume_full_16"
                    | "battery_shell"
            );
            if lighten || name == "gloss_track" {
                let mut themed = rgba.clone();
                for pixel in themed.pixels_mut() {
                    for c in 0..3 {
                        pixel[c] = if lighten {
                            255 - ((255 - pixel[c] as u32) * 2 / 5) as u8
                        } else {
                            (pixel[c] as u32 * 3 / 5) as u8
                        };
                    }
                }
                if let Some(pixel) = themed.pixels().max_by_key(|p| p[3]) {
                    tint.insert(
                        format!("{name}__dark"),
                        [
                            pixel[0] as f32 / 255.0,
                            pixel[1] as f32 / 255.0,
                            pixel[2] as f32 / 255.0,
                            1.0,
                        ],
                    );
                }
                textures.insert(
                    format!("{name}__dark"),
                    renderer.upload(themed.width(), themed.height(), themed.as_raw())?,
                );
            }
            if matches!(
                name.as_str(),
                "hdr_shuffle"
                    | "hdr_repeat"
                    | "hdr_repeat_one"
                    | "bluetooth"
                    | "volume_full_bar"
                    | "volume_mute_bar"
                    | "volume_full_16"
                    | "play"
                    | "pause"
            ) {
                let mut white = rgba.clone();
                for pixel in white.pixels_mut() {
                    pixel[0] = 255;
                    pixel[1] = 255;
                    pixel[2] = 255;
                }
                textures.insert(
                    format!("{name}__white"),
                    renderer.upload(white.width(), white.height(), white.as_raw())?,
                );
            }
        }
        for icon in crate::preview_icons::generate(assets)?
            .into_iter()
            .chain(crate::preview_icons::generate_bluetooth()?)
        {
            textures.insert(
                icon.name.into(),
                renderer.upload(icon.width, icon.height, &icon.pixels)?,
            );
        }
        Ok(Self {
            textures,
            ink,
            tint,
        })
    }
    pub fn get(&self, name: &str) -> Option<Texture> {
        self.textures
            .get(name.strip_suffix(".png").unwrap_or(name))
            .copied()
    }
    pub fn tint(&self, name: &str) -> Option<Color> {
        self.tint
            .get(name.strip_suffix(".png").unwrap_or(name))
            .copied()
    }
    pub fn ink(&self, name: &str) -> Option<(f32, f32)> {
        let name = name.strip_suffix(".png").unwrap_or(name);
        let name = name.split("__").next().unwrap_or(name);
        self.ink.get(name).copied()
    }
    pub fn close(&mut self, renderer: &Renderer) {
        for (_, t) in self.textures.drain() {
            renderer.delete_texture(t);
        }
    }
}
