//! Draws widget views (from `marqueet_core::widgets`) as flat cards on the
//! UI canvas (ADR-0007). Layout: a large card on the left, a narrower one on
//! the right; more presets arrive with settings.

use marqueet_core::Rgb;
use marqueet_core::widgets::{GameOfTheDay, ScoreRow, Scores, StandingsView, Tone, WidgetView};

use crate::ui::{Align, Canvas, Fonts, Paint, TextStyle, Weight};

pub const CARD: Rgb = Rgb::new(0x1b, 0x1c, 0x21);
pub const CARD_EDGE: Rgb = Rgb::new(0x2a, 0x2b, 0x31);
const CHIP: Rgb = Rgb::new(0x26, 0x27, 0x2d);
pub const AMBER: Rgb = Rgb::new(0xf2, 0xa9, 0x3b);
pub const MUTED: Rgb = Rgb::new(0x8b, 0x8d, 0x96);
pub const SOFT: Rgb = Rgb::new(0xd6, 0xd7, 0xdb);
const LIVE: Rgb = Rgb::new(0xe5, 0x48, 0x3b);
const FINAL: Rgb = Rgb::new(0x6f, 0xcf, 0x8e);
const LOST: Rgb = Rgb::new(0x9a, 0x9c, 0xa4);

/// A card's rectangle in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Card {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Two-slot layout: 60% / 40% with margins, scaled to the canvas.
pub fn slots(width: u32, height: u32) -> (f32, [Card; 2]) {
    let s = (width as f32 / 1920.0).min(height as f32 / 648.0);
    let (m, gap) = (40.0 * s, 30.0 * s);
    let (top, h) = (26.0 * s, height as f32 - 52.0 * s);
    let inner = width as f32 - 2.0 * m - gap;
    let left = inner * 0.625;
    (s, [Card { x: m, y: top, w: left, h }, Card { x: m + left + gap, y: top, w: inner - left, h }])
}

pub fn draw(canvas: &mut Canvas, fonts: &mut Fonts, views: &[WidgetView]) {
    canvas.clear();
    let (s, cards) = slots(canvas.width, canvas.height);
    for (view, card) in views.iter().zip(cards) {
        canvas.fill_round_rect(card.x - s, card.y - s, card.w + 2.0 * s, card.h + 2.0 * s, 19.0 * s, CARD_EDGE);
        canvas.fill_round_rect(card.x, card.y, card.w, card.h, 18.0 * s, CARD);
        match view {
            WidgetView::GameOfTheDay(g) => game_of_the_day(canvas, fonts, card, s, g),
            WidgetView::Scores(sc) => scores(canvas, fonts, card, s, sc),
            WidgetView::Standings(st) => standings(canvas, fonts, card, s, st),
            WidgetView::Weather(w) => crate::weather::draw(canvas, fonts, card, s, w, CARD),
            WidgetView::Empty { title, message } => empty(canvas, fonts, card, s, title, message),
        }
    }
}

pub fn title(canvas: &mut Canvas, fonts: &mut Fonts, card: Card, s: f32, text: &str) {
    let style = TextStyle::new(Weight::SemiBold, 34.0 * s, AMBER).tracking(3.0 * s);
    canvas.text(fonts, card.x + 40.0 * s, card.y + 62.0 * s, style, text);
}

fn empty(canvas: &mut Canvas, fonts: &mut Fonts, card: Card, s: f32, heading: &str, message: &str) {
    title(canvas, fonts, card, s, heading);
    let style = TextStyle::new(Weight::Medium, 34.0 * s, MUTED).align(Align::Center);
    canvas.text(fonts, card.x + card.w / 2.0, card.y + card.h / 2.0, style, message);
}

fn game_of_the_day(canvas: &mut Canvas, fonts: &mut Fonts, card: Card, s: f32, g: &GameOfTheDay) {
    let (x, y, w) = (card.x, card.y, card.w);
    title(canvas, fonts, card, s, "GAME OF THE DAY");
    let status = TextStyle::new(Weight::SemiBold, 30.0 * s, if g.live { SOFT } else { MUTED }).align(Align::Right);
    canvas.text(fonts, x + w - 40.0 * s, y + 62.0 * s, status, &g.status);

    // Teams at the sides, big LED scores stacked in the middle.
    let big = |lost| TextStyle::new(Weight::SemiBold, 116.0 * s, if lost { LOST } else { Rgb::WHITE });
    let detail = TextStyle::new(Weight::Medium, 29.0 * s, MUTED);
    canvas.text(fonts, x + 40.0 * s, y + 240.0 * s, big(g.away.lost), &g.away.abbr);
    canvas.text(fonts, x + 44.0 * s, y + 284.0 * s, detail, &g.away.detail);
    canvas.text(fonts, x + w - 40.0 * s, y + 240.0 * s, big(g.home.lost).align(Align::Right), &g.home.abbr);
    canvas.text(fonts, x + w - 44.0 * s, y + 284.0 * s, detail.align(Align::Right), &g.home.detail);

    let px = 14.0 * s;
    if g.away.score.is_none() && g.home.score.is_none() {
        // Not started: no scores yet.
        let vs = TextStyle::new(Weight::SemiBold, 64.0 * s, MUTED).align(Align::Center);
        canvas.text(fonts, x + w / 2.0, y + 232.0 * s, vs, "VS");
    } else {
        for (i, side) in [&g.away, &g.home].into_iter().enumerate() {
            let text = side.score.map_or_else(|| "0".to_owned(), |v| v.to_string());
            let lw = Canvas::led_width(px, &text);
            let color = if side.lost { LOST } else { AMBER };
            canvas.led_text(x + w / 2.0 - lw / 2.0, y + 112.0 * s + i as f32 * 128.0 * s, px, color, true, &text);
        }
    }

    // Situation chips, centered.
    let chip_style = TextStyle::new(Weight::SemiBold, 28.0 * s, Rgb::WHITE);
    let widths: Vec<f32> =
        g.chips.iter().map(|c| fonts.measure(chip_style.weight, chip_style.size, 0.0, c) + 32.0 * s).collect();
    let total = widths.iter().sum::<f32>() + 16.0 * s * g.chips.len().saturating_sub(1) as f32;
    let mut cx = x + w / 2.0 - total / 2.0;
    let chip_y = y + 348.0 * s;
    for (chip, cw) in g.chips.iter().zip(&widths) {
        canvas.fill_round_rect(cx, chip_y, *cw, 48.0 * s, 8.0 * s, CHIP);
        canvas.text(fonts, cx + cw / 2.0, chip_y + 34.0 * s, chip_style.align(Align::Center), chip);
        cx += cw + 16.0 * s;
    }

    // Line score.
    let ty = y + 432.0 * s;
    if ty + 120.0 * s > card.y + card.h {
        return; // Too short to show the table.
    }
    canvas.fill_rect((x + 40.0 * s) as i32, ty as i32, (w - 80.0 * s) as i32, (2.0 * s).max(1.0) as i32, CARD_EDGE);
    let first = x + 40.0 * s;
    let cols_start = x + 40.0 * s + 150.0 * s;
    let cols_end = x + w - 56.0 * s;
    let n = g.columns.len().max(1) as f32;
    let col_x = |i: usize| cols_start + (cols_end - cols_start) * (i as f32 + 0.5) / n;
    let head = TextStyle::new(Weight::Medium, 26.0 * s, MUTED);
    canvas.text(fonts, first, ty + 40.0 * s, head, "TEAM");
    for (i, c) in g.columns.iter().enumerate() {
        canvas.text(fonts, col_x(i), ty + 40.0 * s, head.align(Align::Center), c);
    }
    for (r, (team, cells)) in g.rows.iter().enumerate() {
        let ry = ty + 80.0 * s + r as f32 * 40.0 * s;
        canvas.text(fonts, first, ry, TextStyle::new(Weight::SemiBold, 28.0 * s, Rgb::WHITE), team);
        for (i, cell) in cells.iter().enumerate() {
            let total = i + 1 == cells.len();
            let color = if total { Rgb::WHITE } else { SOFT };
            let weight = if total { Weight::SemiBold } else { Weight::Medium };
            canvas.text(fonts, col_x(i), ry, TextStyle::new(weight, 28.0 * s, color).align(Align::Center), cell);
        }
    }
}

fn tone_color(t: Tone) -> Paint {
    Paint::solid(match t {
        Tone::Live => LIVE,
        Tone::Break => AMBER,
        Tone::Final => FINAL,
        Tone::Upcoming => MUTED,
    })
}

/// How many score rows fit in a card of height `h` at scale `s`.
pub fn rows_that_fit(h: f32, s: f32) -> usize {
    ((h - 100.0 * s) / (66.0 * s)).floor().max(0.0) as usize
}

fn scores(canvas: &mut Canvas, fonts: &mut Fonts, card: Card, s: f32, sc: &Scores) {
    title(canvas, fonts, card, s, &sc.title);
    let (x, w) = (card.x, card.w);
    let fit = rows_that_fit(card.h, s);
    if sc.rows.is_empty() {
        let style = TextStyle::new(Weight::Medium, 30.0 * s, MUTED).align(Align::Center);
        canvas.text(fonts, x + w / 2.0, card.y + card.h / 2.0, style, "No other games");
        return;
    }
    for (i, row) in sc.rows.iter().take(fit).enumerate() {
        let ry = card.y + 124.0 * s + i as f32 * 66.0 * s;
        score_row(canvas, fonts, x, w, ry, s, row);
        if i + 1 < sc.rows.len().min(fit) {
            let line_y = ry + 24.0 * s;
            canvas.fill_rect((x + 36.0 * s) as i32, line_y as i32, (w - 72.0 * s) as i32, s.max(1.0) as i32, CARD_EDGE);
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

fn standings(canvas: &mut Canvas, fonts: &mut Fonts, card: Card, s: f32, st: &StandingsView) {
    let (x, y, w) = (card.x, card.y, card.w);
    title(canvas, fonts, card, s, &st.title);
    let group = TextStyle::new(Weight::SemiBold, 28.0 * s, SOFT).align(Align::Right);
    canvas.text(fonts, x + w - 40.0 * s, y + 62.0 * s, group, &st.group);

    let fit = standings_rows_that_fit(card.h, s);
    let start = standings_window(st.rows.len(), fit, st.focus);
    let (left, right) = (x + 36.0 * s, x + w - 36.0 * s);
    let team_x = left + 44.0 * s;
    let cols_start = x + w * 0.4;
    let n = st.columns.len().max(1) as f32;
    let col_x = |i: usize| cols_start + (right - cols_start) * (i as f32 + 0.5) / n;

    let head_y = y + 118.0 * s;
    let head = TextStyle::new(Weight::Medium, 24.0 * s, MUTED).tracking(1.0 * s);
    canvas.text(fonts, team_x, head_y, head, "TEAM");
    for (i, c) in st.columns.iter().enumerate() {
        canvas.text(fonts, col_x(i), head_y, head.align(Align::Center), c);
    }
    canvas.fill_rect(left as i32, (head_y + 14.0 * s) as i32, (right - left) as i32, s.max(1.0) as i32, CARD_EDGE);

    for (i, row) in st.rows.iter().skip(start).take(fit).enumerate() {
        let ry = head_y + 60.0 * s + i as f32 * 54.0 * s;
        if row.highlight {
            canvas.fill_round_rect(left - 12.0 * s, ry - 36.0 * s, right - left + 24.0 * s, 50.0 * s, 8.0 * s, CHIP);
        }
        let rank = TextStyle::new(Weight::Medium, 26.0 * s, MUTED).align(Align::Right);
        canvas.text(fonts, team_x - 16.0 * s, ry, rank, &row.rank.to_string());
        let team = TextStyle::new(Weight::SemiBold, 30.0 * s, if row.highlight { AMBER } else { Rgb::WHITE });
        canvas.text(fonts, team_x, ry, team, &row.team);
        for (c, cell) in row.cells.iter().enumerate() {
            let last = c + 1 == row.cells.len();
            let style = TextStyle::new(if last { Weight::SemiBold } else { Weight::Medium }, 28.0 * s, SOFT);
            let style = if last { TextStyle { paint: Paint::solid(Rgb::WHITE), ..style } } else { style };
            canvas.text(fonts, col_x(c), ry, style.align(Align::Center), cell);
        }
    }
}

fn score_row(canvas: &mut Canvas, fonts: &mut Fonts, x: f32, w: f32, y: f32, s: f32, row: &ScoreRow) {
    let league = TextStyle::new(Weight::Medium, 23.0 * s, MUTED).tracking(1.5 * s);
    let team = TextStyle::new(Weight::SemiBold, 31.0 * s, Rgb::WHITE);
    canvas.text(fonts, x + 36.0 * s, y, league, &row.league);
    let teams_x = x + 36.0 * s + (w - 72.0 * s) * 0.2;
    let col = (w - 72.0 * s) * 0.28;
    canvas.text(fonts, teams_x, y, team, &row.away);
    canvas.text(fonts, teams_x + col, y, team, &row.home);
    let status = TextStyle { paint: tone_color(row.tone), ..TextStyle::new(Weight::SemiBold, 27.0 * s, MUTED) };
    canvas.text(fonts, x + w - 36.0 * s, y, status.align(Align::Right), &row.status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};
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
            let (_, [a, b]) = slots(w, h);
            assert!(a.x > 0.0 && b.x + b.w < w as f32 && a.y > 0.0 && a.y + a.h < h as f32, "{w}x{h}");
            assert!(a.w > b.w, "game of the day gets the big card");
        }
    }

    #[test]
    fn draws_cards_with_led_scores() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        draw(&mut c, &mut fonts, &views());
        assert_eq!(c.pixel(20, 324)[3], 0, "margin stays transparent");
        let card_px = c.pixel(100, 600);
        assert_eq!([card_px[0], card_px[1], card_px[2]], [CARD.r, CARD.g, CARD.b]);
        let (_, [gotd, _]) = slots(1920, 648);
        let mid = (gotd.x + gotd.w / 2.0) as u32;
        let amber = (mid - 60..mid + 60).any(|x| (130..380).any(|y| c.pixel(x, y)[..3] == [AMBER.r, AMBER.g, AMBER.b]));
        assert!(amber, "LED score digits in the middle of the game card");
    }

    /// Redraw cost; run with `cargo test --release -p marqueet-display redraw_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing"]
    fn redraw_cost() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        let v = views();
        draw(&mut c, &mut fonts, &v); // warm the glyph cache
        let t = std::time::Instant::now();
        for _ in 0..20 {
            draw(&mut c, &mut fonts, &v);
        }
        println!("widget redraw 1920x648: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
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
        let view = standings_view(&mock_standings(now), &[TeamId("mock:nfl:NYJ".into())], None).unwrap();
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        draw(&mut c, &mut fonts, &[WidgetView::Standings(view.clone()), WidgetView::Standings(view)]);
        let (_, [card, _]) = slots(1920, 648);
        let has = |color: Rgb| {
            (card.x as u32..(card.x + card.w) as u32)
                .any(|x| (150..640).any(|y| c.pixel(x, y)[..3] == [color.r, color.g, color.b]))
        };
        assert!(has(CHIP) && has(AMBER), "highlighted favorite row");
    }

    #[test]
    fn draws_the_weather_card_with_icons() {
        use marqueet_core::weather::mock_weather;
        use marqueet_core::widgets::weather_view;
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        let view = WidgetView::Weather(weather_view(&mock_weather(now)));
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        draw(&mut c, &mut fonts, &[view.clone(), view]);
        let (_, [big, small]) = slots(1920, 648);
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
    fn score_rows_are_capped_to_what_fits() {
        assert_eq!(rows_that_fit(596.0, 1.0), 7);
        assert_eq!(rows_that_fit(80.0, 1.0), 0);
    }
}
