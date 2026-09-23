//! HTTP and WebSocket routes.
//!
//! - `GET /ws`: display feed (see `marqueet_core::protocol`).
//! - `GET /api/games`: current games and per-league fetch health, as JSON.
//! - `GET /api/alerts`: the most recent alerts, newest first.
//! - `GET /healthz`: liveness probe.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use marqueet_core::protocol::{PROTOCOL_VERSION, ServerMsg};
use serde_json::json;

use crate::hub::Hub;

pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/api/games", get(api_games))
        .route("/api/alerts", get(api_alerts))
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
    let mut alerts = hub.subscribe_alerts();
    let first = rx.borrow_and_update().clone();
    if !send(&mut socket, &hello).await || !send(&mut socket, &ServerMsg::Content((*first).clone())).await {
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
