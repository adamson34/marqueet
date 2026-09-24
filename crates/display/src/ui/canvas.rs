//! A small CPU canvas: premultiplied RGBA8 pixels (sRGB), with the few
//! primitives the widget area needs. Redrawn only when content changes, then
//! uploaded to the GPU.

use marqueet_core::Rgb;
use marqueet_core::font::BitmapFont;

use super::text::{Face, Fonts};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    pub color: Rgb,
    pub alpha: f32,
}

impl Paint {
    pub const fn solid(color: Rgb) -> Paint {
        Paint { color, alpha: 1.0 }
    }
    pub const fn with_alpha(color: Rgb, alpha: f32) -> Paint {
        Paint { color, alpha }
    }
}

impl From<Rgb> for Paint {
    fn from(color: Rgb) -> Paint {
        Paint::solid(color)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Text style for [`Canvas::text`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub face: Face,
    pub size: f32,
    pub tracking: f32,
    pub paint: Paint,
    pub align: Align,
}

impl TextStyle {
    pub fn new(face: Face, size: f32, color: Rgb) -> TextStyle {
        TextStyle { face, size, tracking: 0.0, paint: Paint::solid(color), align: Align::Left }
    }
    pub fn tracking(self, tracking: f32) -> TextStyle {
        TextStyle { tracking, ..self }
    }
    pub fn align(self, align: Align) -> TextStyle {
        TextStyle { align, ..self }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8, row-major.
    pub data: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Canvas {
        Canvas { width, height, data: vec![0; (width * height * 4) as usize] }
    }

    pub fn clear(&mut self) {
        self.data.fill(0);
    }

    /// Source-over blend of `paint` at coverage `cov` (0..1) into pixel (x, y).
    fn blend(&mut self, x: i32, y: i32, paint: Paint, cov: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = (paint.alpha * cov).clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let src = [paint.color.r, paint.color.g, paint.color.b];
        for (dst, s) in self.data[i..i + 3].iter_mut().zip(src) {
            *dst = (f32::from(s) * a + f32::from(*dst) * (1.0 - a)).round() as u8;
        }
        let da = f32::from(self.data[i + 3]) / 255.0;
        self.data[i + 3] = ((a + da * (1.0 - a)) * 255.0).round() as u8;
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, paint: impl Into<Paint>) {
        let paint = paint.into();
        for py in y.max(0)..(y + h).min(self.height as i32) {
            for px in x.max(0)..(x + w).min(self.width as i32) {
                self.blend(px, py, paint, 1.0);
            }
        }
    }

    /// Opaque fill of whole pixels, skipping the blend math.
    fn fill_opaque(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb) {
        let (x0, x1) = (x0.max(0) as usize, x1.min(self.width as i32).max(0) as usize);
        let px = [color.r, color.g, color.b, 255];
        for y in y0.max(0)..y1.min(self.height as i32) {
            let row = y as usize * self.width as usize * 4;
            let (pixels, _) = self.data[row + x0 * 4..row + x1.max(x0) * 4].as_chunks_mut::<4>();
            pixels.fill(px);
        }
    }

    /// Anti-aliased rounded rectangle.
    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, paint: impl Into<Paint>) {
        let paint = paint.into();
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy, hw, hh) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        // Fast path: the fully covered interior (away from edges and corners)
        // of an opaque rectangle needs no per-pixel math.
        let (ix0, iy0) = ((x + r).ceil() as i32, (y + 1.0).ceil() as i32);
        let (ix1, iy1) = ((x + w - r).floor() as i32, (y + h - 1.0).floor() as i32);
        let (jx0, jy0) = ((x + 1.0).ceil() as i32, (y + r).ceil() as i32);
        let (jx1, jy1) = ((x + w - 1.0).floor() as i32, (y + h - r).floor() as i32);
        let opaque = paint.alpha >= 1.0;
        if opaque {
            self.fill_opaque(ix0, iy0, ix1, iy1, paint.color);
            self.fill_opaque(jx0, jy0, jx1, jy1, paint.color);
        }
        let interior = |px: i32, py: i32| {
            opaque
                && ((px >= ix0 && px < ix1 && py >= iy0 && py < iy1)
                    || (px >= jx0 && px < jx1 && py >= jy0 && py < jy1))
        };
        for py in (y.floor() as i32).max(0)..((y + h).ceil() as i32).min(self.height as i32) {
            for px in (x.floor() as i32).max(0)..((x + w).ceil() as i32).min(self.width as i32) {
                if interior(px, py) {
                    continue;
                }
                let (qx, qy) = ((px as f32 + 0.5 - cx).abs() - hw + r, (py as f32 + 0.5 - cy).abs() - hh + r);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
                let cov = (0.5 - outside).clamp(0.0, 1.0);
                self.blend(px, py, paint, cov);
            }
        }
    }

    /// Anti-aliased convex polygon (points in either winding order).
    pub fn fill_polygon(&mut self, points: &[(f32, f32)], paint: impl Into<Paint>) {
        let paint = paint.into();
        if points.len() < 3 {
            return;
        }
        let area: f32 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum();
        // Each edge as a unit normal pointing outward and an offset, so a
        // pixel's signed distance to the edge is n·p - d.
        let edges: Vec<(f32, f32, f32)> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .filter_map(|(a, b)| {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = dx.hypot(dy);
                if len == 0.0 {
                    return None;
                }
                let (nx, ny) = if area > 0.0 { (dy / len, -dx / len) } else { (-dy / len, dx / len) };
                Some((nx, ny, nx * a.0 + ny * a.1))
            })
            .collect();
        let (x0, x1) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
        let (y0, y1) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        for py in (y0.floor() as i32).max(0)..(y1.ceil() as i32).min(self.height as i32) {
            for px in (x0.floor() as i32).max(0)..(x1.ceil() as i32).min(self.width as i32) {
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                let outside = edges.iter().map(|(nx, ny, d)| nx * cx + ny * cy - d).fold(f32::MIN, f32::max);
                let cov = (0.5 - outside).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, paint, cov);
                }
            }
        }
    }

    /// Text with an outline of `(color, width px)` around each glyph
    /// (tackle-twill numbers). Returns the text width.
    pub fn text_outlined(
        &mut self,
        fonts: &mut Fonts,
        (x, y): (f32, f32),
        style: TextStyle,
        (outline, width): (Rgb, f32),
        text: &str,
    ) -> f32 {
        // Stamp the glyphs around two rings in the outline color, then the
        // fill on top. Drawn only when content changes, so the extra passes
        // cost nothing per frame.
        let ring = TextStyle { paint: Paint::solid(outline), ..style };
        let steps = 16;
        for radius in [width, width * 0.5] {
            for i in 0..steps {
                let a = i as f32 * std::f32::consts::TAU / steps as f32;
                self.text(fonts, x + a.cos() * radius, y + a.sin() * radius, ring, text);
            }
        }
        self.text(fonts, x, y, style, text)
    }

    /// Draws `text` with its baseline at `y`; `x` is the left, center or
    /// right edge depending on `style.align`. Returns the text width.
    pub fn text(&mut self, fonts: &mut Fonts, x: f32, y: f32, style: TextStyle, text: &str) -> f32 {
        let width = fonts.measure(style.face, style.size, style.tracking, text);
        let start = match style.align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        for g in fonts.layout(style.face, style.size, style.tracking, text) {
            let (ox, oy) = ((start + g.x).round() as i32 + g.mask.left, y.round() as i32 - g.mask.top);
            for my in 0..g.mask.height {
                for mx in 0..g.mask.width {
                    let cov = f32::from(g.mask.alpha[(my * g.mask.width + mx) as usize]) / 255.0;
                    self.blend(ox + mx as i32, oy + my as i32, style.paint, cov);
                }
            }
        }
        width
    }

    /// LED-style block text from the project's LED font: each lit pixel is a
    /// `px`-sized square with a faint glow. `(x, y)` is the top-left of the
    /// capitals. Returns the width in pixels.
    pub fn led_text(&mut self, x: f32, y: f32, px: f32, color: Rgb, glow: bool, text: &str) -> f32 {
        let font = BitmapFont::small();
        let block = (px * 0.82).max(1.0);
        let mut cx = x;
        let mut first = true;
        for ch in text.chars() {
            if !first {
                cx += px * f32::from(font.spacing);
            }
            first = false;
            let g = font.glyph(ch);
            for gy in 0..font.height {
                for gx in 0..g.width {
                    if g.lit(gx, gy) {
                        let (bx, by) = (cx + f32::from(gx) * px, y + f32::from(gy) * px);
                        if glow {
                            let spread = px * 0.6;
                            self.fill_round_rect(
                                bx - spread,
                                by - spread,
                                block + spread * 2.0,
                                block + spread * 2.0,
                                spread,
                                Paint::with_alpha(color, 0.12),
                            );
                        }
                        self.fill_round_rect(bx, by, block, block, px * 0.12, color);
                    }
                }
            }
            cx += f32::from(g.width) * px;
        }
        cx - x
    }

    /// Width of [`Canvas::led_text`] output.
    pub fn led_width(px: f32, text: &str) -> f32 {
        BitmapFont::small().text_width(text) as f32 * px
    }

    #[cfg(test)]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_blend_and_clip() {
        let mut c = Canvas::new(10, 10);
        c.fill_rect(-5, -5, 8, 8, Rgb::RED);
        assert_eq!(c.pixel(2, 2), [255, 40, 30, 255]);
        assert_eq!(c.pixel(3, 3), [0, 0, 0, 0]);
        c.fill_rect(0, 0, 1, 1, Paint::with_alpha(Rgb::WHITE, 0.5));
        let p = c.pixel(0, 0);
        assert!(p[1] > 40 && p[1] < 255 && p[3] == 255);
    }

    #[test]
    fn rounded_corners_are_transparent() {
        let mut c = Canvas::new(40, 20);
        c.fill_round_rect(0.0, 0.0, 40.0, 20.0, 8.0, Rgb::WHITE);
        assert_eq!(c.pixel(0, 0)[3], 0, "corner cut");
        assert_eq!(c.pixel(20, 10)[3], 255, "center filled");
        assert!(c.pixel(1, 3)[3] < 255, "anti-aliased edge");
    }

    #[test]
    fn text_draws_ink_where_aligned() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(200, 60);
        let style = TextStyle::new(Face::SemiBold, 40.0, Rgb::WHITE).align(Align::Right);
        let w = c.text(&mut fonts, 190.0, 45.0, style, "BUF");
        assert!(w > 30.0);
        let ink_x: Vec<u32> = (0..200).filter(|&x| (0..60).any(|y| c.pixel(x, y)[3] > 128)).collect();
        assert!(*ink_x.last().unwrap() <= 191 && *ink_x.first().unwrap() >= (190.0 - w - 2.0) as u32);
    }

    #[test]
    fn centered_text_is_balanced() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(200, 60);
        let style = TextStyle::new(Face::Medium, 30.0, Rgb::WHITE).align(Align::Center);
        c.text(&mut fonts, 100.0, 40.0, style, "HALF");
        let ink: Vec<u32> = (0..200).filter(|&x| (0..60).any(|y| c.pixel(x, y)[3] > 128)).collect();
        let (left, right) = (*ink.first().unwrap(), 199 - *ink.last().unwrap());
        assert!(left.abs_diff(right) <= 4, "left margin {left}, right margin {right}");
    }

    #[test]
    fn polygons_fill_inside_and_slant() {
        let mut c = Canvas::new(40, 20);
        // A parallelogram leaning right, in clockwise order.
        c.fill_polygon(&[(0.0, 0.0), (30.0, 0.0), (20.0, 20.0), (0.0, 20.0)], Rgb::WHITE);
        assert_eq!(c.pixel(5, 10)[3], 255, "inside");
        assert_eq!(c.pixel(28, 18)[3], 0, "cut by the slant");
        assert_eq!(c.pixel(35, 2)[3], 0, "outside");
        let mut ccw = Canvas::new(40, 20);
        ccw.fill_polygon(&[(0.0, 20.0), (20.0, 20.0), (30.0, 0.0), (0.0, 0.0)], Rgb::WHITE);
        assert_eq!(ccw, c, "winding order doesn't matter");
    }

    #[test]
    fn outlined_text_has_a_ring() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(120, 80);
        let style = TextStyle::new(Face::Collegiate, 60.0, Rgb::WHITE);
        c.text_outlined(&mut fonts, (10.0, 70.0), style, (Rgb::RED, 4.0), "17");
        let count = |color: Rgb| {
            (0..120)
                .flat_map(|x| (0..80).map(move |y| (x, y)))
                .filter(|&(x, y)| c.pixel(x, y)[..3] == [color.r, color.g, color.b])
                .count()
        };
        assert!(count(Rgb::WHITE) > 100 && count(Rgb::RED) > 100);
    }

    #[test]
    fn led_text_matches_the_led_font() {
        let mut c = Canvas::new(100, 20);
        let w = c.led_text(0.0, 0.0, 2.0, Rgb::AMBER, false, "17");
        assert_eq!(w, Canvas::led_width(2.0, "17"));
        assert_eq!(w, 22.0, "two 5-wide digits + 1 spacing, at 2 px");
        assert_eq!(c.pixel(4, 0)[3], 255, "top of the 1");
    }
}
