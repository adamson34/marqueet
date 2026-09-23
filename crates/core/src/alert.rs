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
    pub created_at: DateTime<Utc>,
}
