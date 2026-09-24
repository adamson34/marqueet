//! Ballpark: a hand-operated scoreboard. Painted stencil lettering on a
//! framed green board, and every number on a black plate hung by hand.

use marqueet_core::Rgb;
use marqueet_core::widgets::{GameOfTheDay, Scores, Tone};

use super::{Kit, possession, split_clock, split_status};
use crate::ui::{Align, Canvas, Face, Fonts, Paint, TextStyle};
use crate::widgets::Card;

/// Inside the board's frame.
const PAD: f32 = 34.0;

/// A number plate with `text` centered on it (blank plates hang empty).
#[allow(clippy::too_many_arguments)]
fn plate(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, x: f32, y: f32, w: f32, h: f32, text: &str, color: Rgb) {
    let p = &kit.p;
    canvas.fill_rect(x as i32, y as i32, w as i32, h as i32, p.plate);
    // A lit top edge and a shadow below, so plates sit on the board.
    let edge = (h * 0.03).max(1.0);
    canvas.fill_rect(x as i32, y as i32, w as i32, edge as i32, p.plate.mix(Rgb::WHITE, 0.08));
    canvas.fill_rect(x as i32, (y + h) as i32, w as i32, (edge * 1.5) as i32, Paint::with_alpha(Rgb::BLACK, 0.35));
    if text.is_empty() || text == "–" {
        return;
    }
    let size = fonts.fit(Face::Plate, h * 0.78, 0.0, text, w - h * 0.2);
    let base = y + h / 2.0 + fonts.cap_height(Face::Plate, size) / 2.0;
    canvas.text(fonts, x + w / 2.0, base, TextStyle::new(Face::Plate, size, color).align(Align::Center), text);
}

/// A lit bulb (who has the ball).
fn bulb(canvas: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgb) {
    canvas.fill_round_rect(cx - r * 2.0, cy - r * 2.0, r * 4.0, r * 4.0, r * 2.0, Paint::with_alpha(color, 0.18));
    canvas.fill_round_rect(cx - r * 1.4, cy - r * 1.4, r * 2.8, r * 2.8, r * 1.4, Paint::with_alpha(color, 0.3));
    canvas.fill_round_rect(cx - r, cy - r, r * 2.0, r * 2.0, r, color);
}

fn painted(kit: &Kit, size: f32) -> TextStyle {
    TextStyle::new(Face::Stencil, size, kit.p.accent).tracking(size * 0.02)
}

pub fn game_of_the_day(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, g: &GameOfTheDay) {
    let Card { x, y, w, h } = card;
    let p = &kit.p;
    kit.panel(canvas, card, s);
    let (left, right) = (x + PAD * s, x + w - PAD * s);

    // Column layout: team names, then a plate per period, then the total.
    let name_w = (w * 0.27).max(160.0 * s);
    let n = g.columns.len().max(1);
    let gap = 12.0 * s;
    let total_gap = 26.0 * s;
    let region = right - (left + name_w);
    let plate_w = ((region - gap * (n - 1) as f32 - total_gap) / n as f32).min(104.0 * s);
    let plate_h = (h * 0.19).min(112.0 * s);
    // Right-aligned plates, with extra room before the total.
    let start = right - (n as f32 * plate_w + (n - 1) as f32 * gap + total_gap);
    let col_x = |i: usize| start + i as f32 * (plate_w + gap) + if i + 1 == n { total_gap } else { 0.0 };

    let head_base = y + PAD * s + 34.0 * s;
    canvas.text(fonts, left, head_base, painted(kit, 34.0 * s), "GAME OF THE DAY");
    for (i, c) in g.columns.iter().enumerate() {
        let label = c.strip_prefix('Q').unwrap_or(c);
        let style = painted(kit, 30.0 * s).align(Align::Center);
        canvas.text(fonts, col_x(i) + plate_w / 2.0, head_base, style, label);
    }

    let ball = possession(&g.chips);
    let first = head_base + 22.0 * s;
    for (r, side) in [&g.away, &g.home].into_iter().enumerate() {
        let ty = first + r as f32 * (plate_h + 16.0 * s);
        let label = if side.name.is_empty() { side.abbr.to_uppercase() } else { side.name.to_uppercase() };
        let color = if side.lost { p.muted } else { p.text };
        let size = fonts.fit(Face::Stencil, 64.0 * s, 0.0, &label, name_w - 60.0 * s);
        let base = ty + plate_h / 2.0 + fonts.cap_height(Face::Stencil, size) / 2.0;
        let label_w = canvas.text(fonts, left, base, TextStyle::new(Face::Stencil, size, color), &label);
        if ball == Some(side.abbr.as_str()) {
            let r = 11.0 * s;
            bulb(canvas, left + label_w + 26.0 * s, ty + plate_h / 2.0, r, p.accent.mix(Rgb::WHITE, 0.2));
        }
        let cells = g.rows.get(r).map(|(_, c)| c.as_slice()).unwrap_or_default();
        for (i, cell) in cells.iter().enumerate().take(n) {
            let total = i + 1 == cells.len();
            let color = if total { p.accent } else { p.text };
            plate(canvas, fonts, kit, col_x(i), ty, plate_w, plate_h, cell, color);
        }
    }

    // Facts along the bottom: the league, the clock and the situation.
    let fy = first + 2.0 * (plate_h + 16.0 * s) + 26.0 * s;
    let fh = (plate_h * 0.78).min(y + h - PAD * s - fy);
    if fh < 40.0 * s {
        return;
    }
    let (league, status) = split_status(&g.status);
    let (a, b) = split_clock(status);
    let mut facts: Vec<String> = [a, b].into_iter().filter(|t| !t.is_empty()).map(str::to_uppercase).collect();
    facts.extend(g.chips.iter().filter(|c| !c.ends_with(" ball")).map(|c| c.to_uppercase()));
    let mut fx = left;
    if !league.is_empty() {
        let base = fy + fh / 2.0 + fonts.cap_height(Face::Stencil, 36.0 * s) / 2.0;
        fx += canvas.text(fonts, fx, base, painted(kit, 36.0 * s), league) + 24.0 * s;
    }
    for fact in facts {
        let size = fh * 0.72;
        let fw = fonts.measure(Face::Plate, size, 0.0, &fact) + fh * 0.5;
        if fx + fw > right {
            break;
        }
        plate(canvas, fonts, kit, fx, fy, fw, fh, &fact, p.text);
        fx += fw + 16.0 * s;
    }
}

/// How many board rows fit in a card of height `h` at scale `s`.
pub fn rows_that_fit(h: f32, s: f32) -> usize {
    ((h - 110.0 * s) / (70.0 * s)).floor().max(0.0) as usize
}

/// The out-of-town board: every game on its own row of plates.
pub fn scores(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, sc: &Scores) {
    let Card { x, y, w, h } = card;
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, &sc.title);
    if sc.rows.is_empty() {
        let style = TextStyle::new(Face::Stencil, 36.0 * s, p.muted).align(Align::Center);
        canvas.text(fonts, x + w / 2.0, y + h / 2.0, style, "NO OTHER GAMES");
        return;
    }
    let (left, right) = (x + PAD * s, x + w - PAD * s);
    let inner = right - left;
    let n = sc.rows.len().min(rows_that_fit(h, s)).max(1);
    let top = y + 94.0 * s;
    let row_h = ((y + h - PAD * s - top) / n as f32).min(78.0 * s);
    let ph = row_h - 12.0 * s;
    // league | away | plate | home | plate | status
    let cols = [0.13, 0.17, 0.13, 0.17, 0.13];
    let status_x = left + inner * cols.iter().sum::<f32>() + 8.0 * s;
    for (i, row) in sc.rows.iter().take(n).enumerate() {
        let ry = top + i as f32 * row_h;
        let mut cx = left;
        let base = |fonts: &Fonts, size: f32| ry + ph / 2.0 + fonts.cap_height(Face::Stencil, size) / 2.0;
        let league_size = fonts.fit(Face::Stencil, 26.0 * s, 0.0, &row.league, inner * cols[0] - 8.0 * s);
        canvas.text(fonts, cx, base(fonts, league_size), painted(kit, league_size), &row.league);
        cx += inner * cols[0];
        for (team, (name_col, plate_col)) in
            [&row.away, &row.home].into_iter().zip([(cols[1], cols[2]), (cols[3], cols[4])])
        {
            let color = if team.lost { p.muted } else { p.text };
            let size = (ph * 0.8).min(44.0 * s);
            let style = TextStyle::new(Face::Stencil, size, color).align(Align::Center);
            canvas.text(fonts, cx + inner * name_col / 2.0, base(fonts, size), style, &team.abbr);
            cx += inner * name_col;
            let score = team.score.map(|v| v.to_string()).unwrap_or_default();
            plate(canvas, fonts, kit, cx, ry, inner * plate_col - 8.0 * s, ph, &score, color);
            cx += inner * plate_col;
        }
        let color = match row.tone {
            Tone::Live | Tone::Break => p.accent,
            Tone::Final | Tone::Upcoming => p.muted,
        };
        plate(canvas, fonts, kit, status_x, ry, right - status_x, ph, &row.status.to_uppercase(), color);
    }
}
