//! HTTP and WebSocket routes.
//!
//! - `GET /ws`: display feed (see `marqueet_core::protocol`).
//! - `GET /api/games`: current games and per-league fetch health, as JSON.
//! - `GET /api/alerts`: the most recent alerts, newest first.
//! - `GET /api/settings`, `PUT /api/settings`: current settings as JSON.
//!   Reading and changing them follows the admin page's rules (see
//!   [`crate::admin::auth`]).
//! - `/api/feeds...`: content from local programs (see [`crate::feed_api`]).
//! - `GET /healthz`: liveness probe.
//! - `/`, `/admin`, `/login`, `/logout`: the admin page (see [`crate::admin`]).

use std::sync::Arc;

use std::net::SocketAddr;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, FromRef, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use marqueet_core::protocol::{PROTOCOL_VERSION, ServerMsg};
use marqueet_core::settings::Settings;
use serde_json::json;

use crate::admin::auth::{Access, Auth};
use crate::hub::Hub;
use crate::{admin, feed_api};

#[derive(Clone, Debug)]
pub struct AppState {
    pub hub: Arc<Hub>,
    pub auth: Arc<Auth>,
}

impl FromRef<AppState> for Arc<Hub> {
    fn from_ref(state: &AppState) -> Arc<Hub> {
        Arc::clone(&state.hub)
    }
}

pub fn router(hub: Arc<Hub>, auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/api/games", get(api_games))
        .route("/api/alerts", get(api_alerts))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/healthz", get(|| async { "ok" }))
        .merge(admin::routes())
        .merge(feed_api::routes())
        .with_state(AppState { hub, auth })
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(hub): State<Arc<Hub>>) -> Response {
    ws.on_upgrade(move |socket| display_client(socket, hub))
}

async fn send(socket: &mut WebSocket, msg: &ServerMsg) -> bool {
    socket.send(Message::Text(msg.to_json().into())).await.is_ok()
}

/// Sends Hello and the current content, then every change until the client
/// goes away.
async fn display_client(mut socket: WebSocket, hub: Arc<Hub>) {
    log::info!("display connected");
    let hello = ServerMsg::Hello { protocol: PROTOCOL_VERSION, server_version: env!("CARGO_PKG_VERSION").into() };
    let mut rx = hub.subscribe();
    let mut display = hub.subscribe_display();
    let mut alerts = hub.subscribe_alerts();
    let first = rx.borrow_and_update().clone();
    let look = display.borrow_and_update().clone();
    if !send(&mut socket, &hello).await
        || !send(&mut socket, &ServerMsg::Display(Box::new((*look).clone()))).await
        || !send(&mut socket, &ServerMsg::Content((*first).clone())).await
    {
        return;
    }
    loop {
        tokio::select! {
            // Content before alerts, so a flash lands on the updated score.
            biased;
            changed = rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let content = rx.borrow_and_update().clone();
                if !send(&mut socket, &ServerMsg::Content((*content).clone())).await {
                    break;
                }
            }
            changed = display.changed() => {
                if changed.is_err() {
                    break;
                }
                let look = display.borrow_and_update().clone();
                if !send(&mut socket, &ServerMsg::Display(Box::new((*look).clone()))).await {
                    break;
                }
            }
            alert = alerts.recv() => match alert {
                Ok(alert) => {
                    if !send(&mut socket, &ServerMsg::Alert(Box::new((*alert).clone()))).await {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => log::warn!("display fell behind; dropped {n} alerts"),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            },
        }
    }
    log::info!("display disconnected");
}

/// Settings include the weather location, so reading them follows the
/// admin page's rules too.
async fn get_settings(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match state.auth.check(peer.ip(), &headers) {
        Access::Granted => Json(state.hub.settings()).into_response(),
        _ => (StatusCode::FORBIDDEN, Json(json!({ "error": "settings are on the admin page" }))).into_response(),
    }
}

async fn put_settings(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(settings): Json<Settings>,
) -> Response {
    let error = match state.auth.check(peer.ip(), &headers) {
        Access::Granted => None,
        Access::NeedsLogin => Some((StatusCode::UNAUTHORIZED, "log in on the admin page first")),
        Access::Forbidden => Some((
            StatusCode::FORBIDDEN,
            "settings can only be changed from the device unless the server has an admin password",
        )),
    };
    if let Some((status, error)) = error {
        return (status, Json(json!({ "error": error }))).into_response();
    }
    match state.hub.apply_settings(settings) {
        Ok(saved) => Json(saved).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn api_alerts(State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    let alerts: Vec<_> = hub.recent_alerts().iter().map(|a| (**a).clone()).collect();
    Json(alerts)
}

async fn api_games(State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    let body = hub.with_store(|store| {
        let leagues: Vec<_> = store
            .leagues()
            .iter()
            .map(|id| {
                let feed = store.feed(id).cloned().unwrap_or_default();
                json!({
                    "id": id,
                    "stale": store.is_stale(id),
                    "failures": feed.failures,
                    "last_error": feed.last_error,
                    "last_success": feed.last_success,
                    "games": feed.games,
                })
            })
            .collect();
        json!({ "status": store.status(), "leagues": leagues })
    });
    Json(body)
}
