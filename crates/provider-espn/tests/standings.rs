//! Standings normalization against saved ESPN responses (see fixtures/README.md).
#![allow(clippy::unwrap_used)]

use chrono::{TimeZone, Utc};
use marqueet_core::sports::TeamId;
use marqueet_core::sports::standings::{Standings, columns};
use marqueet_provider_espn::leagues::find;
use marqueet_provider_espn::standings::normalize_standings;

fn load(league: &str) -> Standings {
    let path = format!("{}/tests/fixtures/standings_{league}.json", env!("CARGO_MANIFEST_DIR"));
    let body = std::fs::read_to_string(&path).unwrap();
    normalize_standings(&body, find(league).unwrap(), Utc.with_ymd_and_hms(2026, 9, 23, 20, 0, 0).unwrap()).unwrap()
}

fn names(s: &Standings) -> Vec<&str> {
    s.groups.iter().map(|g| g.name.as_str()).collect()
}

/// Cells of a group's rows as the widget would show them.
fn table(s: &Standings, group: &str) -> Vec<String> {
    let g = s.groups.iter().find(|g| g.name == group).unwrap();
    let cols = columns(s.sport);
    g.rows
        .iter()
        .map(|r| format!("{} {}", r.abbreviation, cols.iter().map(|(_, f)| f(r)).collect::<Vec<_>>().join(" ")))
        .collect()
}

#[test]
fn nfl_divisions() {
    let s = load("nfl");
    assert_eq!(
        names(&s),
        ["AFC East", "AFC North", "AFC South", "AFC West", "NFC East", "NFC North", "NFC South", "NFC West"]
    );
    assert!(s.groups.iter().all(|g| g.rows.len() == 4));
    assert_eq!(table(&s, "AFC East"), ["BUF 2 0 0 1.000", "NE 1 1 0 .500", "NYJ 1 1 0 .500", "MIA 0 2 0 .000"]);
    let buf = TeamId("espn:nfl:2".into());
    assert_eq!(s.group_of(&buf).unwrap().name, "AFC East");
}

#[test]
fn mlb_divisions_are_sorted_by_win_percentage() {
    let s = load("mlb");
    assert_eq!(names(&s), ["AL East", "AL Cent", "AL West", "NL East", "NL Cent", "NL West"]);
    for g in &s.groups {
        let pct: Vec<f64> = g.rows.iter().map(|r| r.win_pct.as_deref().unwrap().parse().unwrap()).collect();
        assert!(pct.windows(2).all(|w| w[0] >= w[1]), "{}: {pct:?}", g.name);
        assert_eq!(g.rows[0].games_behind.as_deref(), Some("-"), "{} leader", g.name);
    }
    assert!(table(&s, "AL East")[0].starts_with("TB 96 61 .611 -"), "{:?}", table(&s, "AL East"));
}

#[test]
fn nhl_divisions_are_sorted_by_points() {
    let s = load("nhl");
    assert_eq!(names(&s), ["Atlantic", "Metropolitan", "Central", "Pacific"]);
    for g in &s.groups {
        let pts: Vec<i32> = g.rows.iter().map(|r| r.points.unwrap()).collect();
        assert!(pts.windows(2).all(|w| w[0] >= w[1]), "{}: {pts:?}", g.name);
    }
    assert_eq!(table(&s, "Atlantic")[0], "MTL 3 0 0 6");
}

#[test]
fn epl_is_one_table_in_rank_order() {
    let s = load("epl");
    assert_eq!(names(&s), ["Premier League"]);
    let t = table(&s, "Premier League");
    assert_eq!(t.len(), 20);
    assert_eq!(t[0], "MNC 5 5 0 0 15");
    assert_eq!(t[1], "ARS 5 4 0 1 12");
}

#[test]
fn wnba_conferences() {
    let s = load("wnba");
    assert_eq!(names(&s), ["Eastern Conference", "Western Conference"]);
}

#[test]
fn junk_is_an_error_not_a_panic() {
    let nfl = find("nfl").unwrap();
    assert!(normalize_standings("not json", nfl, Utc::now()).is_err());
    assert!(normalize_standings("{}", nfl, Utc::now()).is_err());
    let odd = r#"{"children":[{"name":"X","standings":{"entries":[{"team":{"id":"1","abbreviation":"A"},"stats":[{"name":"wins","value":"3"}]},{"team":{},"stats":[]}]}}]}"#;
    let s = normalize_standings(odd, nfl, Utc::now()).unwrap();
    assert_eq!(s.groups[0].rows.len(), 1, "entries without a team are dropped");
    assert_eq!(s.groups[0].rows[0].wins, 3);
}

#[test]
fn team_lists() {
    use marqueet_provider_espn::standings::normalize_teams;
    let load = |league: &str| {
        let body =
            std::fs::read_to_string(format!("{}/tests/fixtures/teams_{league}.json", env!("CARGO_MANIFEST_DIR")))
                .unwrap();
        normalize_teams(&body, find(league).unwrap()).unwrap()
    };
    let nfl = load("nfl");
    assert_eq!(nfl.len(), 32);
    assert!(nfl.windows(2).all(|w| w[0].name <= w[1].name), "sorted by name");
    let bills = nfl.iter().find(|t| t.abbreviation == "BUF").unwrap();
    assert_eq!((bills.id.0.as_str(), bills.name.as_str()), ("espn:nfl:2", "Buffalo Bills"));
    assert_eq!(load("epl").len(), 20);
    assert!(normalize_teams("nope", find("nfl").unwrap()).is_err());
}
