//! Display themes: how the flat UI under the ticker looks (ADR-0012).
//!
//! A theme is a [`Style`] (the shapes and fonts: broadcast graphics, a
//! ballpark scoreboard, varsity lettering) plus a [`Palette`] of colors by
//! role. Each style ships with a preset palette; people can change any color
//! to build their own. Pure data and color math; the display draws it.

use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::sports::TeamColors;

/// The shapes and fonts of the widget area and crawl.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// TV sports graphics: team-color slabs, score bugs, a white bottom line.
    #[default]
    Broadcast,
    /// A hand-operated ballpark scoreboard: painted panels, number plates.
    Ballpark,
    /// Jersey lettering: scores as two-color tackle-twill numbers.
    Varsity,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Broadcast, Style::Ballpark, Style::Varsity];

    pub fn id(self) -> &'static str {
        match self {
            Style::Broadcast => "broadcast",
            Style::Ballpark => "ballpark",
            Style::Varsity => "varsity",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Style::Broadcast => "Broadcast",
            Style::Ballpark => "Ballpark",
            Style::Varsity => "Varsity",
        }
    }

    /// One line for the admin page.
    pub fn blurb(self) -> &'static str {
        match self {
            Style::Broadcast => "Like the score graphics on TV. Works for any mix of sports, weather and news.",
            Style::Ballpark => "Like an old hand-turned scoreboard: painted green, big number plates.",
            Style::Varsity => "Scores stitched like jersey numbers in each team's colors.",
        }
    }

    pub fn from_id(id: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The style's own colors.
    pub fn palette(self) -> Palette {
        let c = |hex: u32| Rgb::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        match self {
            Style::Broadcast => Palette {
                ground: c(0x0c131b),
                panel: c(0x121c27),
                plate: c(0x05090d),
                text: c(0xeef2f6),
                muted: c(0x7f91a4),
                accent: c(0xffc72c),
                live: c(0xd7262e),
                strip: c(0xf4f6f8),
                strip_text: c(0x0c131b),
            },
            Style::Ballpark => Palette {
                ground: c(0x1d4b35),
                panel: c(0x1a4430),
                plate: c(0x141614),
                text: c(0xf3efe2),
                muted: c(0xb8c9bd),
                accent: c(0xf2c230),
                live: c(0xe0483c),
                strip: c(0x163a28),
                strip_text: c(0xf3efe2),
            },
            Style::Varsity => Palette {
                ground: c(0x1f1d1b),
                panel: c(0x2a2724),
                plate: c(0x141312),
                text: c(0xf1ede4),
                muted: c(0x9a9286),
                accent: c(0xf2b33d),
                live: c(0xff6b5f),
                strip: c(0xf1ede4),
                strip_text: c(0x1f1d1b),
            },
        }
    }
}

/// Theme colors by role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Palette {
    /// Behind everything in the widget area.
    pub ground: Rgb,
    /// Cards and boards.
    pub panel: Rgb,
    /// Score boxes, number plates, dark bars.
    pub plate: Rgb,
    /// Main text.
    pub text: Rgb,
    /// Labels and secondary text.
    pub muted: Rgb,
    /// Highlights: the clock, painted labels, the leader's points.
    pub accent: Rgb,
    /// Games in progress.
    pub live: Rgb,
    /// The crawl's background.
    pub strip: Rgb,
    /// The crawl's text.
    pub strip_text: Rgb,
}

impl Palette {
    /// Role names and labels, in the order the admin page shows them.
    pub const ROLES: [(&'static str, &'static str); 9] = [
        ("ground", "Background"),
        ("panel", "Cards"),
        ("plate", "Score boxes"),
        ("text", "Text"),
        ("muted", "Labels"),
        ("accent", "Highlight"),
        ("live", "Live"),
        ("strip", "Crawl"),
        ("strip_text", "Crawl text"),
    ];

    pub fn get(&self, role: &str) -> Option<Rgb> {
        Some(match role {
            "ground" => self.ground,
            "panel" => self.panel,
            "plate" => self.plate,
            "text" => self.text,
            "muted" => self.muted,
            "accent" => self.accent,
            "live" => self.live,
            "strip" => self.strip,
            "strip_text" => self.strip_text,
            _ => return None,
        })
    }

    /// Sets one role's color; false for an unknown role.
    pub fn set(&mut self, role: &str, color: Rgb) -> bool {
        let slot = match role {
            "ground" => &mut self.ground,
            "panel" => &mut self.panel,
            "plate" => &mut self.plate,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "accent" => &mut self.accent,
            "live" => &mut self.live,
            "strip" => &mut self.strip,
            "strip_text" => &mut self.strip_text,
            _ => return false,
        };
        *slot = color;
        true
    }

    /// Hairlines and dividers on a panel.
    pub fn rule(&self) -> Rgb {
        self.panel.mix(self.text, 0.12)
    }

    /// Small raised fills on a panel (chips, a highlighted row).
    pub fn chip(&self) -> Rgb {
        self.panel.mix(self.text, 0.08)
    }

    /// Between text and muted: numbers that aren't the headline.
    pub fn soft(&self) -> Rgb {
        self.text.mix(self.muted, 0.35)
    }

    /// Secondary crawl text (start times, channels).
    pub fn strip_muted(&self) -> Rgb {
        self.strip_text.mix(self.strip, 0.42)
    }

    /// Color pairs that are hard to read, as (what, on what) labels, for a
    /// warning on the admin page.
    pub fn hard_to_read(&self) -> Vec<(&'static str, &'static str)> {
        let pairs = [
            (self.text, self.panel, "Text", "Cards"),
            (self.text, self.ground, "Text", "Background"),
            (self.muted, self.panel, "Labels", "Cards"),
            (self.text, self.plate, "Text", "Score boxes"),
            (self.strip_text, self.strip, "Crawl text", "Crawl"),
        ];
        pairs.into_iter().filter(|(a, b, ..)| contrast(*a, *b) < 3.0).map(|(.., x, y)| (x, y)).collect()
    }
}

impl Default for Palette {
    fn default() -> Self {
        Style::default().palette()
    }
}

/// A style, its colors, and whether team colors may fill panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub style: Style,
    pub palette: Palette,
    /// Paint panels and numbers in the teams' own colors where the style
    /// uses them. Off: everything stays in the palette.
    pub team_colors: bool,
}

impl Theme {
    /// A style with its own colors.
    pub fn preset(style: Style) -> Theme {
        Theme { style, palette: style.palette(), team_colors: true }
    }

    /// True when the colors are the style's own (not customized).
    pub fn is_preset(&self) -> bool {
        self.palette == self.style.palette()
    }
}

impl Theme {
    /// A short code for sharing a theme: the style, the nine colors in
    /// [`Palette::ROLES`] order, and `plain` when team colors are off.
    /// `ballpark:1d4b35,1a4430,...`.
    pub fn code(&self) -> String {
        let colors: Vec<String> = Palette::ROLES
            .iter()
            .filter_map(|(role, _)| self.palette.get(role))
            .map(|c| c.to_string().trim_start_matches('#').to_owned())
            .collect();
        let plain = if self.team_colors { "" } else { ":plain" };
        format!("{}:{}{plain}", self.style.id(), colors.join(","))
    }

    /// Reads a [`Theme::code`]. Forgiving about spaces, case and `#`.
    pub fn from_code(code: &str) -> Result<Theme, String> {
        let bad = || "that theme code doesn't look right; copy the whole thing".to_owned();
        let code = code.trim().to_ascii_lowercase();
        let mut parts = code.split(':').map(str::trim);
        let style = parts.next().and_then(Style::from_id).ok_or_else(bad)?;
        let colors: Vec<&str> = parts.next().ok_or_else(bad)?.split(',').map(str::trim).collect();
        let team_colors = match parts.next() {
            None => true,
            Some("plain") => false,
            Some(_) => return Err(bad()),
        };
        if colors.len() != Palette::ROLES.len() || parts.next().is_some() {
            return Err(bad());
        }
        let mut palette = style.palette();
        for ((role, _), hex) in Palette::ROLES.iter().zip(colors) {
            let hex = hex.trim_start_matches('#');
            if hex.len() != 6 {
                return Err(bad());
            }
            let color = hex.parse::<Rgb>().map_err(|_| bad())?;
            palette.set(role, color);
        }
        Ok(Theme { style, palette, team_colors })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::preset(Style::default())
    }
}

/// WCAG contrast ratio between two colors, 1.0 to 21.0.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text color for `background`: white, like TV graphics, unless it would
/// be hard to read; then near-black.
pub fn ink_on(background: Rgb) -> Rgb {
    const DARK: Rgb = Rgb::new(0x11, 0x13, 0x16);
    let white = contrast(Rgb::WHITE, background);
    if white >= 3.0 || white >= contrast(DARK, background) { Rgb::WHITE } else { DARK }
}

/// Rough perceptual distance between two colors (0 to about 765).
fn distance(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y));
    // Weighted like the eye: green matters most, blue least.
    (d(a.r, b.r) * 3 + d(a.g, b.g) * 4 + d(a.b, b.b) * 2) / 3
}

/// Fill colors for two teams shown side by side on `ground`: each team's
/// primary, unless it disappears into the ground; and if both teams would
/// look alike (two reds), one switches to its other color.
pub fn team_fills(away: TeamColors, home: TeamColors, ground: Rgb) -> (Rgb, Rgb) {
    const ALIKE: u32 = 110;
    let pick = |c: TeamColors| match c.secondary {
        Some(s) if contrast(c.primary, ground) < 1.25 && contrast(s, ground) > contrast(c.primary, ground) => s,
        _ => c.primary,
    };
    let (a, h) = (pick(away), pick(home));
    if distance(a, h) >= ALIKE {
        return (a, h);
    }
    let usable = |s: Option<Rgb>, other: Rgb| s.filter(|&s| distance(s, other) >= ALIKE && contrast(s, ground) >= 1.25);
    if let Some(s) = usable(away.secondary, h) {
        (s, h)
    } else if let Some(s) = usable(home.secondary, a) {
        (a, s)
    } else {
        (a, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(hex: u32) -> Rgb {
        Rgb::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    fn colors(primary: u32, secondary: u32) -> TeamColors {
        TeamColors { primary: rgb(primary), secondary: Some(rgb(secondary)) }
    }

    #[test]
    fn styles_round_trip_by_id() {
        for s in Style::ALL {
            assert_eq!(Style::from_id(s.id()), Some(s));
        }
        assert_eq!(Style::from_id("disco"), None);
    }

    #[test]
    fn every_preset_is_readable() {
        for s in Style::ALL {
            assert_eq!(s.palette().hard_to_read(), Vec::<(&str, &str)>::new(), "{s:?}");
            assert!(Theme::preset(s).is_preset());
        }
    }

    #[test]
    fn palette_roles_get_and_set() {
        let mut p = Palette::default();
        for (role, _) in Palette::ROLES {
            assert!(p.get(role).is_some(), "{role}");
            assert!(p.set(role, Rgb::WHITE), "{role}");
            assert_eq!(p.get(role), Some(Rgb::WHITE));
        }
        assert!(!p.set("sparkle", Rgb::WHITE));
        assert_eq!(p.get("sparkle"), None);
    }

    #[test]
    fn customized_colors_are_not_the_preset() {
        let mut t = Theme::preset(Style::Ballpark);
        t.palette.accent = Rgb::RED;
        assert!(!t.is_preset());
    }

    #[test]
    fn warns_about_unreadable_custom_colors() {
        let mut p = Palette::default();
        p.text = p.panel.mix(Rgb::WHITE, 0.05);
        assert!(p.hard_to_read().contains(&("Text", "Cards")));
    }

    #[test]
    fn themes_load_from_partial_json() {
        let t: Theme = serde_json::from_str(r#"{"style":"varsity"}"#).unwrap();
        assert_eq!(t.style, Style::Varsity);
        assert!(t.team_colors);
        let json = serde_json::to_string(&Theme::preset(Style::Ballpark)).unwrap();
        assert!(json.contains(r##""accent":"#f2c230""##), "{json}");
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), Theme::preset(Style::Ballpark));
    }

    #[test]
    fn theme_codes_round_trip() {
        let mut t = Theme::preset(Style::Varsity);
        t.palette.accent = Rgb::new(0x12, 0xab, 0xef);
        t.team_colors = false;
        let code = t.code();
        assert!(code.starts_with("varsity:1f1d1b,") && code.ends_with(":plain"), "{code}");
        assert_eq!(Theme::from_code(&code), Ok(t));
        assert_eq!(Theme::from_code(&format!("  {}  ", code.to_uppercase())), Ok(t), "case and spaces");
        let preset = Theme::preset(Style::Ballpark);
        assert_eq!(Theme::from_code(&preset.code()), Ok(preset));
    }

    #[test]
    fn bad_theme_codes_are_refused() {
        for code in [
            "",
            "disco:000000",
            "broadcast",
            "broadcast:0c131b",
            "broadcast:zzzzzz,1,2,3,4,5,6,7,8",
            "broadcast:red,red,red,red,red,red,red,red,red",
        ] {
            assert!(Theme::from_code(code).is_err(), "{code:?}");
        }
        let nine = ["000000"; 9].join(",");
        assert!(Theme::from_code(&format!("broadcast:{nine}:sparkly")).is_err());
        assert!(Theme::from_code(&format!("broadcast:{nine}")).is_ok());
    }

    #[test]
    fn ink_contrasts_with_the_fill() {
        assert_eq!(ink_on(rgb(0x00338d)), Rgb::WHITE, "Buffalo football blue");
        assert_eq!(ink_on(rgb(0xef3e42)), Rgb::WHITE, "LA baseball red reads white, like on TV");
        assert_ne!(ink_on(rgb(0x6eceb2)), Rgb::WHITE, "Brooklyn basketball mint is too light");
        assert_ne!(ink_on(rgb(0xffcd00)), Rgb::WHITE, "Iowa City gold");
        assert!(contrast(Rgb::WHITE, Rgb::BLACK) > 20.0);
        assert!((contrast(Rgb::WHITE, Rgb::WHITE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn team_fills_keep_distinct_primaries() {
        let ground = Style::Broadcast.palette().ground;
        let (a, h) = team_fills(colors(0xe31837, 0xffb612), colors(0x00338d, 0xc60c30), ground);
        assert_eq!((a, h), (rgb(0xe31837), rgb(0x00338d)), "KC Kingdom red, Buffalo blue");
    }

    #[test]
    fn team_fills_split_teams_that_look_alike() {
        let ground = Style::Broadcast.palette().ground;
        // Tuscaloosa crimson and Athens red.
        let (a, h) = team_fills(colors(0x9e1b32, 0x828a8f), colors(0xba0c2f, 0x000000), ground);
        assert_eq!(a, rgb(0x828a8f), "Tuscaloosa switches to its gray");
        assert_eq!(h, rgb(0xba0c2f));
    }

    #[test]
    fn team_fills_avoid_colors_that_vanish_into_the_ground() {
        let ground = Style::Broadcast.palette().ground;
        let (a, _) = team_fills(colors(0x0b1620, 0xc4ced3), colors(0xc8102e, 0xffffff), ground);
        assert_eq!(a, rgb(0xc4ced3), "near-black primary falls back to its secondary");
    }
}
