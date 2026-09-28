//! Leagues served by ESPN's scoreboard endpoints, keyed by Marqueet league id.

use marqueet_core::provider::LeagueInfo;
use marqueet_core::sports::{LeagueId, Sport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeagueDef {
    /// Marqueet league id, e.g. `nfl`.
    pub id: &'static str,
    pub sport: Sport,
    pub name: &'static str,
    /// Path under `/apis/site/v2/sports/`, e.g. `football/nfl`.
    pub path: &'static str,
    /// College basketball plays halves; everything else in basketball plays quarters.
    pub halves: bool,
    pub standings: StandingsLevel,
    /// ESPN's group for the whole top division (`?groups=80`, all of FBS).
    /// The plain scoreboard lists only featured college games, so this is
    /// fetched too, for games with someone's team in them.
    pub division: Option<&'static str>,
}

/// Which standings to fetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandingsLevel {
    /// Divisions (`?level=3`): NFL, MLB, NBA, NHL.
    Divisions,
    /// ESPN's default grouping: conferences, or one table for soccer.
    Default,
    /// Not fetched (college standings are huge and conference-by-conference).
    None,
}

const fn def(id: &'static str, sport: Sport, name: &'static str, path: &'static str) -> LeagueDef {
    let standings = match sport {
        Sport::Soccer => StandingsLevel::Default,
        _ => StandingsLevel::Divisions,
    };
    LeagueDef { id, sport, name, path, halves: false, standings, division: None }
}

pub const LEAGUES: &[LeagueDef] = &[
    def("nfl", Sport::Football, "NFL", "football/nfl"),
    LeagueDef {
        standings: StandingsLevel::None,
        division: Some("80"),
        ..def("ncaaf", Sport::Football, "College Football", "football/college-football")
    },
    def("mlb", Sport::Baseball, "MLB", "baseball/mlb"),
    def("nba", Sport::Basketball, "NBA", "basketball/nba"),
    // Two conferences, no divisions.
    LeagueDef { standings: StandingsLevel::Default, ..def("wnba", Sport::Basketball, "WNBA", "basketball/wnba") },
    LeagueDef {
        halves: true,
        standings: StandingsLevel::None,
        division: Some("50"),
        ..def("ncaam", Sport::Basketball, "Men's College Basketball", "basketball/mens-college-basketball")
    },
    LeagueDef {
        halves: true,
        standings: StandingsLevel::None,
        division: Some("50"),
        ..def("ncaaw", Sport::Basketball, "Women's College Basketball", "basketball/womens-college-basketball")
    },
    def("nhl", Sport::Hockey, "NHL", "hockey/nhl"),
    def("mls", Sport::Soccer, "MLS", "soccer/usa.1"),
    def("epl", Sport::Soccer, "Premier League", "soccer/eng.1"),
    def("ucl", Sport::Soccer, "Champions League", "soccer/uefa.champions"),
];

pub fn find(id: &str) -> Option<&'static LeagueDef> {
    LEAGUES.iter().find(|l| l.id == id)
}

pub fn infos() -> Vec<LeagueInfo> {
    LEAGUES.iter().map(|l| LeagueInfo { id: LeagueId::new(l.id), sport: l.sport, name: l.name.into() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_findable() {
        for l in LEAGUES {
            assert_eq!(find(l.id), Some(l));
            assert_eq!(LEAGUES.iter().filter(|o| o.id == l.id).count(), 1, "{}", l.id);
        }
        assert!(find("curling").is_none());
        assert!(find("ncaam").is_some_and(|l| l.halves));
    }
}
