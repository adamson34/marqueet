//! Pure conversion from ESPN scoreboard JSON to the normalized game schema.
//!
//! Each event is parsed on its own; a malformed event is skipped with a note
//! instead of failing the whole league.

use chrono::{DateTime, NaiveDateTime, Utc};
use marqueet_core::Rgb;
use marqueet_core::provider::{ProviderError, Scoreboard};
use marqueet_core::sports::{
    Athlete, Competitor, CompetitorExtras, Game, GameClock, GameId, GameStatus, HomeAway, InningHalf, LeagueId, Play,
    SeriesInfo, Situation, Sport, Team, TeamColors, TeamId,
};

use crate::leagues::LeagueDef;
use crate::model;

pub const PROVIDER: &str = "espn";

/// Parses a scoreboard response body for `league`.
pub fn normalize(body: &str, league: &LeagueDef, fetched_at: DateTime<Utc>) -> Result<Scoreboard, ProviderError> {
    let root: serde_json::Value = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let events = match root.get("events") {
        Some(serde_json::Value::Array(events)) => events.clone(),
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(_) => return Err(ProviderError::Parse("`events` is not an array".into())),
    };
    let mut board = Scoreboard::default();
    for value in events {
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_owned();
        let parsed = serde_json::from_value::<model::Event>(value).map_err(|e| e.to_string());
        match parsed.and_then(|ev| to_game(&ev, league, fetched_at)) {
            Ok(game) => board.games.push(game),
            Err(reason) => board.skipped.push(format!("{} event {id}: {reason}", league.id)),
        }
    }
    Ok(board)
}

pub(crate) fn team_id(league: &LeagueDef, id: &str) -> TeamId {
    TeamId(format!("{PROVIDER}:{}:{id}", league.id))
}

fn to_game(ev: &model::Event, league: &LeagueDef, fetched_at: DateTime<Utc>) -> Result<Game, String> {
    let comp = ev.competitions.first().ok_or("no competition")?;
    let side = |ha: &str| comp.competitors.iter().find(|c| c.home_away == ha);
    let (home, away) = match (side("home"), side("away")) {
        (Some(h), Some(a)) => (h, a),
        _ => return Err("missing home or away competitor".into()),
    };
    if ev.id.is_empty() {
        return Err("missing id".into());
    }
    let start_time = parse_date(&ev.date).ok_or_else(|| format!("bad date {:?}", ev.date))?;
    let status = map_status(&comp.status.kind);
    let period = u8::try_from(comp.status.period.unwrap_or(0).clamp(0, 99)).unwrap_or(0);
    let short = comp.status.kind.short_detail.trim();
    let baseball = parse_inning(short);

    let (period_label, clock) = labels(league, status, period, comp, baseball);
    let final_ = status == GameStatus::Final;
    // Scores only mean something once a game has actually started.
    let started = comp.status.kind.state != "pre" && !matches!(status, GameStatus::Postponed | GameStatus::Canceled);

    let game = Game {
        id: GameId(format!("{PROVIDER}:{}:{}", league.id, ev.id)),
        provider: PROVIDER.into(),
        league: LeagueId::new(league.id),
        sport: league.sport,
        start_time,
        status,
        clock: GameClock { period, period_label, clock, detail: short.to_owned() },
        home: competitor(home, HomeAway::Home, league, started, final_),
        away: competitor(away, HomeAway::Away, league, started, final_),
        situation: situation(comp, league, status, baseball),
        last_play: comp.situation.as_ref().and_then(|s| s.last_play.as_ref()).map(|p| play(p, league)),
        broadcast: broadcast(&comp.broadcasts),
        venue: comp.venue.as_ref().and_then(|v| v.full_name.clone()),
        series: series(comp, &home.team.id, &away.team.id),
        fetched_at,
        stale: false,
    };
    Ok(game)
}

/// A playoff game's series: wins from `series`, the round and game number
/// from the note ("ALDS - Game 2"), the stage from the round type.
fn series(comp: &model::Competition, home: &str, away: &str) -> Option<SeriesInfo> {
    let s = comp.series.as_ref().filter(|s| s.kind == "playoff")?;
    let wins = |id: &str| {
        let w = s.competitors.iter().find(|c| c.id == id).and_then(|c| c.wins).unwrap_or(0);
        u8::try_from(w.clamp(0, 99)).unwrap_or(0)
    };
    let headline = comp.notes.first().map(|n| n.headline.trim()).unwrap_or("");
    let (round, game) = match headline.rsplit_once(" - Game ") {
        Some((round, n)) => (round.trim(), n.trim().parse::<u8>().ok()),
        None => (headline, None),
    };
    let stage = match comp.kind.as_ref().map(|k| k.abbreviation.as_str()) {
        Some("RD16" | "RD1") => 1,
        Some("QTR") => 2,
        Some("SEMI") => 3,
        Some("FINAL") => 4,
        _ => 0,
    };
    let best_of = u8::try_from(s.total_competitions.unwrap_or(1).clamp(1, 15)).unwrap_or(1);
    Some(SeriesInfo {
        round: if round.is_empty() { "PLAYOFFS".into() } else { round.to_owned() },
        side: bracket_side(round),
        stage,
        game_number: game,
        best_of,
        home_wins: wins(home),
        away_wins: wins(away),
        completed: s.completed,
    })
}

/// The bracket half a round belongs to: "AL"/"NL" from MLB's "ALDS"-style
/// names, or a leading conference word ("East", "AFC").
fn bracket_side(round: &str) -> Option<String> {
    let first = round.split_whitespace().next()?;
    for side in ["AL", "NL"] {
        if first.len() == 4 && first.starts_with(side) {
            return Some(side.into());
        }
    }
    let conference = ["East", "West", "Eastern", "Western", "AFC", "NFC"];
    conference.contains(&first).then(|| first.trim_end_matches("ern").to_owned())
}

/// ESPN dates look like `2026-09-23T19:45Z` (no seconds) or full RFC 3339.
fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%MZ").ok().map(|n| n.and_utc()))
}

pub(crate) fn map_status(t: &model::StatusType) -> GameStatus {
    match t.name.as_str() {
        "STATUS_SCHEDULED" | "STATUS_TIME_TBD" => GameStatus::Scheduled,
        "STATUS_HALFTIME" => GameStatus::Halftime,
        "STATUS_DELAYED" | "STATUS_RAIN_DELAY" | "STATUS_WEATHER_DELAY" | "STATUS_SUSPENDED" => GameStatus::Delayed,
        "STATUS_POSTPONED" => GameStatus::Postponed,
        "STATUS_CANCELED" | "STATUS_CANCELLED" | "STATUS_FORFEIT" | "STATUS_ABANDONED" => GameStatus::Canceled,
        name if name.starts_with("STATUS_FINAL") || name == "STATUS_FULL_TIME" => GameStatus::Final,
        _ => match (t.state.as_str(), t.completed) {
            (_, true) | ("post", _) => GameStatus::Final,
            ("in", _) => GameStatus::InProgress,
            _ => GameStatus::Scheduled,
        },
    }
}

/// Baseball short details: "Top 7th", "Bot 7th", "Mid 6th", "End 6th".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inning {
    Half(InningHalf),
    Middle,
    End,
}

pub(crate) fn parse_inning(short: &str) -> Option<Inning> {
    let word = short.split_whitespace().next()?.to_ascii_lowercase();
    Some(match word.as_str() {
        "top" => Inning::Half(InningHalf::Top),
        "bot" | "bottom" => Inning::Half(InningHalf::Bottom),
        "mid" | "middle" => Inning::Middle,
        "end" => Inning::End,
        _ => return None,
    })
}

fn ordinal(n: u8) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Short period label and (for timed sports) the game clock.
fn labels(
    league: &LeagueDef,
    status: GameStatus,
    period: u8,
    comp: &model::Competition,
    inning: Option<Inning>,
) -> (String, Option<String>) {
    if !matches!(status, GameStatus::InProgress | GameStatus::Halftime | GameStatus::Delayed) || period == 0 {
        return (String::new(), None);
    }
    if status == GameStatus::Halftime {
        return ("HALF".into(), None);
    }
    let clock = comp.status.display_clock.clone().filter(|c| !c.is_empty());
    let end_of_period = comp.status.kind.name == "STATUS_END_PERIOD";
    match league.sport {
        Sport::Baseball => {
            let label = match inning {
                Some(Inning::Middle) => format!("MID {period}"),
                Some(Inning::End) => format!("END {period}"),
                _ => comp.status.kind.short_detail.trim().to_owned(),
            };
            (label, None)
        }
        Sport::Soccer => {
            let label = match period {
                1 => "1H",
                2 => "2H",
                3 | 4 => "ET",
                _ => "PENS",
            };
            (label.into(), clock)
        }
        Sport::Hockey => {
            let label = match period {
                1..=3 => ordinal(period),
                4 => "OT".into(),
                _ => "SO".into(),
            };
            if end_of_period { (format!("END {label}"), None) } else { (label, clock) }
        }
        Sport::Football | Sport::Basketball => {
            let regulation = if league.halves { 2 } else { 4 };
            let label = if period <= regulation {
                if league.halves { format!("{period}H") } else { format!("Q{period}") }
            } else if period == regulation + 1 {
                "OT".into()
            } else {
                format!("{}OT", period - regulation)
            };
            if end_of_period { (format!("END {label}"), None) } else { (label, clock) }
        }
    }
}

fn parse_color(hex: Option<&str>) -> Option<Rgb> {
    hex.map(|h| h.trim()).filter(|h| !h.is_empty()).and_then(|h| h.parse().ok())
}

fn competitor(c: &model::Competitor, side: HomeAway, league: &LeagueDef, started: bool, final_: bool) -> Competitor {
    let t = &c.team;
    let small = |v: Option<i64>| v.and_then(|v| u16::try_from(v).ok());
    Competitor {
        team: Team {
            id: team_id(league, &t.id),
            abbreviation: if t.abbreviation.is_empty() { t.short_display_name.clone() } else { t.abbreviation.clone() },
            short_name: t.short_display_name.clone(),
            display_name: t.display_name.clone(),
            colors: TeamColors {
                primary: parse_color(t.color.as_deref()).unwrap_or(Rgb::new(0x88, 0x88, 0x88)),
                secondary: parse_color(t.alternate_color.as_deref()),
            },
            logo_url: t.logo.clone(),
            record: c.records.first().map(|r| r.summary.clone()).filter(|s| !s.is_empty()),
            rank: c.curated_rank.as_ref().and_then(|r| r.current).filter(|r| (1..=25).contains(r)).map(|r| r as u8),
        },
        home_away: side,
        score: if started { small(c.score) } else { None },
        linescore: c.linescores.iter().filter_map(|l| small(l.value)).collect(),
        winner: if final_ { c.winner } else { None },
        extras: CompetitorExtras { hits: small(c.hits), errors: small(c.errors), ..Default::default() },
    }
}

fn situation(
    comp: &model::Competition,
    league: &LeagueDef,
    status: GameStatus,
    inning: Option<Inning>,
) -> Option<Situation> {
    if status != GameStatus::InProgress {
        return None;
    }
    let s = comp.situation.as_ref();
    let small = |v: Option<i64>| v.and_then(|v| u8::try_from(v).ok());
    match league.sport {
        Sport::Football => {
            let s = s?;
            Some(Situation::Football {
                possession: s.possession.as_deref().filter(|p| !p.is_empty()).map(|p| team_id(league, p)),
                down: small(s.down).filter(|d| (1..=4).contains(d)),
                distance: small(s.distance),
                down_distance_text: s.down_distance_text.clone().filter(|t| !t.is_empty()),
                red_zone: s.is_red_zone,
            })
        }
        Sport::Baseball => {
            let Some(Inning::Half(half)) = inning else { return None };
            let s = s?;
            Some(Situation::Baseball {
                half,
                balls: small(s.balls).unwrap_or(0),
                strikes: small(s.strikes).unwrap_or(0),
                outs: small(s.outs).unwrap_or(0),
                bases: [s.on_first, s.on_second, s.on_third],
            })
        }
        Sport::Hockey => Some(Situation::Hockey { power_play: None }),
        Sport::Basketball => Some(Situation::Basketball),
        Sport::Soccer => Some(Situation::Soccer),
    }
}

/// ESPN's play text on one line: it can start with a space and break
/// before a penalty ("... for 5 yards.\nPENALTY on ...").
pub(crate) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn play(p: &model::LastPlay, league: &LeagueDef) -> Play {
    Play {
        id: p.id.clone(),
        text: one_line(&p.text),
        type_text: p.kind.as_ref().map(|k| k.text.clone()).filter(|t| !t.is_empty()),
        team: p.team.as_ref().filter(|t| !t.id.is_empty()).map(|t| team_id(league, &t.id)),
        score_value: p.score_value.and_then(|v| u8::try_from(v).ok()),
        athletes: p
            .athletes_involved
            .iter()
            .map(|a| Athlete { id: a.id.clone(), name: a.display_name.clone() })
            .collect(),
    }
}

/// The national broadcast if there is one, else the first listed.
fn broadcast(list: &[model::Broadcast]) -> Option<String> {
    list.iter()
        .find(|b| b.market.as_deref() == Some("national"))
        .or_else(|| list.first())
        .and_then(|b| b.names.first().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leagues::find;

    #[test]
    fn play_text_is_one_line() {
        assert_eq!(
            one_line(" (Shotgun) run to the 38 for 5 yards.\nPENALTY on the defense,  holding."),
            "(Shotgun) run to the 38 for 5 yards. PENALTY on the defense, holding."
        );
    }

    fn st(name: &str, state: &str) -> model::StatusType {
        model::StatusType { name: name.into(), state: state.into(), ..Default::default() }
    }

    #[test]
    fn status_names_map_with_state_fallback() {
        assert_eq!(map_status(&st("STATUS_FINAL_OT", "post")), GameStatus::Final);
        assert_eq!(map_status(&st("STATUS_FULL_TIME", "post")), GameStatus::Final);
        assert_eq!(map_status(&st("STATUS_END_PERIOD", "in")), GameStatus::InProgress);
        assert_eq!(map_status(&st("STATUS_SECOND_HALF", "in")), GameStatus::InProgress);
        assert_eq!(map_status(&st("STATUS_RAIN_DELAY", "in")), GameStatus::Delayed);
        assert_eq!(map_status(&st("STATUS_SOMETHING_NEW", "pre")), GameStatus::Scheduled);
        assert_eq!(map_status(&st("STATUS_SOMETHING_NEW", "post")), GameStatus::Final);
    }

    #[test]
    fn innings_and_ordinals() {
        assert_eq!(parse_inning("Top 7th"), Some(Inning::Half(InningHalf::Top)));
        assert_eq!(parse_inning("Bot 9th"), Some(Inning::Half(InningHalf::Bottom)));
        assert_eq!(parse_inning("Mid 6th"), Some(Inning::Middle));
        assert_eq!(parse_inning("Final"), None);
        assert_eq!(
            [1, 2, 3, 4, 11, 12, 13, 21].map(ordinal),
            ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st"]
        );
    }

    #[test]
    fn dates_with_and_without_seconds() {
        assert!(parse_date("2026-09-23T19:45Z").is_some());
        assert!(parse_date("2026-09-23T19:45:00Z").is_some());
        assert!(parse_date("yesterday").is_none());
    }

    #[test]
    fn junk_bodies() {
        let nfl = find("nfl").unwrap();
        let now = Utc::now();
        assert!(matches!(normalize("not json", nfl, now), Err(ProviderError::Parse(_))));
        assert!(matches!(normalize(r#"{"events":{}}"#, nfl, now), Err(ProviderError::Parse(_))));
        assert_eq!(normalize("{}", nfl, now).unwrap(), Scoreboard::default());
    }
}
