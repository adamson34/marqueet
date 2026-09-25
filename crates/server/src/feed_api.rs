//! The feed API: local programs put content on the sign.
//!
//! - `POST /api/feeds/<name>`: replace the feed's ticker segments and crawl
//!   lines (see `marqueet_core::feeds::FeedPost`).
//! - `POST /api/feeds/<name>/alert`: flash the feed's segment, or take over.
//! - `DELETE /api/feeds/<name>`: clear the feed's content.
//! - `GET /api/feeds`: list feeds (admin access, no tokens).
//!
//! Each feed has its own token, created on the admin page and sent as
//! `Authorization: Bearer <token>`, from any address. A wrong token and an
//! unknown feed get the same answer, so names can't be probed. Bodies are
//! limited to 64 KB. See `docs/FEEDS.md`.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use marqueet_core::feeds::{AlertPost, FeedPost};
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::admin::auth::Access;
use crate::hub::FeedError;
use crate::web::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/feeds", get(list))
        .route("/api/feeds/{name}", post(set).delete(clear))
        .route("/api/feeds/{name}/alert", post(alert))
        .layer(DefaultBodyLimit::max(64 * 1024))
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

/// The rejection for a request without `name`'s token, if it lacks it.
fn unauthorized(state: &AppState, name: &str, headers: &HeaderMap) -> Option<Response> {
    let token = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if token.is_some_and(|t| state.hub.feed_authorized(name, t.trim())) {
        return None;
    }
    let mut res = error(StatusCode::UNAUTHORIZED, "unknown feed or wrong token");
    res.headers_mut().insert("www-authenticate", HeaderValue::from_static("Bearer"));
    Some(res)
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|e| format!("invalid JSON: {e}"))
}

async fn set(State(state): State<AppState>, Path(name): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(res) = unauthorized(&state, &name, &headers) {
        return res;
    }
    let post: FeedPost = match parse(&body) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::BAD_REQUEST, e),
    };
    match state.hub.post_feed(&name, &post) {
        Ok(info) => Json(info).into_response(),
        Err(e) => feed_error(e),
    }
}

fn feed_error(e: FeedError) -> Response {
    match e {
        FeedError::Invalid(e) => error(StatusCode::BAD_REQUEST, e),
        FeedError::TooSoon(secs) => {
            let mut res = error(StatusCode::TOO_MANY_REQUESTS, format!("too soon; try again in {secs} s"));
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                res.headers_mut().insert("retry-after", v);
            }
            res
        }
    }
}

async fn clear(State(state): State<AppState>, Path(name): Path<String>, headers: HeaderMap) -> Response {
    if let Some(res) = unauthorized(&state, &name, &headers) {
        return res;
    }
    state.hub.clear_feed(&name);
    StatusCode::NO_CONTENT.into_response()
}

async fn alert(State(state): State<AppState>, Path(name): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(res) = unauthorized(&state, &name, &headers) {
        return res;
    }
    let post: AlertPost = match parse(&body) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::BAD_REQUEST, e),
    };
    match state.hub.feed_alert(&name, &post) {
        Ok(a) => Json(json!({ "id": a.id, "level": a.level, "segment": a.segment_id })).into_response(),
        Err(e) => feed_error(e),
    }
}

async fn list(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match state.auth.check(peer.ip(), &headers) {
        Access::Granted => Json(state.hub.feeds()).into_response(),
        _ => error(StatusCode::FORBIDDEN, "feeds are listed on the admin page"),
    }
}
