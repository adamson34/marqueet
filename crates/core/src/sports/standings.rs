//! League standings, normalized: groups (divisions, conferences or one
//! table) of team rows already in rank order. Providers fill these in.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{LeagueId, Sport, TeamId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Standings {
    pub league: LeagueId,
    pub sport: Sport,
    pub groups: Vec<StandingsGroup>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StandingsGroup {
    /// "AFC East", "AL East", "Atlantic", "Premier League".
    pub name: String,
    /// Best first.
    pub rows: Vec<StandingsRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StandingsRow {
    pub team: TeamId,
    pub abbreviation: String,
    pub wins: u32,
    pub losses: u32,
    /// Ties (football), draws (soccer).
    pub ties: u32,
    /// Overtime losses (hockey).
    pub ot_losses: u32,
    /// Table points (hockey, soccer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<i32>,
    pub games_played: u32,
    /// ".573", as the provider formats it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_pct: Option<String>,
    /// "6", "12.5", or "-" for the leader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub games_behind: Option<String>,
}

impl Standings {
    /// The group `team` is in.
    pub fn group_of(&self, team: &TeamId) -> Option<&StandingsGroup> {
        self.groups.iter().find(|g| g.rows.iter().any(|r| &r.team == team))
    }
}

/// A table column: header and the cell for a row.
pub type Column = (&'static str, fn(&StandingsRow) -> String);

/// Table columns for a sport.
pub fn columns(sport: Sport) -> Vec<Column> {
    match sport {
        Sport::Hockey => vec![
            ("W", |r| r.wins.to_string()),
            ("L", |r| r.losses.to_string()),
            ("OTL", |r| r.ot_losses.to_string()),
            ("PTS", |r| r.points.map_or_else(|| "-".into(), |p| p.to_string())),
        ],
        Sport::Soccer => vec![
            ("P", |r| r.games_played.to_string()),
            ("W", |r| r.wins.to_string()),
            ("D", |r| r.ties.to_string()),
            ("L", |r| r.losses.to_string()),
            ("PTS", |r| r.points.map_or_else(|| "-".into(), |p| p.to_string())),
        ],
        Sport::Football => vec![
            ("W", |r| r.wins.to_string()),
            ("L", |r| r.losses.to_string()),
            ("T", |r| r.ties.to_string()),
            ("PCT", |r| r.win_pct.clone().unwrap_or_else(|| "-".into())),
        ],
        _ => vec![
            ("W", |r| r.wins.to_string()),
            ("L", |r| r.losses.to_string()),
            ("PCT", |r| r.win_pct.clone().unwrap_or_else(|| "-".into())),
            ("GB", |r| r.games_behind.clone().unwrap_or_else(|| "-".into())),
        ],
    }
}
