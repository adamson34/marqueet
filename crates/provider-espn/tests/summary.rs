//! Game summaries (team stats, leaders, scoring) from recorded ESPN
//! responses: a finished NFL game and a live MLB game.

#![allow(clippy::unwrap_used)]

use chrono::Utc;
use marqueet_core::sports::fixtures::mock_games;
use marqueet_core::sports::summary::panels;
use marqueet_core::sports::{Game, GameStatus, LeagueId, Sport, TeamId};
use marqueet_provider_espn::summary::normalize_summary;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// A game whose teams are the recorded summary's (ids from its header).
fn game_for(body: &str, league: &str, sport: Sport) -> Game {
    let doc: serde_json::Value = serde_json::from_str(body).unwrap();
    let teams = doc["header"]["competitions"][0]["competitors"].as_array().unwrap();
    let id = |side: &str| {
        let c = teams.iter().find(|c| c["homeAway"] == side).unwrap();
        (c["team"]["id"].as_str().unwrap().to_owned(), c["team"]["abbreviation"].as_str().unwrap().to_owned())
    };
    let mut g = mock_games(Utc::now()).into_iter().next().unwrap();
    g.league = LeagueId::new(league);
    g.sport = sport;
    let ((aid, aabbr), (hid, habbr)) = (id("away"), id("home"));
    g.away.team.id = TeamId(format!("espn:{league}:{aid}"));
    g.away.team.abbreviation = aabbr;
    g.home.team.id = TeamId(format!("espn:{league}:{hid}"));
    g.home.team.abbreviation = habbr;
    g
}

#[test]
fn football_summary_has_stats_leaders_and_scoring() {
    let body = fixture("summary_nfl");
    let mut game = game_for(&body, "nfl", Sport::Football);
    let s = normalize_summary(&body, &game).unwrap();
    let yards = s.team_stats.iter().find(|t| t.name == "totalYards").expect("total yards");
    assert!(yards.away.parse::<u32>().is_ok() && yards.home.parse::<u32>().is_ok(), "{yards:?}");
    assert!(s.team_stats.iter().any(|t| t.name == "possessionTime"));
    let passing = s.leaders.iter().find(|l| l.category == "Passing Yards").expect("passing leaders");
    assert!(passing.away.as_deref().is_some_and(|t| t.contains("YDS")) && passing.home.is_some(), "{passing:?}");
    assert!(!passing.away.as_deref().unwrap().contains('/'), "completions trimmed: {passing:?}");
    assert!(!s.scoring.is_empty());
    let last = s.scoring.last().unwrap();
    assert!(last.period >= 1 && last.team.is_some() && !last.text.is_empty(), "{last:?}");
    assert!(s.home_win.is_some());

    game.status = GameStatus::Final;
    let p = panels(&s, &game);
    let titles: Vec<&str> = p.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, ["TEAM STATS", "LEADERS", "SCORING"]);
    assert_eq!(p[0].rows[0][1], "TOTAL YARDS", "no win chance once it's over");
}

#[test]
fn baseball_summary_flattens_grouped_stats() {
    let body = fixture("summary_mlb");
    let game = game_for(&body, "mlb", Sport::Baseball);
    let s = normalize_summary(&body, &game).unwrap();
    for key in ["hits", "strikeouts", "errors"] {
        assert!(
            s.team_stats.iter().any(|t| t.name == key),
            "{key}: {:?}",
            s.team_stats.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }
    let p = panels(&s, &game);
    assert_eq!(p[0].title, "TEAM STATS");
    assert!(p[0].rows.iter().any(|r| r[1] == "HITS" || r[1].contains("HIT")), "{:?}", p[0].rows);
}

#[test]
fn junk_is_an_error_and_other_teams_are_ignored() {
    let game = mock_games(Utc::now()).into_iter().next().unwrap();
    assert!(normalize_summary("not json", &game).is_err());
    let s = normalize_summary(&fixture("summary_nfl"), &game).unwrap();
    assert!(s.team_stats.is_empty() && s.leaders.is_empty(), "mock teams aren't in this game");
}
