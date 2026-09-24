//! Varsity: jersey lettering. Each team's score is set like the number on
//! its jersey, in two-color tackle twill; other games are sleeve patches.

use marqueet_core::Rgb;
use marqueet_core::sports::TeamColors;
use marqueet_core::theme::contrast;
use marqueet_core::widgets::{GameOfTheDay, Scores, Tone};

use super::{Kit, faded, ink, split_clock, split_status};
use crate::ui::{Align, Canvas, Face, Fonts, Paint, TextStyle};
use crate::widgets::Card;

fn circle(canvas: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgb) {
    canvas.fill_round_rect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, color);
}

/// The twill outline: the team's other color when it shows against both
/// the jersey and the number, else the theme's highlight, else white or
/// black, whichever stands out from the number.
fn trim(kit: &Kit, colors: TeamColors, jersey: Rgb, number: Rgb) -> Rgb {
    let ok = |c: Rgb| contrast(c, jersey) >= 1.6 && contrast(c, number) >= 1.6;
    let fallback = if contrast(Rgb::WHITE, number) > contrast(Rgb::BLACK, number) { Rgb::WHITE } else { Rgb::BLACK };
    [colors.secondary.filter(|_| kit.team_colors), Some(kit.p.accent)]
        .into_iter()
        .flatten()
        .find(|&c| ok(c))
        .unwrap_or(fallback)
}

/// Athletic mesh: a faint grid of dots over the jersey.
fn mesh(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32, s: f32) {
    let step = (10.0 * s).max(4.0);
    let dot = (2.0 * s).max(1.0) as i32;
    let mut yy = y + step / 2.0;
    while yy < y + h {
        let mut xx = x + step / 2.0;
        while xx < x + w {
            canvas.fill_rect(xx as i32, yy as i32, dot, dot, Paint::with_alpha(Rgb::BLACK, 0.12));
            xx += step;
        }
        yy += step;
    }
}

pub fn game_of_the_day(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, g: &GameOfTheDay) {
    let Card { x, y, w, h } = card;
    let p = &kit.p;
    let r = kit.radius(s);
    let half = w / 2.0;
    let (fa, fh) = kit.team_fills(g.away.colors, g.home.colors);
    let fa = if g.away.lost { faded(fa, p.ground) } else { fa };
    let fh = if g.home.lost { faded(fh, p.ground) } else { fh };
    canvas.fill_round_rect(x, y, w, h, r, fh);
    canvas.fill_round_rect(x, y, half, h, r, fa);
    canvas.fill_rect((x + half - r) as i32, y as i32, r.ceil() as i32 + 1, h as i32, fa);
    mesh(canvas, x, y, w, h, s);

    let scored = g.away.score.is_some() || g.home.score.is_some();
    let patch_r = (h * 0.16).min(96.0 * s);
    for (i, (side, fill)) in [(&g.away, fa), (&g.home, fh)].into_iter().enumerate() {
        let cx = x + half * (i as f32 + 0.5) + if i == 0 { -patch_r * 0.35 } else { patch_r * 0.35 };
        let number = ink(fill);
        let room = half - patch_r * 1.3 - 40.0 * s;
        let name = if side.name.is_empty() { side.abbr.to_uppercase() } else { side.name.to_uppercase() };
        // A logo someone added sits in front of the nameplate, centered with it.
        let logo = kit.logo(side.logo.as_ref());
        let logo_size = if logo.is_some() { 64.0 * s } else { 0.0 };
        let gap = if logo.is_some() { 14.0 * s } else { 0.0 };
        let size = fonts.fit(Face::Collegiate, 54.0 * s, 4.0 * s, &name, room - logo_size - gap);
        let name_w = fonts.measure(Face::Collegiate, size, 4.0 * s, &name);
        let start = cx - (logo_size + gap + name_w) / 2.0;
        if let Some(logo) = logo {
            let top = y + h * 0.2 - fonts.cap_height(Face::Collegiate, size) / 2.0 - logo_size / 2.0;
            canvas.draw_logo(logo, start, top, logo_size, Align::Center);
        }
        let plate = TextStyle::new(Face::Collegiate, size, number).tracking(4.0 * s);
        canvas.text(fonts, start + logo_size + gap, y + h * 0.2, plate, &name);

        // The number: the score, or the abbreviation before kickoff.
        let text = if scored { side.score.unwrap_or(0).to_string() } else { side.abbr.clone() };
        let size = fonts.fit(Face::Collegiate, h * 0.5, 0.0, &text, room);
        let outline = size * 0.035;
        let style = TextStyle::new(Face::Collegiate, size, number).align(Align::Center);
        let trim = trim(kit, side.colors, fill, number);
        canvas.text_outlined(fonts, (cx, y + h * 0.7), style, (trim, outline), &text);

        let detail =
            TextStyle::new(Face::SemiBold, 28.0 * s, number.mix(fill, 0.2)).tracking(2.0 * s).align(Align::Center);
        canvas.text(fonts, cx, y + h * 0.84, detail, &side.detail.to_uppercase());
    }

    // The center patch: league, period and clock.
    let (cx, cy) = (x + half, y + h * 0.46);
    circle(canvas, cx, cy, patch_r + 12.0 * s, p.text);
    circle(canvas, cx, cy, patch_r + 8.0 * s, p.plate);
    circle(canvas, cx, cy, patch_r, p.text);
    let (league, status) = split_status(&g.status);
    let (period, clock) = split_clock(status);
    let dark = p.plate;
    let small = TextStyle::new(Face::SemiBold, 22.0 * s, dark.mix(p.text, 0.45)).tracking(2.0 * s).align(Align::Center);
    canvas.text(fonts, cx, cy - patch_r * 0.42, small, league);
    let period = period.to_uppercase();
    let size = fonts.fit(Face::Collegiate, 50.0 * s, 0.0, &period, patch_r * 1.6);
    let py = if clock.is_empty() { cy + size * 0.35 } else { cy + size * 0.2 };
    canvas.text(fonts, cx, py, TextStyle::new(Face::Collegiate, size, dark).align(Align::Center), &period);
    if !clock.is_empty() {
        let size = fonts.fit(Face::SemiBold, 36.0 * s, 0.0, clock, patch_r * 1.5);
        canvas.text(
            fonts,
            cx,
            cy + patch_r * 0.62,
            TextStyle::new(Face::SemiBold, size, dark).align(Align::Center),
            clock,
        );
    }

    // The situation on a chalk pill along the bottom.
    let chips: Vec<String> = g.chips.iter().map(|c| c.to_uppercase()).collect();
    if chips.is_empty() {
        return;
    }
    let line = chips.join("  ·  ");
    let style = TextStyle::new(Face::SemiBold, 28.0 * s, dark).tracking(1.0 * s);
    let tw = fonts.measure(style.face, style.size, style.tracking, &line);
    let (pw, ph) = ((tw + 44.0 * s).min(w - 40.0 * s), 46.0 * s);
    let (px, py) = (x + (w - pw) / 2.0, y + h - ph - 18.0 * s);
    canvas.fill_round_rect(px, py, pw, ph, ph / 2.0, p.text);
    let base = py + ph / 2.0 + fonts.cap_height(style.face, style.size) / 2.0;
    canvas.text(fonts, x + w / 2.0, base, style.align(Align::Center), &line);
}

/// How many patch rows fit in a card of height `h` at scale `s`.
pub fn rows_that_fit(h: f32, s: f32) -> usize {
    ((h - 104.0 * s) / (72.0 * s)).floor().max(0.0) as usize
}

pub fn scores(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, sc: &Scores) {
    let Card { x, y, w, h } = card;
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, &sc.title);
    if sc.rows.is_empty() {
        let style = TextStyle::new(Face::Medium, 30.0 * s, p.muted).align(Align::Center);
        canvas.text(fonts, x + w / 2.0, y + h / 2.0, style, "No other games");
        return;
    }
    let (left, right) = (x + 32.0 * s, x + w - 32.0 * s);
    let n = sc.rows.len().min(rows_that_fit(h, s)).max(1);
    let top = y + 92.0 * s;
    let row_h = ((y + h - 26.0 * s - top) / n as f32).min(80.0 * s);
    let sh = row_h - 12.0 * s;
    let status_w = (right - left) * 0.22;
    let sw = (right - left - status_w - 20.0 * s) / 2.0;
    for (i, row) in sc.rows.iter().take(n).enumerate() {
        let ry = top + i as f32 * row_h;
        for (j, team) in [&row.away, &row.home].into_iter().enumerate() {
            let sx = left + j as f32 * (sw + 10.0 * s);
            let fill = kit.team_fill(team.colors, p.panel);
            let fill = if team.lost { faded(fill, p.panel) } else { fill };
            let number = ink(fill);
            let edge = trim(kit, team.colors, fill, number);
            let r = 6.0 * s;
            canvas.fill_round_rect(sx, ry, sw, sh, r, edge);
            let t = (3.0 * s).max(1.0);
            canvas.fill_round_rect(sx + t, ry + t, sw - 2.0 * t, sh - 2.0 * t, r - t, fill);
            let size = (sh * 0.56).min(38.0 * s);
            let base = ry + sh / 2.0 + fonts.cap_height(Face::Collegiate, size) / 2.0;
            let mut ax = sx + 16.0 * s;
            if let Some(logo) = kit.logo(team.logo.as_ref()) {
                let ls = sh * 0.7;
                ax += canvas.draw_logo(logo, sx + 8.0 * s, ry + (sh - ls) / 2.0, ls, Align::Center).max(ls);
            }
            canvas.text(fonts, ax, base, TextStyle::new(Face::Collegiate, size, number), &team.abbr);
            if let Some(score) = team.score {
                let style = TextStyle::new(Face::Collegiate, size * 1.1, number).align(Align::Right);
                canvas.text(fonts, sx + sw - 16.0 * s, base, style, &score.to_string());
            }
        }
        let cx = right - status_w / 2.0;
        let league = TextStyle::new(Face::SemiBold, 19.0 * s, p.muted).tracking(2.0 * s).align(Align::Center);
        canvas.text(fonts, cx, ry + sh * 0.38, league, &row.league);
        let color = match row.tone {
            Tone::Live => p.live,
            Tone::Break => p.accent,
            Tone::Final | Tone::Upcoming => p.text,
        };
        let status = row.status.to_uppercase();
        let size = fonts.fit(Face::SemiBold, 30.0 * s, 0.0, &status, status_w);
        canvas.text(
            fonts,
            cx,
            ry + sh * 0.84,
            TextStyle::new(Face::SemiBold, size, color).align(Align::Center),
            &status,
        );
    }
}
