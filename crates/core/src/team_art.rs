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
    /// The team's own words for its big plays, by play ([`WORD_PLAYS`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub words: BTreeMap<String, TakeoverWords>,
    /// LED art (a still or an animation) for the team's takeovers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art: Option<crate::art::TakeoverArt>,
}

/// A team's own words for a big play, in place of the takeover's
/// "TOUCHDOWN": "KINGDOM TD!", with an optional second line ("HEAR THE
/// CROWD") shown where the play would be.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeoverWords {
    pub headline: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

/// Longest custom headline (it shrinks to fit the screen as it is).
pub const WORDS_HEADLINE_MAX: usize = 16;
/// Longest custom second line.
pub const WORDS_LINE_MAX: usize = 40;

/// The plays a team can have its own words for: (id, label, takeover
/// headline it replaces).
pub const WORD_PLAYS: [(&str, &str, &str); 4] = [
    ("touchdown", "Touchdown", "TOUCHDOWN"),
    ("home_run", "Home run", "HOME RUN"),
    ("grand_slam", "Grand slam", "GRAND SLAM"),
    ("goal", "Goal", "GOAL"),
];

impl TakeoverWords {
    /// Trimmed, capitals for the LED font, cut to the limits; `None`
    /// without a headline or for a play that has no takeover.
    pub fn clean(play: &str, headline: &str, line: &str) -> Option<(String, TakeoverWords)> {
        WORD_PLAYS.iter().find(|(id, ..)| *id == play)?;
        let cut = |s: &str, max: usize| {
            s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect::<String>()
        };
        let headline = cut(headline, WORDS_HEADLINE_MAX).to_uppercase();
        let line = Some(cut(line, WORDS_LINE_MAX).to_uppercase()).filter(|l| !l.is_empty());
        (!headline.is_empty()).then(|| (play.to_owned(), TakeoverWords { headline, line }))
    }
}

/// Every word set in `words` cleaned ([`TakeoverWords::clean`]); unknown
/// plays and empty headlines are dropped.
pub fn clean_words(words: &BTreeMap<String, TakeoverWords>) -> BTreeMap<String, TakeoverWords> {
    words
        .iter()
        .filter_map(|(play, w)| TakeoverWords::clean(play, &w.headline, w.line.as_deref().unwrap_or("")))
        .collect()
}

impl TeamArt {
    /// Nothing set: no colors, logo or words.
    pub fn is_empty(&self) -> bool {
        self.colors.is_none() && self.logo.is_none() && self.words.is_empty() && self.art.is_none()
    }
}

/// Puts `team`'s own takeover on `alert`: its LED art, and its words when
/// it has some for this play (the alert's headline says which play it is).
pub fn apply_words(alert: &mut crate::alert::Alert, art: &TeamArtMap, team: &TeamId) {
    let Some(team_art) = art.get(team) else { return };
    let Some(takeover) = alert.takeover.as_mut() else { return };
    if let Some(a) = &team_art.art {
        takeover.art = Some(a.clone());
    }
    let words = &team_art.words;
    let Some((play, ..)) = WORD_PLAYS.iter().find(|(_, _, headline)| takeover.headline == *headline) else { return };
    let Some(w) = words.get(*play) else { return };
    takeover.headline = w.headline.clone();
    alert.title = w.headline.clone();
    if let Some(line) = &w.line {
        takeover.play = Some(line.clone());
    }
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

/// Team art with logos from the scores provider filled in where the owner
/// hasn't added a logo of their own (their colors are kept).
pub fn with_provider_logos(art: &TeamArtMap, logos: &BTreeMap<TeamId, Image>) -> TeamArtMap {
    let mut out = art.clone();
    for (team, image) in logos {
        let entry = out.entry(team.clone()).or_insert_with(|| TeamArt {
            label: String::new(),
            colors: None,
            logo: None,
            words: Default::default(),
            art: Default::default(),
        });
        if entry.logo.is_none() {
            entry.logo = Some(image.clone());
        }
    }
    out
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
    /// The team's own takeover words, by play ("touchdown", "home_run",
    /// "grand_slam", "goal").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub takeovers: BTreeMap<String, TakeoverWords>,
    /// The team's LED takeover art.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover_art: Option<PackArt>,
}

/// Takeover art in a pack: the frames side by side in one base64 PNG.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackArt {
    pub strip_png: String,
    pub frames: u32,
    pub frame_ms: u16,
    #[serde(default)]
    pub placement: crate::art::ArtPlacement,
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

    #[test]
    fn takeover_words_are_cleaned() {
        let (play, w) = TakeoverWords::clean("touchdown", "  kingdom   td! ", "hear  the crowd").unwrap();
        assert_eq!(
            (play.as_str(), w.headline.as_str(), w.line.as_deref()),
            ("touchdown", "KINGDOM TD!", Some("HEAR THE CROWD"))
        );
        let long = TakeoverWords::clean("goal", &"x".repeat(40), &"y".repeat(90)).unwrap().1;
        assert_eq!((long.headline.len(), long.line.unwrap().len()), (WORDS_HEADLINE_MAX, WORDS_LINE_MAX));
        assert!(TakeoverWords::clean("goal", "   ", "a line").is_none(), "a headline is needed");
        assert!(TakeoverWords::clean("field_goal", "KICK", "").is_none(), "only plays that take over");
        assert_eq!(TakeoverWords::clean("goal", "GOAL!", " ").unwrap().1.line, None);
    }

    #[test]
    fn a_teams_words_replace_the_headline() {
        use crate::alert::{AlertLevel, Takeover};
        let team = TeamId("mock:nfl:KC".into());
        let words =
            [("touchdown".to_owned(), TakeoverWords { headline: "KINGDOM TD!".into(), line: Some("HEAR IT".into()) })];
        let art: TeamArtMap = [(
            team.clone(),
            TeamArt { label: String::new(), colors: None, logo: None, words: words.into(), art: Default::default() },
        )]
        .into();
        let mut alert = crate::alert::Alert {
            id: "a".into(),
            level: AlertLevel::Takeover,
            source: "sports".into(),
            segment_id: None,
            title: "TOUCHDOWN".into(),
            detail: None,
            colors: None,
            takeover: Some(Takeover {
                kicker: String::new(),
                headline: "TOUCHDOWN".into(),
                play: Some("a run".into()),
                score: None,
                note: None,
                art: Default::default(),
            }),
            created_at: Utc::now(),
        };
        let mut other = alert.clone();
        apply_words(&mut other, &art, &TeamId("mock:nfl:BUF".into()));
        assert_eq!(other.title, "TOUCHDOWN", "another team keeps the usual words");
        apply_words(&mut alert, &art, &team);
        assert!(alert.takeover.as_ref().unwrap().art.is_none(), "no art set");
        let t = alert.takeover.as_ref().unwrap();
        assert_eq!(
            (alert.title.as_str(), t.headline.as_str(), t.play.as_deref()),
            ("KINGDOM TD!", "KINGDOM TD!", Some("HEAR IT"))
        );
        let mut goal = alert.clone();
        goal.takeover.as_mut().unwrap().headline = "GOAL".into();
        apply_words(&mut goal, &art, &team);
        assert_eq!(goal.takeover.unwrap().headline, "GOAL", "no words for that play");
    }

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
        let art: TeamArtMap = [(
            team.clone(),
            TeamArt {
                label: "x".into(),
                colors: Some(colors),
                logo: None,
                words: Default::default(),
                art: Default::default(),
            },
        )]
        .into();
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
        let art: TeamArtMap = [(
            home.clone(),
            TeamArt {
                label: "x".into(),
                colors: None,
                logo: Some(logo),
                words: Default::default(),
                art: Default::default(),
            },
        )]
        .into();
        add_logos(&mut segs, &games, &art);
        let seg = segs.iter().find(|s| s.id == games[0].id.0).unwrap();
        assert_eq!(seg.parts[0], Part::Logos { top: None, bottom: Some(logo_key(&home)) });
        let changed = segs.iter().zip(&plain).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 1, "only that team's game");
    }

    #[test]
    fn owners_logos_win_over_the_providers() {
        let (a, b) = (TeamId("a".into()), TeamId("b".into()));
        let mine = Image::new(1, 1, vec![1, 1, 1, 255]).unwrap();
        let theirs = Image::new(1, 1, vec![9, 9, 9, 255]).unwrap();
        let colors = TeamColors { primary: Rgb::RED, secondary: None };
        let art: TeamArtMap = [
            (
                a.clone(),
                TeamArt {
                    label: "A".into(),
                    colors: None,
                    logo: Some(mine.clone()),
                    words: Default::default(),
                    art: Default::default(),
                },
            ),
            (
                b.clone(),
                TeamArt {
                    label: "B".into(),
                    colors: Some(colors),
                    logo: None,
                    words: Default::default(),
                    art: Default::default(),
                },
            ),
        ]
        .into();
        let provider: BTreeMap<TeamId, Image> =
            [(a.clone(), theirs.clone()), (b.clone(), theirs.clone()), (TeamId("c".into()), theirs.clone())].into();
        let merged = with_provider_logos(&art, &provider);
        assert_eq!(merged[&a].logo.as_ref(), Some(&mine), "the owner's logo stays");
        assert_eq!((merged[&b].logo.as_ref(), merged[&b].colors), (Some(&theirs), Some(colors)), "colors kept");
        assert!(merged[&TeamId("c".into())].logo.is_some() && merged[&TeamId("c".into())].colors.is_none());
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
                takeovers: Default::default(),
                takeover_art: Default::default(),
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
