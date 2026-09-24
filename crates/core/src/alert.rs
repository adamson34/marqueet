//! Alerts: attention-grabbing moments raised by any source.
//!
//! A touchdown, a stock moving 5% or a severe-weather warning are all
//! alerts. A [`AlertLevel::Flash`] animates the matching ticker segment; a
//! [`AlertLevel::Takeover`] also replaces the widget area for a few seconds.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertLevel {
    Flash,
    Takeover,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    /// Deterministic id so the same moment is never shown twice (e.g.
    /// `espn:nfl:401671789:touchdown:21-14`).
    pub id: String,
    pub level: AlertLevel,
    /// Source that raised it: "sports", "weather", "stocks".
    pub source: String,
    /// Ticker segment to flash, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_id: Option<String>,
    /// Big text for a takeover: "TOUCHDOWN".
    pub title: String,
    /// Supporting line: "BUF 21 - KC 17".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Theme colors (e.g. team primary/secondary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<(Rgb, Rgb)>,
    /// What to draw for a [`AlertLevel::Takeover`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover: Option<Takeover>,
    pub created_at: DateTime<Utc>,
}

/// Content of a full takeover of the widget area (see ADR-0007).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Takeover {
    /// Small line above the headline: "BUFFALO BLIZZARD · Q3 4:12".
    pub kicker: String,
    /// "TOUCHDOWN", "HOME RUN", "GOAL".
    pub headline: String,
    /// The play: "Rico Castellano 12 yd run".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play: Option<String>,
    /// The score after the play; `None` for takeovers that aren't about a
    /// game (e.g. from a feed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreLine>,
    /// Fantasy call-out, e.g. ("YOUR PLAYER", "R. Castellano +7.2 pts") (Phase 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreLine {
    pub away: (String, u16),
    pub home: (String, u16),
    /// Which side just scored (highlighted).
    pub scoring_home: bool,
}
