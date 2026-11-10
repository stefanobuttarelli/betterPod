//! Original ij_vizclock.go frosted digits. Work is restricted to the clock's
//! bounding box; its blur works on a quarter-size RGB grid.
use crate::render::{Renderer, Texture};
use std::path::Path;
pub(super) struct FrostClock {
    font: fontdue::Font,
    text: String,
    width: usize,
    height: usize,
    mask: Vec<u8>,
    rim: Vec<u8>,
    small: Vec<u32>,
    tmp: Vec<u32>,
    pixels: Vec<u8>,
    texture: Option<Texture>,
}
impl FrostClock {
    pub(super) fn load(assets: &Path) -> Result<Self, String> {
        let path = assets.join("fonts/InterDisplay-Light-Digits.otf");
        let font = fontdue::Font::from_bytes(
            std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            fontdue::FontSettings::default(),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            font,
            text: String::new(),
            width: 0,
            height: 0,
            mask: Vec::new(),
            rim: Vec::new(),
            small: Vec::new(),
            tmp: Vec::new(),
            pixels: Vec::new(),
            texture: None,
        })
    }
    fn make(&mut self, text: &str) {
        let mut width = 0_f32;
        let mut previous = None;
        for c in text.chars() {
            if let Some(p) = previous {
                width += self.font.horizontal_kern(p, c, 150.).unwrap_or(0.);
            }
            width += self.font.metrics(c, 150.).advance_width;
            previous = Some(c);
        }
        self.width = (width.ceil() as usize + 24).next_multiple_of(4);
        self.height = (113_usize + 24).next_multiple_of(4);
        let n = self.width * self.height;
        self.mask = vec![0; n];
        let mut x = 12_f32;
        let mut previous = None;
        for c in text.chars() {
            if let Some(p) = previous {
                x += self.font.horizontal_kern(p, c, 150.).unwrap_or(0.);
            }
            let (m, bitmap) = self.font.rasterize(c, 150.);
            let x0 = x.round() as i32 + m.xmin;
            let y0 = 125 - m.height as i32 - m.ymin;
            for y in 0..m.height {
                let yy = y0 + y as i32;
                if yy < 0 || yy >= self.height as i32 {
                    continue;
                }
                for xx in 0..m.width {
                    let px = x0 + xx as i32;
                    if px >= 0 && px < self.width as i32 {
                        let i = yy as usize * self.width + px as usize;
                        self.mask[i] = self.mask[i].max(bitmap[y * m.width + xx]);
                    }
                }
            }
            x += m.advance_width;
            previous = Some(c);
        }
        let erosion = minimum(
            &minimum(&self.mask, self.width, self.height, 1, 0),
            self.width,
            self.height,
            0,
            1,
        );
        let edge: Vec<_> = self
            .mask
            .iter()
            .zip(&erosion)
            .map(|(a, b)| a.saturating_sub(*b))
            .collect();
        self.rim = vec![0; n];
        for y in 1..self.height - 1 {
            for x in 1..self.width - 1 {
                let i = y * self.width + x;
                let s = 4 * edge[i] as u32
                    + 2 * (edge[i - 1] as u32
                        + edge[i + 1] as u32
                        + edge[i - self.width] as u32
                        + edge[i + self.width] as u32)
                    + edge[i - self.width - 1] as u32
                    + edge[i - self.width + 1] as u32
                    + edge[i + self.width - 1] as u32
                    + edge[i + self.width + 1] as u32;
                self.rim[i] = (s * 192 / (16 * 256)).min(255) as u8;
            }
        }
        self.small = vec![0; n / 16 * 3];
        self.tmp = vec![0; self.small.len()];
        self.pixels = vec![0; n * 4];
        self.text = text.into();
    }
    pub(super) fn close(&mut self, r: &Renderer) {
        if let Some(t) = self.texture.take() {
            r.delete_texture(t)
        }
    }
    pub(super) fn draw(
        &mut self,
        r: &mut Renderer,
        text: &str,
        cy: i32,
        alpha: f64,
        source: impl Fn(usize, usize) -> [u8; 3],
    ) -> Result<(), String> {
        if self.text != text {
            self.close(r);
            self.make(text)
        }
        let mut x0 = 320 - (self.width as i32 - 24) / 2 - 12;
        let mut y0 = cy - self.height as i32 / 2;
        x0 -= x0 & 3;
        y0 -= y0 & 3;
        if x0 < 0 || y0 < 0 || x0 + self.width as i32 > 640 || y0 + self.height as i32 > 480 {
            return Ok(());
        }
        let sw = self.width / 4;
        let sh = self.height / 4;
        for sy in 0..sh {
            for sx in 0..sw {
                let mut sum = [0_u32; 3];
                for y in 0..4 {
                    for x in 0..4 {
                        let c = source(x0 as usize + sx * 4 + x, y0 as usize + sy * 4 + y);
                        for ch in 0..3 {
                            sum[ch] += c[ch] as u32;
                        }
                    }
                }
                let i = (sy * sw + sx) * 3;
                for ch in 0..3 {
                    self.small[i + ch] = sum[ch] / 16;
                }
            }
        }
        for _ in 0..2 {
            blur(&self.small, &mut self.tmp, sw, sh, 1, 0);
            blur(&self.tmp, &mut self.small, sw, sh, 0, 1);
        }
        let ka = (alpha * 256. + 0.5) as u32;
        for y in 0..self.height {
            let fy = (y as i32 * 256 + 128) / 4 - 128;
            let sy0 = (fy >> 8).clamp(0, sh as i32 - 1) as usize;
            let sy1 = (sy0 + 1).min(sh - 1);
            let wy = if fy < 0 { 0 } else { (fy & 255) as u32 };
            for x in 0..self.width {
                let i = y * self.width + x;
                let original = source(x0 as usize + x, y0 as usize + y);
                let o = i * 4;
                self.pixels[o..o + 3].copy_from_slice(&original);
                self.pixels[o + 3] = 255;
                let mask = self.mask[i];
                if mask == 0 {
                    continue;
                }
                let fx = (x as i32 * 256 + 128) / 4 - 128;
                let sx0 = (fx >> 8).clamp(0, sw as i32 - 1) as usize;
                let sx1 = (sx0 + 1).min(sw - 1);
                let wx = if fx < 0 { 0 } else { (fx & 255) as u32 };
                let i00 = (sy0 * sw + sx0) * 3;
                let i01 = (sy0 * sw + sx1) * 3;
                let i10 = (sy1 * sw + sx0) * 3;
                let i11 = (sy1 * sw + sx1) * 3;
                let a = (mask as u32 * ka) >> 8;
                let rim = (self.rim[i] as u32 * ka) >> 8;
                for ch in 0..3 {
                    let top = (self.small[i00 + ch] * (256 - wx) + self.small[i01 + ch] * wx) >> 8;
                    let bottom =
                        (self.small[i10 + ch] * (256 - wx) + self.small[i11 + ch] * wx) >> 8;
                    let value = ((((top * (256 - wy) + bottom * wy) >> 8) * 179) >> 8) + 77;
                    let mut result = (value * a + original[ch] as u32 * (255 - a)) / 255;
                    result += (255 - result) * rim / 255;
                    self.pixels[o + ch] = result as u8;
                }
            }
        }
        if let Some(t) = self.texture {
            r.update_texture(t, &self.pixels)?;
        } else {
            self.texture = Some(r.upload(self.width as u32, self.height as u32, &self.pixels)?);
        }
        r.image_uv(
            self.texture.unwrap(),
            [x0 as f32, y0 as f32, self.width as f32, self.height as f32],
            [0., 0., 1., 1.],
            1.,
        );
        Ok(())
    }
}
fn minimum(src: &[u8], w: usize, h: usize, dx: i32, dy: i32) -> Vec<u8> {
    let mut out = vec![0; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut m = 255;
            for k in -2..=2 {
                let xx = x as i32 + k * dx;
                let yy = y as i32 + k * dy;
                if xx < 0 || yy < 0 || xx >= w as i32 || yy >= h as i32 {
                    m = 0;
                    break;
                }
                m = m.min(src[yy as usize * w + xx as usize]);
            }
            out[y * w + x] = m;
        }
    }
    out
}
fn blur(src: &[u32], dst: &mut [u32], w: usize, h: usize, dx: i32, dy: i32) {
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0; 3];
            for k in -2..=2 {
                let xx = (x as i32 + k * dx).clamp(0, w as i32 - 1) as usize;
                let yy = (y as i32 + k * dy).clamp(0, h as i32 - 1) as usize;
                let i = (yy * w + xx) * 3;
                for ch in 0..3 {
                    sum[ch] += src[i + ch];
                }
            }
            let i = (y * w + x) * 3;
            for ch in 0..3 {
                dst[i + ch] = sum[ch] / 5;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn erosion_has_original_two_pixel_radius() {
        let mut source = vec![0; 9 * 9];
        for y in 1..8 {
            for x in 1..8 {
                source[y * 9 + x] = 255;
            }
        }
        let e = minimum(&minimum(&source, 9, 9, 1, 0), 9, 9, 0, 1);
        assert_eq!(e[4 * 9 + 4], 255);
        assert_eq!(e[2 * 9 + 4], 0);
    }
    #[test]
    fn box_keeps_flat_fields() {
        let source = vec![127; 8 * 8 * 3];
        let mut dest = vec![0; source.len()];
        blur(&source, &mut dest, 8, 8, 1, 0);
        assert_eq!(source, dest);
    }
}
