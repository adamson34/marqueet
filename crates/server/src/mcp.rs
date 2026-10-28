//! MCP, the Model Context Protocol, so assistants such as Claude can run the
//! sign: `POST /mcp` speaks JSON-RPC 2.0 over MCP's Streamable HTTP transport
//! and offers the [`crate::tools`] table (ADR-0017).
//!
//! Stateless and small: no sessions and no server-sent events, so every
//! request gets one JSON answer; notifications get `202 Accepted`. Written
//! by hand on `serde_json`, so it needs no new crates. Access is the same
//! as the tools': an API key as `Authorization: Bearer <key>`.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::header::ALLOW;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::post;
use serde_json::{Value, json};

use crate::tools;
use crate::web::AppState;

/// Protocol versions spoken, newest first.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// What the assistant is told about this server when it connects.
const INSTRUCTIONS: &str = "This is a Marqueet: a live LED-style sports ticker on a monitor, with \
widgets below and full-screen takeovers when a team scores. Call status first to see what's on \
screen, get_settings before update_settings, and list_teams for team ids. To give a team its own \
celebration, use set_team_art (colors, words and LED art drawn as text pixels or a PNG/GIF), then \
test_takeover with that team so the owner can see it. Only use team art the owner may use.";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/mcp", post(handle).get(only_post).delete(only_post))
        .layer(DefaultBodyLimit::max(tools::MAX_BODY))
}

/// No server-sent event stream and no sessions to end.
async fn only_post() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, "POST")]).into_response()
}

async fn handle(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(res) = tools::denied(&state, peer, &headers) {
        return res;
    }
    let message = match serde_json::from_slice::<Value>(&body) {
        Ok(m) => m,
        Err(e) => return Json(error(Value::Null, -32700, &format!("parse error: {e}"))).into_response(),
    };
    match respond(&state, message).await {
        Some(answer) => Json(answer).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The protocol version to use: the client's if spoken here, else ours.
pub fn negotiate(requested: Option<&str>) -> &'static str {
    requested.and_then(|r| PROTOCOL_VERSIONS.iter().find(|v| **v == r)).copied().unwrap_or(PROTOCOL_VERSIONS[0])
}

/// The answer to one JSON-RPC message; `None` for a notification or a
/// response from the client, which need none.
pub async fn respond(state: &AppState, message: Value) -> Option<Value> {
    let Some(m) = message.as_object() else {
        return Some(error(Value::Null, -32600, "expected one JSON-RPC message (an object)"));
    };
    let id = m.get("id").cloned();
    let Some(method) = m.get("method").and_then(Value::as_str) else {
        let is_response = m.contains_key("result") || m.contains_key("error");
        return (!is_response).then(|| error(id.unwrap_or(Value::Null), -32600, "no method"));
    };
    let id = id?;
    let params = m.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let version = negotiate(params.get("protocolVersion").and_then(Value::as_str));
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "marqueet", "title": "Marqueet", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools::descriptions() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "tools/call needs a name"));
            };
            if tools::find(name).is_none() {
                return Some(error(id, -32602, &format!("unknown tool {name:?}")));
            }
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            match tools::call(state, name, arguments).await {
                Ok(v) => {
                    let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                    let mut out = json!({ "content": [{ "type": "text", "text": text }], "isError": false });
                    if v.is_object() {
                        out["structuredContent"] = v;
                    }
                    result(id, out)
                }
                // A tool's failure is for the assistant to read and fix.
                Err(e) => result(id, json!({ "content": [{ "type": "text", "text": e }], "isError": true })),
            }
        }
        _ => error(id, -32601, &format!("method {method:?} isn't supported")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_the_clients_version_when_it_can() {
        assert_eq!(negotiate(Some("2025-03-26")), "2025-03-26");
        assert_eq!(negotiate(Some("1999-01-01")), PROTOCOL_VERSIONS[0]);
        assert_eq!(negotiate(None), PROTOCOL_VERSIONS[0]);
    }
}
