//! People's own team colors and logos. Marqueet ships none: this is the
//! place where someone can add art they have for their teams, one team at a
//! time or as a shared "team pack" file. Pure: images arrive decoded; the
//! server does the PNG and file work.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::icons::LedIcon;
use crate::sports::{Game, TeamColors, TeamId};
use crate::ticker::{Part, TickerSegment};

/// Largest logo side kept, in pixels. Big enough for the game of the day on
/// a 1080p screen, small enough to send every logo to the display.
pub const LOGO_MAX: u32 = 128;

/// An RGBA8 image with straight (not premultiplied) alpha, row-major.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    #[serde(with = "b64")]
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

/// Base64 for image bytes in JSON (a third the size of a number array).
mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text.as_bytes()).map_err(serde::de::Error::custom)
    }
}

impl Image {
    /// An image, if `rgba` holds exactly `width` x `height` pixels.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Image> {
        let ok = width > 0 && height > 0 && rgba.len() as u64 == u64::from(width) * u64::from(height) * 4;
        ok.then_some(Image { width, height, rgba })
    }

    /// True when the pixel data matches the size (checks data from outside).
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.rgba.len() as u64 == u64::from(self.width) * u64::from(self.height) * 4
    }

    fn px(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Average of the source box [x0, x1) x [y0, y1) in source pixels,
    /// weighted by alpha so transparent pixels don't darken the edges.
    /// Returns (color, alpha 0..1).
    fn average(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> (Rgb, f32) {
        let (xa, xb) = (x0.floor() as u32, (x1.ceil() as u32).min(self.width).max(x0.floor() as u32 + 1));
        let (ya, yb) = (y0.floor() as u32, (y1.ceil() as u32).min(self.height).max(y0.floor() as u32 + 1));
        let (mut r, mut g, mut b, mut a, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for y in ya..yb.min(self.height) {
            for x in xa..xb.min(self.width) {
                let [pr, pg, pb, pa] = self.px(x, y);
                let alpha = f32::from(pa) / 255.0;
                r += f32::from(pr) * alpha;
                g += f32::from(pg) * alpha;
                b += f32::from(pb) * alpha;
                a += alpha;
                n += 1.0;
            }
        }
        if a <= 0.0 || n == 0.0 {
            return (Rgb::BLACK, 0.0);
        }
        let c = |v: f32| (v / a).round().clamp(0.0, 255.0) as u8;
        (Rgb::new(c(r), c(g), c(b)), a / n)
    }

    /// Scaled down (never up) to fit in `max` x `max`, keeping its shape.
    pub fn fit(&self, max: u32) -> Image {
        let scale = (max as f32 / self.width as f32).min(max as f32 / self.height as f32);
        if scale >= 1.0 {
            return self.clone();
        }
        let w = ((self.width as f32 * scale).round() as u32).max(1);
        let h = ((self.height as f32 * scale).round() as u32).max(1);
        let (sx, sy) = (self.width as f32 / w as f32, self.height as f32 / h as f32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (c, a) = self.average(x as f32 * sx, y as f32 * sy, (x + 1) as f32 * sx, (y + 1) as f32 * sy);
                rgba.extend_from_slice(&[c.r, c.g, c.b, (a * 255.0).round() as u8]);
            }
        }
        Image { width: w, height: h, rgba }
    }

    /// Width of [`Image::led`] at `rows` tall.
    pub fn led_width(&self, rows: u32) -> u32 {
        let w = (self.width as f32 * rows as f32 / self.height as f32).round() as u32;
        w.clamp(1, rows * 3)
    }

    /// The logo as LEDs, `rows` tall: a light is on where the logo is mostly
    /// opaque and not near-black (an LED can't show black).
    pub fn led(&self, rows: u32) -> LedIcon {
        let rows = rows.max(1);
        let w = self.led_width(rows);
        let (sx, sy) = (self.width as f32 / w as f32, self.height as f32 / rows as f32);
        let mut pixels = Vec::with_capacity((w * rows) as usize);
        for y in 0..rows {
            for x in 0..w {
                let (c, a) = self.average(x as f32 * sx, y as f32 * sy, (x + 1) as f32 * sx, (y + 1) as f32 * sy);
                pixels.push((a >= 0.5 && c.luminance() > 0.01).then_some(c));
            }
        }
        LedIcon::from_pixels(w, rows, pixels)
    }
}

/// What someone added for one team.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamArt {
    /// Who it's for, as shown on the admin page ("Kansas City Kingdom").
    pub label: String,
    /// Replaces the colors from the data, everywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<TeamColors>,
    /// Scaled to fit [`LOGO_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<Image>,
}

/// Everyone's team art, by team.
pub type TeamArtMap = BTreeMap<TeamId, TeamArt>;

/// Applies custom colors to every team in `games` that has them.
pub fn recolor(games: &mut [Game], art: &TeamArtMap) {
    if art.is_empty() {
        return;
    }
    for game in games {
        for c in [&mut game.home, &mut game.away] {
            if let Some(colors) = art.get(&c.team.id).and_then(|a| a.colors) {
                c.team.colors = colors;
            }
        }
    }
}

/// The key a team's logo travels under (opaque to the display).
pub fn logo_key(team: &TeamId) -> String {
    team.0.clone()
}

/// A team's logo key, when it has a logo.
pub fn logo_for(art: &TeamArtMap, team: &TeamId) -> Option<String> {
    art.get(team).filter(|a| a.logo.is_some()).map(|_| logo_key(team))
}

/// Puts each game's logos (where someone added them) in front of its
/// segment on the ticker.
pub fn add_logos(segments: &mut [TickerSegment], games: &[Game], art: &TeamArtMap) {
    if !art.values().any(|a| a.logo.is_some()) {
        return;
    }
    for seg in segments {
        let Some(game) = games.iter().find(|g| g.id.0 == seg.id) else { continue };
        let (top, bottom) = (logo_for(art, &game.away.team.id), logo_for(art, &game.home.team.id));
        if top.is_some() || bottom.is_some() {
            seg.parts.insert(0, Part::Logos { top, bottom });
        }
    }
}

/// Current version of the team pack file format.
pub const PACK_VERSION: u32 = 1;

/// A shareable file of team colors and logos:
/// `{"marqueet_team_pack": 1, "teams": [{"team": "espn:nfl:12", ...}]}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamPack {
    pub marqueet_team_pack: u32,
    pub teams: Vec<PackTeam>,
}

/// One team in a pack. Colors are `#rrggbb`; the logo is a base64 PNG.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackTeam {
    pub team: TeamId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_png: Option<String>,
}

impl PackTeam {
    /// The colors to apply: a primary is needed; the secondary is optional.
    pub fn colors(&self) -> Option<TeamColors> {
        self.primary.map(|primary| TeamColors { primary, secondary: self.secondary })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_games;
    use chrono::Utc;

    /// A `w` x `h` image: opaque red on the left half, transparent right.
    fn half_red(w: u32, h: u32) -> Image {
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(if x < w / 2 { &[255, 0, 0, 255] } else { &[0, 0, 0, 0] });
            }
        }
        Image::new(w, h, rgba).unwrap()
    }

    #[test]
    fn images_check_their_size() {
        assert!(Image::new(2, 2, vec![0; 16]).is_some());
        assert!(Image::new(2, 2, vec![0; 15]).is_none());
        assert!(Image::new(0, 2, vec![]).is_none());
        let bad = Image { width: 3, height: 3, rgba: vec![0; 4] };
        assert!(!bad.is_valid());
    }

    #[test]
    fn fit_shrinks_keeping_shape_and_never_grows() {
        let big = half_red(512, 256);
        let small = big.fit(LOGO_MAX);
        assert_eq!((small.width, small.height), (128, 64));
        assert_eq!(small.px(10, 10), [255, 0, 0, 255], "red stays red");
        assert_eq!(small.px(120, 10)[3], 0, "clear stays clear");
        let tiny = half_red(40, 40);
        assert_eq!(tiny.fit(LOGO_MAX), tiny);
    }

    #[test]
    fn transparent_edges_dont_darken_colors() {
        let img = half_red(4, 1).fit(1);
        assert_eq!(img.px(0, 0)[..3], [255, 0, 0], "averaged only over the opaque half");
        assert_eq!(img.px(0, 0)[3], 128);
    }

    #[test]
    fn logos_become_leds() {
        let icon = half_red(64, 32).led(7);
        assert_eq!((icon.width, icon.height), (14, 7));
        assert_eq!(icon.get(0, 3), Some(Rgb::new(255, 0, 0)));
        assert_eq!(icon.get(13, 3), None, "transparent is dark");
        let black = Image::new(1, 1, vec![0, 0, 0, 255]).unwrap();
        assert_eq!(black.led(1).get(0, 0), None, "black is off");
        let wide = Image::new(100, 1, vec![255; 400]).unwrap();
        assert_eq!(wide.led_width(7), 21, "capped at three times the height");
    }

    #[test]
    fn custom_colors_replace_the_data() {
        let mut games = mock_games(Utc::now());
        let team = games[0].home.team.id.clone();
        let colors = TeamColors { primary: Rgb::new(1, 2, 3), secondary: None };
        let art: TeamArtMap = [(team.clone(), TeamArt { label: "x".into(), colors: Some(colors), logo: None })].into();
        recolor(&mut games, &art);
        assert_eq!(games[0].home.team.colors, colors);
        assert_ne!(games[0].away.team.colors, colors, "other teams keep theirs");
        assert_eq!(logo_for(&art, &team), None, "no logo");
    }

    #[test]
    fn logos_lead_their_game_on_the_ticker() {
        use crate::sports::ticker::{FormatOptions, ticker_segments};
        let now = Utc::now();
        let games = mock_games(now);
        let opts = FormatOptions { tz: chrono::FixedOffset::east_opt(0).unwrap(), now };
        let mut segs = ticker_segments(&games, &opts);
        let plain = segs.clone();
        let home = games[0].home.team.id.clone();
        let logo = Image::new(1, 1, vec![255; 4]).unwrap();
        let art: TeamArtMap = [(home.clone(), TeamArt { label: "x".into(), colors: None, logo: Some(logo) })].into();
        add_logos(&mut segs, &games, &art);
        let seg = segs.iter().find(|s| s.id == games[0].id.0).unwrap();
        assert_eq!(seg.parts[0], Part::Logos { top: None, bottom: Some(logo_key(&home)) });
        let changed = segs.iter().zip(&plain).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 1, "only that team's game");
    }

    #[test]
    fn packs_round_trip_as_json() {
        let pack = TeamPack {
            marqueet_team_pack: PACK_VERSION,
            teams: vec![PackTeam {
                team: TeamId("espn:nfl:12".into()),
                name: "Somewhere".into(),
                primary: Some(Rgb::new(0xd6, 0x2a, 0x3c)),
                secondary: None,
                logo_png: None,
            }],
        };
        let json = serde_json::to_string(&pack).unwrap();
        assert!(json.contains(r##""primary":"#d62a3c""##), "{json}");
        assert_eq!(serde_json::from_str::<TeamPack>(&json).unwrap(), pack);
        let minimal: TeamPack = serde_json::from_str(r#"{"marqueet_team_pack":1,"teams":[{"team":"x"}]}"#).unwrap();
        assert_eq!(minimal.teams[0].colors(), None);
    }

    #[test]
    fn images_travel_as_base64() {
        let img = half_red(2, 1);
        let json = serde_json::to_string(&img).unwrap();
        assert!(json.contains(r#""rgba":"/wAA/wAAAAA=""#), "{json}");
        assert_eq!(serde_json::from_str::<Image>(&json).unwrap(), img);
    }
}
