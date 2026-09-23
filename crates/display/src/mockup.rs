//! CONCEPT MOCKUP of planned widgets and takeovers (Phases 3 and 4).
//!
//! Enabled with `--mockup`. Everything here is hard-coded sample content,
//! drawn with the real LED renderer, to show what the widget area is meant
//! to look like. It is not the widget system; that arrives in Phase 4 and
//! this module will be deleted then.

use tickadee_core::Rgb;
use tickadee_core::color::led_team_color;
use tickadee_core::font::BitmapFont;
use tickadee_core::layout::{LedGrid, Rect};
use tickadee_core::sports::{Game, HomeAway};
use tickadee_core::ticker::{Part, Span, TickerSegment, Tint};

/// When the sample touchdown takeover starts and how long it lasts.
pub const TAKEOVER_AT: f64 = 3.0;
pub const TAKEOVER_SECS: f64 = 4.5;
/// The game the mockup features (and scores in the takeover).
pub const FEATURED_GAME: &str = "mock:nfl:1";

/// Rows per widget text line (small font plus a blank row).
pub const LINE_ROWS: u32 = 9;
const LEFT_LINES: u32 = 11;

/// Where each widget line goes.
#[derive(Debug, Clone)]
pub struct WidgetLayout {
    pub left: Vec<LedGrid>,
    pub right: Vec<LedGrid>,
    pub takeover_title: LedGrid,
    pub takeover_lines: Vec<LedGrid>,
}

fn line_grids(area: Rect, lines: u32, line_h: u32) -> Vec<LedGrid> {
    (0..lines)
        .map(|i| LedGrid::fit(Rect { x: area.x, y: area.y + i * line_h, w: area.w, h: line_h }, LINE_ROWS))
        .collect()
}

pub fn layout(widgets: Rect) -> WidgetLayout {
    let margin_x = widgets.w / 40;
    let margin_y = widgets.h / 20;
    let inner = Rect {
        x: widgets.x + margin_x,
        y: widgets.y + margin_y,
        w: widgets.w - 2 * margin_x,
        h: widgets.h - 2 * margin_y,
    };
    let line_h = inner.h / LEFT_LINES;
    let gutter = inner.w / 30;
    let left_w = inner.w * 56 / 100;
    let left = Rect { w: left_w, ..inner };
    let right = Rect { x: inner.x + left_w + gutter, w: inner.w - left_w - gutter, ..inner };

    // Takeover: one big line (large font) over three small ones.
    let title_rows = 18;
    let title_h = inner.h * 45 / 100;
    // Wide enough for "TOUCHDOWN" in the large font (106 LEDs) plus margin.
    let title = LedGrid::fit_centered(Rect { h: title_h, ..inner }, 116, title_rows);
    let small_h = (inner.h - title_h) / 3;
    let lines = line_grids(Rect { y: inner.y + title_h, h: small_h * 3, ..inner }, 3, small_h);
    WidgetLayout {
        left: line_grids(left, LEFT_LINES, line_h),
        right: line_grids(right, LEFT_LINES, line_h),
        takeover_title: title,
        takeover_lines: lines,
    }
}

fn seg(id: String, parts: Vec<Part>) -> TickerSegment {
    TickerSegment { id, parts }
}

fn text(s: &str, tint: Tint) -> Part {
    Part::text(vec![Span::new(s, tint)])
}

/// A fixed-width table cell: right-aligned `s` padded to `width` LEDs.
fn cell_right(s: &str, width: u32, tint: Tint) -> Vec<Part> {
    let w = BitmapFont::small().text_width(s);
    vec![Part::gap(width.saturating_sub(w) as u16), text(s, tint)]
}

/// A fixed-width table cell: left-aligned `s` padded to `width` LEDs.
fn cell_left(s: &str, width: u32, tint: Tint) -> Vec<Part> {
    let w = BitmapFont::small().text_width(s);
    vec![text(s, tint), Part::gap(width.saturating_sub(w) as u16)]
}

fn team_tint(game: &Game, side: HomeAway) -> Tint {
    let c = &game.competitor(side).team.colors;
    Tint::Color(led_team_color(c.primary, c.secondary))
}

/// Left card: game of the day with a line score.
pub fn game_of_the_day(game: &Game) -> Vec<TickerSegment> {
    let clock = format!("{} {}", game.clock.period_label, game.clock.clock.as_deref().unwrap_or(""));
    // Sample quarter-by-quarter scores that add up to the live totals.
    let line = |side: HomeAway| -> Vec<String> {
        let total = game.competitor(side).score.unwrap_or(0);
        let q = match side {
            HomeAway::Away => [7, 3],
            HomeAway::Home => [7, 14],
        };
        let q3 = total.saturating_sub(q[0] + q[1]);
        vec![q[0].to_string(), q[1].to_string(), q3.to_string(), "-".into(), total.to_string()]
    };
    let row = |label: &str, tint: Tint, cells: Vec<String>, cell_tint: Tint| {
        let mut parts = cell_left(label, 26, tint);
        for (i, c) in cells.iter().enumerate() {
            let last = i == cells.len() - 1;
            parts.extend(cell_right(c, if last { 22 } else { 16 }, if last { Tint::Primary } else { cell_tint }));
        }
        parts
    };
    let head = ["1", "2", "3", "4", "T"].map(String::from).to_vec();
    let away = &game.away.team.abbreviation;
    let home = &game.home.team.abbreviation;
    let lines = vec![
        vec![text("GAME OF THE DAY", Tint::Accent)],
        vec![text("NFL  ", Tint::Dim), text(&clock, Tint::Primary), text("  CBS", Tint::Dim)],
        vec![],
        row("", Tint::Dim, head, Tint::Dim),
        row(away, team_tint(game, HomeAway::Away), line(HomeAway::Away), Tint::Dim),
        row(home, team_tint(game, HomeAway::Home), line(HomeAway::Home), Tint::Dim),
        vec![],
        vec![text("2ND & 6 AT KC 14", Tint::Primary)],
        vec![text(&format!("{home} BALL  "), Tint::Primary), text("RED ZONE", Tint::Accent)],
        vec![],
        vec![text("LAST: ALLEN 18 YD RUN", Tint::Dim)],
    ];
    lines.into_iter().enumerate().map(|(i, p)| seg(format!("gotd:{i}"), p)).collect()
}

/// Right card: standings, then a fantasy matchup.
pub fn standings_and_fantasy() -> Vec<TickerSegment> {
    let rec = |team: &str, color: Rgb, w: &str| {
        let mut p = cell_left(team, 24, Tint::Color(color));
        p.extend(cell_right(w, 22, Tint::Primary));
        p
    };
    let pts = |name: &str, v: &str, tint: Tint| {
        let mut p = cell_left(name, 58, tint);
        p.extend(cell_right(v, 28, Tint::Primary));
        p
    };
    let lines = vec![
        vec![text("AFC EAST", Tint::Accent)],
        rec("BUF", Rgb::new(0, 92, 255), "3-0"),
        rec("MIA", Rgb::new(0, 142, 151), "2-1"),
        rec("NYJ", Rgb::new(18, 200, 110), "1-2"),
        rec("NE", Rgb::new(90, 150, 255), "0-3"),
        vec![],
        vec![text("FANTASY  WK 3", Tint::Accent)],
        pts("YOU", "93.4", Tint::Primary),
        pts("RIVAL", "72.1", Tint::Dim),
        pts("ALLEN QB", "30.6", Tint::Primary),
        pts("KELCE TE", "11.2", Tint::Dim),
    ];
    lines.into_iter().enumerate().map(|(i, p)| seg(format!("side:{i}"), p)).collect()
}

/// The touchdown takeover: a big title and three detail lines.
pub fn takeover(game: &Game) -> (TickerSegment, Vec<TickerSegment>) {
    let home = &game.home.team.abbreviation;
    let away = &game.away.team.abbreviation;
    let score = format!("{home} {}   {away} {}", game.home.score.unwrap_or(0), game.away.score.unwrap_or(0));
    let title = seg("takeover:title".into(), vec![text("TOUCHDOWN", team_tint(game, HomeAway::Home))]);
    let lines = vec![
        seg("takeover:0".into(), vec![text(&score, Tint::Primary)]),
        seg("takeover:1".into(), vec![text("J. ALLEN 12 YD RUN", Tint::Primary)]),
        seg("takeover:2".into(), vec![text("YOUR PLAYER  +6.0 PTS", Tint::Accent)]),
    ];
    (title, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tickadee_core::sports::fixtures::mock_games;
    use tickadee_core::sports::ticker::segment_text;

    fn featured() -> Game {
        mock_games(Utc::now()).into_iter().find(|g| g.id.0 == FEATURED_GAME).unwrap()
    }

    #[test]
    fn line_score_adds_up() {
        let lines = game_of_the_day(&featured());
        assert_eq!(lines.len() as u32, LEFT_LINES);
        let buf = segment_text(&lines[5]);
        assert!(buf.starts_with("BUF"), "{buf}");
        assert!(buf.ends_with("7 14 0 - 21"), "{buf}");
    }

    #[test]
    fn layout_fits_the_widget_area() {
        for (w, h) in [(1920, 720), (1366, 512), (1024, 512)] {
            let area = Rect { x: 0, y: 360, w, h };
            let l = layout(area);
            for g in l.left.iter().chain(&l.right).chain(&l.takeover_lines).chain([&l.takeover_title]) {
                assert!(g.band.bottom() <= area.bottom() && g.band.x + g.band.w <= w, "{w}x{h}");
                assert!(g.pitch >= 3, "{w}x{h}");
            }
        }
    }
}
