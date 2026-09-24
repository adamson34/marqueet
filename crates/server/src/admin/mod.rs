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
use marqueet_core::fantasy::points;
use page::{FantasyRow, FeedRow, LeagueHealth, Notice, TeamChoice};

use crate::hub::FantasySearch;
use crate::store::FantasyFeed;

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
        .route("/admin/fantasy/find", axum::routing::post(find_fantasy))
        .route("/admin/fantasy/add", axum::routing::post(add_fantasy))
        .route("/admin/fantasy/remove", axum::routing::post(remove_fantasy))
        .route("/admin/feeds", axum::routing::post(create_feed))
        .route("/admin/feeds/revoke", axum::routing::post(revoke_feed))
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

/// The Host header, for examples that point back at this server.
fn host(headers: &HeaderMap) -> &str {
    headers.get(axum::http::header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("marqueet.local:7878")
}

fn pairs(body: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(body).into_owned().collect()
}

fn render(hub: &Hub, notice: Notice, remote: bool, host: &str) -> String {
    render_with(hub, notice, remote, host, None)
}

fn render_with(hub: &Hub, notice: Notice, remote: bool, host: &str, search: Option<&FantasySearch>) -> String {
    let status = hub.feeds();
    let feeds: Vec<FeedRow> = hub
        .feed_tokens()
        .into_iter()
        .map(|(name, token)| {
            let info = status.iter().find(|f| f.name == name);
            FeedRow {
                segments: info.map_or(0, |i| i.segments),
                expires_at: info.and_then(|i| i.expires_at),
                name,
                token,
            }
        })
        .collect();
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
    let fantasy: Vec<FantasyRow> = settings
        .fantasy
        .iter()
        .map(|f| {
            let feed = hub.with_store(|s| s.fantasy(&(f.league_id.clone(), f.roster_id)).cloned());
            let status = match feed {
                Some(FantasyFeed { matchup: Some(m), .. }) => match &m.opponent {
                    Some(o) => format!("Week {}: {} to {}", m.week, points(m.me.points), points(o.points)),
                    None => format!("Week {}: {} (bye)", m.week, points(m.me.points)),
                },
                Some(FantasyFeed { last_error: Some(e), .. }) => format!("Couldn't load: {e}"),
                _ => "Loading…".into(),
            };
            FantasyRow {
                league_id: f.league_id.clone(),
                roster_id: f.roster_id,
                league: f.league.clone(),
                team: f.team.clone(),
                status,
            }
        })
        .collect();
    let now = Utc::now();
    page::render(&page::View {
        settings: &settings,
        leagues: &leagues,
        teams: &teams,
        health: &health,
        alerts: &alerts,
        zones: &tz::names(),
        feeds: &feeds,
        fantasy: &fantasy,
        fantasy_search: search,
        host,
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
    html(StatusCode::OK, render(&state.hub, notice, !auth::is_local(peer.ip()), host(&headers)))
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
    let current = hub.settings();
    let mut supported: Vec<LeagueId> = hub.supported_leagues().into_iter().map(|l| l.id).collect();
    if supported.is_empty() {
        supported.clone_from(&current.leagues);
    }
    let pairs = pairs(&body);
    let result = async {
        let mut settings = form::apply(&current, &supported, &pairs)?;
        match form::location(&pairs, current.weather.place.as_ref()) {
            form::LocationChange::Keep => {}
            form::LocationChange::Clear => settings.weather.place = None,
            form::LocationChange::Set(place) => settings.weather.place = Some(place),
            form::LocationChange::Lookup(query) => {
                let found = hub.search_places(&query).await?;
                let place =
                    found.into_iter().next().ok_or_else(|| format!("couldn't find a place called {query:?}"))?;
                settings.weather.place = Some(place);
            }
        }
        hub.apply_settings(settings)
    }
    .await;
    match result {
        Ok(_) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved")]).into_response(),
        Err(e) => {
            html(StatusCode::BAD_REQUEST, render(hub, Notice::Error(e), !auth::is_local(peer.ip()), host(&headers)))
        }
    }
}

/// Shared checks for the admin forms that act immediately.
fn admin_form(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    if !auth::same_origin(headers) {
        return Some(html(StatusCode::FORBIDDEN, "cross-site form post refused".into()));
    }
    deny(state.auth.check(peer.ip(), headers))
}

fn field(body: &[u8], key: &str) -> String {
    pairs(body).into_iter().find(|(k, _)| k == key).map(|(_, v)| v.trim().to_owned()).unwrap_or_default()
}

async fn find_fantasy(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let remote = !auth::is_local(peer.ip());
    match state.hub.find_fantasy(&field(&body, "username")).await {
        Ok(search) => {
            html(StatusCode::OK, render_with(&state.hub, Notice::None, remote, host(&headers), Some(&search)))
        }
        Err(e) => html(StatusCode::BAD_REQUEST, render(&state.hub, Notice::Error(e), remote, host(&headers))),
    }
}

async fn add_fantasy(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let roster: u32 = field(&body, "roster_id").parse().unwrap_or(0);
    match state.hub.add_fantasy(&field(&body, "league_id"), &field(&body, "league"), roster).await {
        Ok(()) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved#fantasy")]).into_response(),
        Err(e) => html(
            StatusCode::BAD_REQUEST,
            render(&state.hub, Notice::Error(e), !auth::is_local(peer.ip()), host(&headers)),
        ),
    }
}

async fn remove_fantasy(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let roster: u32 = field(&body, "roster_id").parse().unwrap_or(0);
    match state.hub.remove_fantasy(&field(&body, "league_id"), roster) {
        Ok(()) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved#fantasy")]).into_response(),
        Err(e) => html(
            StatusCode::BAD_REQUEST,
            render(&state.hub, Notice::Error(e), !auth::is_local(peer.ip()), host(&headers)),
        ),
    }
}

async fn create_feed(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    match state.hub.create_feed(&field(&body, "name")) {
        Ok(_) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved#feeds")]).into_response(),
        Err(e) => html(
            StatusCode::BAD_REQUEST,
            render(&state.hub, Notice::Error(e), !auth::is_local(peer.ip()), host(&headers)),
        ),
    }
}

async fn revoke_feed(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    match state.hub.revoke_feed(&field(&body, "name")) {
        Ok(()) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved#feeds")]).into_response(),
        Err(e) => html(
            StatusCode::BAD_REQUEST,
            render(&state.hub, Notice::Error(e), !auth::is_local(peer.ip()), host(&headers)),
        ),
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
