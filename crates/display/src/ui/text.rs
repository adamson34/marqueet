//! Vector text with the bundled fonts (all SIL OFL 1.1; licenses next to
//! them in assets/fonts/), rasterized on the CPU by swash and cached per
//! glyph and size.

use std::collections::HashMap;

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};
use swash::{FontRef, GlyphId};

static MEDIUM: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-Medium.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-SemiBold.ttf");
static HEAVY: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-ExtraBoldItalic.ttf");
static STENCIL: &[u8] = include_bytes!("../../assets/fonts/BigShouldersStencilDisplay-ExtraBold.ttf");
static PLATE: &[u8] = include_bytes!("../../assets/fonts/BigShouldersDisplay-ExtraBold.ttf");
static COLLEGIATE: &[u8] = include_bytes!("../../assets/fonts/Graduate-Regular.ttf");

/// A bundled font face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Barlow Condensed Medium: running text.
    Medium,
    /// Barlow Condensed SemiBold: labels and numbers.
    SemiBold,
    /// Barlow Condensed ExtraBold Italic: broadcast headlines and scores.
    Heavy,
    /// Big Shoulders Stencil Display ExtraBold: painted scoreboard lettering.
    Stencil,
    /// Big Shoulders Display ExtraBold: scoreboard number plates.
    Plate,
    /// Graduate: collegiate block lettering.
    Collegiate,
}

impl Face {
    const ALL: [Face; 6] = [Face::Medium, Face::SemiBold, Face::Heavy, Face::Stencil, Face::Plate, Face::Collegiate];

    fn data(self) -> &'static [u8] {
        match self {
            Face::Medium => MEDIUM,
            Face::SemiBold => SEMIBOLD,
            Face::Heavy => HEAVY,
            Face::Stencil => STENCIL,
            Face::Plate => PLATE,
            Face::Collegiate => COLLEGIATE,
        }
    }
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
    faces: [FontRef<'static>; 6],
    context: ScaleContext,
    /// (face, glyph, size in quarter pixels) -> mask.
    cache: HashMap<(Face, u16, u32), GlyphMask>,
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
        Fonts { faces: Face::ALL.map(|f| load(f.data())), context: ScaleContext::new(), cache: HashMap::new() }
    }

    fn font(&self, face: Face) -> FontRef<'static> {
        self.faces[face as usize]
    }

    fn glyph_id(&self, face: Face, ch: char) -> GlyphId {
        let font = self.font(face);
        let id = font.charmap().map(ch);
        if id == 0 { font.charmap().map('?') } else { id }
    }

    /// Height of capital letters, in pixels (for vertical centering).
    pub fn cap_height(&self, face: Face, size: f32) -> f32 {
        self.font(face).metrics(&[]).scale(size).cap_height
    }

    /// Advance width of `text` at `size` px with extra `tracking` px between
    /// characters.
    pub fn measure(&self, face: Face, size: f32, tracking: f32, text: &str) -> f32 {
        let metrics = self.font(face).glyph_metrics(&[]).scale(size);
        let n = text.chars().count();
        let advance: f32 = text.chars().map(|c| metrics.advance_width(self.glyph_id(face, c))).sum();
        advance + tracking * n.saturating_sub(1) as f32
    }

    /// The size (at most `size`) at which `text` fits in `max_width` px.
    pub fn fit(&self, face: Face, size: f32, tracking: f32, text: &str, max_width: f32) -> f32 {
        let w = self.measure(face, size, tracking, text);
        if w <= max_width || w <= 0.0 { size } else { size * max_width / w }
    }

    /// Lays out `text` and returns each glyph's pen position and mask.
    pub fn layout(&mut self, face: Face, size: f32, tracking: f32, text: &str) -> Vec<Placed<'_>> {
        let font = self.font(face);
        let metrics = font.glyph_metrics(&[]).scale(size);
        let key_size = (size * 4.0).round() as u32;
        let mut positions = Vec::new();
        let mut x = 0.0;
        for ch in text.chars() {
            let id = self.glyph_id(face, ch);
            positions.push((x, id));
            x += metrics.advance_width(id) + tracking;
        }
        for &(_, id) in &positions {
            let key = (face, id, key_size);
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
            .filter_map(|(x, id)| self.cache.get(&(face, id, key_size)).map(|mask| Placed { x, mask }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_and_measure() {
        let f = Fonts::new();
        let w = f.measure(Face::SemiBold, 32.0, 0.0, "GAME OF THE DAY");
        assert!(w > 150.0 && w < 350.0, "{w}");
        assert!(f.measure(Face::SemiBold, 64.0, 0.0, "BUF") > f.measure(Face::SemiBold, 32.0, 0.0, "BUF"));
        assert!(f.measure(Face::Medium, 32.0, 4.0, "AB") > f.measure(Face::Medium, 32.0, 0.0, "AB"));
        assert!(f.cap_height(Face::SemiBold, 100.0) > 50.0);
    }

    #[test]
    fn glyphs_render_and_cache() {
        let mut f = Fonts::new();
        let placed = f.layout(Face::SemiBold, 40.0, 0.0, "KC 27");
        assert_eq!(placed.len(), 5);
        assert!(placed[0].mask.width > 0 && placed[0].mask.alpha.iter().any(|&a| a > 200));
        assert_eq!(placed[2].mask.width, 0, "space has no ink");
        assert!(placed[1].x > placed[0].x);
        let cached = f.cache.len();
        f.layout(Face::SemiBold, 40.0, 0.0, "KC");
        assert_eq!(f.cache.len(), cached, "reused");
    }

    #[test]
    fn every_face_loads_and_draws_digits() {
        let mut f = Fonts::new();
        for face in Face::ALL {
            assert!(f.measure(face, 40.0, 0.0, "17-21") > 40.0, "{face:?}");
            assert!(f.layout(face, 40.0, 0.0, "7").iter().any(|g| g.mask.alpha.iter().any(|&a| a > 200)), "{face:?}");
        }
    }

    #[test]
    fn missing_glyphs_fall_back() {
        let mut f = Fonts::new();
        assert_eq!(f.layout(Face::Medium, 20.0, 0.0, "\u{E000}").len(), 1);
    }
}
