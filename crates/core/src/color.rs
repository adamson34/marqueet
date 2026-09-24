use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// An sRGB color, 8 bits per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);
    pub const AMBER: Rgb = Rgb::new(255, 170, 0);
    pub const RED: Rgb = Rgb::new(255, 40, 30);
    pub const GREEN: Rgb = Rgb::new(40, 255, 90);
    pub const BLUE: Rgb = Rgb::new(60, 140, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Named LED presets accepted in config and on the command line.
    pub fn preset(name: &str) -> Option<Rgb> {
        Some(match name.to_ascii_lowercase().as_str() {
            "amber" => Self::AMBER,
            "red" => Self::RED,
            "green" => Self::GREEN,
            "blue" => Self::BLUE,
            "white" => Self::WHITE,
            _ => return None,
        })
    }

    /// Relative luminance (WCAG definition), 0.0 for black to 1.0 for white.
    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// Multiplies every channel by `factor` (clamped to 0..=1).
    pub fn scale(self, factor: f32) -> Rgb {
        let f = factor.clamp(0.0, 1.0);
        let s = |c: u8| (f32::from(c) * f).round() as u8;
        Rgb::new(s(self.r), s(self.g), s(self.b))
    }

    /// Mixes toward `other` by `t` (0 = self, 1 = other).
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgb::new(m(self.r, other.r), m(self.g, other.g), m(self.b, other.b))
    }

    /// Rescales so the brightest channel is 255, keeping the hue.
    /// An LED can only emit light, so a "dark" color is really a dim LED.
    pub fn saturate_brightness(self) -> Rgb {
        let max = self.r.max(self.g).max(self.b);
        if max == 0 {
            return Rgb::WHITE;
        }
        self.scale_unclamped(255.0 / f32::from(max))
    }

    fn scale_unclamped(self, factor: f32) -> Rgb {
        let s = |c: u8| (f32::from(c) * factor).round().min(255.0) as u8;
        Rgb::new(s(self.r), s(self.g), s(self.b))
    }
}

/// Picks a team color that reads well on a black LED panel.
///
/// Team primaries are often navy or near-black, which vanish on an LED sign.
/// An LED can only emit light, so a dark but colorful primary is shown at
/// full brightness with the same hue (navy becomes blue, midnight green
/// becomes teal), which keeps the team recognizable. Only when the primary is
/// essentially black or gray do we fall back to the secondary.
pub fn led_team_color(primary: Rgb, secondary: Option<Rgb>) -> Rgb {
    const MIN_LUMINANCE: f32 = 0.12;
    if primary.luminance() >= MIN_LUMINANCE {
        return primary;
    }
    let chroma = primary.r.max(primary.g).max(primary.b) - primary.r.min(primary.g).min(primary.b);
    if chroma >= 24 {
        let bright = primary.saturate_brightness();
        return if bright.luminance() >= MIN_LUMINANCE { bright } else { bright.mix(Rgb::WHITE, 0.35) };
    }
    match secondary {
        Some(sec) if sec.luminance() >= MIN_LUMINANCE => sec,
        Some(sec) if sec.r.max(sec.g).max(sec.b) > 0 => led_team_color(sec, None),
        _ => Rgb::WHITE,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid color {0:?}: expected #rrggbb or one of amber, red, green, blue, white")]
pub struct ParseColorError(String);

impl FromStr for Rgb {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(c) = Rgb::preset(s) {
            return Ok(c);
        }
        let hex = s.strip_prefix('#').unwrap_or(s);
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(ParseColorError(s.to_owned()));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ParseColorError(s.to_owned()));
        Ok(Rgb::new(byte(0)?, byte(2)?, byte(4)?))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl TryFrom<String> for Rgb {
    type Error = ParseColorError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Rgb> for String {
    fn from(c: Rgb) -> String {
        c.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_presets() {
        assert_eq!("#ff8800".parse::<Rgb>(), Ok(Rgb::new(255, 136, 0)));
        assert_eq!("00FF00".parse::<Rgb>(), Ok(Rgb::new(0, 255, 0)));
        assert_eq!("Amber".parse::<Rgb>(), Ok(Rgb::AMBER));
        assert!("#fff".parse::<Rgb>().is_err());
        assert!("#gg0000".parse::<Rgb>().is_err());
        assert!("#ffé000".parse::<Rgb>().is_err());
    }

    #[test]
    fn serde_round_trips_as_hex_string() {
        let json = serde_json::to_string(&Rgb::new(1, 2, 255)).unwrap();
        assert_eq!(json, "\"#0102ff\"");
        assert_eq!(serde_json::from_str::<Rgb>(&json).unwrap(), Rgb::new(1, 2, 255));
    }

    #[test]
    fn bright_primary_is_kept() {
        let bright_red = Rgb::new(0xe3, 0x18, 0x37);
        assert_eq!(led_team_color(bright_red, Some(Rgb::new(0xff, 0xb6, 0x12))), bright_red);
    }

    #[test]
    fn dark_colorful_primary_is_brightened_keeping_hue() {
        let navy = Rgb::new(0x00, 0x33, 0x8d);
        let c = led_team_color(navy, Some(Rgb::new(0xc6, 0x0c, 0x30)));
        assert!(c.luminance() >= 0.12, "{c} too dark");
        assert!(c.b == 255 && c.r == 0, "still blue, got {c}");
    }

    #[test]
    fn near_black_primary_falls_back_to_secondary() {
        let black = Rgb::new(0x10, 0x10, 0x10);
        let gold = Rgb::new(0xff, 0xb6, 0x12);
        assert_eq!(led_team_color(black, Some(gold)), gold);
    }

    #[test]
    fn dark_secondary_is_brightened_too() {
        let c = led_team_color(Rgb::BLACK, Some(Rgb::new(0x40, 0x00, 0x00)));
        assert_eq!(c, Rgb::new(255, 0, 0));
    }

    #[test]
    fn pure_black_becomes_visible() {
        assert!(led_team_color(Rgb::BLACK, None).luminance() >= 0.12);
        assert!(led_team_color(Rgb::BLACK, Some(Rgb::BLACK)).luminance() >= 0.12);
    }
}
