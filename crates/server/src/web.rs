//! HTTP and WebSocket routes.
//!
//! - `GET /ws`: display feed (see `marqueet_core::protocol`).
//! - `GET /api/games`: current games and per-league fetch health, as JSON.
//! - `GET /api/alerts`: the most recent alerts, newest first.
//! - `GET /api/settings`, `PUT /api/settings`: current settings as JSON.
//!   Changing settings is only allowed from the device itself until the admin
//!   page adds a password.
//! - `GET /healthz`: liveness probe.

use std::sync::Arc;

use std::net::SocketAddr;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use marqueet_core::protocol::{PROTOCOL_VERSION, ServerMsg};
use marqueet_core::settings::Settings;
use serde_json::json;

use crate::hub::Hub;

pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/api/games", get(api_games))
        .route("/api/alerts", get(api_alerts))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(hub)
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

async fn get_settings(State(hub): State<Arc<Hub>>) -> Json<Settings> {
    Json(hub.settings())
}

async fn put_settings(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(settings): Json<Settings>,
) -> Response {
    if !peer.ip().is_loopback() {
        let body =
            json!({ "error": "settings can only be changed from the device until the admin page has a password" });
        return (StatusCode::FORBIDDEN, Json(body)).into_response();
    }
    match hub.apply_settings(settings) {
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
