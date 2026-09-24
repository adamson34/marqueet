//! Themes for the flat UI (ADR-0012): a [`Kit`] resolves a
//! [`marqueet_core::theme::Theme`] into the colors, faces and card chrome the
//! widgets draw with. Each style also draws its own game of the day and
//! scores list, in its own module.

pub mod ballpark;
pub mod broadcast;
pub mod varsity;

use marqueet_core::Rgb;
use marqueet_core::sports::TeamColors;
use marqueet_core::theme::{Palette, Style, Theme, contrast, ink_on};

use crate::ui::{Canvas, Face, TextStyle};
use crate::widgets::Card;

/// A theme, resolved for drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kit {
    pub style: Style,
    pub p: Palette,
    pub team_colors: bool,
}

impl Kit {
    pub fn new(theme: &Theme) -> Kit {
        Kit { style: theme.style, p: theme.palette, team_colors: theme.team_colors }
    }

    /// Card titles and painted labels.
    pub fn title_face(&self) -> Face {
        match self.style {
            Style::Broadcast => Face::Heavy,
            Style::Ballpark => Face::Stencil,
            Style::Varsity => Face::Collegiate,
        }
    }

    /// Team names in lists and tables.
    pub fn strong_face(&self) -> Face {
        match self.style {
            Style::Broadcast => Face::Heavy,
            Style::Ballpark => Face::Stencil,
            Style::Varsity => Face::SemiBold,
        }
    }

    /// Big numbers (temperature, scores in lists).
    pub fn number_face(&self) -> Face {
        match self.style {
            Style::Broadcast => Face::Heavy,
            Style::Ballpark => Face::Plate,
            Style::Varsity => Face::Collegiate,
        }
    }

    /// Titles and labels in capitals.
    pub fn caps(&self, text: &str) -> String {
        text.to_uppercase()
    }

    /// Corner radius of cards.
    pub fn radius(&self, s: f32) -> f32 {
        match self.style {
            Style::Broadcast | Style::Ballpark => 0.0,
            Style::Varsity => 8.0 * s,
        }
    }

    /// Draws a card's background in the style's chrome.
    pub fn panel(&self, canvas: &mut Canvas, card: Card, s: f32) {
        let Card { x, y, w, h } = card;
        match self.style {
            Style::Broadcast => canvas.fill_rect(x as i32, y as i32, w as i32, h as i32, self.p.panel),
            Style::Ballpark => {
                // A painted board in a darker frame.
                let frame = 8.0 * s;
                canvas.fill_rect(x as i32, y as i32, w as i32, h as i32, self.p.panel.scale(0.72));
                canvas.fill_rect(
                    (x + frame) as i32,
                    (y + frame) as i32,
                    (w - 2.0 * frame) as i32,
                    (h - 2.0 * frame) as i32,
                    self.p.panel,
                );
            }
            Style::Varsity => {
                // A rounded card with a stitched-looking inner trim line.
                let r = self.radius(s);
                canvas.fill_round_rect(x, y, w, h, r, self.p.panel);
                let (inset, line) = (8.0 * s, (2.0 * s).max(1.0));
                canvas.fill_round_rect(x + inset, y + inset, w - 2.0 * inset, h - 2.0 * inset, r * 0.6, self.p.rule());
                canvas.fill_round_rect(
                    x + inset + line,
                    y + inset + line,
                    w - 2.0 * (inset + line),
                    h - 2.0 * (inset + line),
                    r * 0.5,
                    self.p.panel,
                );
            }
        }
    }

    /// A card's title at the top left.
    pub fn title(&self, canvas: &mut Canvas, fonts: &mut crate::ui::Fonts, card: Card, s: f32, text: &str) {
        let (x, y) = (card.x + 40.0 * s, card.y + 62.0 * s);
        let text = self.caps(text);
        match self.style {
            Style::Broadcast => {
                // A short accent bar, then a slanted headline.
                canvas.fill_polygon(
                    &[(x, y - 30.0 * s), (x + 12.0 * s, y - 30.0 * s), (x + 6.0 * s, y), (x - 6.0 * s, y)],
                    self.p.accent,
                );
                let style = TextStyle::new(Face::Heavy, 38.0 * s, self.p.text);
                canvas.text(fonts, x + 22.0 * s, y, style, &text);
            }
            Style::Ballpark => {
                let style = TextStyle::new(Face::Stencil, 38.0 * s, self.p.accent).tracking(1.0 * s);
                canvas.text(fonts, x, y + 4.0 * s, style, &text);
            }
            Style::Varsity => {
                let style = TextStyle::new(Face::Collegiate, 32.0 * s, self.p.text).tracking(1.5 * s);
                canvas.text(fonts, x, y + 4.0 * s, style, &text);
            }
        }
    }

    /// Background and title together.
    pub fn card(&self, canvas: &mut Canvas, fonts: &mut crate::ui::Fonts, card: Card, s: f32, title: &str) {
        self.panel(canvas, card, s);
        self.title(canvas, fonts, card, s, title);
    }

    /// Fills for two teams side by side, or the palette's own when team
    /// colors are off.
    pub fn team_fills(&self, away: TeamColors, home: TeamColors) -> (Rgb, Rgb) {
        if self.team_colors {
            marqueet_core::theme::team_fills(away, home, self.p.ground)
        } else {
            (self.p.plate, self.p.chip())
        }
    }

    /// A single team's fill (lists), readable on `on`.
    pub fn team_fill(&self, c: TeamColors, on: Rgb) -> Rgb {
        if !self.team_colors {
            return self.p.chip();
        }
        match c.secondary {
            Some(sec) if contrast(c.primary, on) < 1.25 && contrast(sec, on) > contrast(c.primary, on) => sec,
            _ => c.primary,
        }
    }
}

/// Text color for a fill.
pub fn ink(fill: Rgb) -> Rgb {
    ink_on(fill)
}

/// A dimmed version of `fill` for a team that lost.
pub fn faded(fill: Rgb, ground: Rgb) -> Rgb {
    fill.mix(ground, 0.55)
}

/// Splits "NFL  |  Q3 4:31" into ("NFL", "Q3 4:31"); a status without a
/// league comes back as ("", status).
pub fn split_status(status: &str) -> (&str, &str) {
    match status.split_once('|') {
        Some((league, rest)) => (league.trim(), rest.trim()),
        None => ("", status.trim()),
    }
}

/// Splits "Q3 4:31" into ("Q3", "4:31") and "FINAL" into ("FINAL", "").
pub fn split_clock(status: &str) -> (&str, &str) {
    status.split_once(' ').map_or((status, ""), |(a, b)| (a.trim(), b.trim()))
}

/// The abbreviation of the team with the ball or the power play, from
/// chips like "BUF ball" (drawn as a marker instead of a chip).
pub fn possession(chips: &[String]) -> Option<&str> {
    chips.iter().find_map(|c| c.strip_suffix(" ball"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_split_into_parts() {
        assert_eq!(split_status("NFL  |  Q3 4:31"), ("NFL", "Q3 4:31"));
        assert_eq!(split_status("FINAL"), ("", "FINAL"));
        assert_eq!(split_clock("Q3 4:31"), ("Q3", "4:31"));
        assert_eq!(split_clock("SUN 1:00 PM"), ("SUN", "1:00 PM"));
        assert_eq!(split_clock("HALF"), ("HALF", ""));
        assert_eq!(possession(&["BUF ball".into(), "2nd & 6".into()]), Some("BUF"));
        assert_eq!(possession(&["1 out".into()]), None);
    }

    #[test]
    fn team_colors_off_uses_the_palette() {
        let mut theme = Theme::default();
        let c = TeamColors { primary: Rgb::RED, secondary: None };
        assert_eq!(Kit::new(&theme).team_fills(c, c).0, Rgb::RED);
        theme.team_colors = false;
        let kit = Kit::new(&theme);
        assert_eq!(kit.team_fills(c, c), (kit.p.plate, kit.p.chip()));
    }
}
