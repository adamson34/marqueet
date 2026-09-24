//! The header bar above the ticker: LIVE badge, league filter, LED clock,
//! in the look's colors and lettering (ADR-0012).

use marqueet_core::Rgb;
use marqueet_core::theme::{Style, Theme, ink_on};

use crate::theme::Kit;
use crate::ui::{Canvas, Face, Fonts, Paint, TextStyle};

/// The header's colors in a look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub bg: Rgb,
    pub line: Rgb,
    pub live: Rgb,
    pub idle: Rgb,
    pub label: Rgb,
    pub clock: Rgb,
}

pub fn colors(theme: &Theme) -> Colors {
    let p = &theme.palette;
    let bg = match theme.style {
        Style::Broadcast | Style::Varsity => p.plate,
        // A darker painted green, the board's top rail.
        Style::Ballpark => p.strip.scale(0.7),
    };
    Colors {
        bg,
        line: bg.mix(p.text, 0.12),
        live: p.live,
        idle: bg.mix(p.text, 0.18),
        label: p.text.mix(p.muted, 0.4),
        clock: p.accent,
    }
}

/// Everything the header shows; redraw only when this changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderState {
    pub live_games: u32,
    /// League filter, e.g. "ALL LEAGUES".
    pub label: String,
    /// Local time, e.g. "3:40 PM".
    pub time: String,
}

pub fn draw(canvas: &mut Canvas, fonts: &mut Fonts, state: &HeaderState, theme: &Theme) {
    let c = colors(theme);
    let kit = Kit::new(theme);
    canvas.clear();
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    let s = h / 72.0;
    canvas.fill_rect(0, 0, canvas.width as i32, canvas.height as i32, c.bg);
    canvas.fill_rect(0, (h - s.max(1.0)) as i32, canvas.width as i32, s.max(1.0) as i32, c.line);

    // Badge: red LIVE while games are on, grey otherwise.
    let live = state.live_games > 0;
    let badge_text = if live { "LIVE" } else { "NO LIVE GAMES" };
    let size = 30.0 * s;
    let text_w = fonts.measure(Face::SemiBold, size, 2.0 * s, badge_text);
    let dot = if live { 18.0 * s } else { 0.0 };
    let (bx, by, bh) = (40.0 * s, 16.0 * s, 40.0 * s);
    let bw = text_w + dot + 28.0 * s;
    let radius = if theme.style == Style::Varsity { 6.0 * s } else { 2.0 * s };
    let badge = if live { c.live } else { c.idle };
    canvas.fill_round_rect(bx, by, bw, bh, radius, badge);
    if live {
        canvas.fill_round_rect(
            bx + 14.0 * s,
            by + bh / 2.0 - 6.0 * s,
            12.0 * s,
            12.0 * s,
            6.0 * s,
            Paint::with_alpha(ink_on(badge), 0.92),
        );
    }
    let baseline = by + bh / 2.0 + fonts.cap_height(Face::SemiBold, size) / 2.0;
    let style = TextStyle::new(Face::SemiBold, size, ink_on(badge)).tracking(2.0 * s);
    canvas.text(fonts, bx + 14.0 * s + dot, baseline, style, badge_text);

    // The league filter in the look's lettering.
    let face = kit.title_face();
    let (size, track) = match theme.style {
        Style::Broadcast => (30.0 * s, 1.0 * s),
        Style::Ballpark => (32.0 * s, 1.5 * s),
        Style::Varsity => (24.0 * s, 2.5 * s),
    };
    let label = TextStyle::new(face, size, c.label).tracking(track);
    let label_base = by + bh / 2.0 + fonts.cap_height(face, size) / 2.0;
    canvas.text(fonts, bx + bw + 26.0 * s, label_base, label, &state.label);

    // LED clock on the right.
    let px = 5.0 * s;
    let clock_w = Canvas::led_width(px, &state.time);
    let clock_top = h / 2.0 - 3.5 * px;
    canvas.led_text(w - 40.0 * s - clock_w, clock_top, px, c.clock, true, &state.time);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(live: u32) -> HeaderState {
        HeaderState { live_games: live, label: "ALL LEAGUES".into(), time: "3:40 PM".into() }
    }

    fn has(c: &Canvas, color: Rgb, x0: u32, x1: u32) -> bool {
        (x0..x1).any(|x| {
            (0..c.height).any(|y| {
                let p = c.pixel(x, y);
                p[3] == 255 && p[0] == color.r && p[1] == color.g && p[2] == color.b
            })
        })
    }

    #[test]
    fn live_badge_is_red_and_idle_badge_is_grey() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 72);
        for style in Style::ALL {
            let theme = Theme::preset(style);
            let k = colors(&theme);
            draw(&mut c, &mut fonts, &state(3), &theme);
            assert!(has(&c, k.live, 40, 160), "{style:?}");
            assert!(has(&c, k.bg, 1000, 1100), "{style:?} background");
            draw(&mut c, &mut fonts, &state(0), &theme);
            assert!(!has(&c, k.live, 40, 160));
            assert!(has(&c, k.idle, 40, 160));
        }
    }

    #[test]
    fn clock_sits_at_the_right_edge() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1366, 51);
        let theme = Theme::default();
        let clock = colors(&theme).clock;
        draw(&mut c, &mut fonts, &state(1), &theme);
        let amber: Vec<u32> =
            (0..1366).filter(|&x| (0..51).any(|y| c.pixel(x, y) == [clock.r, clock.g, clock.b, 255])).collect();
        assert!(*amber.first().unwrap() > 1000 && *amber.last().unwrap() < 1366 - 20, "{amber:?}");
    }
}
