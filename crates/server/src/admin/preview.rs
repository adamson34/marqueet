//! Small SVG sketches of each theme for the admin page: the header, the LED
//! ticker, the crawl and two widgets, in the theme's own colors. Served as
//! images so the page's strict CSP (no inline styles) still holds.

use std::fmt::Write;

use marqueet_core::Rgb;
use marqueet_core::sports::TeamColors;
use marqueet_core::theme::{Style, Theme, ink_on, team_fills};

/// Sample teams for the sketch: generic red and blue, not any real club.
const AWAY: TeamColors =
    TeamColors { primary: Rgb::new(0xc8, 0x10, 0x2e), secondary: Some(Rgb::new(0xff, 0xb6, 0x12)) };
const HOME: TeamColors =
    TeamColors { primary: Rgb::new(0x1d, 0x42, 0x8a), secondary: Some(Rgb::new(0xff, 0xff, 0xff)) };

struct Svg(String);

impl Svg {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgb) {
        let _ = write!(self.0, r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"/>"#);
    }
    fn round(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: Rgb) {
        let _ = write!(self.0, r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{fill}"/>"#);
    }
    fn poly(&mut self, points: &[(f32, f32)], fill: Rgb) {
        let pts: Vec<String> = points.iter().map(|(x, y)| format!("{x},{y}")).collect();
        let _ = write!(self.0, r#"<polygon points="{}" fill="{fill}"/>"#, pts.join(" "));
    }
    fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Rgb) {
        let _ = write!(self.0, r#"<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}"/>"#);
    }
    /// A line of "text" as a bar (the sketch has no real words).
    fn bar(&mut self, x: f32, y: f32, w: f32, fill: Rgb) {
        self.round(x, y, w, 3.0, 1.0, fill);
    }
    #[allow(clippy::too_many_arguments)]
    fn number(&mut self, x: f32, y: f32, size: f32, text: &str, fill: Rgb, stroke: Option<Rgb>, font: &str) {
        let stroke = stroke
            .map_or(String::new(), |s| format!(r#" stroke="{s}" stroke-width="{}" paint-order="stroke""#, size * 0.08));
        let _ = write!(
            self.0,
            r#"<text x="{x}" y="{y}" font-size="{size}" font-family="{font}" font-weight="800" text-anchor="middle" fill="{fill}"{stroke}>{text}</text>"#
        );
    }
}

/// A 320x180 SVG sketch of `theme`.
pub fn svg(theme: &Theme) -> String {
    let p = &theme.palette;
    let mut s = Svg(String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 180" width="320" height="180"><defs><pattern id="d" width="4" height="4" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="1.1" fill="#2a1d05"/></pattern><pattern id="l" width="4" height="4" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="1.3" fill="#ffaa00"/></pattern></defs>"##,
    ));
    // Header and LED ticker: the same in every theme.
    s.rect(0.0, 0.0, 320.0, 12.0, Rgb::new(0x0a, 0x0a, 0x0c));
    s.round(6.0, 3.0, 16.0, 6.0, 1.5, Rgb::new(0xc0, 0x39, 0x2b));
    s.0.push_str(r##"<rect x="0" y="12" width="320" height="46" fill="#070707"/><rect x="0" y="12" width="320" height="46" fill="url(#d)"/><rect x="16" y="20" width="36" height="28" fill="url(#l)"/><rect x="64" y="20" width="24" height="28" fill="url(#l)"/><rect x="176" y="20" width="44" height="28" fill="url(#l)"/><rect x="236" y="20" width="24" height="28" fill="url(#l)"/>"##);

    // The crawl.
    let (cy, ch) = (58.0, 14.0);
    s.rect(0.0, cy, 320.0, ch, p.strip);
    match theme.style {
        Style::Broadcast => s.poly(&[(0.0, cy), (46.0, cy), (42.0, cy + ch), (0.0, cy + ch)], p.strip_text),
        Style::Ballpark => s.rect(0.0, cy, 44.0, ch, p.strip),
        Style::Varsity => s.rect(0.0, cy, 44.0, ch, p.strip_text),
    }
    let tag = if theme.style == Style::Ballpark { p.accent } else { p.strip };
    s.bar(8.0, cy + 5.5, 26.0, tag);
    for x in [56.0, 150.0, 244.0] {
        let league = if theme.style == Style::Ballpark { p.accent } else { p.strip_text };
        s.bar(x, cy + 5.5, 12.0, league);
        s.bar(x + 16.0, cy + 5.5, 34.0, p.strip_text);
        s.bar(x + 54.0, cy + 5.5, 22.0, p.strip_text.mix(p.strip, 0.42));
    }

    // The widget area: the game of the day and a scores list.
    s.rect(0.0, 72.0, 320.0, 108.0, p.ground);
    let (fa, fh) =
        if theme.team_colors { team_fills(AWAY, HOME, p.ground) } else { (p.plate, p.panel.mix(p.text, 0.08)) };
    let (gx, gy, gw, gh) = (8.0, 78.0, 190.0, 96.0);
    let (lx, lw) = (206.0, 106.0);
    match theme.style {
        Style::Broadcast => {
            s.rect(gx, gy, gw, gh, p.panel);
            let (mid, sh) = (gx + gw / 2.0, 54.0);
            s.poly(&[(gx, gy), (mid + 6.0, gy), (mid - 6.0, gy + sh), (gx, gy + sh)], fa);
            s.poly(&[(mid + 6.0, gy), (gx + gw, gy), (gx + gw, gy + sh), (mid - 6.0, gy + sh)], fh);
            s.bar(gx + 8.0, gy + 24.0, 30.0, ink_on(fa));
            s.bar(gx + gw - 38.0, gy + 24.0, 30.0, ink_on(fh));
            s.rect(mid - 34.0, gy + 12.0, 68.0, 30.0, p.plate);
            s.number(mid - 20.0, gy + 34.0, 18.0, "17", p.text, None, "Arial Narrow, sans-serif");
            s.number(mid + 20.0, gy + 34.0, 18.0, "21", p.text, None, "Arial Narrow, sans-serif");
            s.bar(mid - 4.0, gy + 30.0, 8.0, p.accent);
            s.rect(gx, gy + sh, gw, 12.0, p.plate);
            s.bar(gx + 8.0, gy + sh + 4.5, 24.0, p.accent);
            s.bar(gx + 38.0, gy + sh + 4.5, 30.0, p.text);
            for r in 0..2 {
                s.bar(gx + 8.0, gy + sh + 20.0 + r as f32 * 10.0, gw - 16.0, p.panel.mix(p.text, 0.2));
            }
            for (i, tone) in
                [p.live, p.accent, p.panel.mix(p.text, 0.08), p.live, p.panel.mix(p.text, 0.08)].into_iter().enumerate()
            {
                let y = gy + i as f32 * 19.6;
                s.rect(lx, y, lw - 26.0, 17.0, p.panel);
                s.rect(lx, y, 2.5, 8.5, fa);
                s.rect(lx, y + 8.5, 2.5, 8.5, fh);
                s.bar(lx + 6.0, y + 3.0, 12.0, p.text);
                s.bar(lx + 6.0, y + 11.0, 12.0, p.text);
                s.rect(lx + lw - 26.0, y, 26.0, 17.0, tone);
            }
        }
        Style::Ballpark => {
            for (x, w) in [(gx, gw), (lx, lw)] {
                s.rect(x, gy, w, gh, p.panel.scale(0.72));
                s.rect(x + 2.0, gy + 2.0, w - 4.0, gh - 4.0, p.panel);
            }
            s.bar(gx + 8.0, gy + 8.0, 40.0, p.accent);
            for r in 0..2 {
                let y = gy + 18.0 + r as f32 * 24.0;
                s.bar(gx + 8.0, y + 8.0, 34.0, p.text);
                for c in 0..5 {
                    let x = gx + 80.0 + c as f32 * 21.0 + if c == 4 { 6.0 } else { 0.0 };
                    s.rect(x, y, 18.0, 20.0, p.plate);
                    if c < 3 || c == 4 {
                        s.bar(x + 5.0, y + 8.5, 8.0, if c == 4 { p.accent } else { p.text });
                    }
                }
            }
            for c in 0..3 {
                s.rect(gx + 8.0 + c as f32 * 30.0, gy + 70.0, 26.0, 16.0, p.plate);
            }
            s.bar(lx + 8.0, gy + 8.0, 30.0, p.accent);
            for i in 0..5 {
                let y = gy + 20.0 + i as f32 * 14.0;
                for c in 0..3 {
                    s.rect(lx + 10.0 + c as f32 * 32.0, y, 22.0, 11.0, p.plate);
                }
            }
        }
        Style::Varsity => {
            s.round(gx, gy, gw, gh, 3.0, fh);
            s.round(gx, gy, gw / 2.0 + 3.0, gh, 3.0, fa);
            s.rect(gx + gw / 2.0 - 3.0, gy, 6.0, gh, fa);
            let serif = "Georgia, serif";
            s.number(
                gx + gw * 0.25,
                gy + 66.0,
                42.0,
                "17",
                ink_on(fa),
                Some(AWAY.secondary.unwrap_or(p.accent)),
                serif,
            );
            s.number(gx + gw * 0.75, gy + 66.0, 42.0, "21", ink_on(fh), Some(p.accent), serif);
            s.circle(gx + gw / 2.0, gy + 44.0, 17.0, p.text);
            s.circle(gx + gw / 2.0, gy + 44.0, 15.5, p.plate);
            s.circle(gx + gw / 2.0, gy + 44.0, 14.0, p.text);
            s.round(lx, gy, lw, gh, 4.0, p.panel);
            s.bar(lx + 8.0, gy + 8.0, 28.0, p.text);
            for i in 0..5 {
                let y = gy + 18.0 + i as f32 * 15.0;
                s.round(lx + 6.0, y, 34.0, 12.0, 2.0, fa);
                s.round(lx + 43.0, y, 34.0, 12.0, 2.0, fh);
                s.bar(lx + 84.0, y + 4.5, 16.0, if i < 2 { p.live } else { p.text });
            }
        }
    }
    s.0.push_str("</svg>");
    s.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sketches_use_the_theme_colors() {
        for style in Style::ALL {
            let theme = Theme::preset(style);
            let svg = svg(&theme);
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{style:?}");
            for color in [theme.palette.ground, theme.palette.strip, theme.palette.accent] {
                assert!(svg.contains(&color.to_string()), "{style:?} has {color}");
            }
        }
    }

    #[test]
    fn team_colors_off_keeps_to_the_palette() {
        let mut theme = Theme::preset(Style::Broadcast);
        assert!(svg(&theme).contains(&AWAY.primary.to_string()));
        theme.team_colors = false;
        assert!(!svg(&theme).contains(&AWAY.primary.to_string()));
    }
}
