//! Generic ticker content and the LED strip rasterizer.
//!
//! Sources describe what to show as [`TickerSegment`]s; they never deal with
//! pixels. The rasterizer turns a list of segments into an [`LedBitmap`]
//! (one texel per LED) for a band with a given number of LED rows. The
//! display uploads that bitmap and scrolls through it on the GPU.

use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::font::BitmapFont;
use std::collections::HashMap;
use std::sync::Arc;

use crate::icons::{self, LedIcon};
use crate::team_art::Image;

/// One item on the ticker, e.g. a game, a stock quote or a headline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TickerSegment {
    /// Stable identity (e.g. a game id) so updates and flashes can target it.
    pub id: String,
    pub parts: Vec<Part>,
}

/// A horizontal piece of a segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    /// One line of text, vertically centered. Uses the large font when the
    /// band is tall enough.
    Text { spans: Vec<Span> },
    /// Two lines stacked (e.g. away/home). On bands too short for two lines
    /// the lines are drawn side by side instead.
    Stack { top: Vec<Span>, bottom: Vec<Span>, align: Align },
    /// Blank LED columns (in small-font units; doubled with the large font).
    Gap { cols: u16 },
    /// A built-in multi-color icon (see `assets/icons.txt`), aligned with
    /// capital letters and doubled with the large font. Unknown names draw
    /// nothing.
    Icon { name: String },
    /// Logos someone added for the teams of a [`Part::Stack`] that follows:
    /// one per line when stacked, side by side otherwise. Keys name logos
    /// the display was sent; missing ones leave a blank.
    Logos {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    pub tint: Tint,
}

/// Color of a span, resolved against the configured LED color at raster time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "tint", content = "color")]
pub enum Tint {
    /// The configured LED color.
    #[default]
    Primary,
    /// The LED color at reduced brightness, for secondary info.
    Dim,
    /// Highlight color (e.g. red zone, winner, breaking news).
    Accent,
    /// An explicit color, e.g. a team color.
    Color(Rgb),
}

impl Span {
    pub fn new(text: impl Into<String>, tint: Tint) -> Self {
        Self { text: text.into(), tint }
    }
    pub fn primary(text: impl Into<String>) -> Self {
        Self::new(text, Tint::Primary)
    }
    pub fn dim(text: impl Into<String>) -> Self {
        Self::new(text, Tint::Dim)
    }
}

impl Part {
    pub fn text(spans: Vec<Span>) -> Self {
        Part::Text { spans }
    }
    pub fn icon(name: impl Into<String>) -> Self {
        Part::Icon { name: name.into() }
    }
    pub fn stack(top: Vec<Span>, bottom: Vec<Span>, align: Align) -> Self {
        Part::Stack { top, bottom, align }
    }
    pub fn gap(cols: u16) -> Self {
        Part::Gap { cols }
    }
}

/// Colors used when resolving [`Tint`]s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub primary: Rgb,
    pub accent: Rgb,
    pub dim_factor: f32,
}

impl Palette {
    pub fn new(primary: Rgb) -> Self {
        let accent = if primary == Rgb::RED { Rgb::WHITE } else { Rgb::RED };
        Self { primary, accent, dim_factor: 0.5 }
    }

    pub fn resolve(&self, tint: Tint) -> Rgb {
        match tint {
            Tint::Primary => self.primary,
            Tint::Dim => self.primary.scale(self.dim_factor),
            Tint::Accent => self.accent,
            Tint::Color(c) => c,
        }
    }
}

/// How a segment is drawn; used to animate flashes.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum RasterStyle {
    #[default]
    Normal,
    /// Background LEDs lit, text dark, like a sign inverting to grab attention.
    Inverted(Rgb),
    /// Text pushed toward white.
    Boost(f32),
}

/// A row-major RGBA8 image where each texel is one LED. Alpha is 255 for lit
/// LEDs and 0 for unlit ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedBitmap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl LedBitmap {
    pub fn new(width: u32, height: u32) -> Self {
        // In usize, so a big width can't wrap around to a small buffer.
        let len = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        Self { width, height, data: vec![0; len] }
    }

    pub fn set(&mut self, x: u32, y: u32, c: Rgb) {
        if x < self.width && y < self.height {
            let i = ((y * self.width + x) * 4) as usize;
            self.data[i..i + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Option<Rgb> {
        let i = ((y * self.width + x) * 4) as usize;
        (self.data[i + 3] != 0).then(|| Rgb::new(self.data[i], self.data[i + 1], self.data[i + 2]))
    }

    /// Copies columns `[x0, x0 + src.width)` from `src` into self.
    pub fn blit_columns(&mut self, src: &LedBitmap, x0: u32) {
        debug_assert_eq!(src.height, self.height);
        for y in 0..self.height.min(src.height) {
            for x in 0..src.width {
                let dx = x0 + x;
                if dx >= self.width {
                    break;
                }
                let s = ((y * src.width + x) * 4) as usize;
                let d = ((y * self.width + dx) * 4) as usize;
                self.data[d..d + 4].copy_from_slice(&src.data[s..s + 4]);
            }
        }
    }

    /// Copies all of `src` with its top-left corner at (`x0`, `y0`), clipping
    /// anything that falls outside.
    pub fn blit(&mut self, src: &LedBitmap, x0: u32, y0: u32) {
        for y in 0..src.height {
            for x in 0..src.width {
                let (dx, dy) = (x0 + x, y0 + y);
                if dx < self.width && dy < self.height {
                    let s = ((y * src.width + x) * 4) as usize;
                    let d = ((dy * self.width + dx) * 4) as usize;
                    self.data[d..d + 4].copy_from_slice(&src.data[s..s + 4]);
                }
            }
        }
    }

    /// A copy with every column at or beyond `cols` turned off.
    pub fn reveal(&self, cols: u32) -> LedBitmap {
        let mut out = self.clone();
        for y in 0..self.height {
            for x in cols.min(self.width)..self.width {
                let i = ((y * self.width + x) * 4) as usize;
                out.data[i..i + 4].copy_from_slice(&[0; 4]);
            }
        }
        out
    }

    /// ASCII view for tests and debugging: `#` lit, `.` unlit.
    pub fn to_ascii(&self) -> String {
        let mut s = String::new();
        for y in 0..self.height {
            for x in 0..self.width {
                s.push(if self.get(x, y).is_some() { '#' } else { '.' });
            }
            s.push('\n');
        }
        s
    }
}

/// Minimum rows for two small-font lines stacked with a one-row gap.
pub const STACK_ROWS: u32 = 17;
/// Minimum rows for the large (Scale2x) font's capitals.
pub const LARGE_TEXT_ROWS: u32 = 14;

/// Rasterizes segments for one LED band.
#[derive(Clone, Debug)]
pub struct Rasterizer<'f> {
    pub rows: u32,
    pub palette: Palette,
    small: &'f BitmapFont,
    large: &'f BitmapFont,
    /// Blank columns plus a dim separator glyph between segments.
    pub separator: Option<char>,
    /// Team logos for [`Part::Logos`], by key.
    pub logos: Arc<Logos>,
}

/// Logo images by key, for [`Part::Logos`].
pub type Logos = HashMap<String, Image>;

impl Rasterizer<'static> {
    pub fn new(rows: u32, palette: Palette) -> Self {
        Self::with_fonts(rows, palette, BitmapFont::small(), BitmapFont::large())
    }
}

impl<'f> Rasterizer<'f> {
    pub fn with_fonts(rows: u32, palette: Palette, small: &'f BitmapFont, large: &'f BitmapFont) -> Self {
        Self { rows, palette, small, large, separator: Some('◆'), logos: Arc::default() }
    }

    /// The same rasterizer drawing these logos.
    pub fn with_logos(self, logos: Arc<Logos>) -> Self {
        Self { logos, ..self }
    }

    /// Height of a logo: a line's capitals when stacked, else the text's.
    fn logo_rows(&self) -> u32 {
        let font = if self.stacks() { self.small } else { self.text_font() };
        u32::from(font.cap_height).max(1)
    }

    fn logo(&self, key: Option<&String>) -> Option<&Image> {
        key.and_then(|k| self.logos.get(k))
    }

    fn stacks(&self) -> bool {
        self.rows >= STACK_ROWS
    }

    /// Font used for single-line text on this band.
    fn text_font(&self) -> &'f BitmapFont {
        if self.rows >= LARGE_TEXT_ROWS + 2 { self.large } else { self.small }
    }

    fn spans_width(font: &BitmapFont, spans: &[Span]) -> u32 {
        let mut w = 0;
        let mut first = true;
        for span in spans {
            for ch in span.text.chars() {
                if !first {
                    w += u32::from(font.spacing);
                }
                first = false;
                w += u32::from(font.glyph(ch).width);
            }
        }
        w
    }

    fn gap_scale(&self) -> u32 {
        if std::ptr::eq(self.text_font(), self.large) { 2 } else { 1 }
    }

    fn part_width(&self, part: &Part) -> u32 {
        match part {
            Part::Text { spans } => Self::spans_width(self.text_font(), spans),
            Part::Stack { top, bottom, .. } => {
                let (t, b) = (Self::spans_width(self.small, top), Self::spans_width(self.small, bottom));
                if self.stacks() {
                    t.max(b)
                } else if t == 0 || b == 0 {
                    t + b
                } else {
                    t + INLINE_STACK_GAP + b
                }
            }
            Part::Gap { cols } => u32::from(*cols).saturating_mul(self.gap_scale()),
            Part::Icon { name } => self.icon(name).map_or(0, |i| i.width),
            Part::Logos { top, bottom } => {
                let rows = self.logo_rows();
                let (t, b) = (self.logo(top.as_ref()), self.logo(bottom.as_ref()));
                let width = |i: Option<&Image>| i.map_or(0, |i| i.led_width(rows));
                if t.is_none() && b.is_none() {
                    0
                } else if self.stacks() {
                    width(t).max(width(b)) + LOGO_GAP
                } else {
                    width(t) + width(b) + LOGO_GAP * u32::from(t.is_some() && b.is_some()) + LOGO_GAP
                }
            }
        }
    }

    fn icon(&self, name: &str) -> Option<&'static LedIcon> {
        icons::icon(name, std::ptr::eq(self.text_font(), self.large))
    }

    /// Width in LEDs of a segment on this band.
    pub fn segment_width(&self, seg: &TickerSegment) -> u32 {
        seg.parts.iter().fold(0u32, |w, p| w.saturating_add(self.part_width(p)))
    }

    /// Columns taken by the gap and separator after each segment. The
    /// separator glyph is always drawn in the small font so it stays a
    /// subtle marker on tall bands.
    pub fn separator_width(&self) -> u32 {
        let pad = SEPARATOR_PAD * self.gap_scale();
        match self.separator {
            Some(ch) => pad * 2 + u32::from(self.small.glyph(ch).width),
            None => 0,
        }
    }

    /// Draws `spans` with its left edge at `x` and glyph top at `top`.
    fn draw_spans(&self, bmp: &mut LedBitmap, font: &BitmapFont, spans: &[Span], x: u32, top: i32, style: RasterStyle) {
        let mut cx = x;
        let mut first = true;
        for span in spans {
            let color = self.style_color(self.palette.resolve(span.tint), style);
            for ch in span.text.chars() {
                if !first {
                    cx += u32::from(font.spacing);
                }
                first = false;
                let g = font.glyph(ch);
                for gy in 0..font.height {
                    let y = top + i32::from(gy);
                    if y < 0 {
                        continue;
                    }
                    for gx in 0..g.width {
                        if g.lit(gx, gy) {
                            let (px, py) = (cx + u32::from(gx), y as u32);
                            match style {
                                RasterStyle::Inverted(_) => clear(bmp, px, py),
                                _ => bmp.set(px, py, color),
                            }
                        }
                    }
                }
                cx += u32::from(g.width);
            }
        }
    }

    fn style_color(&self, c: Rgb, style: RasterStyle) -> Rgb {
        match style {
            RasterStyle::Boost(t) => c.saturate_brightness().mix(Rgb::WHITE, t),
            _ => c,
        }
    }

    /// Top row for text whose capitals should be vertically centered.
    fn centered_top(&self, font: &BitmapFont) -> i32 {
        (self.rows as i32 - i32::from(font.cap_height)) / 2
    }

    /// Renders one segment to its own bitmap.
    pub fn render_segment(&self, seg: &TickerSegment, style: RasterStyle) -> LedBitmap {
        let width = self.segment_width(seg);
        let mut bmp = LedBitmap::new(width, self.rows);
        if let RasterStyle::Inverted(bg) = style {
            for y in 0..self.rows {
                for x in 0..width {
                    bmp.set(x, y, bg);
                }
            }
        }
        let mut x = 0;
        for part in &seg.parts {
            let w = self.part_width(part);
            match part {
                Part::Text { spans } => {
                    let font = self.text_font();
                    self.draw_spans(&mut bmp, font, spans, x, self.centered_top(font), style);
                }
                Part::Stack { top, bottom, align } => {
                    let f = self.small;
                    if self.stacks() {
                        let cap = i32::from(f.cap_height);
                        let y0 = (self.rows as i32 - (cap * 2 + 1)) / 2;
                        for (spans, y) in [(top, y0), (bottom, y0 + cap + 1)] {
                            let sw = Self::spans_width(f, spans);
                            let sx = match align {
                                Align::Left => x,
                                Align::Right => x + (w - sw),
                            };
                            self.draw_spans(&mut bmp, f, spans, sx, y, style);
                        }
                    } else {
                        let top_w = Self::spans_width(f, top);
                        let y = self.centered_top(f);
                        self.draw_spans(&mut bmp, f, top, x, y, style);
                        let bx = if top_w == 0 { x } else { x + top_w + INLINE_STACK_GAP };
                        self.draw_spans(&mut bmp, f, bottom, bx, y, style);
                    }
                }
                Part::Gap { .. } => {}
                Part::Logos { top, bottom } => {
                    let rows = self.logo_rows();
                    let icons = [self.logo(top.as_ref()), self.logo(bottom.as_ref())].map(|i| i.map(|i| i.led(rows)));
                    let cap = rows as i32;
                    let placed: [(u32, i32); 2] = if self.stacks() {
                        let y0 = (self.rows as i32 - (cap * 2 + 1)) / 2;
                        [(x, y0), (x, y0 + cap + 1)]
                    } else {
                        let y = self.centered_top(self.text_font());
                        let first = icons[0].as_ref().map_or(0, |i| i.width + LOGO_GAP);
                        [(x, y), (x + first, y)]
                    };
                    for (icon, (ix0, iy0)) in icons.iter().zip(placed) {
                        let Some(icon) = icon else { continue };
                        for iy in 0..icon.height {
                            let y = iy0 + iy as i32;
                            for ix in 0..icon.width {
                                if let (Some(c), true) = (icon.get(ix, iy), y >= 0) {
                                    match style {
                                        RasterStyle::Inverted(_) => clear(&mut bmp, ix0 + ix, y as u32),
                                        _ => bmp.set(ix0 + ix, y as u32, self.style_color(c, style)),
                                    }
                                }
                            }
                        }
                    }
                }
                Part::Icon { name } => {
                    if let Some(icon) = self.icon(name) {
                        let top = self.centered_top(self.text_font());
                        for iy in 0..icon.height {
                            let y = top + iy as i32;
                            for ix in 0..icon.width {
                                if let (Some(c), true) = (icon.get(ix, iy), y >= 0) {
                                    match style {
                                        RasterStyle::Inverted(_) => clear(&mut bmp, x + ix, y as u32),
                                        _ => bmp.set(x + ix, y as u32, self.style_color(c, style)),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            x += w;
        }
        bmp
    }

    fn render_separator(&self) -> LedBitmap {
        let mut bmp = LedBitmap::new(self.separator_width(), self.rows);
        if let Some(ch) = self.separator {
            let spans = [Span::new(ch.to_string(), Tint::Dim)];
            let x = SEPARATOR_PAD * self.gap_scale();
            self.draw_spans(&mut bmp, self.small, &spans, x, self.centered_top(self.small), RasterStyle::Normal);
        }
        bmp
    }

    /// Lays out segments end to end (each followed by a separator) into one
    /// looping strip at least `min_width` columns wide.
    pub fn build_strip(&self, segments: &[TickerSegment], min_width: u32) -> Strip {
        self.build_strip_within(segments, min_width, u32::MAX)
    }

    /// [`Rasterizer::build_strip`], keeping only the segments that fit in
    /// `max_width` LEDs (measured before anything is drawn, so oversized
    /// content never allocates).
    pub fn build_strip_within(&self, segments: &[TickerSegment], min_width: u32, max_width: u32) -> Strip {
        let sep = self.render_separator();
        let mut total = 0u32;
        let mut kept = Vec::with_capacity(segments.len());
        for seg in segments {
            let w = self.segment_width(seg).saturating_add(sep.width);
            if total.saturating_add(w) > max_width {
                continue;
            }
            total += w;
            kept.push(seg.clone());
        }
        let segments = &kept[..];
        let min_width = min_width.min(max_width);
        let rendered: Vec<LedBitmap> = segments.iter().map(|s| self.render_segment(s, RasterStyle::Normal)).collect();
        let content: u32 = rendered.iter().map(|b| b.width + sep.width).sum();
        let width = content.max(min_width).max(1);
        let mut bitmap = LedBitmap::new(width, self.rows);
        let mut spans = Vec::with_capacity(segments.len());
        let mut x = 0;
        for (seg, bmp) in segments.iter().zip(&rendered) {
            bitmap.blit_columns(bmp, x);
            spans.push(SegmentSpan { id: seg.id.clone(), start: x, width: bmp.width });
            x += bmp.width;
            bitmap.blit_columns(&sep, x);
            x += sep.width;
        }
        Strip { bitmap, spans }
    }
}

/// Columns between the two lines of a stack drawn inline on a short band.
const INLINE_STACK_GAP: u32 = 3;
/// Columns after a team logo.
const LOGO_GAP: u32 = 2;
/// Blank columns on each side of the separator glyph.
const SEPARATOR_PAD: u32 = 6;

fn clear(bmp: &mut LedBitmap, x: u32, y: u32) {
    if x < bmp.width && y < bmp.height {
        let i = ((y * bmp.width + x) * 4) as usize;
        bmp.data[i..i + 4].copy_from_slice(&[0; 4]);
    }
}

/// Where a segment sits inside a [`Strip`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentSpan {
    pub id: String,
    pub start: u32,
    pub width: u32,
}

/// A rendered, looping ticker strip.
#[derive(Clone, Debug)]
pub struct Strip {
    pub bitmap: LedBitmap,
    pub spans: Vec<SegmentSpan>,
}

impl Strip {
    pub fn width(&self) -> u32 {
        self.bitmap.width
    }

    pub fn span(&self, id: &str) -> Option<&SegmentSpan> {
        self.spans.iter().find(|s| s.id == id)
    }

    /// Maps a scroll position in `self` to the equivalent position in `new`,
    /// so replacing the strip (scores changed, games added) does not make the
    /// ticker jump: the segment at the left edge stays at the left edge.
    pub fn remap_position(&self, new: &Strip, pos: f64) -> f64 {
        let w = f64::from(self.width().max(1));
        let pos = pos.rem_euclid(w);
        let anchor =
            self.spans.iter().rev().find(|s| f64::from(s.start) <= pos).and_then(|s| new.span(&s.id).map(|n| (s, n)));
        let mapped = match anchor {
            Some((old, new_span)) => {
                let offset = pos - f64::from(old.start);
                // If the anchored segment shrank, keep within it (or its separator).
                f64::from(new_span.start) + offset.min(f64::from(new_span.width + 1))
            }
            None => pos,
        };
        mapped.rem_euclid(f64::from(new.width().max(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logos_draw_beside_each_stacked_team() {
        let red = Image::new(2, 2, [255, 0, 0, 255].repeat(4)).unwrap();
        let logos: Logos = [("a".to_owned(), red)].into();
        let rast = Rasterizer::new(19, Palette::new(Rgb::AMBER)).with_logos(Arc::new(logos));
        let seg = TickerSegment {
            id: "g".into(),
            parts: vec![
                Part::Logos { top: Some("a".into()), bottom: Some("missing".into()) },
                Part::stack(vec![Span::primary("KC")], vec![Span::primary("BUF")], Align::Left),
            ],
        };
        let bmp = rast.render_segment(&seg, RasterStyle::Normal);
        let rows = u32::from(BitmapFont::small().cap_height);
        let red_rows: Vec<u32> = (0..19).filter(|&y| bmp.get(0, y) == Some(Rgb::new(255, 0, 0))).collect();
        assert_eq!(red_rows.len() as u32, rows, "one line tall, on the top line only");
        let without = Rasterizer::new(19, Palette::new(Rgb::AMBER));
        assert_eq!(without.segment_width(&seg) + rows + LOGO_GAP, rast.segment_width(&seg), "no logos, no room");
    }

    fn seg(id: &str, text: &str) -> TickerSegment {
        TickerSegment { id: id.into(), parts: vec![Part::text(vec![Span::primary(text)])] }
    }

    fn pal() -> Palette {
        Palette::new(Rgb::AMBER)
    }

    #[test]
    fn short_band_uses_small_font_centered() {
        let r = Rasterizer::new(9, pal());
        let bmp = r.render_segment(&seg("a", "I"), RasterStyle::Normal);
        assert_eq!(bmp.width, 3);
        // cap height 7 in 9 rows → one blank row above.
        assert_eq!(bmp.to_ascii(), "...\n###\n.#.\n.#.\n.#.\n.#.\n.#.\n###\n...\n");
    }

    #[test]
    fn icons_keep_their_colors_and_line_up_with_capitals() {
        let icon = |rows| {
            let r = Rasterizer::new(rows, pal());
            let seg = TickerSegment { id: "w".into(), parts: vec![Part::icon("rain"), Part::icon("no_such_icon")] };
            (r.segment_width(&seg), r.render_segment(&seg, RasterStyle::Normal))
        };
        let (w, bmp) = icon(9);
        assert_eq!(w, 9, "unknown icons take no space");
        let blue = crate::icons::icon("rain", false).unwrap().get(1, 5).unwrap();
        assert_eq!(bmp.get(1, 6), Some(blue), "rain stays blue on an amber sign, one row down like capitals");
        let (w, bmp) = icon(18);
        assert_eq!((w, bmp.height), (18, 18), "doubled on tall bands");
        let lit: Vec<u32> = (0..18).filter(|&y| (0..w).any(|x| bmp.get(x, y).is_some())).collect();
        assert_eq!((lit.first(), lit.last()), (Some(&2), Some(&15)));
    }

    #[test]
    fn tall_band_uses_large_font() {
        let r = Rasterizer::new(18, pal());
        assert_eq!(r.segment_width(&seg("a", "AB")), 22);
        let bmp = r.render_segment(&seg("a", "A"), RasterStyle::Normal);
        let lit_rows: Vec<u32> = (0..18).filter(|&y| (0..bmp.width).any(|x| bmp.get(x, y).is_some())).collect();
        // 14 rows of capitals, centered in 18 → rows 2..=15.
        assert_eq!(lit_rows.first(), Some(&2));
        assert_eq!(lit_rows.last(), Some(&15));
    }

    #[test]
    fn spans_take_their_tint() {
        let r = Rasterizer::new(9, pal());
        let team = Rgb::new(10, 200, 30);
        let s = TickerSegment {
            id: "x".into(),
            parts: vec![Part::text(vec![Span::new("I", Tint::Color(team)), Span::dim("I")])],
        };
        let bmp = r.render_segment(&s, RasterStyle::Normal);
        assert_eq!(bmp.get(0, 1), Some(team));
        assert_eq!(bmp.get(4, 1), Some(Rgb::AMBER.scale(0.5)));
    }

    #[test]
    fn stack_draws_two_lines_on_tall_band_and_inline_on_short() {
        let s = TickerSegment {
            id: "g".into(),
            parts: vec![Part::stack(vec![Span::primary("KC")], vec![Span::primary("BUF")], Align::Left)],
        };
        let tall = Rasterizer::new(18, pal());
        assert_eq!(tall.segment_width(&s), 17); // "BUF" = 5+1+5+1+5
        let bmp = tall.render_segment(&s, RasterStyle::Normal);
        let rows_lit: Vec<bool> = (0..18).map(|y| (0..bmp.width).any(|x| bmp.get(x, y).is_some())).collect();
        // Two 7-row lines separated by a blank row, block centered: rows 1-7 and 9-15.
        let expected: Vec<bool> = (0..18).map(|y| (1..=7).contains(&y) || (9..=15).contains(&y)).collect();
        assert_eq!(rows_lit, expected);

        let short = Rasterizer::new(9, pal());
        assert_eq!(short.segment_width(&s), 11 + 3 + 17);
    }

    #[test]
    fn right_aligned_stack_aligns_right_edges() {
        let s = TickerSegment {
            id: "g".into(),
            parts: vec![Part::stack(vec![Span::primary("7")], vec![Span::primary("21")], Align::Right)],
        };
        let r = Rasterizer::new(18, pal());
        let bmp = r.render_segment(&s, RasterStyle::Normal);
        assert_eq!(bmp.width, 11);
        // '7' top row is "#####" at columns 6..11.
        assert!((6..11).all(|x| bmp.get(x, 1).is_some()));
        assert!((0..6).all(|x| bmp.get(x, 1).is_none()));
    }

    #[test]
    fn blit_places_and_clips() {
        let mut dst = LedBitmap::new(3, 2);
        let mut src = LedBitmap::new(2, 2);
        src.set(0, 0, Rgb::RED);
        src.set(1, 1, Rgb::WHITE);
        dst.blit(&src, 2, 0);
        assert_eq!(dst.get(2, 0), Some(Rgb::RED));
        assert_eq!(dst.get(2, 1), None);
        assert_eq!(dst.reveal(2).get(2, 0), None);
        assert_eq!(dst.reveal(3).get(2, 0), Some(Rgb::RED));
    }

    #[test]
    fn inverted_style_lights_background_and_clears_text() {
        let r = Rasterizer::new(9, pal());
        let bmp = r.render_segment(&seg("a", "I"), RasterStyle::Inverted(Rgb::RED));
        assert_eq!(bmp.get(0, 0), Some(Rgb::RED));
        assert_eq!(bmp.get(0, 1), None);
    }

    #[test]
    fn strip_places_segments_with_separators_and_pads_to_min_width() {
        let r = Rasterizer::new(9, pal());
        let strip = r.build_strip(&[seg("a", "I"), seg("b", "II")], 0);
        let sep = r.separator_width();
        assert_eq!(sep, 6 + 5 + 6);
        assert_eq!(strip.spans[0], SegmentSpan { id: "a".into(), start: 0, width: 3 });
        assert_eq!(strip.spans[1], SegmentSpan { id: "b".into(), start: 3 + sep, width: 7 });
        assert_eq!(strip.width(), 3 + sep + 7 + sep);

        let padded = r.build_strip(&[seg("a", "I")], 500);
        assert_eq!(padded.width(), 500);
        assert!(r.build_strip(&[], 0).width() >= 1);
        // Oversized content is dropped before anything is drawn.
        let huge = TickerSegment { id: "huge".into(), parts: vec![Part::gap(u16::MAX); 2000] };
        let limited = r.build_strip_within(&[seg("a", "I"), huge, seg("b", "II")], 0, 2048);
        let ids: Vec<&str> = limited.spans.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(limited.width() <= 2048);
    }

    #[test]
    fn remap_keeps_left_edge_segment_in_place() {
        let r = Rasterizer::new(9, pal());
        let old = r.build_strip(&[seg("a", "AAAA"), seg("b", "B"), seg("c", "C")], 0);
        // "a" grew; "b" moved right.
        let new = r.build_strip(&[seg("a", "AAAAAAAA"), seg("b", "B"), seg("c", "C")], 0);
        let b_old = old.span("b").unwrap().start;
        let b_new = new.span("b").unwrap().start;
        assert_eq!(old.remap_position(&new, f64::from(b_old) + 2.5), f64::from(b_new) + 2.5);
        // Position inside "a" is unchanged.
        assert_eq!(old.remap_position(&new, 4.0), 4.0);
    }

    #[test]
    fn remap_falls_back_when_segment_removed() {
        let r = Rasterizer::new(9, pal());
        let old = r.build_strip(&[seg("a", "A"), seg("b", "B")], 0);
        let new = r.build_strip(&[seg("a", "A")], 0);
        let p = old.remap_position(&new, f64::from(old.span("b").unwrap().start));
        assert!(p >= 0.0 && p < f64::from(new.width()));
    }
}
