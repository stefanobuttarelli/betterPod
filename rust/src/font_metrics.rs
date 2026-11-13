//! OpenType pair positioning. Rasterization stays in fontdue; advances and
//! kerning use the font's original GPOS data at 1/64-pixel precision.
use std::collections::HashMap;
use ttf_parser::{
    gpos::{PairAdjustment, PositioningSubtable},
    Face, GlyphId, Tag,
};
pub struct MetricsFont {
    data: Vec<u8>,
    units: f32,
    pairs: HashMap<(u16, u16), i16>,
}
impl MetricsFont {
    pub fn new(data: Vec<u8>) -> Result<Self, String> {
        let face = Face::parse(&data, 0).map_err(|e| e.to_string())?;
        let units = face.units_per_em() as f32;
        Ok(Self {
            data,
            units,
            pairs: HashMap::new(),
        })
    }
    pub fn kern(&mut self, left: u16, right: u16, size: f32) -> f32 {
        let units = if let Some(value) = self.pairs.get(&(left, right)) {
            *value
        } else {
            let value = Face::parse(&self.data, 0)
                .ok()
                .and_then(|face| gpos_pair(&face, GlyphId(left), GlyphId(right)))
                .unwrap_or(0);
            if self.pairs.len() < 65536 {
                self.pairs.insert((left, right), value);
            }
            value
        };
        fixed(units as f32 * size / self.units)
    }
}
pub fn fixed(value: f32) -> f32 {
    (value * 64.0).round() / 64.0
}
fn gpos_pair(face: &Face<'_>, left: GlyphId, right: GlyphId) -> Option<i16> {
    let table = face.tables().gpos?;
    for feature in table.features {
        if feature.tag != Tag::from_bytes(b"kern") {
            continue;
        }
        for index in feature.lookup_indices {
            let Some(lookup) = table.lookups.get(index) else {
                continue;
            };
            for subtable in lookup.subtables.into_iter::<PositioningSubtable>() {
                let PositioningSubtable::Pair(pair) = subtable else {
                    continue;
                };
                match pair {
                    PairAdjustment::Format1 { coverage, sets } => {
                        if let Some(index) = coverage.get(left) {
                            if let Some((a, _)) = sets.get(index).and_then(|set| set.get(right)) {
                                return Some(a.x_advance);
                            }
                        }
                    }
                    PairAdjustment::Format2 {
                        coverage,
                        classes,
                        matrix,
                    } => {
                        if coverage.contains(left) {
                            if let Some((a, _)) =
                                matrix.get((classes.0.get(left), classes.1.get(right)))
                            {
                                return Some(a.x_advance);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inter_gpos_scales_with_font_size() {
        let native = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../port/betterpod/assets/fonts/Inter-Regular.otf");
        let path = if native.is_file() {
            native
        } else {
            std::path::PathBuf::from("/repo/port/betterpod/assets/fonts/Inter-Regular.otf")
        };
        let data = std::fs::read(path).expect("The original Inter fixture must be available");
        let face = Face::parse(&data, 0).unwrap();
        let a = face.glyph_index('A').unwrap().0;
        let v = face.glyph_index('V').unwrap().0;
        let mut font = MetricsFont::new(data).unwrap();
        let small = font.kern(a, v, 16.0);
        let large = font.kern(a, v, 32.0);
        assert!(small < 0.0);
        assert!((large - small * 2.0).abs() <= 1.0 / 64.0);
    }
}
