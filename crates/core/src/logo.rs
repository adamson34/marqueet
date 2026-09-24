//! The Marqueet mark: a parakeet drawn on an LED dot grid.
//!
//! One source of truth (`assets/mark.txt`, `assets/favicon.txt`) feeds both
//! the SVGs in `media/` and the LED welcome/boot screen, so the logo on the
//! website and on the device are the same dots.

use std::fmt::Write as _;
use std::sync::OnceLock;

use crate::color::Rgb;
use crate::ticker::LedBitmap;

const MARK_SRC: &str = include_str!("../assets/mark.txt");
const FAVICON_SRC: &str = include_str!("../assets/favicon.txt");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Empty,
    /// A filled dot (the bird's barred crown, beak, wing and tail).
    Solid,
    /// A ring (the light face and chest); drawn dimmer on the LED panel.
    Ring,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotMark {
    pub width: u32,
    pub height: u32,
    dots: Vec<Dot>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {msg}")]
pub struct MarkError {
    pub line: usize,
    pub msg: String,
}

impl DotMark {
    pub fn mark() -> &'static DotMark {
        static MARK: OnceLock<DotMark> = OnceLock::new();
        #[allow(clippy::expect_used)] // Covered by tests; a broken built-in asset is a build bug.
        MARK.get_or_init(|| DotMark::parse(MARK_SRC).expect("built-in mark parses"))
    }

    pub fn favicon() -> &'static DotMark {
        static FAV: OnceLock<DotMark> = OnceLock::new();
        #[allow(clippy::expect_used)]
        FAV.get_or_init(|| DotMark::parse(FAVICON_SRC).expect("built-in favicon parses"))
    }

    /// Parses rows of `#` (solid), `o` (ring) and `.` (empty). Lines starting
    /// with `;` are comments. Short rows are padded with empty dots.
    pub fn parse(src: &str) -> Result<DotMark, MarkError> {
        let mut rows: Vec<Vec<Dot>> = Vec::new();
        for (i, line) in src.lines().enumerate() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            let row = line
                .chars()
                .map(|c| match c {
                    '#' => Ok(Dot::Solid),
                    'o' => Ok(Dot::Ring),
                    '.' => Ok(Dot::Empty),
                    other => Err(MarkError { line: i + 1, msg: format!("unexpected {other:?}") }),
                })
                .collect::<Result<Vec<_>, _>>()?;
            rows.push(row);
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return Err(MarkError { line: 0, msg: "mark has no dots".into() });
        }
        let mut dots = Vec::with_capacity(width * rows.len());
        for mut row in rows.iter().cloned() {
            row.resize(width, Dot::Empty);
            dots.extend(row);
        }
        Ok(DotMark { width: width as u32, height: rows.len() as u32, dots })
    }

    pub fn dot(&self, x: u32, y: u32) -> Dot {
        self.dots[(y * self.width + x) as usize]
    }

    /// SVG with one circle per dot on a 4-unit grid. `color` is any SVG
    /// paint, e.g. `#15171c` or `currentColor`.
    pub fn to_svg(&self, color: &str) -> String {
        const PITCH: f32 = 4.0;
        const R: f32 = 1.6;
        const RING_R: f32 = 1.15;
        const RING_W: f32 = 0.7;
        let (w, h) = (self.width as f32 * PITCH, self.height as f32 * PITCH);
        let mut s = String::new();
        let _ = writeln!(
            s,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w2}" height="{h2}" shape-rendering="geometricPrecision">"#,
            w2 = w * 2.0,
            h2 = h * 2.0,
        );
        for y in 0..self.height {
            for x in 0..self.width {
                let (cx, cy) = ((x as f32 + 0.5) * PITCH, (y as f32 + 0.5) * PITCH);
                match self.dot(x, y) {
                    Dot::Solid => {
                        let _ = writeln!(s, r#"  <circle cx="{cx}" cy="{cy}" r="{R}" fill="{color}"/>"#);
                    }
                    Dot::Ring => {
                        let _ = writeln!(
                            s,
                            r#"  <circle cx="{cx}" cy="{cy}" r="{RING_R}" fill="none" stroke="{color}" stroke-width="{RING_W}"/>"#
                        );
                    }
                    Dot::Empty => {}
                }
            }
        }
        s.push_str("</svg>\n");
        s
    }

    /// A square app icon: the mark in `color`, centered with a margin on a
    /// rounded `background` square (app stores want square icons).
    pub fn to_icon_svg(&self, color: &str, background: &str) -> String {
        const PITCH: f32 = 4.0;
        let (w, h) = (self.width as f32 * PITCH, self.height as f32 * PITCH);
        let side = w.max(h) * 1.3;
        let (dx, dy) = ((side - w) / 2.0, (side - h) / 2.0);
        let inner = self.to_svg(color);
        let body: String =
            inner.lines().filter(|l| l.trim_start().starts_with("<circle")).collect::<Vec<_>>().join("\n");
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" width="256" height="256" shape-rendering="geometricPrecision">
  <rect width="{side}" height="{side}" rx="{r}" fill="{background}"/>
  <g transform="translate({dx} {dy})">
{body}
  </g>
</svg>
"#,
            r = (side * 0.18).round(),
        )
    }

    /// LED bitmap of the mark: solid dots in `solid`, rings in `ring`.
    pub fn to_led_bitmap(&self, solid: Rgb, ring: Rgb) -> LedBitmap {
        let mut bmp = LedBitmap::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                match self.dot(x, y) {
                    Dot::Solid => bmp.set(x, y, solid),
                    Dot::Ring => bmp.set(x, y, ring),
                    Dot::Empty => {}
                }
            }
        }
        bmp
    }
}

/// Brand colors used for the generated media files.
pub mod brand {
    /// LED amber, the default ticker color.
    pub const AMBER: &str = "#ffaa00";
    /// Near-black for light backgrounds.
    pub const INK: &str = "#15171c";
    /// Warm off-white for dark backgrounds.
    pub const PAPER: &str = "#f3efe6";
}

/// Every generated file in `media/`: (file name, SVG contents).
pub fn media_files() -> Vec<(&'static str, String)> {
    let (mark, fav) = (DotMark::mark(), DotMark::favicon());
    vec![
        ("marqueet-mark.svg", mark.to_svg(brand::AMBER)),
        ("marqueet-mark-ink.svg", mark.to_svg(brand::INK)),
        ("marqueet-mark-paper.svg", mark.to_svg(brand::PAPER)),
        ("marqueet-favicon.svg", fav.to_svg("currentColor")),
        ("marqueet-favicon-ink.svg", fav.to_svg(brand::INK)),
        ("marqueet-favicon-paper.svg", fav.to_svg(brand::PAPER)),
        ("marqueet-icon.svg", mark.to_icon_svg(brand::AMBER, brand::INK)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn built_in_marks_parse() {
        let m = DotMark::mark();
        assert_eq!((m.width, m.height), (20, 18));
        assert_eq!(m.dot(10, 0), Dot::Solid, "top of the crown");
        assert_eq!(m.dot(13, 4), Dot::Empty, "the eye");
        assert_eq!(m.dot(19, 5), Dot::Solid, "tip of the hooked beak");
        let f = DotMark::favicon();
        assert!(f.width <= 10 && f.height <= 8, "favicon must stay tiny");
    }

    #[test]
    fn parse_pads_rows_and_rejects_junk() {
        let m = DotMark::parse("; c\n#o\n#\n").unwrap();
        assert_eq!((m.width, m.height), (2, 2));
        assert_eq!(m.dot(1, 1), Dot::Empty);
        assert!(DotMark::parse("#x").is_err());
        assert!(DotMark::parse("; only comments").is_err());
    }

    #[test]
    fn svg_has_one_circle_per_dot() {
        let m = DotMark::parse("#o.\n").unwrap();
        let svg = m.to_svg("red");
        assert_eq!(svg.matches("<circle").count(), 2);
        assert!(svg.contains(r#"fill="red""#));
        assert!(svg.contains(r#"stroke="red""#));
        assert!(svg.contains(r#"viewBox="0 0 12 4""#));
    }

    #[test]
    fn icon_is_square_with_the_mark_inside() {
        let m = DotMark::mark();
        let svg = m.to_icon_svg("#ffaa00", "#15171c");
        let side = 20.0 * 4.0 * 1.3;
        assert!(svg.contains(&format!(r#"viewBox="0 0 {side} {side}""#)), "{svg}");
        assert_eq!(svg.matches("<circle").count(), m.to_svg("x").matches("<circle").count());
        assert!(svg.contains(r##"fill="#15171c""##));
    }

    #[test]
    fn led_bitmap_uses_both_colors() {
        let m = DotMark::parse("#o.\n").unwrap();
        let b = m.to_led_bitmap(Rgb::WHITE, Rgb::RED);
        assert_eq!(b.get(0, 0), Some(Rgb::WHITE));
        assert_eq!(b.get(1, 0), Some(Rgb::RED));
        assert_eq!(b.get(2, 0), None);
    }

    /// The SVGs in `media/` are generated from the dot grids. This fails if
    /// they drift; regenerate with `MARQUEET_BLESS=1 cargo test -p marqueet-core logo`.
    #[test]
    fn media_svgs_are_up_to_date() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../media");
        let bless = std::env::var_os("MARQUEET_BLESS").is_some();
        for (name, expected) in media_files() {
            let path = dir.join(name);
            if bless {
                std::fs::write(&path, &expected).unwrap();
                continue;
            }
            let actual = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                actual == expected,
                "media/{name} is out of date; run MARQUEET_BLESS=1 cargo test -p marqueet-core logo"
            );
        }
    }
}
