//! Parsing tests against saved (anonymized) Sleeper responses; see
//! fixtures/README.md.
#![allow(clippy::unwrap_used)]

use chrono::Utc;
use marqueet_provider_sleeper::parse;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn user_and_leagues() {
    let u = parse::user(&fixture("user")).unwrap().unwrap();
    assert_eq!((u.id.as_str(), u.display_name.as_str()), ("2000000000000000001", "Example"));
    assert_eq!(parse::user("null").unwrap(), None, "unknown usernames");
    let leagues = parse::leagues(&fixture("user_leagues")).unwrap();
    assert_eq!(leagues.len(), 1);
    assert_eq!((leagues[0].name.as_str(), leagues[0].teams), ("Example League", 12));
    assert!(parse::leagues("null").unwrap().is_empty());
}

#[test]
fn teams_are_named_by_team_name_then_manager() {
    let teams = parse::teams(&fixture("rosters"), &fixture("users")).unwrap();
    assert_eq!(teams.len(), 12);
    assert!(teams.windows(2).all(|w| w[0].roster_id < w[1].roster_id));
    assert!(teams.iter().any(|t| t.name.starts_with("Team ")));
    assert!(teams.iter().any(|t| t.name.starts_with("manager")), "no team name: the manager's name");
}

#[test]
fn a_real_week_three_matchup() {
    let players = parse::players(&fixture("players_subset")).unwrap();
    let m = parse::matchup_from_bodies(
        &fixture("league"),
        &fixture("users"),
        &fixture("rosters"),
        &fixture("matchups_week3"),
        &players,
        1,
        3,
        Utc::now(),
    )
    .unwrap();
    assert_eq!((m.league.as_str(), m.week), ("Example League", 3));
    assert!((m.me.points - 100.82).abs() < 0.01);
    assert_eq!(m.me.record, "7-7");
    let slots: Vec<&str> = m.me.starters.iter().map(|s| s.slot.as_str()).collect();
    assert_eq!(slots, ["QB", "RB", "RB", "WR", "WR", "TE", "FLEX", "K", "DEF"]);
    assert_eq!(m.me.starters[0].points, 15.82);
    let def = m.me.starters.last().unwrap();
    assert_eq!((def.name.as_str(), def.position.as_str()), ("PHI", "DEF"));
    assert!(m.me.starters[0].name.contains(". "), "short names: {}", m.me.starters[0].name);
    assert!(m.me.starters[0].espn_id.is_some());
    let opp = m.opponent.as_ref().expect("same matchup_id");
    assert_ne!(opp.roster_id, 1);
    let total: f32 = opp.starters.iter().map(|s| s.points).sum();
    assert!((total - opp.points).abs() < 0.1, "starters add up to the team total");
    let espn = m.me.starters[0].espn_id.clone().unwrap();
    assert_eq!(m.starter_by_espn_id(&espn).map(|(s, mine)| (s.slot.as_str(), mine)), Some(("QB", true)));
}

#[test]
fn missing_teams_and_junk() {
    let players = parse::players(&fixture("players_subset")).unwrap();
    let err = parse::matchup_from_bodies(
        &fixture("league"),
        &fixture("users"),
        &fixture("rosters"),
        &fixture("matchups_week3"),
        &players,
        99,
        3,
        Utc::now(),
    );
    assert!(err.is_err());
    assert!(parse::state("nope").is_err());
    let state = parse::state(&fixture("state")).unwrap();
    assert_eq!((state.week, state.season.as_str()), (3, "2026"));
}
