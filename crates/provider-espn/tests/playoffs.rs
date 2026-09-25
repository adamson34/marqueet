//! The 2025 MLB postseason, recorded day by day from ESPN's scoreboard
//! (trimmed to what the normalizer reads), built into a bracket.
#![allow(clippy::unwrap_used)]

use chrono::{TimeZone, Utc};
use marqueet_core::sports::playoffs::bracket;
use marqueet_core::sports::{Game, LeagueId};
use marqueet_provider_espn::leagues::find;
use marqueet_provider_espn::normalize::normalize;

fn postseason() -> Vec<Game> {
    let path = format!("{}/tests/fixtures/mlb_postseason_2025.json", env!("CARGO_MANIFEST_DIR"));
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let fetched = Utc.with_ymd_and_hms(2025, 11, 3, 12, 0, 0).unwrap();
    let mut games = Vec::new();
    for day in doc["days"].as_object().unwrap().values() {
        games.extend(normalize(&day.to_string(), find("mlb").unwrap(), fetched).unwrap().games);
    }
    games
}

#[test]
fn every_playoff_game_has_its_series() {
    let games = postseason();
    assert!(games.len() > 40);
    assert!(games.iter().all(|g| g.series.is_some()));
    let g = games.iter().find(|g| g.series.as_ref().unwrap().round == "ALDS").unwrap();
    let s = g.series.as_ref().unwrap();
    assert_eq!((s.stage, s.best_of, s.side.as_deref()), (2, 5, Some("AL")));
    assert!(s.game_number.is_some());
    let ws = games.iter().find(|g| g.series.as_ref().unwrap().round == "World Series").unwrap();
    assert_eq!((ws.series.as_ref().unwrap().stage, ws.series.as_ref().unwrap().side.clone()), (4, None));
}

#[test]
fn the_2025_bracket() {
    let b = bracket(&postseason(), &LeagueId::new("mlb")).unwrap();
    let counts: Vec<usize> = b.columns.iter().map(|c| c.series.len()).collect();
    assert_eq!(counts, [4, 4, 2, 1]);
    let ws = &b.columns[3].series[0];
    let winner = ws.winner().unwrap();
    assert_eq!(ws.teams.iter().map(|t| t.wins).max(), Some(4));
    assert!(ws.completed && ws.teams.iter().any(|t| t.wins == 3), "went seven");
    // The champion came through the championship series in the same half.
    let lcs = b.columns[2].series.iter().find(|s| s.winner().map(|w| &w.id) == Some(&winner.id)).unwrap();
    assert!(lcs.completed);
    // Every later series sits level with the series that fed it.
    for (stage, col) in b.columns.iter().enumerate().skip(1) {
        let prev = &b.columns[stage - 1].series;
        for s in &col.series {
            for f in prev.iter().filter(|f| f.winner().is_some_and(|w| s.teams.iter().any(|t| t.id == w.id))) {
                // Row centres, as a fraction of the column's height.
                let centre = |i: usize, n: usize| (i as f32 + 0.5) / n as f32;
                let si = centre(col.series.iter().position(|x| x == s).unwrap(), col.series.len());
                let fi = centre(prev.iter().position(|x| x == f).unwrap(), prev.len());
                assert!((si - fi).abs() <= 0.25, "{} feeds {} out of line", f.round, s.round);
            }
        }
    }
    assert_eq!(b.columns[2].series[0].side.as_deref(), Some("AL"), "AL on top");
}
