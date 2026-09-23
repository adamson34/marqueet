//! Mock games covering every sport and status, used by the display's mock
//! feed and by tests. Start times are relative to `now` so the data always
//! looks current.

use chrono::{DateTime, Duration, Utc};

use super::*;
use crate::color::Rgb;

fn rgb(hex: u32) -> Rgb {
    Rgb::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn team(league: &str, abbr: &str, name: &str, primary: u32, secondary: u32) -> Team {
    Team {
        id: TeamId(format!("mock:{league}:{abbr}")),
        abbreviation: abbr.into(),
        short_name: name.rsplit(' ').next().unwrap_or(name).into(),
        display_name: name.into(),
        colors: TeamColors { primary: rgb(primary), secondary: Some(rgb(secondary)) },
        logo_url: None,
        record: None,
        rank: None,
    }
}

fn side(team: Team, home_away: HomeAway, score: Option<u16>) -> Competitor {
    Competitor { team, home_away, score, linescore: Vec::new(), winner: None, extras: Default::default() }
}

struct Spec {
    id: &'static str,
    league: &'static str,
    sport: Sport,
    start: Duration,
    status: GameStatus,
    period: u8,
    label: &'static str,
    clock: Option<&'static str>,
    detail: &'static str,
    away: (Team, Option<u16>),
    home: (Team, Option<u16>),
    broadcast: Option<&'static str>,
}

fn build(now: DateTime<Utc>, s: Spec) -> Game {
    let mut away = side(s.away.0, HomeAway::Away, s.away.1);
    let mut home = side(s.home.0, HomeAway::Home, s.home.1);
    if let (GameStatus::Final, Some(a), Some(h)) = (s.status, away.score, home.score) {
        away.winner = Some(a > h);
        home.winner = Some(h > a);
    }
    Game {
        id: GameId(s.id.into()),
        provider: "mock".into(),
        league: LeagueId::new(s.league),
        sport: s.sport,
        start_time: now + s.start,
        status: s.status,
        clock: GameClock {
            period: s.period,
            period_label: s.label.into(),
            clock: s.clock.map(Into::into),
            detail: s.detail.into(),
        },
        home,
        away,
        situation: None,
        last_play: None,
        broadcast: s.broadcast.map(Into::into),
        venue: None,
        fetched_at: now,
        stale: false,
    }
}

/// A realistic mixed slate: live, final, scheduled, halftime; all five sports.
pub fn mock_games(now: DateTime<Utc>) -> Vec<Game> {
    use GameStatus::*;
    let h = Duration::hours;
    let m = Duration::minutes;

    let kc = team("nfl", "KC", "Kansas City Chiefs", 0xE31837, 0xFFB81C);
    let buf = team("nfl", "BUF", "Buffalo Bills", 0x00338D, 0xC60C30);
    let mut nfl1 = build(
        now,
        Spec {
            id: "mock:nfl:1",
            league: "nfl",
            sport: Sport::Football,
            start: -h(2),
            status: InProgress,
            period: 3,
            label: "Q3",
            clock: Some("4:32"),
            detail: "4:32 - 3rd Quarter",
            away: (kc, Some(17)),
            home: (buf.clone(), Some(21)),
            broadcast: Some("CBS"),
        },
    );
    nfl1.situation = Some(Situation::Football {
        possession: Some(buf.id),
        down: Some(2),
        distance: Some(6),
        down_distance_text: Some("2nd & 6 at KC 14".into()),
        red_zone: true,
    });

    let nfl2 = build(
        now,
        Spec {
            id: "mock:nfl:2",
            league: "nfl",
            sport: Sport::Football,
            start: -h(4),
            status: Final,
            period: 4,
            label: "Q4",
            clock: None,
            detail: "Final",
            away: (team("nfl", "PHI", "Philadelphia Eagles", 0x004C54, 0xA5ACAF), Some(27)),
            home: (team("nfl", "DAL", "Dallas Cowboys", 0x003594, 0x869397), Some(20)),
            broadcast: Some("FOX"),
        },
    );
    let nfl3 = build(
        now,
        Spec {
            id: "mock:nfl:3",
            league: "nfl",
            sport: Sport::Football,
            start: h(25),
            status: Scheduled,
            period: 0,
            label: "",
            clock: None,
            detail: "Mon 1:00 PM",
            away: (team("nfl", "NYJ", "New York Jets", 0x125740, 0xFFFFFF), None),
            home: (team("nfl", "NE", "New England Patriots", 0x002244, 0xC60C30), None),
            broadcast: Some("CBS"),
        },
    );
    let nfl4 = build(
        now,
        Spec {
            id: "mock:nfl:4",
            league: "nfl",
            sport: Sport::Football,
            start: h(52) + m(20),
            status: Scheduled,
            period: 0,
            label: "",
            clock: None,
            detail: "Tue 8:20 PM",
            away: (team("nfl", "SF", "San Francisco 49ers", 0xAA0000, 0xB3995D), None),
            home: (team("nfl", "GB", "Green Bay Packers", 0x203731, 0xFFB612), None),
            broadcast: Some("NBC"),
        },
    );

    let mut ala = team("ncaaf", "ALA", "Alabama Crimson Tide", 0x9E1B32, 0x828A8F);
    ala.rank = Some(12);
    let mut uga = team("ncaaf", "UGA", "Georgia Bulldogs", 0xBA0C2F, 0x000000);
    uga.rank = Some(5);
    let ncaaf1 = build(
        now,
        Spec {
            id: "mock:ncaaf:1",
            league: "ncaaf",
            sport: Sport::Football,
            start: -h(1) - m(40),
            status: Halftime,
            period: 2,
            label: "HALF",
            clock: None,
            detail: "Halftime",
            away: (ala, Some(14)),
            home: (uga, Some(10)),
            broadcast: Some("ESPN"),
        },
    );

    let mut mlb1 = build(
        now,
        Spec {
            id: "mock:mlb:1",
            league: "mlb",
            sport: Sport::Baseball,
            start: -h(2),
            status: InProgress,
            period: 7,
            label: "Top 7th",
            clock: None,
            detail: "Top 7th",
            away: (team("mlb", "LAD", "Los Angeles Dodgers", 0x005A9C, 0xEF3E42), Some(3)),
            home: (team("mlb", "CHC", "Chicago Cubs", 0x0E3386, 0xCC3433), Some(2)),
            broadcast: Some("TBS"),
        },
    );
    mlb1.situation =
        Some(Situation::Baseball { half: InningHalf::Top, balls: 1, strikes: 2, outs: 1, bases: [true, false, true] });
    let mlb2 = build(
        now,
        Spec {
            id: "mock:mlb:2",
            league: "mlb",
            sport: Sport::Baseball,
            start: -h(5),
            status: Final,
            period: 10,
            label: "10th",
            clock: None,
            detail: "Final/10",
            away: (team("mlb", "STL", "St. Louis Cardinals", 0xC41E3A, 0x0C2340), Some(4)),
            home: (team("mlb", "NYY", "New York Yankees", 0x0C2340, 0xC4CED3), Some(5)),
            broadcast: None,
        },
    );

    let wnba1 = build(
        now,
        Spec {
            id: "mock:wnba:1",
            league: "wnba",
            sport: Sport::Basketball,
            start: -h(2),
            status: InProgress,
            period: 4,
            label: "Q4",
            clock: Some("2:14"),
            detail: "2:14 - 4th",
            away: (team("wnba", "NYL", "New York Liberty", 0x6ECEB2, 0x000000), Some(78)),
            home: (team("wnba", "LV", "Las Vegas Aces", 0xC8102E, 0x000000), Some(81)),
            broadcast: Some("ABC"),
        },
    );

    let tor = team("nhl", "TOR", "Toronto Maple Leafs", 0x00205B, 0xFFFFFF);
    let mut nhl1 = build(
        now,
        Spec {
            id: "mock:nhl:1",
            league: "nhl",
            sport: Sport::Hockey,
            start: -h(1),
            status: InProgress,
            period: 2,
            label: "2nd",
            clock: Some("11:05"),
            detail: "11:05 - 2nd",
            away: (team("nhl", "MTL", "Montreal Canadiens", 0xAF1E2D, 0x192168), Some(1)),
            home: (tor.clone(), Some(2)),
            broadcast: Some("SN"),
        },
    );
    nhl1.situation = Some(Situation::Hockey { power_play: Some(tor.id) });

    let mut epl1 = build(
        now,
        Spec {
            id: "mock:epl:1",
            league: "epl",
            sport: Sport::Soccer,
            start: -h(1) - m(10),
            status: InProgress,
            period: 2,
            label: "2H",
            clock: Some("67'"),
            detail: "67'",
            away: (team("epl", "ARS", "Arsenal", 0xEF0107, 0x063672), Some(1)),
            home: (team("epl", "LIV", "Liverpool", 0xC8102E, 0x00B2A9), Some(1)),
            broadcast: Some("NBC"),
        },
    );
    epl1.situation = Some(Situation::Soccer);
    let epl2 = build(
        now,
        Spec {
            id: "mock:epl:2",
            league: "epl",
            sport: Sport::Soccer,
            start: h(4) + m(30),
            status: Scheduled,
            period: 0,
            label: "",
            clock: None,
            detail: "4:30 PM",
            away: (team("epl", "MUN", "Manchester United", 0xDA291C, 0xFBE122), None),
            home: (team("epl", "CHE", "Chelsea", 0x034694, 0xDBA111), None),
            broadcast: Some("USA"),
        },
    );

    vec![nfl1, nfl2, nfl3, nfl4, ncaaf1, mlb1, mlb2, wnba1, nhl1, epl1, epl2]
}

/// Standings to go with [`mock_games`]: two NFL divisions and the top of the
/// Premier League table. Team ids match the mock games where they overlap.
pub fn mock_standings(now: DateTime<Utc>) -> Vec<super::standings::Standings> {
    use super::standings::{Standings, StandingsGroup, StandingsRow};
    let row = |league: &str, abbr: &str, w: u32, l: u32, t: u32, pts: Option<i32>| StandingsRow {
        team: TeamId(format!("mock:{league}:{abbr}")),
        abbreviation: abbr.into(),
        wins: w,
        losses: l,
        ties: t,
        ot_losses: 0,
        points: pts,
        games_played: w + l + t,
        win_pct: pts.is_none().then(|| {
            let pct = f64::from(w) / f64::from((w + l).max(1));
            if pct >= 1.0 { "1.000".into() } else { format!("{pct:.3}").trim_start_matches('0').to_owned() }
        }),
        games_behind: None,
    };
    let nfl = |abbr: &str, w, l| row("nfl", abbr, w, l, 0, None);
    let epl = |abbr: &str, w, d, l| row("epl", abbr, w, l, d, Some((w * 3 + d) as i32));
    vec![
        Standings {
            league: LeagueId::new("nfl"),
            sport: Sport::Football,
            groups: vec![
                StandingsGroup {
                    name: "AFC East".into(),
                    rows: vec![nfl("BUF", 3, 0), nfl("NE", 2, 1), nfl("NYJ", 1, 2), nfl("MIA", 0, 3)],
                },
                StandingsGroup {
                    name: "AFC West".into(),
                    rows: vec![nfl("KC", 2, 1), nfl("LAC", 2, 1), nfl("DEN", 1, 2), nfl("LV", 1, 2)],
                },
                StandingsGroup {
                    name: "NFC East".into(),
                    rows: vec![nfl("PHI", 3, 0), nfl("DAL", 2, 1), nfl("WSH", 1, 2), nfl("NYG", 0, 3)],
                },
            ],
            fetched_at: now,
        },
        Standings {
            league: LeagueId::new("epl"),
            sport: Sport::Soccer,
            groups: vec![StandingsGroup {
                name: "Premier League".into(),
                rows: vec![
                    epl("MNC", 5, 0, 0),
                    epl("ARS", 4, 0, 1),
                    epl("LIV", 3, 1, 1),
                    epl("CHE", 3, 1, 1),
                    epl("BHA", 3, 1, 1),
                    epl("TOT", 2, 2, 1),
                    epl("AVL", 2, 1, 2),
                    epl("NEW", 2, 1, 2),
                    epl("MUN", 1, 2, 2),
                    epl("BRE", 1, 2, 2),
                ],
            }],
            fetched_at: now,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_every_sport_and_major_status() {
        let games = mock_games(Utc::now());
        for sport in [Sport::Football, Sport::Basketball, Sport::Baseball, Sport::Hockey, Sport::Soccer] {
            assert!(games.iter().any(|g| g.sport == sport), "{sport:?}");
        }
        for status in [GameStatus::InProgress, GameStatus::Halftime, GameStatus::Final, GameStatus::Scheduled] {
            assert!(games.iter().any(|g| g.status == status), "{status:?}");
        }
    }

    #[test]
    fn ids_are_unique_and_schema_round_trips_through_json() {
        let games = mock_games(Utc::now());
        let mut ids: Vec<_> = games.iter().map(|g| g.id.0.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), games.len());
        let json = serde_json::to_string(&games).unwrap();
        let back: Vec<Game> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, games);
    }
}
