//! Messages from the server to the display over the local WebSocket
//! (`ws://<host>:7878/ws`), encoded as JSON text frames.
//!
//! The display is source-agnostic (ADR-0003): it receives ready-to-render
//! ticker and crawl segments, never raw games.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::alert::Alert;
use crate::config::DisplayConfig;
use crate::team_art::Image;
use crate::ticker::TickerSegment;
use crate::widgets::WidgetView;

/// Bumped when a change would confuse an older display.
pub const PROTOCOL_VERSION: u32 = 5;

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
    /// How the display should look; sent after `Hello` and on every change.
    Display(Box<DisplayState>),
    /// Team logos someone added, by key (see [`crate::team_art`]); sent after
    /// the first `Content` and whenever they change, in chunks (see
    /// [`logo_messages`]). `replace` starts a new set; later chunks add to it.
    Logos {
        logos: BTreeMap<String, Image>,
        #[serde(default = "yes")]
        replace: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayState {
    pub config: DisplayConfig,
    /// Quiet hours: blank the screen.
    #[serde(default)]
    pub screen_off: bool,
    /// The configured time zone's current UTC offset, in seconds, for the
    /// display's clock and start times. `None`: use the display's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_offset: Option<i32>,
    /// First boot: what to show so someone can set the device up. Only ever
    /// sent to displays on the device itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<SetupInfo>,
}

/// The first-boot screen's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupInfo {
    /// One-time 6-digit code to enter on the setup page.
    pub code: String,
    /// Where to open the setup page, best first: `http://marqueet.local:7878/setup`,
    /// then by IP address.
    pub urls: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub ticker: Vec<TickerSegment>,
    pub crawl: Vec<TickerSegment>,
    /// Tag in front of the crawl ("TONIGHT", "UP NEXT").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crawl_label: Option<String>,
    pub status: FeedStatus,
    /// Widget area content, in slot order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<WidgetView>,
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

fn yes() -> bool {
    true
}

/// Largest logo chunk, in bytes of pixel data (about 5.4 MB as base64 JSON,
/// well under WebSocket frame limits).
pub const LOGO_CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// A set of logos as `Logos` messages of at most [`LOGO_CHUNK_BYTES`] each;
/// the first replaces what the display had. An empty set is one message.
pub fn logo_messages(logos: &BTreeMap<String, Image>) -> Vec<ServerMsg> {
    let mut out = Vec::new();
    let mut chunk = BTreeMap::new();
    let mut size = 0;
    for (key, image) in logos {
        if !chunk.is_empty() && size + image.rgba.len() > LOGO_CHUNK_BYTES {
            out.push(ServerMsg::Logos { logos: std::mem::take(&mut chunk), replace: out.is_empty() });
            size = 0;
        }
        size += image.rgba.len();
        chunk.insert(key.clone(), image.clone());
    }
    if !chunk.is_empty() || out.is_empty() {
        out.push(ServerMsg::Logos { logos: chunk, replace: out.is_empty() });
    }
    out
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
                crawl_label: Some("TONIGHT".into()),
                status: FeedStatus { live_games: 2, stale_leagues: vec!["nfl".into()], updated_at: None },
                widgets: vec![WidgetView::Empty { title: "SCORES".into(), message: "None".into() }],
            }),
            ServerMsg::Logos {
                logos: [("t".into(), Image::new(1, 1, vec![1, 2, 3, 4]).unwrap())].into(),
                replace: true,
            },
        ];
        for m in msgs {
            let json = m.to_json();
            assert!(json.starts_with(r#"{"type":""#), "{json}");
            assert_eq!(ServerMsg::from_json(&json).unwrap(), m);
        }
    }

    #[test]
    fn logos_go_in_chunks_that_replace_then_add() {
        let big = |n: u8| Image::new(512, 512, vec![n; 512 * 512 * 4]).unwrap(); // 1 MiB each
        let logos: BTreeMap<String, Image> = (0..10).map(|i| (format!("t{i}"), big(i))).collect();
        let msgs = logo_messages(&logos);
        assert_eq!(msgs.len(), 3, "4 + 4 + 2");
        let mut seen = BTreeMap::new();
        for (i, m) in msgs.iter().enumerate() {
            let ServerMsg::Logos { logos, replace } = m else { panic!() };
            assert_eq!(*replace, i == 0);
            assert!(logos.values().map(|l| l.rgba.len()).sum::<usize>() <= LOGO_CHUNK_BYTES);
            seen.extend(logos.clone());
        }
        assert_eq!(seen, logos);
        let empty = logo_messages(&BTreeMap::new());
        assert!(matches!(&empty[..], [ServerMsg::Logos { logos, replace: true }] if logos.is_empty()), "clears");
    }

    #[test]
    fn unknown_message_types_are_an_error_not_a_panic() {
        assert!(ServerMsg::from_json(r#"{"type":"from_the_future"}"#).is_err());
    }
}
