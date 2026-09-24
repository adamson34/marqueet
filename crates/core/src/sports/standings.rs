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
    /// "East Division", "AL East", "Atlantic", "Premier League".
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

/// "1ST", "2ND", "3RD", "11TH", "22ND".
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "TH",
        (1, _) => "ST",
        (2, _) => "ND",
        (3, _) => "RD",
        _ => "TH",
    };
    format!("{n}{suffix}")
}

/// A short record for the ticker: "3-0", "10-2-1", "3-0-1" (hockey),
/// "15 PTS" (soccer).
pub fn short_record(sport: Sport, r: &StandingsRow) -> String {
    match sport {
        Sport::Soccer => format!("{} PTS", r.points.unwrap_or(0)),
        Sport::Hockey => format!("{}-{}-{}", r.wins, r.losses, r.ot_losses),
        _ if r.ties > 0 => format!("{}-{}-{}", r.wins, r.losses, r.ties),
        _ => format!("{}-{}", r.wins, r.losses),
    }
}

/// A favorite's standing as two ticker lines: ("BUF 1ST", "EAST DIVISION 3-0"),
/// or ("IRW 12TH", "5 PTS") for a single table.
pub fn standing_line(s: &Standings, team: &TeamId) -> Option<(String, String)> {
    let group = s.group_of(team)?;
    let (i, row) = group.rows.iter().enumerate().find(|(_, r)| &r.team == team)?;
    let top = format!("{} {}", row.abbreviation, ordinal(i + 1));
    let record = short_record(s.sport, row);
    let bottom = if s.groups.len() == 1 { record } else { format!("{} {record}", group.name.to_uppercase()) };
    Some((top, bottom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_standings;
    use chrono::Utc;

    #[test]
    fn ordinals() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101].into_iter().map(ordinal).collect();
        assert_eq!(got, ["1ST", "2ND", "3RD", "4TH", "11TH", "12TH", "13TH", "21ST", "22ND", "23RD", "101ST"]);
    }

    #[test]
    fn standing_lines_for_the_ticker() {
        let all = mock_standings(Utc::now());
        let team = |id: &str| TeamId(id.into());
        assert_eq!(standing_line(&all[0], &team("mock:nfl:BUF")), Some(("BUF 1ST".into(), "EAST DIVISION 3-0".into())));
        assert_eq!(standing_line(&all[0], &team("mock:nfl:NYS")), Some(("NYS 3RD".into(), "EAST DIVISION 1-2".into())));
        assert_eq!(standing_line(&all[1], &team("mock:epl:IRW")), Some(("IRW 9TH".into(), "5 PTS".into())));
        assert_eq!(standing_line(&all[0], &team("mock:epl:IRW")), None, "other league");
        let mut tie = all[0].groups[0].rows[0].clone();
        tie.ties = 1;
        assert_eq!(short_record(Sport::Football, &tie), "3-0-1");
    }
}
