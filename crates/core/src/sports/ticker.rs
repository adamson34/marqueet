//! Formats games as ticker segments.

use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};

use super::{Competitor, Game, GameStatus, InningHalf, Situation, Sport};
use crate::color::led_team_color;
use crate::ticker::{Align, Part, Span, TickerSegment, Tint};

#[derive(Clone, Copy, Debug)]
pub struct FormatOptions {
    /// Viewer's time zone for start times.
    pub tz: FixedOffset,
    /// "Now", for deciding whether to show a weekday on start times.
    pub now: DateTime<Utc>,
}

/// Display label for a league key.
pub fn league_label(league: &str) -> String {
    match league {
        "ncaaf" => "NCAAF".into(),
        "ncaam" => "NCAAM".into(),
        "ncaaw" => "NCAAW".into(),
        other => other.to_uppercase(),
    }
}

/// Main ticker content: each league's games under a league header, live
/// games first, then finals, then upcoming.
pub fn ticker_segments(games: &[Game], opts: &FormatOptions) -> Vec<TickerSegment> {
    let mut leagues: Vec<&str> = Vec::new();
    for g in games {
        if !leagues.contains(&g.league.as_str()) {
            leagues.push(g.league.as_str());
        }
    }
    let mut out = Vec::new();
    for league in leagues {
        let mut in_league: Vec<&Game> = games.iter().filter(|g| g.league.as_str() == league).collect();
        in_league.sort_by_key(|g| (status_rank(g.status), g.start_time));
        out.push(TickerSegment {
            id: format!("league:{league}"),
            parts: vec![Part::text(vec![Span::new(league_label(league), Tint::Accent)])],
        });
        out.extend(in_league.into_iter().map(|g| game_segment(g, opts)));
    }
    out
}

fn status_rank(s: GameStatus) -> u8 {
    match s {
        GameStatus::InProgress | GameStatus::Halftime | GameStatus::Delayed => 0,
        GameStatus::Final => 1,
        GameStatus::Scheduled => 2,
        GameStatus::Postponed | GameStatus::Canceled => 3,
    }
}

/// One game: team abbreviations stacked, scores stacked, status stacked.
pub fn game_segment(game: &Game, opts: &FormatOptions) -> TickerSegment {
    let final_ = game.status == GameStatus::Final;
    let loser_dim = |c: &Competitor| final_ && c.winner == Some(false);

    let team_line = |c: &Competitor| {
        let mut spans = Vec::new();
        if let Some(rank) = c.team.rank {
            spans.push(Span::dim(format!("{rank} ")));
        }
        let tint = if loser_dim(c) {
            Tint::Dim
        } else {
            Tint::Color(led_team_color(c.team.colors.primary, c.team.colors.secondary))
        };
        spans.push(Span::new(c.team.abbreviation.clone(), tint));
        if let Some(marker) = possession_marker(game, c) {
            spans.push(marker);
        }
        spans
    };

    let mut parts = vec![Part::stack(team_line(&game.away), team_line(&game.home), Align::Left)];

    if let (Some(a), Some(h)) = (game.away.score, game.home.score) {
        let score =
            |c: &Competitor, v: u16| Span::new(v.to_string(), if loser_dim(c) { Tint::Dim } else { Tint::Primary });
        parts.push(Part::gap(4));
        parts.push(Part::stack(vec![score(&game.away, a)], vec![score(&game.home, h)], Align::Right));
    }

    let (top, bottom) = status_lines(game, opts);
    parts.push(Part::gap(5));
    parts.push(Part::stack(top, bottom, Align::Left));

    TickerSegment { id: game.id.0.clone(), parts }
}

fn possession_marker(game: &Game, c: &Competitor) -> Option<Span> {
    match &game.situation {
        Some(Situation::Football { possession: Some(p), red_zone, .. }) if *p == c.team.id => {
            Some(Span::new(" ◀", if *red_zone { Tint::Accent } else { Tint::Primary }))
        }
        Some(Situation::Hockey { power_play: Some(p) }) if *p == c.team.id => Some(Span::new(" PP", Tint::Accent)),
        _ => None,
    }
}

fn status_lines(game: &Game, opts: &FormatOptions) -> (Vec<Span>, Vec<Span>) {
    let c = &game.clock;
    match game.status {
        GameStatus::InProgress => match (&game.situation, game.sport) {
            (Some(Situation::Baseball { half, outs, .. }), _) => {
                let arrow = if *half == InningHalf::Top { '▲' } else { '▼' };
                let outs = if *outs == 1 { "1 OUT".to_owned() } else { format!("{outs} OUTS") };
                (vec![Span::primary(format!("{arrow}{}", c.period))], vec![Span::dim(outs)])
            }
            (_, Sport::Baseball) => (vec![Span::primary(c.period_label.clone())], vec![]),
            _ => (
                vec![Span::primary(c.period_label.clone())],
                c.clock.clone().map(|t| vec![Span::dim(t)]).unwrap_or_default(),
            ),
        },
        GameStatus::Halftime => (vec![Span::primary("HALF")], vec![]),
        GameStatus::Delayed => (vec![Span::new("DELAY", Tint::Accent)], vec![Span::dim(c.period_label.clone())]),
        GameStatus::Final => {
            let label = final_label(&c.detail);
            (vec![Span::primary(label)], vec![])
        }
        GameStatus::Scheduled => {
            let (day, time) = start_labels(game.start_time, opts);
            let top = match day {
                Some(day) => vec![Span::dim(format!("{day} ")), Span::primary(time)],
                None => vec![Span::primary(time)],
            };
            (top, game.broadcast.clone().map(|b| vec![Span::dim(b)]).unwrap_or_default())
        }
        GameStatus::Postponed => (vec![Span::dim("PPD")], vec![]),
        GameStatus::Canceled => (vec![Span::dim("CANC")], vec![]),
    }
}

/// "Final/OT" → "F/OT"; anything else → "FINAL".
fn final_label(detail: &str) -> String {
    let upper = detail.to_uppercase();
    match upper.strip_prefix("FINAL/") {
        Some(rest) if !rest.is_empty() && rest.len() <= 4 => format!("F/{rest}"),
        _ => "FINAL".into(),
    }
}

/// ("SUN", "1:00PM"), or (None, time) for games later today.
fn start_labels(start: DateTime<Utc>, opts: &FormatOptions) -> (Option<String>, String) {
    let local = start.with_timezone(&opts.tz);
    let today = opts.now.with_timezone(&opts.tz).date_naive();
    let time = local.format("%-I:%M%p").to_string();
    let day = (local.date_naive() != today).then(|| local.weekday().to_string().to_uppercase());
    (day, time)
}

/// Crawl content: upcoming games in start order, one segment each. Spans are
/// league (accent), matchup (primary), start time and TV (dim); the display
/// sets them as flat text after the tag from [`crawl_label`].
pub fn crawl_segments(games: &[Game], opts: &FormatOptions) -> Vec<TickerSegment> {
    let mut upcoming: Vec<&Game> = games.iter().filter(|g| g.status == GameStatus::Scheduled).collect();
    upcoming.sort_by_key(|g| g.start_time);
    upcoming
        .into_iter()
        .map(|g| {
            let local = g.start_time.with_timezone(&opts.tz);
            let today = opts.now.with_timezone(&opts.tz).date_naive();
            let time = local.format("%-I:%M %p").to_string();
            let when = if local.date_naive() == today {
                time
            } else {
                format!("{} {time}", local.weekday().to_string().to_uppercase())
            };
            let mut spans = vec![
                Span::new(league_label(g.league.as_str()), Tint::Accent),
                Span::primary(format!(" {} at {}", g.away.team.abbreviation, g.home.team.abbreviation)),
                Span::dim(format!("  {when}")),
            ];
            if let Some(b) = &g.broadcast {
                spans.push(Span::dim(format!("  {b}")));
            }
            TickerSegment { id: format!("crawl:{}", g.id.0), parts: vec![Part::text(spans)] }
        })
        .collect()
}

/// The tag in front of the crawl: TONIGHT when the next game starts this
/// evening, TODAY when it's earlier today, otherwise UP NEXT. `None` when
/// nothing is scheduled.
pub fn crawl_label(games: &[Game], opts: &FormatOptions) -> Option<String> {
    let next = games.iter().filter(|g| g.status == GameStatus::Scheduled).map(|g| g.start_time).min()?;
    let local = next.with_timezone(&opts.tz);
    let today = local.date_naive() == opts.now.with_timezone(&opts.tz).date_naive();
    Some(
        match (today, local.hour() >= 17) {
            (true, true) => "TONIGHT",
            (true, false) => "TODAY",
            (false, _) => "UP NEXT",
        }
        .into(),
    )
}

/// Flattens a segment to plain text, for tests and logging. Stacks render as
/// `top/bottom`, gaps as a single space, icons as `[name]`; logos are left
/// out.
pub fn segment_text(seg: &TickerSegment) -> String {
    let join = |spans: &[Span]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
    seg.parts
        .iter()
        .map(|p| match p {
            Part::Text { spans } => join(spans),
            Part::Stack { top, bottom, .. } => format!("{}/{}", join(top), join(bottom)),
            Part::Gap { .. } => " ".into(),
            Part::Icon { name } => format!("[{name}]"),
            Part::Logos { .. } => String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures;
    use chrono::TimeZone;

    fn opts() -> FormatOptions {
        FormatOptions {
            tz: FixedOffset::west_opt(4 * 3600).unwrap(), // EDT
            now: Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap(),
        }
    }

    fn game(id: &str) -> Game {
        fixtures::mock_games(opts().now).into_iter().find(|g| g.id.0 == id).unwrap()
    }

    fn text(id: &str) -> String {
        segment_text(&game_segment(&game(id), &opts()))
    }

    #[test]
    fn live_football_shows_clock_and_possession() {
        assert_eq!(text("mock:nfl:1"), "KC/BUF ◀ 17/21 Q3/4:32");
        let seg = game_segment(&game("mock:nfl:1"), &opts());
        let Part::Stack { bottom, .. } = &seg.parts[0] else { panic!() };
        assert_eq!(bottom[1].tint, Tint::Accent, "red zone possession is highlighted");
    }

    #[test]
    fn final_dims_loser_and_shows_overtime() {
        let seg = game_segment(&game("mock:mlb:2"), &opts());
        assert_eq!(segment_text(&seg), "STL/NYE 4/5 F/10/");
        let Part::Stack { top, .. } = &seg.parts[0] else { panic!() };
        assert_eq!(top[0].tint, Tint::Dim, "STL lost");
    }

    #[test]
    fn baseball_shows_inning_half_and_outs() {
        assert_eq!(text("mock:mlb:1"), "LA/CHI 3/2 ▲7/1 OUT");
    }

    #[test]
    fn scheduled_shows_local_time_day_and_network() {
        // Tomorrow 17:00 UTC = 1:00 PM EDT on a different day.
        assert_eq!(text("mock:nfl:3"), "NYS/BOS MON 1:00PM/CBS");
    }

    #[test]
    fn college_ranks_and_halftime() {
        assert_eq!(text("mock:ncaaf:1"), "12 TUS/5 ATH 14/10 HALF/");
    }

    #[test]
    fn hockey_power_play_and_soccer_minute() {
        assert_eq!(text("mock:nhl:1"), "MTL/TOR PP 1/2 2nd/11:05");
        assert_eq!(text("mock:epl:1"), "HIG/MER 1/1 2H/67'");
    }

    #[test]
    fn ticker_groups_by_league_with_live_games_first() {
        let games = fixtures::mock_games(opts().now);
        let segs = ticker_segments(&games, &opts());
        let ids: Vec<&str> = segs.iter().map(|s| s.id.as_str()).collect();
        let nfl = ids.iter().position(|i| *i == "league:nfl").unwrap();
        assert_eq!(&ids[nfl..nfl + 5], ["league:nfl", "mock:nfl:1", "mock:nfl:2", "mock:nfl:3", "mock:nfl:4"]);
        assert_eq!(segs.iter().filter(|s| s.id.starts_with("league:")).count(), 6);
    }

    #[test]
    fn crawl_lists_upcoming_in_start_order() {
        let segs = crawl_segments(&fixtures::mock_games(opts().now), &opts());
        let texts: Vec<String> = segs.iter().map(segment_text).collect();
        assert_eq!(texts[0], "EPL IRW at THB  4:30 PM  USA");
        assert_eq!(texts[1], "NFL NYS at BOS  MON 1:00 PM  CBS");
        assert_eq!(
            segs[0].parts,
            vec![Part::text(vec![
                Span::new("EPL", Tint::Accent),
                Span::primary(" IRW at THB"),
                Span::dim("  4:30 PM"),
                Span::dim("  USA"),
            ])]
        );
    }

    #[test]
    fn crawl_label_says_when_the_next_game_is() {
        let games = fixtures::mock_games(opts().now);
        let at = |h: u32| {
            let mut g = games.clone();
            for game in &mut g {
                if game.status == GameStatus::Scheduled {
                    game.start_time = opts().now.date_naive().and_hms_opt(h, 0, 0).unwrap().and_utc();
                }
            }
            crawl_label(&g, &FormatOptions { tz: chrono::FixedOffset::east_opt(0).unwrap(), now: opts().now })
        };
        assert_eq!(at(20).as_deref(), Some("TONIGHT"));
        assert_eq!(at(13).as_deref(), Some("TODAY"));
        let later: Vec<Game> = games
            .iter()
            .cloned()
            .map(|mut g| {
                g.start_time = opts().now + chrono::Duration::days(2);
                g
            })
            .collect();
        let mut sched = later;
        sched.iter_mut().for_each(|g| g.status = GameStatus::Scheduled);
        assert_eq!(crawl_label(&sched, &opts()).as_deref(), Some("UP NEXT"));
        assert_eq!(crawl_label(&[], &opts()), None);
    }

    #[test]
    fn final_label_variants() {
        assert_eq!(final_label("Final"), "FINAL");
        assert_eq!(final_label("Final/OT"), "F/OT");
        assert_eq!(final_label("Final/2OT"), "F/2OT");
        assert_eq!(final_label("Final/Shootout"), "FINAL");
    }
}
