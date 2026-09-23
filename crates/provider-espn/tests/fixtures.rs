//! Normalization tests against saved ESPN responses (see fixtures/README.md).
#![allow(clippy::unwrap_used)]

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use marqueet_core::Rgb;
use marqueet_core::provider::Scoreboard;
use marqueet_core::sports::ticker::{FormatOptions, game_segment, segment_text};
use marqueet_core::sports::{Game, GameStatus, InningHalf, Situation};
use marqueet_provider_espn::leagues::find;
use marqueet_provider_espn::normalize::normalize;

fn fetched() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 23, 20, 0, 0).unwrap()
}

fn load(league: &str, file: &str) -> Scoreboard {
    let path = format!("{}/tests/fixtures/{file}.json", env!("CARGO_MANIFEST_DIR"));
    let body = std::fs::read_to_string(&path).unwrap();
    normalize(&body, find(league).unwrap(), fetched()).unwrap()
}

fn by_abbr<'a>(board: &'a Scoreboard, away: &str, home: &str) -> &'a Game {
    board
        .games
        .iter()
        .find(|g| g.away.team.abbreviation == away && g.home.team.abbreviation == home)
        .unwrap_or_else(|| panic!("no {away} @ {home}"))
}

fn text(game: &Game) -> String {
    let opts = FormatOptions {
        tz: FixedOffset::west_opt(4 * 3600).unwrap(),
        now: Utc.with_ymd_and_hms(2026, 9, 25, 16, 0, 0).unwrap(),
    };
    segment_text(&game_segment(game, &opts))
}

#[test]
fn every_real_capture_parses_cleanly() {
    for (league, file) in [
        ("nfl", "nfl"),
        ("ncaaf", "ncaaf"),
        ("mlb", "mlb"),
        ("wnba", "wnba"),
        ("nhl", "nhl"),
        ("epl", "epl"),
        ("mls", "mls"),
    ] {
        let board = load(league, file);
        assert!(!board.games.is_empty(), "{league}");
        assert!(board.skipped.is_empty(), "{league}: {:?}", board.skipped);
        for g in &board.games {
            assert!(g.id.0.starts_with(&format!("espn:{league}:")), "{}", g.id.0);
            assert!(!g.home.team.abbreviation.is_empty() && !g.away.team.abbreviation.is_empty());
            assert_eq!(g.fetched_at, fetched());
        }
    }
}

#[test]
fn nfl_scheduled_game() {
    let board = load("nfl", "nfl");
    let g = by_abbr(&board, "ATL", "GB");
    assert_eq!(g.status, GameStatus::Scheduled);
    assert_eq!(g.start_time, Utc.with_ymd_and_hms(2026, 9, 25, 0, 15, 0).unwrap());
    assert_eq!((g.home.score, g.away.score), (None, None), "no 0-0 before kickoff");
    assert_eq!(g.broadcast.as_deref(), Some("Prime Video"));
    assert_eq!(g.home.team.colors.primary, Rgb::new(0x20, 0x4e, 0x32));
    assert_eq!(g.home.team.record.as_deref(), Some("1-1"));
    assert_eq!(g.home.team.rank, None, "curatedRank 99 means unranked");
    assert_eq!(g.venue.as_deref(), Some("Lambeau Field"));
}

#[test]
fn mlb_live_between_innings_and_final() {
    let board = load("mlb", "mlb");
    let live = by_abbr(&board, "MIN", "SF");
    assert_eq!(live.status, GameStatus::InProgress);
    assert_eq!(live.clock.period, 6);
    assert_eq!(live.clock.period_label, "MID 6");
    assert!(live.situation.is_none(), "no batting half between innings");
    assert_eq!((live.away.score, live.home.score), (Some(2), Some(2)));
    assert_eq!(live.home.extras.hits, Some(4));
    assert!(live.last_play.is_some());
    assert_eq!(text(live), "MIN/SF 2/2 MID 6/");

    let fin = by_abbr(&board, "WSH", "DET");
    assert_eq!(fin.status, GameStatus::Final);
    assert_eq!((fin.away.winner, fin.home.winner), (Some(true), Some(false)));
    assert_eq!(fin.home.linescore.len(), 9);
    assert_eq!(text(fin), "WSH/DET 4/2 FINAL/");
}

#[test]
fn soccer_full_time_is_final() {
    let board = load("epl", "epl");
    let g = by_abbr(&board, "LIV", "BOU");
    assert_eq!(g.status, GameStatus::Final);
    assert_eq!(g.clock.detail, "FT");
    assert_eq!(text(g), "LIV/BOU 1/0 FINAL/");
}

#[test]
fn college_rankings() {
    let board = load("ncaaf", "ncaaf");
    let g = by_abbr(&board, "NU", "IU");
    assert_eq!(g.home.team.rank, Some(5));
    assert_eq!(g.away.team.rank, None);
}

mod synthetic {
    use super::*;

    /// Loads event `index` from the mixed-league synthetic file as `league`.
    fn one(league: &str, index: usize) -> Game {
        let path = format!("{}/tests/fixtures/synthetic_live.json", env!("CARGO_MANIFEST_DIR"));
        let root: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let body = serde_json::json!({ "events": [root["events"][index].clone()] }).to_string();
        let board = normalize(&body, find(league).unwrap(), fetched()).unwrap();
        assert!(board.skipped.is_empty(), "{:?}", board.skipped);
        board.games.into_iter().next().unwrap()
    }

    #[test]
    fn nfl_live_red_zone() {
        let g = one("nfl", 0);
        assert_eq!(g.status, GameStatus::InProgress);
        assert_eq!((g.clock.period_label.as_str(), g.clock.clock.as_deref()), ("Q3", Some("4:32")));
        let Some(Situation::Football { possession, down, distance, red_zone, down_distance_text }) = &g.situation
        else {
            panic!("{:?}", g.situation)
        };
        assert_eq!(possession.as_ref(), Some(&g.home.team.id));
        assert_eq!((*down, *distance, *red_zone), (Some(2), Some(6), true));
        assert_eq!(down_distance_text.as_deref(), Some("2nd & 6 at ATL 14"));
        let play = g.last_play.as_ref().unwrap();
        assert_eq!(play.athletes[0].name, "Josh Jacobs");
        assert_eq!(text(&g), "ATL/GB ◀ 17/21 Q3/4:32");
    }

    #[test]
    fn nfl_final_overtime() {
        let g = one("nfl", 1);
        assert_eq!(g.status, GameStatus::Final);
        assert!(text(&g).ends_with("F/OT/"), "{}", text(&g));
        assert_eq!(g.away.winner, Some(true));
    }

    #[test]
    fn college_halftime() {
        let g = one("ncaaf", 2);
        assert_eq!(g.status, GameStatus::Halftime);
        assert!(text(&g).ends_with("14/10 HALF/") || text(&g).ends_with("10/14 HALF/"), "{}", text(&g));
    }

    #[test]
    fn hockey_live_and_intermission() {
        let live = one("nhl", 3);
        assert_eq!((live.clock.period_label.as_str(), live.clock.clock.as_deref()), ("2nd", Some("11:05")));
        let int = one("nhl", 4);
        assert_eq!(int.status, GameStatus::InProgress);
        assert_eq!((int.clock.period_label.as_str(), int.clock.clock.as_deref()), ("END 1st", None));
    }

    #[test]
    fn wnba_fourth_quarter() {
        let g = one("wnba", 5);
        assert_eq!((g.clock.period_label.as_str(), g.clock.clock.as_deref()), ("Q4", Some("2:14")));
    }

    #[test]
    fn soccer_second_half_minute() {
        let g = one("epl", 6);
        assert_eq!((g.clock.period_label.as_str(), g.clock.clock.as_deref()), ("2H", Some("67'")));
        assert_eq!((g.home.winner, g.away.winner), (None, None));
    }

    #[test]
    fn baseball_top_half_with_runners() {
        let g = one("mlb", 7);
        assert_eq!(
            g.situation,
            Some(Situation::Baseball {
                half: InningHalf::Top,
                balls: 1,
                strikes: 2,
                outs: 1,
                bases: [true, false, true]
            })
        );
        assert!(text(&g).ends_with("▲7/1 OUT"), "{}", text(&g));
    }

    #[test]
    fn delays_and_postponements() {
        assert_eq!(one("mlb", 8).status, GameStatus::Delayed);
        let ppd = one("mlb", 9);
        assert_eq!(ppd.status, GameStatus::Postponed);
        assert_eq!(
            (ppd.home.score, ppd.clock.period_label.as_str()),
            (None, ""),
            "no 0-0 for a game that never happened"
        );
    }

    #[test]
    fn malformed_event_is_skipped_not_fatal() {
        let path = format!("{}/tests/fixtures/synthetic_live.json", env!("CARGO_MANIFEST_DIR"));
        let board = normalize(&std::fs::read_to_string(path).unwrap(), find("nfl").unwrap(), fetched()).unwrap();
        assert_eq!(board.games.len(), 10);
        assert_eq!(board.skipped.len(), 1);
        assert!(board.skipped[0].contains("missing home or away"), "{:?}", board.skipped);
    }
}
