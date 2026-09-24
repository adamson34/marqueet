//! Custom feeds: ticker content, crawl lines and alerts pushed by local
//! programs through the feed API (`POST /api/feeds/<name>`). Anything that can
//! send JSON can put a stock price, a server status or a doorbell on the sign.
//!
//! This module validates what they send and turns it into ordinary
//! [`TickerSegment`]s and [`Alert`]s, so the display never knows the
//! difference (ADR-0003). Pure.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::alert::{Alert, AlertLevel, Takeover};
use crate::color::Rgb;
use crate::ticker::{Align, Part, Span, TickerSegment, Tint};

pub const MAX_SEGMENTS: usize = 20;
pub const MAX_CRAWL: usize = 20;
/// Characters of text in one segment or crawl line.
pub const MAX_TEXT: usize = 160;
pub const DEFAULT_TTL_SECS: u64 = 15 * 60;
pub const MAX_TTL_SECS: u64 = 24 * 60 * 60;
pub const MIN_TTL_SECS: u64 = 10;

/// Feed names: 1 to 32 of `a-z`, `0-9`, `-`, `_`.
pub fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Where a feed's segments go in the ticker loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    /// After the weather, before the leagues.
    Start,
    /// After the leagues.
    #[default]
    End,
}

/// One segment as sent by a script. Either the simple form (`text`, with an
/// optional `detail` line, `icon` and `color`) or full `parts`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SegmentIn {
    /// Stable id within the feed, so alerts can flash this segment.
    pub id: Option<String>,
    pub text: Option<String>,
    /// A second, dimmer line (stacked under `text` on tall tickers).
    pub detail: Option<String>,
    /// A built-in LED icon, e.g. `sun`, `rain` (see `assets/icons.txt`).
    pub icon: Option<String>,
    /// `#rrggbb`, a preset (`amber`, `red`, `green`, `blue`, `white`), or
    /// `accent` / `dim`.
    pub color: Option<String>,
    pub parts: Option<Vec<Part>>,
}

/// Body of `POST /api/feeds/<name>`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeedPost {
    pub segments: Vec<SegmentIn>,
    pub crawl: Vec<SegmentIn>,
    /// Seconds until the content disappears unless refreshed (default 15 min).
    pub ttl: Option<u64>,
    pub position: Position,
}

/// A feed's current content.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Feed {
    pub name: String,
    pub segments: Vec<TickerSegment>,
    pub crawl: Vec<TickerSegment>,
    pub position: Position,
    pub expires_at: DateTime<Utc>,
}

fn tint(color: Option<&str>) -> Result<Tint, String> {
    Ok(match color.map(str::trim) {
        None | Some("") | Some("primary") => Tint::Primary,
        Some("accent") => Tint::Accent,
        Some("dim") => Tint::Dim,
        Some(c) => Tint::Color(c.parse::<Rgb>().map_err(|e| e.to_string())?),
    })
}

fn text_len(parts: &[Part]) -> usize {
    let spans = |s: &[Span]| s.iter().map(|s| s.text.chars().count()).sum::<usize>();
    parts
        .iter()
        .map(|p| match p {
            Part::Text { spans: s } => spans(s),
            Part::Stack { top, bottom, .. } => spans(top) + spans(bottom),
            Part::Gap { .. } | Part::Icon { .. } | Part::Logos { .. } => 0,
        })
        .sum()
}

fn segment(feed: &str, index: usize, s: &SegmentIn) -> Result<TickerSegment, String> {
    let local = s.id.clone().unwrap_or_else(|| index.to_string());
    if !valid_name(&local) {
        return Err(format!("segment id {local:?}: use 1-32 of a-z, 0-9, - and _"));
    }
    let parts = match (&s.parts, &s.text) {
        (Some(parts), None) => parts.clone(),
        (None, Some(text)) => {
            let tint = tint(s.color.as_deref())?;
            let mut parts = Vec::new();
            if let Some(icon) = &s.icon {
                parts.push(Part::icon(icon.clone()));
                parts.push(Part::gap(3));
            }
            parts.push(match &s.detail {
                Some(detail) => {
                    Part::stack(vec![Span::new(text.clone(), tint)], vec![Span::dim(detail.clone())], Align::Left)
                }
                None => Part::text(vec![Span::new(text.clone(), tint)]),
            });
            parts
        }
        _ => return Err(format!("segment {local:?}: send either \"text\" or \"parts\"")),
    };
    let len = text_len(&parts);
    if len == 0 {
        return Err(format!("segment {local:?} has no text"));
    }
    if len > MAX_TEXT {
        return Err(format!("segment {local:?} is {len} characters; the limit is {MAX_TEXT}"));
    }
    Ok(TickerSegment { id: format!("feed:{feed}:{local}"), parts })
}

/// Validates a post and turns it into the feed's new content.
pub fn accept(name: &str, post: &FeedPost, now: DateTime<Utc>) -> Result<Feed, String> {
    if !valid_name(name) {
        return Err(format!("feed name {name:?}: use 1-32 of a-z, 0-9, - and _"));
    }
    if post.segments.len() > MAX_SEGMENTS || post.crawl.len() > MAX_CRAWL {
        return Err(format!("at most {MAX_SEGMENTS} segments and {MAX_CRAWL} crawl lines"));
    }
    let segments = post.segments.iter().enumerate().map(|(i, s)| segment(name, i, s)).collect::<Result<Vec<_>, _>>()?;
    let crawl = post
        .crawl
        .iter()
        .enumerate()
        .map(|(i, s)| segment(name, i, s).map(|seg| TickerSegment { id: format!("{}:crawl", seg.id), ..seg }))
        .collect::<Result<Vec<_>, _>>()?;
    let ttl = post.ttl.unwrap_or(DEFAULT_TTL_SECS).clamp(MIN_TTL_SECS, MAX_TTL_SECS);
    Ok(Feed {
        name: name.to_owned(),
        segments,
        crawl,
        position: post.position,
        expires_at: now + Duration::seconds(ttl as i64),
    })
}

/// Body of `POST /api/feeds/<name>/alert`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertPost {
    /// "DOORBELL", "BUILD FAILED".
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    /// `flash` (default) or `takeover`.
    #[serde(default = "flash")]
    pub level: AlertLevel,
    /// Segment id (within this feed) to flash; defaults to the feed's first.
    #[serde(default)]
    pub segment: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

fn flash() -> AlertLevel {
    AlertLevel::Flash
}

/// Validates an alert post. `feed` is the feed's current content, if any.
pub fn alert(name: &str, post: &AlertPost, feed: Option<&Feed>, now: DateTime<Utc>) -> Result<Alert, String> {
    let title = post.title.trim();
    if title.is_empty() || title.chars().count() > 40 {
        return Err("title must be 1 to 40 characters".into());
    }
    let detail = post.detail.as_deref().map(str::trim).filter(|d| !d.is_empty());
    if detail.is_some_and(|d| d.chars().count() > 80) {
        return Err("detail must be at most 80 characters".into());
    }
    let color = match tint(post.color.as_deref())? {
        Tint::Color(c) => Some(c),
        _ => None,
    };
    let segment_id = match &post.segment {
        Some(id) => {
            let full = format!("feed:{name}:{id}");
            if !feed.is_some_and(|f| f.segments.iter().any(|s| s.id == full)) {
                return Err(format!("feed {name:?} has no segment {id:?}"));
            }
            Some(full)
        }
        None => feed.and_then(|f| f.segments.first()).map(|s| s.id.clone()),
    };
    let takeover = (post.level == AlertLevel::Takeover).then(|| Takeover {
        kicker: name.to_uppercase().replace(['-', '_'], " "),
        headline: title.to_uppercase(),
        play: detail.map(str::to_owned),
        score: None,
        note: None,
    });
    Ok(Alert {
        id: format!("feed:{name}:{}", now.timestamp_nanos_opt().unwrap_or_default()),
        level: post.level,
        source: format!("feed:{name}"),
        segment_id,
        title: title.to_owned(),
        detail: detail.map(str::to_owned),
        colors: color.map(|c| (c, c)),
        takeover,
        created_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::ticker::segment_text;

    fn post(json: &str) -> FeedPost {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn simple_segments_become_ticker_segments() {
        let now = Utc::now();
        let p = post(
            r##"{"segments":[{"id":"aapl","text":"AAPL 189.20","detail":"+1.2%","color":"#33cc66"},
                 {"icon":"sun","text":"PATIO 74°"}],"crawl":[{"text":"Backups ran at 3:00 AM"}],"ttl":60}"##,
        );
        let f = accept("stocks", &p, now).unwrap();
        assert_eq!(f.segments[0].id, "feed:stocks:aapl");
        assert_eq!(segment_text(&f.segments[0]), "AAPL 189.20/+1.2%");
        assert_eq!(f.segments[1].id, "feed:stocks:1", "index when no id");
        assert_eq!(segment_text(&f.segments[1]), "[sun] PATIO 74°");
        let Part::Stack { top, .. } = &f.segments[0].parts[0] else { panic!() };
        assert_eq!(top[0].tint, Tint::Color(Rgb::new(0x33, 0xcc, 0x66)));
        assert_eq!(f.crawl[0].id, "feed:stocks:0:crawl");
        assert_eq!(f.expires_at, now + Duration::seconds(60));
        assert_eq!(f.position, Position::End);
    }

    #[test]
    fn full_parts_are_accepted() {
        let p = post(
            r#"{"segments":[{"parts":[{"kind":"text","spans":[{"text":"HI","tint":{"tint":"accent"}}]}]}],"position":"start"}"#,
        );
        let f = accept("x", &p, Utc::now()).unwrap();
        assert_eq!(segment_text(&f.segments[0]), "HI");
        assert_eq!(f.position, Position::Start);
    }

    #[test]
    fn bad_posts_are_explained() {
        let now = Utc::now();
        let err = |name: &str, json: &str| accept(name, &post(json), now).unwrap_err();
        assert!(err("Stocks!", "{}").contains("feed name"));
        assert!(err("x", r#"{"segments":[{}]}"#).contains("either"));
        assert!(err("x", r#"{"segments":[{"text":""}]}"#).contains("no text"));
        assert!(err("x", &format!(r#"{{"segments":[{{"text":"{}"}}]}}"#, "A".repeat(200))).contains("limit"));
        assert!(err("x", r#"{"segments":[{"id":"Bad Id","text":"a"}]}"#).contains("segment id"));
        assert!(err("x", r#"{"segments":[{"text":"a","color":"plaid"}]}"#).contains("color"));
        let many = format!(r#"{{"segments":[{}]}}"#, vec![r#"{"text":"a"}"#; 21].join(","));
        assert!(err("x", &many).contains("at most"));
        assert!(serde_json::from_str::<FeedPost>(r#"{"segmets":[]}"#).is_err(), "typos are caught");
        let f = accept("x", &post(r#"{"ttl":999999}"#), now).unwrap();
        assert_eq!(f.expires_at, now + Duration::seconds(MAX_TTL_SECS as i64), "ttl is capped");
    }

    #[test]
    fn alerts_flash_the_feed_or_take_over() {
        let now = Utc::now();
        let f = accept("home", &post(r#"{"segments":[{"id":"door","text":"FRONT DOOR"}]}"#), now).unwrap();
        let a: AlertPost = serde_json::from_str(r#"{"title":"Doorbell"}"#).unwrap();
        let flash = alert("home", &a, Some(&f), now).unwrap();
        assert_eq!((flash.level, flash.segment_id.as_deref()), (AlertLevel::Flash, Some("feed:home:door")));
        assert!(flash.takeover.is_none());

        let a: AlertPost =
            serde_json::from_str(r#"{"title":"Doorbell","detail":"Front door","level":"takeover","color":"blue"}"#)
                .unwrap();
        let t = alert("home", &a, None, now).unwrap();
        let takeover = t.takeover.unwrap();
        assert_eq!((takeover.kicker.as_str(), takeover.headline.as_str()), ("HOME", "DOORBELL"));
        assert_eq!((takeover.play.as_deref(), takeover.score), (Some("Front door"), None));
        assert_eq!(t.colors, Some((Rgb::BLUE, Rgb::BLUE)));

        let bad: AlertPost = serde_json::from_str(r#"{"title":"x","segment":"nope"}"#).unwrap();
        assert!(alert("home", &bad, Some(&f), now).unwrap_err().contains("no segment"));
        let empty: AlertPost = serde_json::from_str(r#"{"title":"  "}"#).unwrap();
        assert!(alert("home", &empty, None, now).is_err());
    }

    #[test]
    fn names() {
        assert!(valid_name("stocks") && valid_name("home-lab_2"));
        assert!(!valid_name("") && !valid_name("Stocks") && !valid_name("a b") && !valid_name(&"a".repeat(33)));
    }
}
