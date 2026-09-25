//! Multi-color LED icons for ticker segments ([`crate::ticker::Part::Icon`]),
//! from `assets/icons.txt`. Sources name an icon; the rasterizer draws it.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::color::Rgb;

const ICONS_SRC: &str = include_str!("../assets/icons.txt");

/// A small colored bitmap; `None` pixels are off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedIcon {
    pub width: u32,
    pub height: u32,
    pixels: Vec<Option<Rgb>>,
}

impl LedIcon {
    /// An icon from row-major pixels (`None` is off). Panics in debug
    /// builds if the sizes don't match.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<Option<Rgb>>) -> LedIcon {
        debug_assert_eq!(pixels.len(), (width * height) as usize);
        LedIcon { width, height, pixels }
    }

    pub fn get(&self, x: u32, y: u32) -> Option<Rgb> {
        (x < self.width && y < self.height).then(|| self.pixels[(y * self.width + x) as usize]).flatten()
    }

    /// Double size with Scale2x (as the large font is made), so diagonals
    /// stay smooth.
    pub fn scale2x(&self) -> LedIcon {
        let (w, h) = (self.width as i32, self.height as i32);
        let at = |x: i32, y: i32| if x >= 0 && y >= 0 && x < w && y < h { self.get(x as u32, y as u32) } else { None };
        let (w2, h2) = (self.width * 2, self.height * 2);
        let mut px = vec![None; (w2 * h2) as usize];
        for y in 0..h {
            for x in 0..w {
                let p = at(x, y);
                let (a, b, c, d) = (at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
                let e0 = if c == a && c != d && a != b { a } else { p };
                let e1 = if a == b && a != c && b != d { b } else { p };
                let e2 = if d == c && d != b && c != a { c } else { p };
                let e3 = if b == d && b != a && d != c { d } else { p };
                let (ox, oy) = (x as u32 * 2, y as u32 * 2);
                px[(oy * w2 + ox) as usize] = e0;
                px[(oy * w2 + ox + 1) as usize] = e1;
                px[((oy + 1) * w2 + ox) as usize] = e2;
                px[((oy + 1) * w2 + ox + 1) as usize] = e3;
            }
        }
        LedIcon { width: w2, height: h2, pixels: px }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("icons line {line}: {msg}")]
pub struct IconError {
    pub line: usize,
    pub msg: String,
}

fn color(ch: char) -> Option<Rgb> {
    Some(match ch {
        'y' => Rgb::new(0xff, 0xe0, 0x4a),
        'm' => Rgb::new(0xf3, 0xea, 0xd0),
        'w' => Rgb::new(0xe6, 0xe8, 0xee),
        'g' => Rgb::new(0x9a, 0xa0, 0xab),
        'b' => Rgb::new(0x4d, 0x9b, 0xff),
        's' => Rgb::new(0xff, 0xff, 0xff),
        _ => return None,
    })
}

/// Parses the icon file format (see `assets/icons.txt`).
pub fn parse(src: &str) -> Result<HashMap<String, LedIcon>, IconError> {
    let mut out = HashMap::new();
    let mut current: Option<(String, Vec<Vec<Option<Rgb>>>)> = None;
    let mut finish = |cur: Option<(String, Vec<Vec<Option<Rgb>>>)>, line: usize| -> Result<(), IconError> {
        let Some((name, rows)) = cur else { return Ok(()) };
        let width = rows.first().map_or(0, Vec::len);
        if width == 0 || rows.iter().any(|r| r.len() != width) {
            return Err(IconError { line, msg: format!("icon {name:?}: rows must be the same, non-zero width") });
        }
        let icon = LedIcon { width: width as u32, height: rows.len() as u32, pixels: rows.concat() };
        out.insert(name, icon);
        Ok(())
    };
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix("= ") {
            finish(current.take(), i + 1)?;
            current = Some((name.trim().to_owned(), Vec::new()));
            continue;
        }
        let Some((name, rows)) = current.as_mut() else {
            return Err(IconError { line: i + 1, msg: "pixels before any \"= name\" header".into() });
        };
        let row = line
            .chars()
            .map(|c| match c {
                '.' => Ok(None),
                c => color(c)
                    .map(Some)
                    .ok_or_else(|| IconError { line: i + 1, msg: format!("icon {name:?}: unknown color {c:?}") }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(row);
    }
    finish(current, src.lines().count())?;
    Ok(out)
}

fn builtin() -> &'static HashMap<String, (LedIcon, LedIcon)> {
    static ICONS: OnceLock<HashMap<String, (LedIcon, LedIcon)>> = OnceLock::new();
    ICONS.get_or_init(|| {
        #[allow(clippy::expect_used)] // Covered by tests; a broken built-in asset is a build bug.
        let icons = parse(ICONS_SRC).expect("built-in icons parse");
        icons
            .into_iter()
            .map(|(name, small)| {
                let big = small.scale2x();
                (name, (small, big))
            })
            .collect()
    })
}

/// A built-in icon at small (7 rows) or large (14 rows) size.
pub fn icon(name: &str, large: bool) -> Option<&'static LedIcon> {
    builtin().get(name).map(|(small, big)| if large { big } else { small })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_icons_parse_at_cap_height() {
        for name in ["sun", "moon", "cloud", "partly_cloudy", "partly_cloudy_night", "fog", "rain", "snow", "thunder"] {
            let small = icon(name, false).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(small.height, 7, "{name}");
            let big = icon(name, true).unwrap();
            assert_eq!((big.width, big.height), (small.width * 2, 14), "{name}");
        }
        assert!(icon("curling_stone", false).is_none());
        assert_eq!(icon("rain", false).unwrap().get(1, 5), color('b'));
        assert_eq!(icon("sun", false).unwrap().get(1, 0), None);
    }

    #[test]
    fn bad_icons_are_reported_with_a_line() {
        assert_eq!(parse("= x\n#.\n").unwrap_err().line, 2, "unknown color");
        assert_eq!(parse("= x\nyy\ny\n").unwrap_err().line, 3, "ragged rows");
        assert_eq!(parse("yy\n").unwrap_err().line, 1, "no header");
    }
}
