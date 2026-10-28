//! Tools for assistants: what the admin page does, as named actions that
//! take and return JSON (ADR-0017). One table backs both
//! `POST /api/tools/<name>` and the MCP endpoint ([`crate::mcp`]), so
//! someone can tell Claude "use my team's colors and make their goals
//! flash" and it can do it.
//!
//! Every call needs admin access: the device itself, a logged-in session, or
//! an API key from the admin page ([`crate::admin::auth`]). Making or
//! revoking API keys and changing the password stay on the admin page.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use base64::Engine;
use chrono::Utc;
use marqueet_core::art::{ArtPlacement, TakeoverArt};
use marqueet_core::feeds::{AlertPost, FeedPost};
use marqueet_core::sports::{TeamColors, TeamId};
use marqueet_core::team_art::{TakeoverWords, TeamArt, WORD_PLAYS};
use marqueet_core::test_alerts::TestKind;
use marqueet_core::weather::Place;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::admin::auth::{self, Access};
use crate::admin::known_teams;
use crate::hub::FeedError;
use crate::web::AppState;
use crate::{team_art, tz};

/// Largest request: LED art as base64 or as text pixels fits well inside.
pub const MAX_BODY: usize = 4 * 1024 * 1024;

/// A tool: its name, what it does (for the assistant), and its arguments as
/// a JSON Schema.
#[derive(Debug)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// Only reads; changes nothing on the sign.
    pub read_only: bool,
    schema: fn() -> Value,
}

impl Tool {
    pub fn input_schema(&self) -> Value {
        (self.schema)()
    }
}

fn no_args() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "status",
        description: "What the sign is showing: followed leagues and whether their data is fresh, \
                      today's games with scores and status, recent alerts (scores, weather) and \
                      feeds. Start here.",
        read_only: true,
        schema: no_args,
    },
    Tool {
        name: "get_settings",
        description: "All settings as JSON: leagues, favorite teams, takeovers, widgets, the \
                      display's look (theme, LED color, brightness), quiet hours, time zone, \
                      weather, fantasy, spotlight. Read this before update_settings to see the shape.",
        read_only: true,
        schema: no_args,
    },
    Tool {
        name: "update_settings",
        description: "Change settings with a JSON merge patch (RFC 7396): give only the fields to \
                      change, nested objects merge, null resets a field to its default, arrays \
                      replace. Example: {\"patch\": {\"leagues\": [\"nfl\", \"epl\"], \
                      \"takeovers\": \"favorites\"}}. Returns the saved settings. For the weather \
                      location use set_location.",
        read_only: false,
        schema: || {
            object(
                json!({ "patch": { "type": "object", "description": "Fields to change, shaped like get_settings." } }),
                &["patch"],
            )
        },
    },
    Tool {
        name: "set_location",
        description: "Set where the weather is for, by place name (looked up) or \"lat, lon\"; \
                      the place's time zone is used if none is set. {\"clear\": true} removes it.",
        read_only: false,
        schema: || {
            object(
                json!({
                    "query": { "type": "string", "description": "\"Kansas City\" or \"39.1, -94.58\"" },
                    "clear": { "type": "boolean" }
                }),
                &[],
            )
        },
    },
    Tool {
        name: "list_leagues",
        description: "Leagues the sign can follow (id, name, sport) and whether each is followed.",
        read_only: true,
        schema: no_args,
    },
    Tool {
        name: "list_teams",
        description: "Teams in the followed leagues: id (use it in other tools), league, name, \
                      whether it's a favorite and whether it has its own colors, logo, words or \
                      art. Filter by league id and/or part of the name.",
        read_only: true,
        schema: || {
            object(
                json!({
                    "league": { "type": "string", "description": "League id, e.g. \"epl\"" },
                    "query": { "type": "string", "description": "Part of the team's name" }
                }),
                &[],
            )
        },
    },
    Tool {
        name: "get_team_art",
        description: "A team's own colors, takeover words, LED art (size, frames, speed, \
                      placement) and whether it has a logo.",
        read_only: true,
        schema: || object(json!({ "team": { "type": "string", "description": "Team id from list_teams" } }), &["team"]),
    },
    Tool {
        name: "set_team_art",
        description: "Give a team its own look for the ticker, widgets and takeovers when it \
                      scores. Fields left out stay as they are; null removes one. \
                      colors: {primary, secondary} as #rrggbb. words: per play (touchdown, \
                      home_run, grand_slam, goal) a headline of up to 16 characters and an \
                      optional line of up to 40, shown instead of \"GOAL\". art: LED art played in \
                      the takeover, one LED per pixel, up to 96x48 and 48 frames, either \
                      image_base64 (a PNG, a PNG strip with frames_in_strip frames side by side, \
                      or an animated GIF) or pixels {palette: {\"R\": \"#95103e\"}, frames: \
                      [[\"row\", ...], ...]} where each character is one LED and . or space is \
                      off; frame_ms 40-2000; placement above (over the words), intro (on its own \
                      first, about one loop up to 4 s) or behind (dimmed behind the words). \
                      logo_png_base64: a PNG logo. Only use art the owner may use: Marqueet \
                      ships no team logos. Check it with test_takeover.",
        read_only: false,
        schema: || {
            object(
                json!({
                    "team": { "type": "string", "description": "Team id from list_teams" },
                    "colors": {
                        "type": ["object", "null"],
                        "properties": { "primary": { "type": "string" }, "secondary": { "type": "string" } },
                        "required": ["primary"]
                    },
                    "words": {
                        "type": "object",
                        "description": "Play id -> {headline, line} or null to remove",
                        "additionalProperties": {
                            "type": ["object", "null"],
                            "properties": { "headline": { "type": "string" }, "line": { "type": "string" } },
                            "required": ["headline"]
                        }
                    },
                    "art": {
                        "type": ["object", "null"],
                        "properties": {
                            "image_base64": { "type": "string" },
                            "frames_in_strip": { "type": "integer", "minimum": 1 },
                            "pixels": {
                                "type": "object",
                                "properties": {
                                    "palette": { "type": "object", "additionalProperties": { "type": "string" } },
                                    "frames": { "type": "array", "items": { "type": "array", "items": { "type": "string" } } }
                                },
                                "required": ["palette", "frames"]
                            },
                            "frame_ms": { "type": "integer", "minimum": 40, "maximum": 2000 },
                            "placement": { "type": "string", "enum": ["above", "intro", "behind"] }
                        }
                    },
                    "logo_png_base64": { "type": ["string", "null"] }
                }),
                &["team"],
            )
        },
    },
    Tool {
        name: "remove_team_art",
        description: "Remove everything a team was given (colors, logo, words, art).",
        read_only: false,
        schema: || object(json!({ "team": { "type": "string" } }), &["team"]),
    },
    Tool {
        name: "test_takeover",
        description: "Play a sample takeover on the screen now, to check how it looks. kind: \
                      touchdown, home_run, grand_slam, hockey_goal, soccer_goal, weather or feed. \
                      With team, it's that team's (with its colors, words and art).",
        read_only: false,
        schema: || {
            let kinds: Vec<&str> = TestKind::ALL.iter().map(|k| k.id()).collect();
            object(
                json!({
                    "kind": { "type": "string", "enum": kinds },
                    "team": { "type": "string", "description": "Team id from list_teams" }
                }),
                &["kind"],
            )
        },
    },
    Tool {
        name: "list_feeds",
        description: "Feeds (content from the owner's own scripts) and what each is showing.",
        read_only: true,
        schema: no_args,
    },
    Tool {
        name: "create_feed",
        description: "Make a feed for a script to post to; returns its token, shown only this \
                      once. Name: 1-32 of a-z, 0-9, - and _.",
        read_only: false,
        schema: || object(json!({ "name": { "type": "string" } }), &["name"]),
    },
    Tool {
        name: "revoke_feed",
        description: "Delete a feed, its token and what it's showing.",
        read_only: false,
        schema: || object(json!({ "name": { "type": "string" } }), &["name"]),
    },
    Tool {
        name: "post_to_feed",
        description: "Put content on the sign through a feed, as its script would: \
                      content is {segments: [{text, detail?, color?, id?}], crawl: [...], ttl?: \
                      seconds (default 900)}; color is primary, accent, dim or #rrggbb. Replaces \
                      the feed's content. See docs/FEEDS.md.",
        read_only: false,
        schema: || {
            object(json!({ "name": { "type": "string" }, "content": { "type": "object" } }), &["name", "content"])
        },
    },
    Tool {
        name: "feed_alert",
        description: "Flash a feed's segment or take over the screen: alert is {title (1-40), \
                      detail?, level: flash|takeover, segment?, color?}. Rate limited.",
        read_only: false,
        schema: || object(json!({ "name": { "type": "string" }, "alert": { "type": "object" } }), &["name", "alert"]),
    },
    Tool {
        name: "find_fantasy",
        description: "Look up a Sleeper username's fantasy leagues and their teams (for add_fantasy).",
        read_only: true,
        schema: || object(json!({ "username": { "type": "string" } }), &["username"]),
    },
    Tool {
        name: "add_fantasy",
        description: "Follow a fantasy team found with find_fantasy.",
        read_only: false,
        schema: || {
            object(
                json!({
                    "league_id": { "type": "string" },
                    "league": { "type": "string", "description": "The league's name" },
                    "roster_id": { "type": "integer" }
                }),
                &["league_id", "league", "roster_id"],
            )
        },
    },
    Tool {
        name: "remove_fantasy",
        description: "Stop following a fantasy team.",
        read_only: false,
        schema: || {
            object(
                json!({ "league_id": { "type": "string" }, "roster_id": { "type": "integer" } }),
                &["league_id", "roster_id"],
            )
        },
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Arguments as `T`; a missing body is `{}`.
fn args<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    let v = if v.is_null() { Value::Object(Map::new()) } else { v };
    serde_json::from_value(v).map_err(|e| format!("arguments: {e}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamArg {
    team: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameArg {
    name: String,
}

/// Runs `name` with `arguments`. Errors are for the assistant to read.
pub async fn call(state: &AppState, name: &str, arguments: Value) -> Result<Value, String> {
    let hub = &state.hub;
    match name {
        "status" => Ok(status(state)),
        "get_settings" => Ok(json!(hub.settings())),
        "update_settings" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                patch: Value,
            }
            let a: A = args(arguments)?;
            let settings = hub.settings().patched(&a.patch)?;
            Ok(json!(hub.apply_settings(settings)?))
        }
        "set_location" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                #[serde(default)]
                query: Option<String>,
                #[serde(default)]
                clear: bool,
            }
            let a: A = args(arguments)?;
            let mut settings = hub.settings();
            let query = a.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
            settings.weather.place = match (a.clear, query) {
                (true, _) => None,
                (false, None) => return Err("give a query, or clear: true".into()),
                (false, Some(q)) => Some(match Place::from_coordinates(q) {
                    Some(p) => p,
                    None => hub
                        .search_places(q)
                        .await?
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("couldn't find a place called {q:?}"))?,
                }),
            };
            tz::adopt_place_zone(&mut settings);
            let saved = hub.apply_settings(settings)?;
            Ok(json!({ "place": saved.weather.place, "time_zone": saved.time_zone }))
        }
        "list_leagues" => {
            let followed = hub.settings().leagues;
            let leagues: Vec<Value> = hub
                .supported_leagues()
                .into_iter()
                .map(|l| json!({ "id": l.id, "name": l.name, "sport": l.sport, "followed": followed.contains(&l.id) }))
                .collect();
            Ok(json!(leagues))
        }
        "list_teams" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                #[serde(default)]
                league: Option<String>,
                #[serde(default)]
                query: Option<String>,
            }
            let a: A = args(arguments)?;
            let query = a.query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());
            let (favorites, art) = (hub.settings().favorites, hub.team_art());
            let teams: Vec<Value> = hub
                .with_store(known_teams)
                .into_iter()
                .filter(|t| a.league.as_deref().is_none_or(|l| t.league.as_str() == l.trim().to_lowercase()))
                .filter(|t| query.as_deref().is_none_or(|q| t.name.to_lowercase().contains(q)))
                .take(300)
                .map(|t| {
                    let own = art.get(&t.id).map(|a| {
                        json!({
                            "colors": a.colors.is_some(), "logo": a.logo.is_some(),
                            "words": !a.words.is_empty(), "art": a.art.is_some()
                        })
                    });
                    json!({
                        "id": t.id, "league": t.league, "name": t.name,
                        "favorite": favorites.contains(&t.id), "own": own
                    })
                })
                .collect();
            Ok(json!(teams))
        }
        "get_team_art" => {
            let a: TeamArg = args(arguments)?;
            let team = TeamId(a.team);
            Ok(hub.team_art().get(&team).map_or(Value::Null, |art| art_summary(&team, art)))
        }
        "set_team_art" => set_team_art(state, arguments),
        "remove_team_art" => {
            let a: TeamArg = args(arguments)?;
            hub.remove_team_art(&TeamId(a.team))?;
            Ok(json!({ "removed": true }))
        }
        "test_takeover" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                kind: String,
                #[serde(default)]
                team: Option<String>,
            }
            let a: A = args(arguments)?;
            let kind = TestKind::from_id(&a.kind).ok_or_else(|| format!("unknown kind {:?}", a.kind))?;
            let team = a.team.filter(|t| !t.trim().is_empty()).map(TeamId);
            hub.test_takeover(kind, team.as_ref())?;
            Ok(json!({ "playing": true, "note": "It shows for about 10 seconds; takeovers play one at a time." }))
        }
        "list_feeds" => Ok(json!(hub.feeds())),
        "create_feed" => {
            let a: NameArg = args(arguments)?;
            let token = hub.create_feed(&a.name)?;
            Ok(json!({
                "name": a.name, "token": token,
                "note": "Shown only this once. Scripts send it as Authorization: Bearer <token> to POST /api/feeds/<name>."
            }))
        }
        "revoke_feed" => {
            let a: NameArg = args(arguments)?;
            hub.revoke_feed(&a.name)?;
            Ok(json!({ "revoked": true }))
        }
        "post_to_feed" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                name: String,
                content: FeedPost,
            }
            let a: A = args(arguments)?;
            if !hub.feed_names().contains(&a.name) {
                return Err(format!("no feed called {:?}; make it with create_feed", a.name));
            }
            hub.post_feed(&a.name, &a.content).map(|info| json!(info)).map_err(feed_error)
        }
        "feed_alert" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                name: String,
                alert: AlertPost,
            }
            let a: A = args(arguments)?;
            if !hub.feed_names().contains(&a.name) {
                return Err(format!("no feed called {:?}", a.name));
            }
            let alert = hub.feed_alert(&a.name, &a.alert).map_err(feed_error)?;
            Ok(json!({ "id": alert.id, "level": alert.level }))
        }
        "find_fantasy" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                username: String,
            }
            let a: A = args(arguments)?;
            let found = hub.find_fantasy(a.username.trim()).await?;
            let leagues: Vec<Value> =
                found.leagues.iter().map(|(league, teams)| json!({ "league": league, "teams": teams })).collect();
            Ok(json!({ "user": found.user, "leagues": leagues }))
        }
        "add_fantasy" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                league_id: String,
                league: String,
                roster_id: u32,
            }
            let a: A = args(arguments)?;
            hub.add_fantasy(&a.league_id, &a.league, a.roster_id).await?;
            Ok(json!({ "following": hub.settings().fantasy }))
        }
        "remove_fantasy" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct A {
                league_id: String,
                roster_id: u32,
            }
            let a: A = args(arguments)?;
            hub.remove_fantasy(&a.league_id, a.roster_id)?;
            Ok(json!({ "following": hub.settings().fantasy }))
        }
        _ => Err(format!("no tool called {name:?}")),
    }
}

fn feed_error(e: FeedError) -> String {
    match e {
        FeedError::Invalid(e) => e,
        FeedError::TooSoon(secs) => format!("too soon; try again in {secs} s"),
    }
}

fn status(state: &AppState) -> Value {
    let hub = &state.hub;
    let settings = hub.settings();
    let now = Utc::now();
    let offset = tz::offset(&settings, now);
    let (leagues, games) = hub.with_store(|store| {
        let leagues: Vec<Value> = store
            .leagues()
            .iter()
            .map(|id| {
                let feed = store.feed(id).cloned().unwrap_or_default();
                json!({
                    "id": id, "games": feed.games.len(), "stale": store.is_stale(id),
                    "last_error": feed.last_error, "last_success": feed.last_success
                })
            })
            .collect();
        let games: Vec<Value> = store
            .games()
            .iter()
            .map(|g| {
                let side = |c: &marqueet_core::sports::Competitor| {
                    json!({ "team": c.team.id, "name": c.team.display_name, "abbr": c.team.abbreviation, "score": c.score })
                };
                json!({
                    "id": g.id, "league": g.league, "away": side(&g.away), "home": side(&g.home),
                    "status": marqueet_core::widgets::status_text(g, offset, now).0
                })
            })
            .collect();
        (leagues, games)
    });
    let alerts: Vec<Value> = hub
        .recent_alerts()
        .iter()
        .take(10)
        .map(|a| json!({ "title": a.title, "detail": a.detail, "source": a.source, "at": a.created_at }))
        .collect();
    json!({
        "leagues": leagues, "games": games, "recent_alerts": alerts, "feeds": hub.feeds(),
        "favorites": settings.favorites, "takeovers": settings.takeovers
    })
}

fn art_summary(team: &TeamId, art: &TeamArt) -> Value {
    let colors = art
        .colors
        .as_ref()
        .map(|c| json!({ "primary": c.primary.to_string(), "secondary": c.secondary.map(|s| s.to_string()) }));
    let led = art.art.as_ref().map(|a| {
        let (w, h) = a.size();
        json!({
            "width": w, "height": h, "frames": a.frames.len(), "frame_ms": a.frame_ms,
            "placement": a.placement.id()
        })
    });
    let logo = art.logo.as_ref().map(|l| json!({ "width": l.width, "height": l.height }));
    json!({ "team": team, "label": art.label, "colors": colors, "words": art.words, "art": led, "logo": logo })
}

fn base64_bytes(s: &str, what: &str) -> Result<Vec<u8>, String> {
    // Tolerate a data: URL and line breaks.
    let s = s.split_once(";base64,").map_or(s, |(_, b)| b);
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(clean).map_err(|e| format!("{what} isn't base64: {e}"))
}

fn led_art(v: &Value) -> Result<TakeoverArt, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Pixels {
        palette: BTreeMap<String, String>,
        frames: Vec<Vec<String>>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct A {
        #[serde(default)]
        image_base64: Option<String>,
        #[serde(default)]
        frames_in_strip: Option<u32>,
        #[serde(default)]
        pixels: Option<Pixels>,
        #[serde(default)]
        frame_ms: Option<u16>,
        #[serde(default)]
        placement: Option<String>,
    }
    let a: A = serde_json::from_value(v.clone()).map_err(|e| format!("art: {e}"))?;
    let (frames, file_ms) = match (a.image_base64, a.pixels) {
        (Some(b64), None) => {
            let bytes = base64_bytes(&b64, "art.image_base64")?;
            marqueet_core::art::decode(&bytes, a.frames_in_strip.unwrap_or(1))?
        }
        (None, Some(p)) => (marqueet_core::art::from_pixels(&p.palette, &p.frames)?, None),
        _ => return Err("art needs image_base64 or pixels (one of them)".into()),
    };
    let ms = a.frame_ms.or(file_ms).unwrap_or(marqueet_core::art::FRAME_MS_DEFAULT);
    let placement = match a.placement.as_deref() {
        None => ArtPlacement::default(),
        Some(p) => ArtPlacement::from_id(p).ok_or_else(|| format!("placement {p:?}: above, intro or behind"))?,
    };
    TakeoverArt::new(frames, ms, placement)
}

fn set_team_art(state: &AppState, arguments: Value) -> Result<Value, String> {
    let Value::Object(mut fields) = arguments else { return Err("arguments have to be an object".into()) };
    let team = match fields.remove("team") {
        Some(Value::String(t)) if !t.trim().is_empty() => TeamId(t.trim().to_owned()),
        _ => return Err("team is required (an id from list_teams)".into()),
    };
    let hub = &state.hub;
    let current = hub.team_art().get(&team).cloned();
    let known = hub.with_store(known_teams).into_iter().find(|t| t.id == team).map(|t| t.name);
    let label = match (known, &current) {
        (Some(name), _) => name,
        (None, Some(c)) => c.label.clone(),
        (None, None) => return Err(format!("no team {:?} in the followed leagues; see list_teams", team.0)),
    };
    let mut art = current.unwrap_or_else(|| TeamArt {
        label: String::new(),
        colors: None,
        logo: None,
        words: BTreeMap::new(),
        art: None,
    });
    art.label = label;
    for (key, v) in fields {
        match (key.as_str(), v) {
            ("colors", Value::Null) => art.colors = None,
            ("colors", v) => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct C {
                    primary: String,
                    #[serde(default)]
                    secondary: Option<String>,
                }
                let c: C = serde_json::from_value(v).map_err(|e| format!("colors: {e}"))?;
                let parse = |s: &str| s.trim().parse().map_err(|_| format!("{s:?} isn't a color"));
                let secondary = c.secondary.as_deref().map(parse).transpose()?;
                art.colors = Some(TeamColors { primary: parse(&c.primary)?, secondary });
            }
            ("words", Value::Null) => art.words.clear(),
            ("words", Value::Object(plays)) => {
                for (play, w) in plays {
                    if w.is_null() {
                        art.words.remove(&play);
                        continue;
                    }
                    let text = |k: &str| w.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
                    let (play, words) =
                        TakeoverWords::clean(&play, &text("headline"), &text("line")).ok_or_else(|| {
                            let plays: Vec<&str> = WORD_PLAYS.iter().map(|(id, ..)| *id).collect();
                            format!("words for {play:?}: plays are {} and a headline is needed", plays.join(", "))
                        })?;
                    art.words.insert(play, words);
                }
            }
            ("art", Value::Null) => art.art = None,
            ("art", v) => art.art = Some(led_art(&v)?),
            ("logo_png_base64", Value::Null) => art.logo = None,
            ("logo_png_base64", Value::String(s)) => {
                art.logo = Some(team_art::decode_png(&base64_bytes(&s, "logo_png_base64")?)?);
            }
            (key, _) => return Err(format!("unknown or malformed field {key:?}")),
        }
    }
    if art.is_empty() {
        hub.remove_team_art(&team)?;
        return Ok(Value::Null);
    }
    hub.set_team_art(vec![(team.clone(), art.clone())])?;
    Ok(art_summary(&team, &art))
}

// HTTP: `GET /api/tools` lists them, `POST /api/tools/<name>` runs one.

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tools", get(list))
        .route("/api/tools/{name}", post(run))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

/// The JSON rejection for a request without admin access, if it lacks it.
pub fn denied(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    // A browser page on another site can't use a session to call tools.
    if !auth::same_origin(headers) {
        return Some(error(StatusCode::FORBIDDEN, "cross-site request refused"));
    }
    match state.auth.check(peer.ip(), headers) {
        Access::Granted => None,
        Access::Setup => Some(error(StatusCode::FORBIDDEN, "finish setup first: open /setup")),
        Access::NeedsLogin => {
            let mut res =
                error(StatusCode::UNAUTHORIZED, "send an API key from the admin page as Authorization: Bearer <key>");
            res.headers_mut().insert("www-authenticate", HeaderValue::from_static("Bearer"));
            Some(res)
        }
    }
}

/// Every tool as MCP lists it: name, description, input schema, hints.
pub fn descriptions() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema(),
                "annotations": { "readOnlyHint": t.read_only, "openWorldHint": false }
            })
        })
        .collect()
}

async fn list(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(res) = denied(&state, peer, &headers) {
        return res;
    }
    Json(descriptions()).into_response()
}

async fn run(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(res) = denied(&state, peer, &headers) {
        return res;
    }
    if find(&name).is_none() {
        return error(StatusCode::NOT_FOUND, format!("no tool called {name:?}; GET /api/tools lists them"));
    }
    let arguments = if body.iter().all(u8::is_ascii_whitespace) {
        Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return error(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")),
        }
    };
    match call(&state, &name, arguments).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_an_object_schema_and_a_unique_name() {
        for (i, t) in TOOLS.iter().enumerate() {
            let s = t.input_schema();
            assert_eq!(s["type"], "object", "{}", t.name);
            assert!(s["properties"].is_object(), "{}", t.name);
            assert!(TOOLS[..i].iter().all(|o| o.name != t.name), "{} twice", t.name);
            assert!(t.name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{}", t.name);
        }
        let kinds = &find("test_takeover").unwrap().input_schema()["properties"]["kind"]["enum"];
        assert_eq!(kinds.as_array().unwrap().len(), TestKind::ALL.len());
    }

    #[test]
    fn art_from_pixels_or_an_image() {
        let pixels = json!({
            "pixels": { "palette": { "R": "#ff0000" }, "frames": [["R.", ".R"], [".R", "R."]] },
            "frame_ms": 80, "placement": "intro"
        });
        let art = led_art(&pixels).unwrap();
        assert_eq!((art.size(), art.frames.len(), art.frame_ms, art.placement), ((2, 2), 2, 80, ArtPlacement::Intro));
        assert!(led_art(&json!({})).unwrap_err().contains("one of them"));
        assert!(led_art(&json!({ "image_base64": "!!" })).unwrap_err().contains("base64"));
        assert!(led_art(&json!({ "pixels": pixels["pixels"], "placement": "sideways" })).is_err());
        let png = base64::engine::general_purpose::STANDARD.encode(b"not a png");
        assert!(led_art(&json!({ "image_base64": format!("data:image/png;base64,{png}") })).is_err());
    }
}
