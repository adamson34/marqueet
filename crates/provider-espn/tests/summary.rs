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

#[test]
fn a_live_at_bat_has_its_count_players_and_pitches() {
    use marqueet_core::sports::summary::Call;
    let body = fixture("summary_mlb_at_bat");
    let s = normalize_summary(&body, &game_for(&body, "mlb", Sport::Baseball)).unwrap();
    let ab = s.at_bat.unwrap();
    assert_eq!((ab.balls, ab.strikes, ab.outs), (0, 1, 0));
    let (pitcher, batter) = (ab.pitcher.unwrap(), ab.batter.unwrap());
    assert!(pitcher.line.contains(" IP, ") && pitcher.line.ends_with(" P"), "{}", pitcher.line);
    assert!(batter.line.contains('-'), "hits-at-bats: {}", batter.line);
    assert!(!pitcher.name.is_empty() && !batter.name.is_empty());
    assert_eq!(ab.pitches.len(), 1);
    let p = &ab.pitches[0];
    assert_eq!((p.number, p.call, p.result.as_str()), (1, Call::Strike, "STRIKE SWINGING"));
    assert!(p.kind.ends_with(char::is_numeric), "type and speed: {}", p.kind);
    let (x, y) = p.at.unwrap();
    assert!(x > 1.0 && y > 1.0, "a chased pitch low and away: ({x}, {y})");
}

#[test]
fn a_finished_at_bat_stays_up_until_the_next_starts() {
    use marqueet_core::sports::summary::Call;
    let body = fixture("summary_mlb_between_batters");
    let s = normalize_summary(&body, &game_for(&body, "mlb", Sport::Baseball)).unwrap();
    let ab = s.at_bat.unwrap();
    assert_eq!(ab.outs, 2);
    assert_eq!(ab.pitches.len(), 1);
    assert_eq!((ab.pitches[0].call, ab.pitches[0].result.as_str()), (Call::InPlay, "GROUND OUT"));

    // Once the next batter is up, the last one's pitches go.
    let mut doc: serde_json::Value = serde_json::from_str(&body).unwrap();
    doc["situation"]["batter"]["playerId"] = serde_json::json!(1);
    let s = normalize_summary(&doc.to_string(), &game_for(&body, "mlb", Sport::Baseball)).unwrap();
    assert!(s.at_bat.unwrap().pitches.is_empty(), "those were the previous batter's");
}

#[test]
fn football_has_no_at_bat() {
    let body = fixture("summary_nfl");
    assert!(normalize_summary(&body, &game_for(&body, "nfl", Sport::Football)).unwrap().at_bat.is_none());
}

#[test]
fn a_drive_in_progress_has_the_ball_the_down_and_the_plays() {
    // Recorded live: the home team's drive, 2nd & 20 at the opponent's 40
    // after a holding penalty.
    let body = fixture("summary_ncaaf_current_drive");
    let d = normalize_summary(&body, &game_for(&body, "ncaaf", Sport::Football)).unwrap().drive.unwrap();
    assert!(d.home);
    assert_eq!((d.start, d.ball, d.first_down), (29, 60, Some(80)));
    assert!(d.down.starts_with("2nd & 20"), "{}", d.down);
    assert_eq!((d.plays, d.yards, d.result.as_deref()), (8, 31, None));
    assert_eq!(d.recent.len(), 4);
    assert_eq!(
        (d.recent[0].kind.as_str(), d.recent[0].yards),
        ("Penalty", -10),
        "a penalty that backs them up is a loss"
    );
    for p in &d.recent {
        assert!(!p.text.contains('#') && !p.text.contains("Shotgun") && !p.text.starts_with('('), "{}", p.text);
    }
}

#[test]
fn between_drives_the_last_one_shows_how_it_ended() {
    let body = fixture("summary_nfl_drives");
    let d = normalize_summary(&body, &game_for(&body, "nfl", Sport::Football)).unwrap().drive.unwrap();
    assert!(d.result.is_some());
    assert!(d.down.is_empty() && d.first_down.is_none(), "no down to play");
    assert!(d.recent.iter().all(|p| !p.kind.starts_with("End ")), "clock markers skipped");
}
