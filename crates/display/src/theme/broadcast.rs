//! Broadcast: TV sports graphics. The game of the day is split into the two
//! teams' colors with a score box on the seam; other games are score bugs.

use marqueet_core::widgets::{GameOfTheDay, RowTeam, Scores, Side, Tone};

use super::{Kit, faded, ink, possession, split_clock, split_status};
use crate::ui::{Align, Canvas, Face, Fonts, TextStyle};
use crate::widgets::Card;

fn name(side: &Side) -> String {
    if side.name.is_empty() { side.abbr.to_uppercase() } else { side.name.to_uppercase() }
}

pub fn game_of_the_day(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, g: &GameOfTheDay) {
    let Card { x, y, w, h } = card;
    let p = &kit.p;
    kit.panel(canvas, card, s);

    // Two slabs in team colors, meeting at a slanted seam.
    let slab_h = (h * 0.56).round();
    let (mid, lean) = (x + w / 2.0, 34.0 * s);
    let (fa, fh) = kit.team_fills(g.away.colors, g.home.colors);
    let fa = if g.away.lost { faded(fa, p.ground) } else { fa };
    let fh = if g.home.lost { faded(fh, p.ground) } else { fh };
    canvas.fill_polygon(&[(x, y), (mid + lean, y), (mid - lean, y + slab_h), (x, y + slab_h)], fa);
    canvas.fill_polygon(&[(mid + lean, y), (x + w, y), (x + w, y + slab_h), (mid - lean, y + slab_h)], fh);

    // The score box on the seam.
    let scored = g.away.score.is_some() || g.home.score.is_some();
    let (bw, bh) = if scored { (400.0 * s, 180.0 * s) } else { (230.0 * s, 150.0 * s) };
    let (bx, by) = (mid - bw / 2.0, y + (slab_h - bh) / 2.0);
    canvas.fill_rect(bx as i32, by as i32, bw as i32, bh as i32, p.plate);
    let (league, status) = split_status(&g.status);
    let (period, clock) = split_clock(status);
    let center = if scored { bx + bw / 2.0 } else { mid };
    let period_style = TextStyle::new(Face::Heavy, 44.0 * s, p.text).align(Align::Center);
    let clock_style = TextStyle::new(Face::Heavy, 38.0 * s, p.accent).align(Align::Center);
    let (py, cy) = if clock.is_empty() { (by + bh * 0.62, 0.0) } else { (by + bh * 0.46, by + bh * 0.76) };
    let period_size = fonts.fit(Face::Heavy, 44.0 * s, 0.0, &period.to_uppercase(), bw * 0.3);
    canvas.text(fonts, center, py, TextStyle { size: period_size, ..period_style }, &period.to_uppercase());
    if !clock.is_empty() {
        let size = fonts.fit(Face::Heavy, 38.0 * s, 0.0, clock, if scored { bw * 0.32 } else { bw * 0.85 });
        canvas.text(fonts, center, cy, TextStyle { size, ..clock_style }, &clock.to_uppercase());
    }
    let ball = possession(&g.chips);
    if scored {
        let cap = fonts.cap_height(Face::Heavy, 120.0 * s);
        for (side, cx) in [(&g.away, bx + bw * 0.18), (&g.home, bx + bw * 0.82)] {
            let color = if side.lost { p.muted } else { p.text };
            let text = side.score.map_or_else(|| "0".to_owned(), |v| v.to_string());
            let size = fonts.fit(Face::Heavy, 120.0 * s, 0.0, &text, bw * 0.34);
            let style = TextStyle::new(Face::Heavy, size, color).align(Align::Center);
            canvas.text(fonts, cx, by + bh / 2.0 + cap / 2.0, style, &text);
            if ball == Some(side.abbr.as_str()) {
                // A bar under the score of the team with the ball.
                let bar_w = 64.0 * s;
                canvas.fill_rect(
                    (cx - bar_w / 2.0) as i32,
                    (by + bh - 18.0 * s) as i32,
                    bar_w as i32,
                    (7.0 * s) as i32,
                    p.accent,
                );
            }
        }
    }

    // Team names in the slabs, fitted to the room beside the box.
    let pad = 44.0 * s;
    let room = bx - 22.0 * s - (x + pad);
    for (side, fill, left) in [(&g.away, fa, true), (&g.home, fh, false)] {
        let ink = ink(fill);
        let text = name(side);
        // A logo someone added goes at the outer edge, the name inside it.
        let mut edge = if left { x + pad } else { x + w - pad };
        let mut room = room;
        if let Some(logo) = kit.logo(side.logo.as_ref()) {
            let size = (slab_h * 0.5).min(room * 0.4);
            let (lx, align) = if left { (edge, Align::Left) } else { (edge - size, Align::Right) };
            let drawn = canvas.draw_logo(logo, lx, y + (slab_h - size) / 2.0 - 12.0 * s, size, align) + 20.0 * s;
            edge += if left { drawn } else { -drawn };
            room -= drawn;
        }
        let size = fonts.fit(Face::Heavy, 100.0 * s, 0.0, &text, room);
        let align = if left { Align::Left } else { Align::Right };
        let base = y + slab_h * 0.56;
        canvas.text(fonts, edge, base, TextStyle::new(Face::Heavy, size, ink).align(align), &text);
        let detail = TextStyle::new(Face::SemiBold, 28.0 * s, ink.mix(fill, 0.2)).align(align);
        canvas.text(fonts, edge, base + 44.0 * s, detail, &side.detail.to_uppercase());
    }

    // The situation bar under the slabs.
    let (dy, dh) = (y + slab_h, 70.0 * s);
    canvas.fill_rect(x as i32, dy as i32, w as i32, dh as i32, p.plate);
    let base = dy + dh / 2.0 + fonts.cap_height(Face::Heavy, 34.0 * s) / 2.0;
    let mut cx = x + pad;
    let chips: Vec<String> = g.chips.iter().filter(|c| !c.ends_with(" ball")).map(|c| c.to_uppercase()).collect();
    let items = if chips.is_empty() { vec![status.to_uppercase()] } else { chips };
    for (i, item) in items.iter().enumerate() {
        let color = if i == 0 { p.accent } else { p.text };
        cx += canvas.text(fonts, cx, base, TextStyle::new(Face::Heavy, 34.0 * s, color), item) + 36.0 * s;
    }
    if !league.is_empty() {
        let style = TextStyle::new(Face::Heavy, 30.0 * s, p.muted).align(Align::Right);
        canvas.text(fonts, x + w - pad, base, style, league);
    }

    // Line score, when there's room.
    let top = dy + dh + 18.0 * s;
    let row = ((y + h - 12.0 * s - top) / 3.0).min(46.0 * s);
    if row < 30.0 * s || g.rows.is_empty() {
        return;
    }
    let cols_start = x + pad + 150.0 * s;
    let cols_end = x + w - pad - 10.0 * s;
    let n = g.columns.len().max(1) as f32;
    let col_x = |i: usize| cols_start + (cols_end - cols_start) * (i as f32 + 0.5) / n;
    let head = TextStyle::new(Face::SemiBold, 24.0 * s, p.muted).align(Align::Center);
    let first = top + row * 0.7;
    for (i, c) in g.columns.iter().enumerate() {
        canvas.text(fonts, col_x(i), first, head, c);
    }
    for (r, (team, cells)) in g.rows.iter().enumerate() {
        let ry = first + row * (r as f32 + 1.0);
        canvas.fill_rect(
            (x + pad) as i32,
            (ry - row * 0.78) as i32,
            (w - 2.0 * pad) as i32,
            (1.5 * s).max(1.0) as i32,
            p.rule(),
        );
        canvas.text(fonts, x + pad, ry, TextStyle::new(Face::Heavy, 32.0 * s, p.text), team);
        for (i, cell) in cells.iter().enumerate() {
            let total = i + 1 == cells.len();
            let style = if total {
                TextStyle::new(Face::Heavy, 36.0 * s, p.text)
            } else {
                TextStyle::new(Face::SemiBold, 30.0 * s, p.soft())
            };
            canvas.text(fonts, col_x(i), ry, style.align(Align::Center), cell);
        }
    }
}

/// How many score bugs fit in a card of height `h` at scale `s`.
pub fn rows_that_fit(h: f32, s: f32) -> usize {
    ((h + 10.0 * s) / (82.0 * s)).floor().max(0.0) as usize
}

pub fn scores(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, sc: &Scores) {
    let Card { x, y, w, h } = card;
    let p = &kit.p;
    if sc.rows.is_empty() {
        kit.card(canvas, fonts, card, s, &sc.title);
        let style = TextStyle::new(Face::Medium, 30.0 * s, p.muted).align(Align::Center);
        canvas.text(fonts, x + w / 2.0, y + h / 2.0, style, "No other games");
        return;
    }
    let n = sc.rows.len().min(rows_that_fit(h, s)).max(1);
    let gap = 10.0 * s;
    let rh = ((h - gap * (n - 1) as f32) / n as f32).min(110.0 * s);
    let status_w = 170.0 * s;
    for (i, row) in sc.rows.iter().take(n).enumerate() {
        let ry = y + i as f32 * (rh + gap);
        canvas.fill_rect(x as i32, ry as i32, (w - status_w) as i32, rh as i32, p.panel);
        let line = rh / 2.0;
        for (j, team) in [&row.away, &row.home].into_iter().enumerate() {
            bug_line(
                canvas,
                fonts,
                kit,
                x,
                ry + j as f32 * line,
                w - status_w,
                line,
                s,
                team,
                (j == 0).then_some(&row.league),
            );
        }
        let (fill, text) = match row.tone {
            Tone::Live => (p.live, ink(p.live)),
            Tone::Break => (p.accent, ink(p.accent)),
            Tone::Final | Tone::Upcoming => (p.chip(), p.soft()),
        };
        let sx = x + w - status_w;
        canvas.fill_rect(sx as i32, ry as i32, status_w as i32, rh as i32, fill);
        let label = row.status.to_uppercase();
        let size = fonts.fit(Face::Heavy, 32.0 * s, 0.0, &label, status_w - 20.0 * s);
        let base = ry + rh / 2.0 + fonts.cap_height(Face::Heavy, size) / 2.0;
        canvas.text(
            fonts,
            sx + status_w / 2.0,
            base,
            TextStyle::new(Face::Heavy, size, text).align(Align::Center),
            &label,
        );
    }
}

/// One team's line in a score bug: color chip, abbreviation, score.
#[allow(clippy::too_many_arguments)]
fn bug_line(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    s: f32,
    team: &RowTeam,
    league: Option<&String>,
) {
    let p = &kit.p;
    let chip = kit.team_fill(team.colors, p.panel);
    canvas.fill_rect(x as i32, y as i32, (10.0 * s) as i32, h.ceil() as i32, chip);
    let size = (h * 0.72).min(34.0 * s);
    let base = y + h / 2.0 + fonts.cap_height(Face::Heavy, size) / 2.0;
    let color = if team.lost { p.muted } else { p.text };
    let mut ax = x + 28.0 * s;
    if let Some(logo) = kit.logo(team.logo.as_ref()) {
        let size = h * 0.8;
        ax += canvas.draw_logo(logo, ax - 6.0 * s, y + (h - size) / 2.0, size, Align::Center).max(size) + 2.0 * s;
    }
    let abbr_w = canvas.text(fonts, ax, base, TextStyle::new(Face::Heavy, size, color), &team.abbr);
    if let Some(league) = league {
        let style = TextStyle::new(Face::SemiBold, size * 0.6, p.muted).tracking(1.0 * s);
        canvas.text(fonts, ax + abbr_w.max(70.0 * s) + 20.0 * s, base, style, league);
    }
    if let Some(score) = team.score {
        let style = TextStyle::new(Face::Heavy, size * 1.05, color).align(Align::Right);
        canvas.text(fonts, x + w - 20.0 * s, base, style, &score.to_string());
    }
}
