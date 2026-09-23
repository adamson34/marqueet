//! The admin page: `GET/POST /admin`, `/login`, `/logout`.
//!
//! Server-rendered HTML and one small hand-written script (drag to reorder
//! leagues). No JS toolchain, no third-party front-end code, and the page
//! works with scripting off.

pub mod auth;
pub mod form;
pub mod page;

use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use chrono::Utc;
use marqueet_core::sports::LeagueId;

use crate::hub::Hub;
use crate::tz;
use crate::web::AppState;
use auth::Access;
use page::{LeagueHealth, Notice, TeamChoice};

const CSS: &str = include_str!("admin.css");
const JS: &str = include_str!("admin.js");
const CSP: &str = "default-src 'none'; style-src 'self'; script-src 'self'; img-src 'self'; \
                   form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(|| async { redirect("/admin") }))
        .route("/admin", get(show).post(save))
        .route("/admin/admin.css", get(|| async { asset("text/css; charset=utf-8", CSS) }))
        .route("/admin/admin.js", get(|| async { asset("text/javascript; charset=utf-8", JS) }))
        .route("/login", get(login_page).post(login))
        .route("/logout", axum::routing::post(logout))
}

fn asset(kind: &'static str, body: &'static str) -> Response {
    ([(CONTENT_TYPE, kind), (CACHE_CONTROL, "no-cache")], body).into_response()
}

fn redirect(to: &'static str) -> Response {
    (StatusCode::SEE_OTHER, [(LOCATION, to)]).into_response()
}

/// An HTML page with the admin security headers.
fn html(status: StatusCode, body: String) -> Response {
    let mut res = (status, body).into_response();
    let h = res.headers_mut();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert("content-security-policy", HeaderValue::from_static(CSP));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// The response for a request that may not see the admin page, if any.
fn deny(access: Access) -> Option<Response> {
    match access {
        Access::Granted => None,
        Access::NeedsLogin => Some(redirect("/login")),
        Access::Forbidden => Some(html(StatusCode::FORBIDDEN, page::forbidden())),
    }
}

fn pairs(body: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(body).into_owned().collect()
}

fn render(hub: &Hub, notice: Notice, remote: bool) -> String {
    let settings = hub.settings();
    let leagues = hub.supported_leagues();
    let (teams, health) = hub.with_store(|store| {
        let mut teams: Vec<TeamChoice> = Vec::new();
        for g in store.games() {
            for c in [&g.away, &g.home] {
                if !teams.iter().any(|t| t.id == c.team.id) {
                    teams.push(TeamChoice {
                        id: c.team.id.clone(),
                        league: g.league.clone(),
                        name: c.team.display_name.clone(),
                    });
                }
            }
        }
        teams.sort_by(|a, b| a.name.cmp(&b.name));
        let health: Vec<LeagueHealth> = store
            .leagues()
            .iter()
            .map(|id| {
                let feed = store.feed(id).cloned().unwrap_or_default();
                LeagueHealth {
                    id: id.clone(),
                    games: feed.games.len(),
                    stale: store.is_stale(id),
                    failures: feed.failures,
                    last_error: feed.last_error,
                    last_success: feed.last_success,
                }
            })
            .collect();
        (teams, health)
    });
    let alerts: Vec<_> = hub.recent_alerts().iter().map(|a| (**a).clone()).collect();
    let now = Utc::now();
    page::render(&page::View {
        settings: &settings,
        leagues: &leagues,
        teams: &teams,
        health: &health,
        alerts: &alerts,
        zones: &tz::names(),
        notice,
        remote,
        tz: tz::offset(&settings, now),
        now,
    })
}

async fn show(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(denied) = deny(state.auth.check(peer.ip(), &headers)) {
        return denied;
    }
    let notice = if uri.query() == Some("saved") { Notice::Saved } else { Notice::None };
    html(StatusCode::OK, render(&state.hub, notice, !auth::is_local(peer.ip())))
}

async fn save(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !auth::same_origin(&headers) {
        return html(StatusCode::FORBIDDEN, "cross-site form post refused".into());
    }
    if let Some(denied) = deny(state.auth.check(peer.ip(), &headers)) {
        return denied;
    }
    let hub = &state.hub;
    let mut supported: Vec<LeagueId> = hub.supported_leagues().into_iter().map(|l| l.id).collect();
    if supported.is_empty() {
        supported = hub.settings().leagues;
    }
    let result = form::apply(&hub.settings(), &supported, &pairs(&body)).and_then(|s| hub.apply_settings(s));
    match result {
        Ok(_) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved")]).into_response(),
        Err(e) => html(StatusCode::BAD_REQUEST, render(hub, Notice::Error(e), !auth::is_local(peer.ip()))),
    }
}

async fn login_page(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Response {
    if auth::is_local(peer.ip()) {
        return redirect("/admin");
    }
    if !state.auth.has_password() {
        return html(StatusCode::FORBIDDEN, page::forbidden());
    }
    html(StatusCode::OK, page::login(false))
}

async fn login(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if !auth::same_origin(&headers) {
        return html(StatusCode::FORBIDDEN, "cross-site form post refused".into());
    }
    if !state.auth.has_password() {
        return html(StatusCode::FORBIDDEN, page::forbidden());
    }
    let attempt = pairs(&body).into_iter().find(|(k, _)| k == "password").map(|(_, v)| v).unwrap_or_default();
    match state.auth.login(&attempt) {
        Some(token) => {
            let mut res = redirect("/admin");
            if let Ok(v) = HeaderValue::from_str(&auth::session_cookie(&token)) {
                res.headers_mut().insert(SET_COOKIE, v);
            }
            res
        }
        None => {
            // Slow down guessing.
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            html(StatusCode::UNAUTHORIZED, page::login(true))
        }
    }
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !auth::same_origin(&headers) {
        return html(StatusCode::FORBIDDEN, "cross-site form post refused".into());
    }
    state.auth.logout(&headers);
    let mut res = redirect("/login");
    if let Ok(v) = HeaderValue::from_str(&auth::clear_cookie()) {
        res.headers_mut().insert(SET_COOKIE, v);
    }
    res
}
