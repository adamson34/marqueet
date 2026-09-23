//! CPU-side state of one LED band: its content strip, scroll position and
//! active flash animations. The GPU side only receives bitmaps and numbers.

use tickadee_core::Rgb;
use tickadee_core::ticker::{LedBitmap, RasterStyle, Rasterizer, Strip, TickerSegment};

/// How long a score flash lasts, in seconds.
pub const FLASH_SECS: f64 = 3.0;
const BLINK_SECS: f64 = 1.8;
const BLINK_HZ: f64 = 6.0;

/// The style a flashing segment has `elapsed` seconds into its flash:
/// rapid inverted blinking, then a boost that fades back to normal.
pub fn flash_style(elapsed: f64, color: Rgb) -> RasterStyle {
    if !(0.0..FLASH_SECS).contains(&elapsed) {
        RasterStyle::Normal
    } else if elapsed < BLINK_SECS {
        if ((elapsed * BLINK_HZ) as u64).is_multiple_of(2) {
            RasterStyle::Inverted(color)
        } else {
            RasterStyle::Boost(0.7)
        }
    } else {
        let t = (elapsed - BLINK_SECS) / (FLASH_SECS - BLINK_SECS);
        // Quantize so we only re-upload when the look actually changes.
        let level = ((1.0 - t) * 8.0).ceil() / 8.0 * 0.7;
        if level <= 0.0 { RasterStyle::Normal } else { RasterStyle::Boost(level as f32) }
    }
}

#[derive(Debug, Clone)]
struct Flash {
    segment_id: String,
    started: f64,
    color: Rgb,
    applied: Option<RasterStyle>,
}

/// A pending texture update.
#[derive(Debug, Clone, PartialEq)]
pub enum Upload {
    /// Replace the whole strip.
    Full,
    /// Replace columns starting at `start` with this bitmap.
    Columns { start: u32, bitmap: LedBitmap },
}

#[derive(Debug)]
pub struct Band {
    pub rast: Rasterizer<'static>,
    segments: Vec<TickerSegment>,
    pub strip: Strip,
    /// Scroll position in LED columns (left edge of the visible window).
    pos: f64,
    /// Columns per second; 0 for a static panel.
    pub speed: f64,
    /// Scrolling bands loop; static bands show the strip once, centered.
    pub wrap: bool,
    /// Visible columns.
    pub cols: u32,
    /// Hard limit from the GPU texture size.
    pub max_width: u32,
    flashes: Vec<Flash>,
    pub uploads: Vec<Upload>,
}

impl Band {
    pub fn new(rast: Rasterizer<'static>, cols: u32, speed: f64, wrap: bool, max_width: u32) -> Self {
        let strip = rast.build_strip(&[], cols);
        Band {
            rast,
            segments: Vec::new(),
            strip,
            pos: 0.0,
            speed,
            wrap,
            cols,
            max_width,
            flashes: Vec::new(),
            uploads: vec![Upload::Full],
        }
    }

    fn min_width(&self) -> u32 {
        // A looping strip must be wider than the screen, or a segment could
        // appear twice at once.
        if self.wrap { self.cols + 1 } else { 1 }
    }

    /// Replaces the content, keeping the scroll position anchored to the
    /// segment currently at the left edge.
    pub fn set_segments(&mut self, segments: Vec<TickerSegment>) {
        if segments == self.segments {
            return;
        }
        let mut new = self.rast.build_strip(&segments, self.min_width());
        if new.width() > self.max_width {
            log::warn!("ticker content is {} LEDs wide; truncating to {}", new.width(), self.max_width);
            new.spans.retain(|s| s.start + s.width <= self.max_width);
            let mut bmp = LedBitmap::new(self.max_width, new.bitmap.height);
            bmp.blit_columns(&new.bitmap, 0);
            new.bitmap = bmp;
        }
        self.pos = if self.wrap { self.strip.remap_position(&new, self.pos) } else { 0.0 };
        self.strip = new;
        self.segments = segments;
        self.uploads.clear();
        self.uploads.push(Upload::Full);
        for f in &mut self.flashes {
            f.applied = None;
        }
    }

    /// Shows a fixed image instead of text segments (e.g. the welcome logo).
    pub fn set_bitmap(&mut self, bitmap: LedBitmap) {
        if bitmap == self.strip.bitmap {
            return;
        }
        self.segments.clear();
        self.flashes.clear();
        self.strip = Strip { bitmap, spans: Vec::new() };
        self.pos = 0.0;
        self.uploads.clear();
        self.uploads.push(Upload::Full);
    }

    pub fn flash(&mut self, segment_id: &str, color: Rgb, now: f64) {
        self.flashes.retain(|f| f.segment_id != segment_id);
        self.flashes.push(Flash { segment_id: segment_id.into(), started: now, color, applied: None });
    }

    #[cfg(test)]
    pub fn is_flashing(&self, segment_id: &str) -> bool {
        self.flashes.iter().any(|f| f.segment_id == segment_id)
    }

    /// Advances scrolling and flash animations to time `now` (seconds).
    pub fn update(&mut self, dt: f64, now: f64) {
        if self.wrap {
            self.pos = (self.pos + self.speed * dt).rem_euclid(f64::from(self.strip.width().max(1)));
        }
        let mut done = Vec::new();
        for (i, f) in self.flashes.iter_mut().enumerate() {
            let style = flash_style(now - f.started, f.color);
            if f.applied != Some(style) {
                let seg = self.segments.iter().find(|s| s.id == f.segment_id);
                if let (Some(seg), Some(span)) = (seg, self.strip.span(&f.segment_id)) {
                    let bitmap = self.rast.render_segment(seg, style);
                    if bitmap.width == span.width {
                        self.uploads.push(Upload::Columns { start: span.start, bitmap });
                    }
                }
                f.applied = Some(style);
            }
            if now - f.started >= FLASH_SECS {
                done.push(i);
            }
        }
        for i in done.into_iter().rev() {
            self.flashes.remove(i);
        }
    }

    /// Integer column at the left edge and the fraction toward the next.
    /// For static bands, a negative offset that centers the content.
    pub fn scroll(&self) -> (i32, f32) {
        if self.wrap {
            (self.pos.floor() as i32, self.pos.fract() as f32)
        } else {
            (-((self.cols as i32 - self.strip.width() as i32) / 2), 0.0)
        }
    }

    /// Jumps so segment `id` starts a few columns from the left edge.
    pub fn scroll_to(&mut self, id: &str) -> bool {
        match self.strip.span(id) {
            Some(span) if self.wrap => {
                self.pos = f64::from(span.start.saturating_sub(4));
                true
            }
            _ => false,
        }
    }

    pub fn take_uploads(&mut self) -> Vec<Upload> {
        std::mem::take(&mut self.uploads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tickadee_core::ticker::{Palette, Part, Span};

    fn seg(id: &str, text: &str) -> TickerSegment {
        TickerSegment { id: id.into(), parts: vec![Part::text(vec![Span::primary(text)])] }
    }

    fn band(cols: u32) -> Band {
        Band::new(Rasterizer::new(9, Palette::new(Rgb::AMBER)), cols, 10.0, true, 100_000)
    }

    #[test]
    fn flash_blinks_then_fades_then_ends() {
        let c = Rgb::RED;
        assert_eq!(flash_style(0.0, c), RasterStyle::Inverted(c));
        assert_eq!(flash_style(0.2, c), RasterStyle::Boost(0.7));
        assert_eq!(flash_style(0.34, c), RasterStyle::Inverted(c));
        assert!(matches!(flash_style(2.5, c), RasterStyle::Boost(b) if b > 0.0 && b < 0.7));
        assert_eq!(flash_style(FLASH_SECS, c), RasterStyle::Normal);
    }

    #[test]
    fn scrolls_and_wraps() {
        let mut b = band(10);
        b.set_segments(vec![seg("a", "HELLO")]);
        let w = f64::from(b.strip.width());
        b.update(1.5, 1.5);
        assert_eq!(b.scroll(), (15, 0.0));
        b.update(w / 10.0, 99.0);
        assert_eq!(b.scroll().0, 15);
    }

    #[test]
    fn loop_is_wider_than_screen() {
        let mut b = band(500);
        b.set_segments(vec![seg("a", "HI")]);
        assert!(b.strip.width() > 500);
    }

    #[test]
    fn static_band_centers_content() {
        let mut b = Band::new(Rasterizer::new(9, Palette::new(Rgb::AMBER)), 21, 0.0, false, 100_000);
        b.set_segments(vec![seg("a", "I")]); // 3 wide + separator
        b.rast.separator = None;
        b.set_segments(vec![seg("b", "I")]);
        assert_eq!(b.strip.width(), 3);
        assert_eq!(b.scroll(), (-9, 0.0));
    }

    #[test]
    fn identical_content_does_not_reupload() {
        let mut b = band(10);
        b.set_segments(vec![seg("a", "X")]);
        b.take_uploads();
        b.set_segments(vec![seg("a", "X")]);
        assert!(b.take_uploads().is_empty());
    }

    #[test]
    fn flash_patches_only_the_segment_columns() {
        let mut b = band(10);
        b.set_segments(vec![seg("a", "AA"), seg("b", "BBB")]);
        b.take_uploads();
        b.flash("b", Rgb::RED, 0.0);
        b.update(0.0, 0.0);
        let span = b.strip.span("b").unwrap().clone();
        match b.take_uploads().as_slice() {
            [Upload::Columns { start, bitmap }] => {
                assert_eq!(*start, span.start);
                assert_eq!(bitmap.width, span.width);
                assert_eq!(bitmap.get(0, 0), Some(Rgb::RED), "inverted background lit");
            }
            other => panic!("unexpected uploads {other:?}"),
        }
        // Same style next frame: no new upload.
        b.update(0.01, 0.01);
        assert!(b.take_uploads().is_empty());
        // After the flash: restores normal rendering, then stops.
        b.update(0.0, FLASH_SECS + 0.1);
        assert_eq!(b.take_uploads().len(), 1);
        assert!(!b.is_flashing("b"));
    }

    #[test]
    fn content_change_keeps_left_edge_anchor() {
        let mut b = band(10);
        b.set_segments(vec![seg("a", "A"), seg("b", "BBBB"), seg("c", "C")]);
        let c_start = b.strip.span("c").unwrap().start;
        b.update(f64::from(c_start) / 10.0, 0.0);
        assert_eq!(b.scroll().0 as u32, c_start);
        b.set_segments(vec![seg("a", "AAAAAA"), seg("b", "BBBB"), seg("c", "C")]);
        assert_eq!(b.scroll().0 as u32, b.strip.span("c").unwrap().start);
    }
}
