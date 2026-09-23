//! LED bitmap fonts.
//!
//! The source font lives in `fonts/led5x8.txt` as ASCII art so it can be
//! edited without code. [`BitmapFont::large`] derives a double-size font with
//! Scale2x, which keeps strokes crisp and rounds off diagonals.

use std::collections::HashMap;
use std::sync::OnceLock;

const LED5X8_SRC: &str = include_str!("../fonts/led5x8.txt");

/// One glyph: `width` x font height pixels, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub width: u16,
    pixels: Vec<bool>,
}

impl Glyph {
    pub fn lit(&self, x: u16, y: u16) -> bool {
        self.pixels[usize::from(y) * usize::from(self.width) + usize::from(x)]
    }
}

#[derive(Clone, Debug)]
pub struct BitmapFont {
    pub height: u16,
    /// Blank columns between adjacent glyphs.
    pub spacing: u16,
    /// Rows from the top to the bottom of capital letters (exclusive).
    pub cap_height: u16,
    glyphs: HashMap<char, Glyph>,
    fallback: Glyph,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FontError {
    #[error("line {line}: {msg}")]
    Parse { line: usize, msg: String },
}

impl BitmapFont {
    /// The built-in 5x7 (+descender) font.
    pub fn small() -> &'static BitmapFont {
        static FONT: OnceLock<BitmapFont> = OnceLock::new();
        #[allow(clippy::expect_used)] // Covered by tests; a broken built-in font is a build bug.
        FONT.get_or_init(|| BitmapFont::parse(LED5X8_SRC).expect("built-in LED font parses"))
    }

    /// The built-in font at double size, via Scale2x.
    pub fn large() -> &'static BitmapFont {
        static FONT: OnceLock<BitmapFont> = OnceLock::new();
        FONT.get_or_init(|| BitmapFont::small().scale2x())
    }

    pub fn parse(src: &str) -> Result<BitmapFont, FontError> {
        let err = |line: usize, msg: &str| FontError::Parse { line: line + 1, msg: msg.to_owned() };
        let mut height: Option<u16> = None;
        let mut glyphs = HashMap::new();
        let mut current: Option<(char, usize, Vec<String>)> = None;

        let finish = |cur: Option<(char, usize, Vec<String>)>,
                      height: u16,
                      glyphs: &mut HashMap<char, Glyph>|
         -> Result<(), FontError> {
            let Some((ch, line, rows)) = cur else { return Ok(()) };
            if rows.is_empty() {
                return Err(err(line, "glyph has no rows"));
            }
            if rows.len() > usize::from(height) {
                return Err(err(line, "glyph taller than font height"));
            }
            let width = rows[0].len();
            if rows.iter().any(|r| r.len() != width) {
                return Err(err(line, "glyph rows differ in width"));
            }
            let mut pixels: Vec<bool> = rows.iter().flat_map(|r| r.chars().map(|c| c == '#')).collect();
            pixels.resize(width * usize::from(height), false);
            let width = u16::try_from(width).map_err(|_| err(line, "glyph too wide"))?;
            if glyphs.insert(ch, Glyph { width, pixels }).is_some() {
                return Err(err(line, "duplicate glyph"));
            }
            Ok(())
        };

        for (i, raw) in src.lines().enumerate() {
            let line = raw.trim_end();
            if line.starts_with(';') {
                continue;
            }
            if line.is_empty() {
                if let Some(h) = height {
                    finish(current.take(), h, &mut glyphs)?;
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("height ") {
                height = Some(rest.trim().parse().map_err(|_| err(i, "bad height"))?);
            } else if let Some(rest) = line.strip_prefix("= ") {
                let h = height.ok_or_else(|| err(i, "glyph before height"))?;
                finish(current.take(), h, &mut glyphs)?;
                let ch = match rest {
                    "space" => ' ',
                    _ => {
                        let mut it = rest.chars();
                        match (it.next(), it.next()) {
                            (Some(c), None) => c,
                            _ => return Err(err(i, "glyph header must name one character")),
                        }
                    }
                };
                current = Some((ch, i, Vec::new()));
            } else if line.chars().all(|c| c == '#' || c == '.') {
                match current.as_mut() {
                    Some((_, _, rows)) => rows.push(line.to_owned()),
                    None => return Err(err(i, "pixel row outside a glyph")),
                }
            } else {
                return Err(err(i, "unrecognized line"));
            }
        }
        let height = height.ok_or_else(|| err(0, "missing height"))?;
        finish(current.take(), height, &mut glyphs)?;

        let fallback = glyphs.get(&'?').cloned().ok_or_else(|| err(0, "font needs a '?' glyph"))?;
        let cap_height = glyphs.get(&'H').map_or(height, |g| {
            (0..height).rev().find(|&y| (0..g.width).any(|x| g.lit(x, y))).map_or(height, |y| y + 1)
        });
        Ok(BitmapFont { height, spacing: 1, cap_height, glyphs, fallback })
    }

    /// Looks up a glyph; unknown characters render as `?`.
    pub fn glyph(&self, ch: char) -> &Glyph {
        self.glyphs.get(&ch).unwrap_or(&self.fallback)
    }

    pub fn has_glyph(&self, ch: char) -> bool {
        self.glyphs.contains_key(&ch)
    }

    /// Width in LED columns of `text` including inter-glyph spacing.
    pub fn text_width(&self, text: &str) -> u32 {
        let mut w = 0u32;
        for (i, ch) in text.chars().enumerate() {
            if i > 0 {
                w += u32::from(self.spacing);
            }
            w += u32::from(self.glyph(ch).width);
        }
        w
    }

    /// Doubles the font with the Scale2x (EPX) algorithm.
    pub fn scale2x(&self) -> BitmapFont {
        let h = self.height;
        let scale = |g: &Glyph| {
            let w = g.width;
            let at =
                |x: i32, y: i32| x >= 0 && y >= 0 && x < i32::from(w) && y < i32::from(h) && g.lit(x as u16, y as u16);
            let (w2, h2) = (usize::from(w) * 2, usize::from(h) * 2);
            let mut px = vec![false; w2 * h2];
            for y in 0..i32::from(h) {
                for x in 0..i32::from(w) {
                    let p = at(x, y);
                    let (a, b, c, d) = (at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
                    let e0 = if c == a && c != d && a != b { a } else { p };
                    let e1 = if a == b && a != c && b != d { b } else { p };
                    let e2 = if d == c && d != b && c != a { c } else { p };
                    let e3 = if b == d && b != a && d != c { d } else { p };
                    let (ox, oy) = (x as usize * 2, y as usize * 2);
                    px[oy * w2 + ox] = e0;
                    px[oy * w2 + ox + 1] = e1;
                    px[(oy + 1) * w2 + ox] = e2;
                    px[(oy + 1) * w2 + ox + 1] = e3;
                }
            }
            Glyph { width: w * 2, pixels: px }
        };
        BitmapFont {
            height: h * 2,
            spacing: self.spacing * 2,
            cap_height: self.cap_height * 2,
            glyphs: self.glyphs.iter().map(|(c, g)| (*c, scale(g))).collect(),
            fallback: scale(&self.fallback),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(font: &BitmapFont, ch: char) -> Vec<String> {
        let g = font.glyph(ch);
        (0..font.height).map(|y| (0..g.width).map(|x| if g.lit(x, y) { '#' } else { '.' }).collect()).collect()
    }

    #[test]
    fn built_in_font_covers_printable_ascii_and_symbols() {
        let f = BitmapFont::small();
        assert_eq!(f.height, 8);
        assert_eq!(f.cap_height, 7);
        for ch in (' '..='~').chain("▲▼◀▶◆•·°█".chars()) {
            assert!(f.has_glyph(ch), "missing glyph {ch:?}");
        }
    }

    #[test]
    fn digits_are_tabular() {
        let f = BitmapFont::small();
        for d in '0'..='9' {
            assert_eq!(f.glyph(d).width, 5, "digit {d} must be 5 wide so scores do not jitter");
        }
    }

    #[test]
    fn glyph_shape_matches_source() {
        assert_eq!(
            render(BitmapFont::small(), 'T'),
            ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..", "....."]
        );
    }

    #[test]
    fn text_width_includes_spacing() {
        let f = BitmapFont::small();
        assert_eq!(f.text_width(""), 0);
        assert_eq!(f.text_width("A"), 5);
        assert_eq!(f.text_width("AB"), 11);
        assert_eq!(f.text_width("I1"), 3 + 1 + 5);
    }

    #[test]
    fn unknown_chars_fall_back_to_question_mark() {
        let f = BitmapFont::small();
        assert_eq!(f.glyph('☃'), f.glyph('?'));
    }

    #[test]
    fn scale2x_doubles_and_smooths_diagonals() {
        let big = BitmapFont::large();
        assert_eq!(big.height, 16);
        assert_eq!(big.cap_height, 14);
        assert_eq!(big.glyph('A').width, 10);
        assert_eq!(big.text_width("AB"), 22);
        // A vertical stroke doubles cleanly.
        let l = render(big, 'l');
        assert_eq!(&l[0], "##..");
        // A diagonal gets smoothed: '/' is not just 2x2 blocks.
        let slash = render(big, '/');
        let blocky = slash.iter().all(|row| row.as_bytes().chunks(2).all(|p| p[0] == p[1]))
            && slash.chunks(2).all(|r| r[0] == r[1]);
        assert!(!blocky, "Scale2x should smooth diagonals:\n{}", slash.join("\n"));
    }

    #[test]
    fn parse_rejects_malformed_fonts() {
        assert!(BitmapFont::parse("= A\n#").is_err(), "glyph before height");
        assert!(BitmapFont::parse("height 2\n= ?\n##\n#\n").is_err(), "ragged rows");
        assert!(BitmapFont::parse("height 1\n= ?\n#\n#\n").is_err(), "too tall");
        assert!(BitmapFont::parse("height 1\n= A\n#\n").is_err(), "no fallback");
        assert!(BitmapFont::parse("height 1\n= ?\n#\n\n= ?\n#\n").is_err(), "duplicate");
        assert!(BitmapFont::parse("height 1\n= ?\nx\n").is_err(), "bad row");
    }

    #[test]
    fn parse_pads_short_glyphs_with_blank_rows() {
        let f = BitmapFont::parse("height 3\n= ?\n#\n").unwrap();
        assert_eq!(render(&f, '?'), ["#", ".", "."]);
    }
}
