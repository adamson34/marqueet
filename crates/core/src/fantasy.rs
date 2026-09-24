//! Fantasy football, normalized: a team's matchup this week with each
//! starter's live points. Providers (Sleeper first) fill these in. Pure.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A fantasy platform account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyUser {
    pub id: String,
    pub display_name: String,
}

/// A league the user is in, for picking one on the admin page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyLeagueInfo {
    pub id: String,
    pub name: String,
    pub season: String,
    pub teams: u32,
}

/// A team in a league, for picking one on the admin page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyTeamInfo {
    pub roster_id: u32,
    /// Team name, or the manager's name when there's none.
    pub name: String,
    pub owner_id: Option<String>,
}

/// A starter and what they've scored this week.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Starter {
    /// Lineup slot: "QB", "RB", "FLEX", "K", "DEF".
    pub slot: String,
    pub player_id: String,
    /// "J. Allen", or the team for a defense: "PHI".
    pub name: String,
    pub position: String,
    /// NFL team abbreviation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// ESPN athlete id, to match plays in live games.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub espn_id: Option<String>,
    pub points: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FantasyTeam {
    pub roster_id: u32,
    pub name: String,
    /// "7-7" or "7-6-1".
    pub record: String,
    pub points: f32,
    pub starters: Vec<Starter>,
}

/// One team's matchup this week.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Matchup {
    pub league_id: String,
    pub league: String,
    pub week: u32,
    pub me: FantasyTeam,
    /// `None` on a bye (or in leagues without head-to-head matchups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opponent: Option<FantasyTeam>,
    pub fetched_at: DateTime<Utc>,
}

impl Matchup {
    /// The starter with this ESPN athlete id on either side, and whether
    /// they're on my team.
    pub fn starter_by_espn_id(&self, espn_id: &str) -> Option<(&Starter, bool)> {
        let mine = self.me.starters.iter().find(|s| s.espn_id.as_deref() == Some(espn_id)).map(|s| (s, true));
        mine.or_else(|| {
            self.opponent.as_ref()?.starters.iter().find(|s| s.espn_id.as_deref() == Some(espn_id)).map(|s| (s, false))
        })
    }
}

/// Points as fantasy apps show them: "88.42" → "88.4".
pub fn points(p: f32) -> String {
    format!("{p:.1}")
}
