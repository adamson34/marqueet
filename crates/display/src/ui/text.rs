//! Vector text with the bundled Barlow Condensed (SIL OFL 1.1), rasterized
//! on the CPU by swash and cached per glyph and size.

use std::collections::HashMap;

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};
use swash::{FontRef, GlyphId};

static MEDIUM: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-Medium.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-SemiBold.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Medium,
    SemiBold,
}

/// A rendered glyph: an 8-bit coverage mask positioned relative to the pen
/// (left/top from the baseline origin, y up).
#[derive(Clone, Debug)]
pub struct GlyphMask {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

/// A glyph placed on a line: pen x (px from the line start) and its mask.
#[derive(Debug)]
pub struct Placed<'a> {
    pub x: f32,
    pub mask: &'a GlyphMask,
}

pub struct Fonts {
    medium: FontRef<'static>,
    semibold: FontRef<'static>,
    context: ScaleContext,
    /// (weight, glyph, size in quarter pixels) -> mask.
    cache: HashMap<(Weight, u16, u32), GlyphMask>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts").field("cached_glyphs", &self.cache.len()).finish()
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Fonts {
        #[allow(clippy::expect_used)] // Bundled at compile time; covered by tests.
        let load = |data: &'static [u8]| FontRef::from_index(data, 0).expect("bundled font parses");
        Fonts { medium: load(MEDIUM), semibold: load(SEMIBOLD), context: ScaleContext::new(), cache: HashMap::new() }
    }

    fn font(&self, weight: Weight) -> FontRef<'static> {
        match weight {
            Weight::Medium => self.medium,
            Weight::SemiBold => self.semibold,
        }
    }

    fn glyph_id(&self, weight: Weight, ch: char) -> GlyphId {
        let font = self.font(weight);
        let id = font.charmap().map(ch);
        if id == 0 { font.charmap().map('?') } else { id }
    }

    /// Height of capital letters, in pixels (for vertical centering).
    pub fn cap_height(&self, weight: Weight, size: f32) -> f32 {
        self.font(weight).metrics(&[]).scale(size).cap_height
    }

    /// Advance width of `text` at `size` px with extra `tracking` px between
    /// characters.
    pub fn measure(&self, weight: Weight, size: f32, tracking: f32, text: &str) -> f32 {
        let metrics = self.font(weight).glyph_metrics(&[]).scale(size);
        let n = text.chars().count();
        let advance: f32 = text.chars().map(|c| metrics.advance_width(self.glyph_id(weight, c))).sum();
        advance + tracking * n.saturating_sub(1) as f32
    }

    /// Lays out `text` and returns each glyph's pen position and mask.
    pub fn layout(&mut self, weight: Weight, size: f32, tracking: f32, text: &str) -> Vec<Placed<'_>> {
        let font = self.font(weight);
        let metrics = font.glyph_metrics(&[]).scale(size);
        let key_size = (size * 4.0).round() as u32;
        let mut positions = Vec::new();
        let mut x = 0.0;
        for ch in text.chars() {
            let id = self.glyph_id(weight, ch);
            positions.push((x, id));
            x += metrics.advance_width(id) + tracking;
        }
        for &(_, id) in &positions {
            let key = (weight, id, key_size);
            if !self.cache.contains_key(&key) {
                let mut scaler = self.context.builder(font).size(size).hint(true).build();
                let image = Render::new(&[Source::Outline])
                    .format(Format::Alpha)
                    .offset(Vector::new(0.0, 0.0))
                    .render(&mut scaler, id);
                let mask = match image {
                    Some(img) => GlyphMask {
                        left: img.placement.left,
                        top: img.placement.top,
                        width: img.placement.width,
                        height: img.placement.height,
                        alpha: img.data,
                    },
                    None => GlyphMask { left: 0, top: 0, width: 0, height: 0, alpha: Vec::new() },
                };
                self.cache.insert(key, mask);
            }
        }
        positions
            .into_iter()
            .filter_map(|(x, id)| self.cache.get(&(weight, id, key_size)).map(|mask| Placed { x, mask }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_and_measure() {
        let f = Fonts::new();
        let w = f.measure(Weight::SemiBold, 32.0, 0.0, "GAME OF THE DAY");
        assert!(w > 150.0 && w < 350.0, "{w}");
        assert!(f.measure(Weight::SemiBold, 64.0, 0.0, "BUF") > f.measure(Weight::SemiBold, 32.0, 0.0, "BUF"));
        assert!(f.measure(Weight::Medium, 32.0, 4.0, "AB") > f.measure(Weight::Medium, 32.0, 0.0, "AB"));
        assert!(f.cap_height(Weight::SemiBold, 100.0) > 50.0);
    }

    #[test]
    fn glyphs_render_and_cache() {
        let mut f = Fonts::new();
        let placed = f.layout(Weight::SemiBold, 40.0, 0.0, "KC 27");
        assert_eq!(placed.len(), 5);
        assert!(placed[0].mask.width > 0 && placed[0].mask.alpha.iter().any(|&a| a > 200));
        assert_eq!(placed[2].mask.width, 0, "space has no ink");
        assert!(placed[1].x > placed[0].x);
        let cached = f.cache.len();
        f.layout(Weight::SemiBold, 40.0, 0.0, "KC");
        assert_eq!(f.cache.len(), cached, "reused");
    }

    #[test]
    fn missing_glyphs_fall_back() {
        let mut f = Fonts::new();
        assert_eq!(f.layout(Weight::Medium, 20.0, 0.0, "\u{E000}").len(), 1);
    }
}
