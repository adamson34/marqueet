//! Draws widget views (from `marqueet_core::widgets`) as flat UI on the
//! canvas (ADR-0007), in the configured theme (ADR-0012). Each style draws
//! its own game of the day and scores list (`theme::*`); standings, weather
//! and fantasy share one drawing here, in the theme's colors and faces.

use marqueet_core::config::WidgetLayout;
use marqueet_core::theme::Theme;
use marqueet_core::ticker::Logos;
use marqueet_core::widgets::{BracketView, FantasyView, SeriesView, SpotlightView, StandingsView, WidgetView};

use crate::theme::{Kit, ballpark, broadcast, varsity};
use crate::ui::{Align, Canvas, Face, Fonts, Paint, TextStyle};
use marqueet_core::theme::Style;

/// A card's rectangle in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Card {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Card rectangles for `layout`, with margins, scaled to the canvas.
pub fn slots(width: u32, height: u32, layout: WidgetLayout) -> (f32, Vec<Card>) {
    let s = (width as f32 / 1920.0).min(height as f32 / 648.0);
    let (m, gap) = (40.0 * s, 30.0 * s);
    let (top, h) = (26.0 * s, height as f32 - 52.0 * s);
    let widths = layout.widths();
    let inner = width as f32 - 2.0 * m - gap * (widths.len() - 1) as f32;
    let mut x = m;
    let cards = widths
        .iter()
        .map(|f| {
            let card = Card { x, y: top, w: inner * f, h };
            x += card.w + gap;
            card
        })
        .collect();
    (s, cards)
}

/// Width each widget was designed for (at scale 1); narrower cards scale
/// their contents down so nothing overlaps.
fn design_width(view: &WidgetView) -> f32 {
    match view {
        WidgetView::GameOfTheDay(_) => 760.0,
        _ => 640.0,
    }
}

pub fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    views: &[WidgetView],
    layout: WidgetLayout,
    theme: &Theme,
    logos: &Logos,
    page: usize,
) {
    let kit = Kit::new(theme).with_logos(logos);
    canvas.clear();
    canvas.fill_rect(0, 0, canvas.width as i32, canvas.height as i32, kit.p.ground);
    if let Some(WidgetView::Spotlight(v)) = views.first() {
        // One game across the whole area, whatever the layout, with a
        // fantasy matchup in a column on the right when there is one.
        let (s, cards) = slots(canvas.width, canvas.height, WidgetLayout::Single);
        let mut card = cards[0];
        if let Some(WidgetView::Fantasy(f)) = views.get(1) {
            let w = (card.w * 0.3).round();
            let side = Card { x: card.x + card.w - w, w, ..card };
            fantasy(canvas, fonts, &kit, side, s * (w / (640.0 * s)).min(1.0), f);
            card.w -= w + 30.0 * s;
        }
        spotlight(canvas, fonts, &kit, card, s, v, page);
        return;
    }
    let (s, cards) = slots(canvas.width, canvas.height, layout);
    for (view, card) in views.iter().zip(cards) {
        let s = s * (card.w / (design_width(view) * s)).min(1.0);
        match view {
            WidgetView::GameOfTheDay(g) => match kit.style {
                Style::Broadcast => broadcast::game_of_the_day(canvas, fonts, &kit, card, s, g),
                Style::Ballpark => ballpark::game_of_the_day(canvas, fonts, &kit, card, s, g),
                Style::Varsity => varsity::game_of_the_day(canvas, fonts, &kit, card, s, g),
            },
            WidgetView::Scores(sc) => match kit.style {
                Style::Broadcast => broadcast::scores(canvas, fonts, &kit, card, s, sc),
                Style::Ballpark => ballpark::scores(canvas, fonts, &kit, card, s, sc),
                Style::Varsity => varsity::scores(canvas, fonts, &kit, card, s, sc),
            },
            WidgetView::Standings(st) => standings(canvas, fonts, &kit, card, s, st),
            WidgetView::Weather(w) => crate::weather::draw(canvas, fonts, &kit, card, s, w),
            WidgetView::Fantasy(f) => fantasy(canvas, fonts, &kit, card, s, f),
            WidgetView::Bracket(b) => bracket(canvas, fonts, &kit, card, s, b),
            WidgetView::Empty { title, message } => empty(canvas, fonts, &kit, card, s, title, message),
            // Drawn above, before the other views.
            WidgetView::Spotlight(_) => {}
        }
    }
}

/// The spotlighted game: the look's game view, with a strip along the
/// bottom for the last play and where it's on.
fn spotlight(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, v: &SpotlightView, page: usize) {
    let strip_h = 78.0 * s;
    let gap = 14.0 * s;
    let mut game = Card { h: card.h - strip_h - gap, ..card };
    // Stats beside the game, one panel at a time; an at-bat in progress
    // takes their place.
    if let Some(ab) = &v.at_bat {
        let panel_w = (card.w * 0.38).round();
        game.w = card.w - panel_w - gap;
        let area = Card { x: game.x + game.w + gap, w: panel_w, ..game };
        at_bat_panel(canvas, fonts, kit, area, s, ab);
    } else if !v.panels.is_empty() || v.drive.is_some() {
        let panel_w = (card.w * 0.38).round();
        game.w = card.w - panel_w - gap;
        let area = Card { x: game.x + game.w + gap, w: panel_w, ..game };
        // A drive takes its turn first, then the stat panels.
        let pages = v.panels.len() + usize::from(v.drive.is_some());
        let turn = page % pages;
        match (&v.drive, turn) {
            (Some(d), 0) => drive_panel(canvas, fonts, kit, area, s, d, &v.game),
            (drive, i) => {
                let i = i - usize::from(drive.is_some());
                stat_panel(canvas, fonts, kit, area, s, &v.panels[i], i, v.panels.len());
            }
        }
    }
    match kit.style {
        Style::Broadcast => broadcast::game_of_the_day(canvas, fonts, kit, game, s, &v.game),
        Style::Ballpark => ballpark::game_of_the_day(canvas, fonts, kit, game, s, &v.game),
        Style::Varsity => varsity::game_of_the_day(canvas, fonts, kit, game, s, &v.game),
    }
    let p = &kit.p;
    let (x, y, w) = (card.x, card.y + card.h - strip_h, card.w);
    canvas.fill_round_rect(x, y, w, strip_h, kit.radius(s), p.plate);
    let pad = 28.0 * s;
    let base = y + strip_h / 2.0 + fonts.cap_height(kit.title_face(), 30.0 * s) / 2.0;
    let mut tx = x + pad;
    let right = x + w - pad;
    let note_w = match &v.note {
        Some(note) => {
            let style = TextStyle::new(Face::SemiBold, 24.0 * s, p.muted).align(Align::Right);
            let size = fonts.fit(style.face, style.size, 0.0, note, w * 0.3);
            canvas.text(fonts, right, base, TextStyle { size, ..style }, note) + 30.0 * s
        }
        None => 0.0,
    };
    let last = match (&v.last_play, &v.last_score) {
        (Some(play), _) => Some(("LAST PLAY", play)),
        (None, Some(score)) => Some(("LAST SCORE", score)),
        _ => None,
    };
    if let Some((title, play)) = last {
        let label = TextStyle::new(kit.title_face(), 28.0 * s, p.accent).tracking(1.0 * s);
        tx += canvas.text(fonts, tx, base, label, title) + 22.0 * s;
        let room = right - note_w - tx;
        let size = fonts.fit(Face::SemiBold, 30.0 * s, 0.0, play, room).max(18.0 * s);
        // Too long even at the smallest size: cut it with an ellipsis.
        let text = ellipsize(fonts, Face::SemiBold, size, play, room);
        canvas.text(fonts, tx, base, TextStyle::new(Face::SemiBold, size, p.text), &text);
    }
}

/// `text` cut to fit `room` with a trailing "…" (whole when it fits; at
/// least a few characters however narrow).
fn ellipsize(fonts: &mut Fonts, face: Face, size: f32, text: &str, room: f32) -> String {
    if fonts.measure(face, size, 0.0, text) <= room {
        return text.to_owned();
    }
    let mut kept: Vec<char> = text.chars().collect();
    loop {
        kept.pop();
        let cut = format!("{}…", kept.iter().collect::<String>().trim_end());
        if kept.len() <= 4 || fonts.measure(face, size, 0.0, &cut) <= room {
            return cut;
        }
    }
}

/// A playoff bracket: a column per round, each series a small box with its
/// two teams and wins, lines joining a series to the ones that fed it.
fn bracket(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, b: &BracketView) {
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, &b.title);
    let Card { x, y, w, h } = card;
    let pad = 28.0 * s;
    let n = b.columns.len().max(1) as f32;
    let gap = 26.0 * s;
    let col_w = (w - 2.0 * pad - gap * (n - 1.0)) / n;
    let head = y + 104.0 * s;
    let (top, bottom) = (head + 16.0 * s, y + h - 18.0 * s);
    let row_h = 30.0 * s;
    let box_h = 2.0 * row_h;
    let note_h = 20.0 * s;

    // Each series' box, by column: (x, centre y).
    let mut centres: Vec<Vec<(f32, f32)>> = Vec::new();
    for (ci, col) in b.columns.iter().enumerate() {
        let cx = x + pad + ci as f32 * (col_w + gap);
        let label = TextStyle::new(Face::SemiBold, 17.0 * s, p.muted).tracking(1.0 * s);
        let size = fonts.fit(Face::SemiBold, 17.0 * s, 1.0 * s, &col.title, col_w);
        canvas.text(fonts, cx, head, TextStyle { size, ..label }, &col.title);
        let k = col.series.len().max(1) as f32;
        let slot = (bottom - top) / k;
        let mut here = Vec::new();
        if col.series.is_empty() {
            let cy = top + slot / 2.0;
            canvas.fill_round_rect(cx, cy - box_h / 2.0, col_w, box_h, kit.radius(s), p.plate);
            let tbd = TextStyle::new(Face::SemiBold, 22.0 * s, p.muted).align(Align::Center);
            canvas.text(fonts, cx + col_w / 2.0, cy + fonts.cap_height(Face::SemiBold, 22.0 * s) / 2.0, tbd, "TBD");
            here.push((cx, cy));
        }
        for (si, series) in col.series.iter().enumerate() {
            // Centre the box (and its note) in its share of the column.
            let with_note = if series.next.is_empty() { 0.0 } else { note_h };
            let cy = top + slot * (si as f32 + 0.5) - with_note / 2.0;
            series_box(canvas, fonts, kit, s, cx, cy - box_h / 2.0, col_w, row_h, series);
            if !series.next.is_empty() {
                let color = if series.live { p.live } else { p.muted };
                let style = TextStyle::new(Face::SemiBold, 16.0 * s, color).tracking(0.5 * s);
                let size = fonts.fit(Face::SemiBold, 16.0 * s, 0.5 * s, &series.next, col_w);
                canvas.text(
                    fonts,
                    cx + 2.0 * s,
                    cy + box_h / 2.0 + 18.0 * s,
                    TextStyle { size, ..style },
                    &series.next,
                );
            }
            here.push((cx, cy));
        }
        centres.push(here);
    }

    // Lines from each series to the one it fed: straight when one feeds
    // one, an elbow when two feed one.
    let line = s.max(1.0) * 2.0;
    for ci in 1..centres.len() {
        let (prev, cur) = (&centres[ci - 1], &centres[ci]);
        if b.columns[ci].series.is_empty() {
            continue;
        }
        let feeds: Vec<(usize, usize)> = if prev.len() == cur.len() {
            (0..cur.len()).map(|j| (j, j)).collect()
        } else if prev.len() == 2 * cur.len() {
            (0..prev.len()).map(|j| (j, j / 2)).collect()
        } else {
            Vec::new()
        };
        for (from, to) in feeds {
            let ((fx, fy), (tx, ty)) = (prev[from], cur[to]);
            let (x0, x1) = (fx + col_w, tx);
            let mid = (x0 + x1) / 2.0;
            let c = p.muted.mix(p.panel, 0.45);
            canvas.fill_rect(x0 as i32, fy as i32, (mid - x0) as i32, line as i32, c);
            let (y0, y1) = (fy.min(ty), fy.max(ty));
            canvas.fill_rect(mid as i32, y0 as i32, line as i32, (y1 - y0 + line) as i32, c);
            canvas.fill_rect(mid as i32, ty as i32, (x1 - mid) as i32, line as i32, c);
        }
    }
}

/// One series: two team rows (colour chip, abbreviation, wins).
#[allow(clippy::too_many_arguments)]
fn series_box(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    s: f32,
    x: f32,
    y: f32,
    w: f32,
    row_h: f32,
    series: &SeriesView,
) {
    let p = &kit.p;
    canvas.fill_round_rect(x, y, w, 2.0 * row_h, kit.radius(s), p.plate);
    if series.live {
        canvas.fill_rect(x as i32, y as i32, (3.0 * s) as i32, (2.0 * row_h) as i32, p.live);
    }
    for (i, t) in series.teams.iter().enumerate() {
        let ry = y + i as f32 * row_h;
        if i == 1 {
            canvas.fill_rect((x + 8.0 * s) as i32, ry as i32, (w - 16.0 * s) as i32, s.max(1.0) as i32, p.rule());
        }
        let chip = if t.out { t.color.mix(p.plate, 0.6) } else { t.color };
        canvas.fill_round_rect(x + 10.0 * s, ry + row_h * 0.25, 6.0 * s, row_h * 0.5, 2.0 * s, chip);
        let color = if t.out {
            p.muted
        } else if t.favorite {
            p.accent
        } else {
            p.text
        };
        let face = if t.won { kit.strong_face() } else { Face::SemiBold };
        let base = ry + row_h / 2.0 + fonts.cap_height(face, 21.0 * s) / 2.0;
        canvas.text(fonts, x + 24.0 * s, base, TextStyle::new(face, 21.0 * s, color), &t.abbr);
        let wins = TextStyle::new(kit.number_face(), 22.0 * s, color).align(Align::Right);
        canvas.text(fonts, x + w - 12.0 * s, base, wins, &t.wins.to_string());
    }
}

/// Football's drive: the offense, the drive so far, down and distance, a
/// field with the drive's start, the ball and the first-down line (the
/// offense always going right), and the latest plays.
fn drive_panel(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    card: Card,
    s: f32,
    d: &marqueet_core::sports::summary::Drive,
    game: &marqueet_core::widgets::GameOfTheDay,
) {
    use marqueet_core::Rgb;
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, "DRIVE");
    let Card { x, y, w, h } = card;
    let (left, right) = (x + 32.0 * s, x + w - 32.0 * s);
    let (offense, defense) = if d.home { (&game.home, &game.away) } else { (&game.away, &game.home) };
    let (off_color, def_color) = (offense.colors.primary, defense.colors.primary);

    // Who has it and the drive so far.
    let mut ty = y + 104.0 * s;
    let team_w = canvas.text(fonts, left, ty, TextStyle::new(kit.strong_face(), 28.0 * s, p.text), &d.team);
    let so_far = format!(
        "{} PLAY{} · {} YD{} · {}",
        d.plays,
        if d.plays == 1 { "" } else { "S" },
        d.yards,
        if d.yards.abs() == 1 { "" } else { "S" },
        d.time
    );
    let room = right - left - team_w - 16.0 * s;
    let text = ellipsize(fonts, Face::SemiBold, 19.0 * s, so_far.trim_end_matches(" · "), room);
    canvas.text(fonts, right, ty, TextStyle::new(Face::SemiBold, 19.0 * s, p.muted).align(Align::Right), &text);

    // Down and distance, or how the drive ended.
    ty += 44.0 * s;
    let red_zone = d.ball >= 80 && d.result.is_none();
    let (headline, color) = match &d.result {
        Some(r) => (r.to_uppercase(), p.accent),
        None if red_zone => (d.down.to_uppercase(), p.live),
        None => (d.down.to_uppercase(), p.text),
    };
    let size = fonts.fit(kit.strong_face(), 32.0 * s, 0.0, &headline, right - left).max(18.0 * s);
    canvas.text(fonts, left, ty, TextStyle::new(kit.strong_face(), size, color), &headline);

    // The field: own end zone left, theirs right, a line every 10 yards.
    let fy = ty + 22.0 * s;
    let fh = 64.0 * s;
    let ez = (right - left) * 0.08;
    let (fx0, fx1) = (left + ez, right - ez);
    let at = |yard: u8| fx0 + (fx1 - fx0) * f32::from(yard.min(100)) / 100.0;
    canvas.fill_round_rect(left, fy, right - left, fh, kit.radius(s), p.plate);
    canvas.fill_rect(left as i32, fy as i32, ez as i32, fh as i32, off_color.mix(p.plate, 0.35));
    canvas.fill_rect(fx1 as i32, fy as i32, ez as i32, fh as i32, def_color.mix(p.plate, 0.35));
    let line = (2.0 * s).max(1.0);
    for yard in (10..100).step_by(10) {
        let lx = at(yard as u8);
        let c = if yard == 50 { p.muted } else { p.muted.mix(p.plate, 0.55) };
        canvas.fill_rect(lx as i32, fy as i32, line as i32, fh as i32, c);
    }
    // The drive so far, the first-down line, then the ball.
    let (a, b) = (at(d.start.min(d.ball)), at(d.start.max(d.ball)));
    canvas.fill_rect(
        a as i32,
        (fy + fh * 0.4) as i32,
        (b - a).max(line) as i32,
        (fh * 0.2) as i32,
        off_color.mix(p.plate, 0.2),
    );
    if let Some(fd) = d.first_down {
        canvas.fill_rect(
            at(fd) as i32,
            (fy - 4.0 * s) as i32,
            (3.0 * s) as i32,
            (fh + 8.0 * s) as i32,
            Rgb::new(255, 210, 0),
        );
    }
    let bx = at(d.ball);
    let r = fh * 0.22;
    canvas.fill_round_rect(bx - r * 1.4, fy + fh / 2.0 - r, r * 2.8, r * 2.0, r, Rgb::new(150, 82, 40));
    canvas.fill_rect(
        (bx - r * 0.6) as i32,
        (fy + fh / 2.0 - s) as i32,
        (r * 1.2) as i32,
        (2.0 * s).max(1.0) as i32,
        Rgb::WHITE,
    );

    // The latest plays: yards, then what happened.
    let mut ly = fy + fh + 40.0 * s;
    let row_h = ((y + h - 16.0 * s - ly) / 4.0).clamp(26.0 * s, 36.0 * s);
    for play in &d.recent {
        if ly > y + h - 12.0 * s {
            break;
        }
        let (yards, c) = match play.yards {
            n if n > 0 => (format!("+{n}"), Rgb::new(70, 185, 105)),
            n if n < 0 => (n.to_string(), Rgb::new(225, 70, 60)),
            _ => ("0".to_owned(), p.muted),
        };
        let yard_w = 52.0 * s;
        canvas.text(
            fonts,
            left + yard_w - 10.0 * s,
            ly,
            TextStyle::new(kit.number_face(), 22.0 * s, c).align(Align::Right),
            &yards,
        );
        let room = right - left - yard_w;
        let text = ellipsize(fonts, Face::Medium, 19.0 * s, &play.text, room);
        canvas.text(fonts, left + yard_w, ly, TextStyle::new(Face::Medium, 19.0 * s, p.soft()), &text);
        ly += row_h;
    }
}

/// Pitch colors: balls green, strikes red, in play blue (as broadcasts do).
fn call_color(call: marqueet_core::sports::summary::Call) -> marqueet_core::Rgb {
    use marqueet_core::sports::summary::Call;
    match call {
        Call::Ball => marqueet_core::Rgb::new(46, 160, 90),
        Call::Strike => marqueet_core::Rgb::new(214, 48, 44),
        Call::InPlay => marqueet_core::Rgb::new(52, 110, 214),
    }
}

/// Baseball's at-bat in progress: pitcher and batter with their lines, the
/// strike zone with this at-bat's pitches, the count, outs and runners, and
/// the pitches in words.
fn at_bat_panel(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    card: Card,
    s: f32,
    ab: &marqueet_core::sports::summary::AtBat,
) {
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, "AT BAT");
    let Card { x, y, w, h } = card;
    let (left, right) = (x + 32.0 * s, x + w - 32.0 * s);
    let mut ty = y + 104.0 * s;

    // Pitcher and batter: a label, the name, the line.
    for (label, who) in [("P", &ab.pitcher), ("AB", &ab.batter)] {
        let Some(who) = who else { continue };
        let tag = TextStyle::new(Face::SemiBold, 18.0 * s, p.muted).tracking(1.0 * s);
        canvas.text(fonts, left, ty, tag, label);
        let name_x = left + 44.0 * s;
        let name_w = canvas.text(fonts, name_x, ty, TextStyle::new(Face::SemiBold, 26.0 * s, p.text), &who.name);
        let room = right - name_x - name_w - 16.0 * s;
        let line = TextStyle::new(Face::Medium, 19.0 * s, p.soft()).align(Align::Right);
        let size = fonts.fit(Face::Medium, 19.0 * s, 0.0, &who.line, room).max(13.0 * s);
        let text = ellipsize(fonts, Face::Medium, size, &who.line, room);
        canvas.text(fonts, right, ty, TextStyle { size, ..line }, &text);
        ty += 40.0 * s;
    }

    // The zone (left) and the count, outs and bases (right).
    let list_rows = ab.pitches.len().clamp(1, 4) as f32;
    let list_h = list_rows * 30.0 * s + 10.0 * s;
    let top = ty + 4.0 * s;
    let zone_h = (y + h - 20.0 * s - list_h - top).max(60.0 * s);
    let box_w = (zone_h * 0.9).min((right - left) * 0.5);
    let bx = left;
    canvas.fill_round_rect(bx, top, box_w, zone_h, kit.radius(s), p.plate);
    // The box shows -1.7..1.7 across and -1.5..1.5 down; the zone is -1..1.
    let (span_x, span_y) = (1.7_f32, 1.5_f32);
    let to_px = |(nx, ny): (f32, f32)| {
        let px = bx + box_w / 2.0 + nx.clamp(-span_x, span_x) / span_x * (box_w / 2.0);
        let py = top + zone_h / 2.0 + ny.clamp(-span_y, span_y) / span_y * (zone_h / 2.0);
        (px, py)
    };
    let (zx0, zy0) = to_px((-1.0, -1.0));
    let (zx1, zy1) = to_px((1.0, 1.0));
    let t = (2.0 * s).max(1.0);
    let edge = p.muted;
    canvas.fill_rect(zx0 as i32, zy0 as i32, (zx1 - zx0) as i32, t as i32, edge);
    canvas.fill_rect(zx0 as i32, zy1 as i32, (zx1 - zx0 + t) as i32, t as i32, edge);
    canvas.fill_rect(zx0 as i32, zy0 as i32, t as i32, (zy1 - zy0) as i32, edge);
    canvas.fill_rect(zx1 as i32, zy0 as i32, t as i32, (zy1 - zy0) as i32, edge);
    let r = (zone_h * 0.075).clamp(9.0 * s, 16.0 * s);
    for pitch in &ab.pitches {
        let Some(at) = pitch.at else { continue };
        let (px, py) = to_px(at);
        canvas.fill_round_rect(px - r, py - r, 2.0 * r, 2.0 * r, r, call_color(pitch.call));
        let n = TextStyle::new(Face::SemiBold, r * 1.2, marqueet_core::Rgb::WHITE).align(Align::Center);
        canvas.text(fonts, px, py + fonts.cap_height(Face::SemiBold, r * 1.2) / 2.0, n, &pitch.number.to_string());
    }

    // Count and outs as dots, then the bases.
    let cx = bx + box_w + 28.0 * s;
    let dot = 13.0 * s;
    let mut row_y = top + 16.0 * s;
    for (label, n, of, color) in [
        ("B", ab.balls, 4, call_color(marqueet_core::sports::summary::Call::Ball)),
        ("S", ab.strikes, 3, call_color(marqueet_core::sports::summary::Call::Strike)),
        ("O", ab.outs, 3, p.accent),
    ] {
        let base = row_y + dot;
        canvas.text(fonts, cx, base, TextStyle::new(Face::SemiBold, 20.0 * s, p.muted), label);
        for i in 0..of {
            let dx = cx + 30.0 * s + f32::from(i) * (dot * 2.0 + 6.0 * s);
            let fill = if i < n { color } else { p.plate };
            canvas.fill_round_rect(dx, row_y + 2.0 * s, dot * 2.0, dot * 2.0, dot, fill);
        }
        row_y += dot * 2.0 + 12.0 * s;
    }
    // The bases, in the room right of the dots.
    let dots_right = cx + 30.0 * s + 4.0 * (dot * 2.0 + 6.0 * s);
    let d = ((right - dots_right) * 0.5).min(zone_h * 0.5).clamp(30.0 * s, 90.0 * s);
    let (mx, my) = ((dots_right + right) / 2.0, top + zone_h / 2.0);
    let diamond = |canvas: &mut Canvas, (ox, oy): (f32, f32), on: bool| {
        let q = d * 0.36;
        let pts = [(ox, oy - q), (ox + q, oy), (ox, oy + q), (ox - q, oy)];
        canvas.fill_polygon(&pts, if on { p.accent } else { p.plate });
    };
    let off = d * 0.62;
    diamond(canvas, (mx + off, my), ab.bases[0]);
    diamond(canvas, (mx, my - off), ab.bases[1]);
    diamond(canvas, (mx - off, my), ab.bases[2]);

    // The pitches in words, newest first.
    let mut ly = y + h - 20.0 * s - list_h + 26.0 * s;
    for pitch in ab.pitches.iter().rev().take(4) {
        let r = 11.0 * s;
        canvas.fill_round_rect(left, ly - r - 7.0 * s, 2.0 * r, 2.0 * r, r, call_color(pitch.call));
        let n = TextStyle::new(Face::SemiBold, 15.0 * s, marqueet_core::Rgb::WHITE).align(Align::Center);
        canvas.text(fonts, left + r, ly - 2.0 * s, n, &pitch.number.to_string());
        let what = TextStyle::new(Face::SemiBold, 20.0 * s, p.text);
        let w1 = canvas.text(fonts, left + 2.0 * r + 12.0 * s, ly, what, &pitch.result);
        let kind = TextStyle::new(Face::Medium, 18.0 * s, p.muted).align(Align::Right);
        let room = right - (left + 2.0 * r + 12.0 * s + w1 + 16.0 * s);
        let text = ellipsize(fonts, Face::Medium, 18.0 * s, &pitch.kind, room);
        canvas.text(fonts, right, ly, kind, &text);
        ly += 30.0 * s;
    }
    if ab.pitches.is_empty() {
        let hint = TextStyle::new(Face::Medium, 20.0 * s, p.muted);
        canvas.text(fonts, left, ly, hint, "First pitch coming up");
    }
}

/// One stat panel: a title (with "2/3" when several take turns) and rows.
#[allow(clippy::too_many_arguments)]
fn stat_panel(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    card: Card,
    s: f32,
    panel: &marqueet_core::widgets::StatPanel,
    index: usize,
    count: usize,
) {
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, &panel.title);
    let Card { x, y, w, h } = card;
    let (left, right) = (x + 32.0 * s, x + w - 32.0 * s);
    if count > 1 {
        let style = TextStyle::new(Face::SemiBold, 22.0 * s, p.muted).align(Align::Right);
        canvas.text(fonts, right, y + 62.0 * s, style, &format!("{}/{count}", index + 1));
    }
    let top = y + 96.0 * s;
    let n = panel.rows.len().max(1) as f32;
    // Long values (leaders' names) get their own line under the label.
    let two_line = !panel.text_rows && panel.rows.iter().any(|r| r[0].chars().count() > 9 || r[2].chars().count() > 9);
    let row_h = ((y + h - 20.0 * s - top) / n).min(if two_line { 84.0 } else { 56.0 } * s);
    for (i, row) in panel.rows.iter().enumerate() {
        let ry = top + i as f32 * row_h;
        if i > 0 {
            canvas.fill_rect(left as i32, ry as i32, (right - left) as i32, s.max(1.0) as i32, p.rule());
        }
        if panel.text_rows {
            let base = ry + row_h / 2.0 + fonts.cap_height(Face::SemiBold, 24.0 * s) / 2.0;
            let when = TextStyle::new(Face::SemiBold, 20.0 * s, p.muted);
            canvas.text(fonts, left, base, when, &row[0]);
            let score = TextStyle::new(kit.number_face(), 26.0 * s, p.text).align(Align::Right);
            let score_w = canvas.text(fonts, right, base, score, &row[2]);
            let (tx, room) = (left + 96.0 * s, right - score_w - 16.0 * s - (left + 96.0 * s));
            let size = fonts.fit(Face::Medium, 22.0 * s, 0.0, &row[1], room).max(15.0 * s);
            let text = ellipsize(fonts, Face::Medium, size, &row[1], room);
            canvas.text(fonts, tx, base, TextStyle::new(Face::Medium, size, p.soft()), &text);
        } else if two_line {
            let label = TextStyle::new(Face::SemiBold, 18.0 * s, p.muted).tracking(1.0 * s).align(Align::Center);
            canvas.text(fonts, x + w / 2.0, ry + 26.0 * s, label, &row[1]);
            let vbase = ry + row_h - 18.0 * s;
            let half = (right - left) / 2.0 - 8.0 * s;
            for (value, at, align) in [(&row[0], left, Align::Left), (&row[2], right, Align::Right)] {
                let size = fonts.fit(Face::SemiBold, 24.0 * s, 0.0, value, half);
                canvas.text(fonts, at, vbase, TextStyle::new(Face::SemiBold, size, p.text).align(align), value);
            }
        } else {
            let base = ry + row_h / 2.0 + fonts.cap_height(kit.number_face(), 30.0 * s) / 2.0;
            let value = |a| TextStyle::new(kit.number_face(), 30.0 * s, p.text).align(a);
            canvas.text(fonts, left, base, value(Align::Left), &row[0]);
            canvas.text(fonts, right, base, value(Align::Right), &row[2]);
            let label = TextStyle::new(Face::SemiBold, 20.0 * s, p.muted).tracking(1.0 * s).align(Align::Center);
            let size = fonts.fit(Face::SemiBold, 20.0 * s, 1.0 * s, &row[1], w * 0.5);
            canvas.text(fonts, x + w / 2.0, base - 4.0 * s, TextStyle { size, ..label }, &row[1]);
        }
    }
}

fn empty(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, heading: &str, message: &str) {
    kit.card(canvas, fonts, card, s, heading);
    let style = TextStyle::new(Face::Medium, 34.0 * s, kit.p.muted).align(Align::Center);
    canvas.text(fonts, card.x + card.w / 2.0, card.y + card.h / 2.0, style, message);
}

/// Fantasy matchup: names and LED totals up top, then starters side by
/// side with their lineup slot in the middle.
fn fantasy(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, f: &FantasyView) {
    let (x, y, w, h) = (card.x, card.y, card.w, card.h);
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, &f.title);
    let sub = TextStyle::new(Face::SemiBold, 24.0 * s, p.soft()).align(Align::Right);
    canvas.text(fonts, x + w - 40.0 * s, y + 62.0 * s, sub, &f.subtitle);

    let (left, right, mid) = (x + 36.0 * s, x + w - 36.0 * s, x + w / 2.0);
    let name = |lead: bool| TextStyle::new(kit.strong_face(), 30.0 * s, if lead { p.text } else { p.muted });
    let record = TextStyle::new(Face::Medium, 22.0 * s, p.muted);
    let px = 9.0 * s;
    canvas.text(fonts, left, y + 118.0 * s, name(f.me.leading), &f.me.name);
    canvas.text(fonts, left, y + 146.0 * s, record, &f.me.record);
    let color = |lead: bool| if lead { p.accent } else { p.muted };
    canvas.led_text(left, y + 160.0 * s, px, color(f.me.leading), true, &f.me.points);
    if let Some(o) = &f.opponent {
        canvas.text(fonts, right, y + 118.0 * s, name(o.leading).align(Align::Right), &o.name);
        canvas.text(fonts, right, y + 146.0 * s, record.align(Align::Right), &o.record);
        let lw = Canvas::led_width(px, &o.points);
        canvas.led_text(right - lw, y + 160.0 * s, px, color(o.leading), true, &o.points);
    }

    let top = y + 244.0 * s;
    canvas.fill_rect(left as i32, top as i32, (right - left) as i32, s.max(1.0) as i32, p.rule());
    if f.lines.is_empty() {
        return;
    }
    let row = ((y + h - 20.0 * s - top) / f.lines.len() as f32).min(36.0 * s);
    let player = TextStyle::new(Face::Medium, (row * 0.66).min(24.0 * s), p.text);
    let pts = TextStyle::new(Face::SemiBold, (row * 0.66).min(24.0 * s), p.soft());
    let slot = TextStyle::new(Face::Medium, (row * 0.56).min(20.0 * s), p.muted).tracking(1.0 * s).align(Align::Center);
    for (i, line) in f.lines.iter().enumerate() {
        let by = top + row * (i as f32 + 1.0) - row * 0.22;
        canvas.text(fonts, left, by, player, &line.mine.0);
        canvas.text(fonts, mid - 44.0 * s, by, pts.align(Align::Right), &line.mine.1);
        canvas.text(fonts, mid, by, slot, &line.slot);
        if let Some((n, p)) = &line.theirs {
            canvas.text(fonts, mid + 44.0 * s, by, pts, p);
            canvas.text(fonts, right, by, player.align(Align::Right), n);
        }
    }
}

/// How many standings rows fit in a card of height `h` at scale `s`.
pub fn standings_rows_that_fit(h: f32, s: f32) -> usize {
    ((h - 150.0 * s) / (54.0 * s)).floor().max(0.0) as usize
}

/// First row to show so that `focus` is visible (with a row of context
/// below it when possible) in a window of `fit` rows out of `len`.
pub fn standings_window(len: usize, fit: usize, focus: Option<usize>) -> usize {
    match focus {
        Some(f) if fit > 0 && f + 1 >= fit => (f + 2).min(len).saturating_sub(fit),
        _ => 0,
    }
}

fn standings(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, st: &StandingsView) {
    let (x, y, w) = (card.x, card.y, card.w);
    let p = &kit.p;
    kit.card(canvas, fonts, card, s, &st.title);
    let group = TextStyle::new(Face::SemiBold, 28.0 * s, p.soft()).align(Align::Right);
    canvas.text(fonts, x + w - 40.0 * s, y + 62.0 * s, group, &st.group);

    let fit = standings_rows_that_fit(card.h, s);
    let start = standings_window(st.rows.len(), fit, st.focus);
    let (left, right) = (x + 36.0 * s, x + w - 36.0 * s);
    let team_x = left + 44.0 * s;
    let cols_start = x + w * 0.4;
    let n = st.columns.len().max(1) as f32;
    let col_x = |i: usize| cols_start + (right - cols_start) * (i as f32 + 0.5) / n;

    let head_y = y + 118.0 * s;
    let head = TextStyle::new(Face::Medium, 24.0 * s, p.muted).tracking(1.0 * s);
    canvas.text(fonts, team_x, head_y, head, "TEAM");
    for (i, c) in st.columns.iter().enumerate() {
        canvas.text(fonts, col_x(i), head_y, head.align(Align::Center), c);
    }
    canvas.fill_rect(left as i32, (head_y + 14.0 * s) as i32, (right - left) as i32, s.max(1.0) as i32, p.rule());

    for (i, row) in st.rows.iter().skip(start).take(fit).enumerate() {
        let ry = head_y + 60.0 * s + i as f32 * 54.0 * s;
        if row.highlight {
            let r = kit.radius(s).min(8.0 * s);
            canvas.fill_round_rect(left - 12.0 * s, ry - 36.0 * s, right - left + 24.0 * s, 50.0 * s, r, p.chip());
        }
        let rank = TextStyle::new(Face::Medium, 26.0 * s, p.muted).align(Align::Right);
        canvas.text(fonts, team_x - 16.0 * s, ry, rank, &row.rank.to_string());
        let team = TextStyle::new(kit.strong_face(), 30.0 * s, if row.highlight { p.accent } else { p.text });
        canvas.text(fonts, team_x, ry, team, &row.team);
        for (c, cell) in row.cells.iter().enumerate() {
            let last = c + 1 == row.cells.len();
            let style = TextStyle::new(if last { Face::SemiBold } else { Face::Medium }, 28.0 * s, p.soft());
            let style = if last { TextStyle { paint: Paint::solid(p.text), ..style } } else { style };
            canvas.text(fonts, col_x(c), ry, style.align(Align::Center), cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};
    use marqueet_core::Rgb;
    use marqueet_core::sports::fixtures::mock_games;
    use marqueet_core::widgets::default_views;

    fn views() -> Vec<WidgetView> {
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        let mut games = mock_games(now);
        games.retain(|g| g.id.0 != "mock:epl:1"); // feature KC-BUF, not the draw
        default_views(&games, &[], FixedOffset::west_opt(4 * 3600).unwrap(), now)
    }

    #[test]
    fn slots_fill_the_width_and_stay_inside() {
        for (w, h) in [(1920, 648), (1366, 461), (1024, 461)] {
            let (_, cards) = slots(w, h, WidgetLayout::WideLeft);
            let [a, b] = [cards[0], cards[1]];
            assert!(a.x > 0.0 && b.x + b.w < w as f32 && a.y > 0.0 && a.y + a.h < h as f32, "{w}x{h}");
            assert!(a.w > b.w, "game of the day gets the big card");
        }
    }

    #[test]
    fn every_layout_tiles_the_width_without_overlap() {
        for layout in WidgetLayout::ALL {
            for (w, h) in [(1920, 648), (1024, 461)] {
                let (_, cards) = slots(w, h, layout);
                assert_eq!(cards.len(), layout.slots());
                assert!(cards[0].x > 0.0 && cards.last().map(|c| c.x + c.w).unwrap() < w as f32 - 1.0, "{layout:?}");
                assert!(cards.windows(2).all(|p| p[0].x + p[0].w < p[1].x), "{layout:?} overlap");
            }
        }
        let (_, three) = slots(1920, 648, WidgetLayout::Three);
        assert!((three[0].w - three[2].w).abs() < 0.5);
    }

    /// A made-up round badge: a green disc with a white ring, clear corners.
    fn badge() -> marqueet_core::team_art::Image {
        let n = 64;
        let mut rgba = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let d = ((x as f32 - 31.5).powi(2) + (y as f32 - 31.5).powi(2)).sqrt();
                rgba.extend_from_slice(match d {
                    d if d < 24.0 => &[0, 200, 90, 255],
                    d if d < 31.0 => &[255, 255, 255, 255],
                    _ => &[0, 0, 0, 0],
                });
            }
        }
        marqueet_core::team_art::Image::new(n, n, rgba).unwrap()
    }

    /// Views with the badge added for every team in the featured game and
    /// the scores list.
    fn views_with_logos() -> (Vec<WidgetView>, Logos) {
        use marqueet_core::team_art::{TeamArt, TeamArtMap, logo_key};
        use marqueet_core::widgets::{WidgetData, build_views};
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        let mut games = mock_games(now);
        games.retain(|g| g.id.0 != "mock:epl:1");
        let art: TeamArtMap = games
            .iter()
            .flat_map(|g| [&g.away, &g.home])
            .map(|c| {
                (
                    c.team.id.clone(),
                    TeamArt {
                        label: String::new(),
                        colors: None,
                        logo: Some(badge()),
                        words: Default::default(),
                        art: Default::default(),
                    },
                )
            })
            .collect();
        let logos: Logos = art.keys().map(|t| (logo_key(t), badge())).collect();
        let data = WidgetData { games: &games, art: Some(&art), ..WidgetData::default() };
        let slots = marqueet_core::settings::Settings::default().widgets;
        (build_views(&slots, &data, FixedOffset::west_opt(4 * 3600).unwrap(), now), logos)
    }

    #[test]
    fn every_style_draws_logos_people_added() {
        let (v, logos) = views_with_logos();
        let green = Rgb::new(0, 200, 90);
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        let mut fonts = Fonts::new();
        for style in Style::ALL {
            let mut c = Canvas::new(1920, 648);
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &Theme::preset(style), &logos, 0);
            assert!(has_in(&c, cards[0], green), "{style:?} game of the day");
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &Theme::preset(style), &Logos::new(), 0);
            assert!(!has_in(&c, cards[0], green), "{style:?} without logos");
        }
    }

    fn has_in(c: &Canvas, card: Card, color: Rgb) -> bool {
        (card.x as u32..(card.x + card.w) as u32).any(|x| {
            (card.y as u32..(card.y + card.h) as u32).any(|y| c.pixel(x, y)[..3] == [color.r, color.g, color.b])
        })
    }

    fn gotd(views: &[WidgetView]) -> &marqueet_core::widgets::GameOfTheDay {
        match &views[0] {
            WidgetView::GameOfTheDay(g) => g,
            other => panic!("expected the game of the day, got {other:?}"),
        }
    }

    #[test]
    fn every_style_draws_on_its_ground() {
        let mut fonts = Fonts::new();
        let v = views();
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        for style in Style::ALL {
            let theme = Theme::preset(style);
            let p = theme.palette;
            let mut c = Canvas::new(1920, 648);
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new(), 0);
            assert_eq!(c.pixel(20, 324), [p.ground.r, p.ground.g, p.ground.b, 255], "{style:?} ground in the margin");
            assert!(has_in(&c, cards[0], p.text) || has_in(&c, cards[0], Rgb::WHITE), "{style:?} game text");
            assert!(has_in(&c, cards[1], p.text) || has_in(&c, cards[1], Rgb::WHITE), "{style:?} scores text");
        }
    }

    #[test]
    fn broadcast_and_varsity_fill_the_game_with_team_colors() {
        let mut fonts = Fonts::new();
        let v = views();
        let g = gotd(&v);
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        for style in [Style::Broadcast, Style::Varsity] {
            let mut theme = Theme::preset(style);
            let (away, home) = Kit::new(&theme).team_fills(g.away.colors, g.home.colors);
            let mut c = Canvas::new(1920, 648);
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new(), 0);
            assert!(has_in(&c, cards[0], away) && has_in(&c, cards[0], home), "{style:?}");
            theme.team_colors = false;
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new(), 0);
            assert!(!has_in(&c, cards[0], away) && !has_in(&c, cards[0], home), "{style:?} palette only");
        }
    }

    #[test]
    fn ballpark_hangs_number_plates() {
        let mut fonts = Fonts::new();
        let theme = Theme::preset(Style::Ballpark);
        let mut c = Canvas::new(1920, 648);
        draw(&mut c, &mut fonts, &views(), WidgetLayout::WideLeft, &theme, &Logos::new(), 0);
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        for card in cards {
            assert!(has_in(&c, card, theme.palette.plate) && has_in(&c, card, theme.palette.accent));
        }
    }

    #[test]
    fn narrow_and_small_screens_draw_every_style() {
        let mut fonts = Fonts::new();
        let v = views();
        for style in Style::ALL {
            for layout in WidgetLayout::ALL {
                for (w, h) in [(1024, 461), (1920, 648)] {
                    let mut c = Canvas::new(w, h);
                    draw(&mut c, &mut fonts, &v, layout, &Theme::preset(style), &Logos::new(), 0);
                }
            }
        }
    }

    #[test]
    fn score_lists_cap_rows_to_what_fits() {
        assert_eq!(broadcast::rows_that_fit(596.0, 1.0), 7);
        assert_eq!(ballpark::rows_that_fit(596.0, 1.0), 6);
        assert_eq!(varsity::rows_that_fit(596.0, 1.0), 6);
        assert_eq!(broadcast::rows_that_fit(40.0, 1.0), 0);
    }

    /// Writes each style's widget area to PNGs for eyeballing; run with
    /// `MARQUEET_WIDGETS_DIR=out cargo test -p marqueet-display dump_widgets -- --ignored`.
    #[test]
    #[ignore = "writes files"]
    fn dump_widgets() {
        let dir = std::path::PathBuf::from(std::env::var("MARQUEET_WIDGETS_DIR").unwrap());
        let mut fonts = Fonts::new();
        let (with, logos) = views_with_logos();
        for style in Style::ALL {
            let mut c = Canvas::new(1920, 648);
            draw(&mut c, &mut fonts, &views(), WidgetLayout::WideLeft, &Theme::preset(style), &Logos::new(), 0);
            crate::screenshot::write_png(&dir.join(format!("{}.png", style.id())), (1920, 648), &c.data).unwrap();
            draw(&mut c, &mut fonts, &with, WidgetLayout::WideLeft, &Theme::preset(style), &logos, 0);
            crate::screenshot::write_png(&dir.join(format!("{}-logos.png", style.id())), (1920, 648), &c.data).unwrap();
        }
    }

    /// Redraw cost; run with `cargo test --release -p marqueet-display redraw_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing"]
    fn redraw_cost() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        let v = views();
        for style in Style::ALL {
            let theme = Theme::preset(style);
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new(), 0); // warm the glyph cache
            let t = std::time::Instant::now();
            for _ in 0..20 {
                draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new(), 0);
            }
            println!("{style:?} widget redraw 1920x648: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        }
    }

    #[test]
    fn standings_keep_the_focus_row_in_view() {
        assert_eq!(standings_window(20, 8, None), 0);
        assert_eq!(standings_window(20, 8, Some(3)), 0, "already visible");
        assert_eq!(standings_window(20, 8, Some(7)), 1, "last visible row gets one of context below");
        assert_eq!(standings_window(20, 8, Some(12)), 6);
        assert_eq!(standings_window(20, 8, Some(19)), 12, "bottom of the table");
        assert_eq!(standings_window(4, 8, Some(3)), 0);
    }

    #[test]
    fn draws_standings_with_the_favorite_marked() {
        use marqueet_core::sports::TeamId;
        use marqueet_core::sports::fixtures::mock_standings;
        use marqueet_core::widgets::standings_view;
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        let view = standings_view(&mock_standings(now), &[TeamId("mock:nfl:NYS".into())], None).unwrap();
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        draw(
            &mut c,
            &mut fonts,
            &[WidgetView::Standings(view.clone()), WidgetView::Standings(view)],
            WidgetLayout::WideLeft,
            &Theme::default(),
            &Logos::new(),
            0,
        );
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        let card = cards[0];
        let has = |color: Rgb| {
            (card.x as u32..(card.x + card.w) as u32)
                .any(|x| (150..640).any(|y| c.pixel(x, y)[..3] == [color.r, color.g, color.b]))
        };
        let p = Theme::default().palette;
        assert!(has(p.chip()) && has(p.accent), "highlighted favorite row");
    }

    #[test]
    fn draws_the_weather_card_with_icons() {
        use marqueet_core::weather::mock_weather;
        use marqueet_core::widgets::weather_view;
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        let view = WidgetView::Weather(weather_view(&mock_weather(now)));
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        draw(&mut c, &mut fonts, &[view.clone(), view], WidgetLayout::WideLeft, &Theme::default(), &Logos::new(), 0);
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        let (big, small) = (cards[0], cards[1]);
        for card in [big, small] {
            let has = |color: Rgb| {
                (card.x as u32..(card.x + card.w) as u32)
                    .any(|x| (60..640).any(|y| c.pixel(x, y)[..3] == [color.r, color.g, color.b]))
            };
            assert!(has(crate::weather::SUN), "sun icon");
            assert!(has(crate::weather::RAIN), "rain in the forecast");
        }
    }

    #[test]
    fn draws_the_fantasy_matchup() {
        use marqueet_core::fantasy::mock_matchup;
        use marqueet_core::widgets::fantasy_view;
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        let view = WidgetView::Fantasy(fantasy_view(&mock_matchup(now)));
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        let theme = Theme::default();
        draw(&mut c, &mut fonts, &[view.clone(), view], WidgetLayout::WideLeft, &theme, &Logos::new(), 0);
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        let a = theme.palette.accent;
        for card in cards {
            let amber = (card.x as u32..(card.x + card.w) as u32)
                .any(|x| (100..260).any(|y| c.pixel(x, y)[..3] == [a.r, a.g, a.b]));
            assert!(amber, "the leader's LED total");
        }
    }

    #[test]
    fn ellipsize_ends() {
        let mut fonts = Fonts::new();
        let long = "(Shotgun) run up the middle to the 38 for 5 yards. PENALTY on the defense, holding, 10 yards.";
        let room = fonts.measure(Face::SemiBold, 30.0, 0.0, "run up the middle");
        let cut = ellipsize(&mut fonts, Face::SemiBold, 30.0, long, room);
        assert!(cut.ends_with('…') && fonts.measure(Face::SemiBold, 30.0, 0.0, &cut) <= room, "{cut}");
        assert_eq!(ellipsize(&mut fonts, Face::SemiBold, 30.0, "short", 1000.0), "short");
        assert_eq!(ellipsize(&mut fonts, Face::SemiBold, 30.0, long, 1.0).chars().count(), 5, "stops at a few chars");
    }
}
