use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sport {
    Football,
    Basketball,
    Baseball,
    Hockey,
    Soccer,
}

/// League key, e.g. `nfl`, `mlb`, `epl`. Open-ended so providers can add
/// leagues without a schema change.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LeagueId(pub String);

impl LeagueId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LeagueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Provider-namespaced team id, e.g. `espn:nfl:12`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TeamId(pub String);

/// Provider-namespaced game id, e.g. `espn:nfl:401671789`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameId(pub String);

impl GameId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamColors {
    pub primary: Rgb,
    pub secondary: Option<Rgb>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: TeamId,
    /// "KC"
    pub abbreviation: String,
    /// "Kingdom"
    pub short_name: String,
    /// "Kansas City Kingdom"
    pub display_name: String,
    pub colors: TeamColors,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    /// "10-3", "82-71"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    /// College poll ranking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeAway {
    Home,
    Away,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CompetitorExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hits: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shots: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeouts: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Competitor {
    pub team: Team,
    pub home_away: HomeAway,
    /// `None` before the game starts.
    pub score: Option<u16>,
    /// Points per period / inning.
    #[serde(default)]
    pub linescore: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<bool>,
    #[serde(default)]
    pub extras: CompetitorExtras,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    Scheduled,
    InProgress,
    Halftime,
    Delayed,
    Final,
    Postponed,
    Canceled,
}

impl GameStatus {
    /// Whether the game is underway (including breaks and delays mid-game).
    pub fn is_live(self) -> bool {
        matches!(self, GameStatus::InProgress | GameStatus::Halftime | GameStatus::Delayed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameClock {
    /// Quarter, inning, period or half (1-based; 0 before start).
    pub period: u8,
    /// Short label: "Q3", "▲7", "2nd", "OT", "HT".
    pub period_label: String,
    /// Game clock if the sport has one: "4:32".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    /// Provider's full status text: "4:32 - 3rd", "Final/OT", "7:30 PM ET".
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InningHalf {
    Top,
    Bottom,
}

/// Live, sport-specific state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "sport", rename_all = "snake_case")]
pub enum Situation {
    Football {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        possession: Option<TeamId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        down: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<u8>,
        /// "3rd & 7 at KC 45"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        down_distance_text: Option<String>,
        #[serde(default)]
        red_zone: bool,
    },
    Baseball {
        half: InningHalf,
        balls: u8,
        strikes: u8,
        outs: u8,
        /// First, second, third.
        bases: [bool; 3],
    },
    Hockey {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        power_play: Option<TeamId>,
    },
    Basketball,
    Soccer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Athlete {
    /// Provider athlete id; fantasy plugins map players through this.
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Play {
    pub id: String,
    pub text: String,
    /// Provider's play type, e.g. "Passing Touchdown", "Home Run".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_value: Option<u8>,
    #[serde(default)]
    pub athletes: Vec<Athlete>,
}

/// One game in a normalized, provider-independent shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: GameId,
    pub provider: String,
    pub league: LeagueId,
    pub sport: Sport,
    pub start_time: DateTime<Utc>,
    pub status: GameStatus,
    pub clock: GameClock,
    pub home: Competitor,
    pub away: Competitor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub situation: Option<Situation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_play: Option<Play>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    /// When this snapshot was fetched from the provider.
    pub fetched_at: DateTime<Utc>,
    /// True when served from cache after fetch failures.
    #[serde(default)]
    pub stale: bool,
}

impl Game {
    pub fn competitor(&self, side: HomeAway) -> &Competitor {
        match side {
            HomeAway::Home => &self.home,
            HomeAway::Away => &self.away,
        }
    }
}
