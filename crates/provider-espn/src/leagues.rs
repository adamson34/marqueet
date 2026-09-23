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
}

const fn def(id: &'static str, sport: Sport, name: &'static str, path: &'static str) -> LeagueDef {
    LeagueDef { id, sport, name, path, halves: false }
}

pub const LEAGUES: &[LeagueDef] = &[
    def("nfl", Sport::Football, "NFL", "football/nfl"),
    def("ncaaf", Sport::Football, "College Football", "football/college-football"),
    def("mlb", Sport::Baseball, "MLB", "baseball/mlb"),
    def("nba", Sport::Basketball, "NBA", "basketball/nba"),
    def("wnba", Sport::Basketball, "WNBA", "basketball/wnba"),
    LeagueDef {
        halves: true,
        ..def("ncaam", Sport::Basketball, "Men's College Basketball", "basketball/mens-college-basketball")
    },
    LeagueDef {
        halves: true,
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
