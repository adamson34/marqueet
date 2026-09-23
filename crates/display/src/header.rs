//! The header bar above the ticker: LIVE badge, league filter, LED clock.

use marqueet_core::Rgb;

use crate::ui::{Canvas, Fonts, Paint, TextStyle, Weight};

pub const BG: Rgb = Rgb::new(0x0a, 0x0a, 0x0c);
const LINE: Rgb = Rgb::new(0x2a, 0x2b, 0x31);
const LIVE_RED: Rgb = Rgb::new(0xc0, 0x39, 0x2b);
const IDLE: Rgb = Rgb::new(0x3a, 0x3b, 0x42);
const LABEL: Rgb = Rgb::new(0xc8, 0xc9, 0xce);
const CLOCK: Rgb = Rgb::new(0xf2, 0xa9, 0x3b);

/// Everything the header shows; redraw only when this changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderState {
    pub live_games: u32,
    /// League filter, e.g. "ALL LEAGUES".
    pub label: String,
    /// Local time, e.g. "3:40 PM".
    pub time: String,
}

pub fn draw(canvas: &mut Canvas, fonts: &mut Fonts, state: &HeaderState) {
    canvas.clear();
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    let s = h / 72.0;
    canvas.fill_rect(0, 0, canvas.width as i32, canvas.height as i32, BG);
    canvas.fill_rect(0, (h - s.max(1.0)) as i32, canvas.width as i32, s.max(1.0) as i32, LINE);

    // Badge: red LIVE while games are on, grey otherwise.
    let live = state.live_games > 0;
    let badge_text = if live { "LIVE" } else { "NO LIVE GAMES" };
    let size = 30.0 * s;
    let text_w = fonts.measure(Weight::SemiBold, size, 2.0 * s, badge_text);
    let dot = if live { 18.0 * s } else { 0.0 };
    let (bx, by, bh) = (40.0 * s, 16.0 * s, 40.0 * s);
    let bw = text_w + dot + 28.0 * s;
    canvas.fill_round_rect(bx, by, bw, bh, 6.0 * s, if live { LIVE_RED } else { IDLE });
    if live {
        canvas.fill_round_rect(
            bx + 14.0 * s,
            by + bh / 2.0 - 6.0 * s,
            12.0 * s,
            12.0 * s,
            6.0 * s,
            Paint::with_alpha(Rgb::WHITE, 0.92),
        );
    }
    let baseline = by + bh / 2.0 + fonts.cap_height(Weight::SemiBold, size) / 2.0;
    let style = TextStyle::new(Weight::SemiBold, size, Rgb::WHITE).tracking(2.0 * s);
    canvas.text(fonts, bx + 14.0 * s + dot, baseline, style, badge_text);

    let label = TextStyle::new(Weight::Medium, 28.0 * s, LABEL).tracking(3.0 * s);
    canvas.text(fonts, bx + bw + 26.0 * s, baseline, label, &state.label);

    // LED clock on the right.
    let px = 5.0 * s;
    let clock_w = Canvas::led_width(px, &state.time);
    let clock_top = h / 2.0 - 3.5 * px;
    canvas.led_text(w - 40.0 * s - clock_w, clock_top, px, CLOCK, true, &state.time);
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
        draw(&mut c, &mut fonts, &state(3));
        assert!(has(&c, LIVE_RED, 40, 160));
        draw(&mut c, &mut fonts, &state(0));
        assert!(!has(&c, LIVE_RED, 40, 160));
        assert!(has(&c, IDLE, 40, 160));
    }

    #[test]
    fn clock_sits_at_the_right_edge() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1366, 51);
        draw(&mut c, &mut fonts, &state(1));
        let amber: Vec<u32> =
            (0..1366).filter(|&x| (0..51).any(|y| c.pixel(x, y) == [CLOCK.r, CLOCK.g, CLOCK.b, 255])).collect();
        assert!(*amber.first().unwrap() > 1000 && *amber.last().unwrap() < 1366 - 20, "{amber:?}");
    }
}
