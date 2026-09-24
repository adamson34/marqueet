//! Draws widget views (from `marqueet_core::widgets`) as flat UI on the
//! canvas (ADR-0007), in the configured theme (ADR-0012). Each style draws
//! its own game of the day and scores list (`theme::*`); standings, weather
//! and fantasy share one drawing here, in the theme's colors and faces.

use marqueet_core::config::WidgetLayout;
use marqueet_core::theme::Theme;
use marqueet_core::ticker::Logos;
use marqueet_core::widgets::{FantasyView, StandingsView, WidgetView};

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
) {
    let kit = Kit::new(theme).with_logos(logos);
    canvas.clear();
    canvas.fill_rect(0, 0, canvas.width as i32, canvas.height as i32, kit.p.ground);
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
            WidgetView::Empty { title, message } => empty(canvas, fonts, &kit, card, s, title, message),
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
            .map(|c| (c.team.id.clone(), TeamArt { label: String::new(), colors: None, logo: Some(badge()) }))
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
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &Theme::preset(style), &logos);
            assert!(has_in(&c, cards[0], green), "{style:?} game of the day");
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &Theme::preset(style), &Logos::new());
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
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new());
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
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new());
            assert!(has_in(&c, cards[0], away) && has_in(&c, cards[0], home), "{style:?}");
            theme.team_colors = false;
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new());
            assert!(!has_in(&c, cards[0], away) && !has_in(&c, cards[0], home), "{style:?} palette only");
        }
    }

    #[test]
    fn ballpark_hangs_number_plates() {
        let mut fonts = Fonts::new();
        let theme = Theme::preset(Style::Ballpark);
        let mut c = Canvas::new(1920, 648);
        draw(&mut c, &mut fonts, &views(), WidgetLayout::WideLeft, &theme, &Logos::new());
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
                    draw(&mut c, &mut fonts, &v, layout, &Theme::preset(style), &Logos::new());
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
            draw(&mut c, &mut fonts, &views(), WidgetLayout::WideLeft, &Theme::preset(style), &Logos::new());
            crate::screenshot::write_png(&dir.join(format!("{}.png", style.id())), (1920, 648), &c.data).unwrap();
            draw(&mut c, &mut fonts, &with, WidgetLayout::WideLeft, &Theme::preset(style), &logos);
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
            draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new()); // warm the glyph cache
            let t = std::time::Instant::now();
            for _ in 0..20 {
                draw(&mut c, &mut fonts, &v, WidgetLayout::WideLeft, &theme, &Logos::new());
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
        draw(&mut c, &mut fonts, &[view.clone(), view], WidgetLayout::WideLeft, &Theme::default(), &Logos::new());
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
        draw(&mut c, &mut fonts, &[view.clone(), view], WidgetLayout::WideLeft, &theme, &Logos::new());
        let (_, cards) = slots(1920, 648, WidgetLayout::WideLeft);
        let a = theme.palette.accent;
        for card in cards {
            let amber = (card.x as u32..(card.x + card.w) as u32)
                .any(|x| (100..260).any(|y| c.pixel(x, y)[..3] == [a.r, a.g, a.b]));
            assert!(amber, "the leader's LED total");
        }
    }
}
