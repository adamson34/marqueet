//! Messages from the server to the display over the local WebSocket
//! (`ws://<host>:7878/ws`), encoded as JSON text frames.
//!
//! The display is source-agnostic (ADR-0003): it receives ready-to-render
//! ticker and crawl segments, never raw games.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::alert::Alert;
use crate::ticker::TickerSegment;

/// Bumped when a change would confuse an older display.
pub const PROTOCOL_VERSION: u32 = 1;

/// Default port for the display feed and (later) the admin page.
pub const DEFAULT_PORT: u16 = 7878;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// First message on every connection.
    Hello { protocol: u32, server_version: String },
    /// Everything the ticker and crawl should show right now. Sent after
    /// `Hello` and again whenever it changes.
    Content(Content),
    /// A flash or takeover (Phase 3).
    Alert(Box<Alert>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub ticker: Vec<TickerSegment>,
    pub crawl: Vec<TickerSegment>,
    pub status: FeedStatus,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedStatus {
    /// Games currently in progress across all leagues.
    pub live_games: u32,
    /// Leagues whose data is old because recent fetches failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_leagues: Vec<String>,
    /// When the newest data was fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl ServerMsg {
    pub fn to_json(&self) -> String {
        // Serializing these plain data types cannot fail.
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<ServerMsg, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticker::{Part, Span};

    #[test]
    fn messages_round_trip_with_a_type_tag() {
        let msgs = [
            ServerMsg::Hello { protocol: PROTOCOL_VERSION, server_version: "0.1.0".into() },
            ServerMsg::Content(Content {
                ticker: vec![TickerSegment { id: "a".into(), parts: vec![Part::text(vec![Span::primary("HI")])] }],
                crawl: vec![],
                status: FeedStatus { live_games: 2, stale_leagues: vec!["nfl".into()], updated_at: None },
            }),
        ];
        for m in msgs {
            let json = m.to_json();
            assert!(json.starts_with(r#"{"type":""#), "{json}");
            assert_eq!(ServerMsg::from_json(&json).unwrap(), m);
        }
    }

    #[test]
    fn unknown_message_types_are_an_error_not_a_panic() {
        assert!(ServerMsg::from_json(r#"{"type":"from_the_future"}"#).is_err());
    }
}
