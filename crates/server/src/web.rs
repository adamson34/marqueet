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
use marqueet_core::protocol::{DisplayState, PROTOCOL_VERSION, ServerMsg, SetupInfo};
use marqueet_core::settings::Settings;
use serde_json::json;
use tokio::sync::watch;

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

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    // Only a display on the device itself may see the first-boot code.
    let setup = crate::admin::auth::is_local(peer.ip()).then(|| state.auth.subscribe_setup());
    ws.on_upgrade(move |socket| display_client(socket, state.hub, setup))
}

/// The display settings message, with the first-boot screen when set up is
/// pending and the display is local.
fn display_msg(look: &DisplayState, setup: Option<&watch::Receiver<Option<SetupInfo>>>) -> ServerMsg {
    let mut state = look.clone();
    state.setup = setup.and_then(|s| s.borrow().clone());
    ServerMsg::Display(Box::new(state))
}

async fn send(socket: &mut WebSocket, msg: &ServerMsg) -> bool {
    socket.send(Message::Text(msg.to_json().into())).await.is_ok()
}

/// Sends Hello and the current content, then every change until the client
/// goes away.
async fn display_client(mut socket: WebSocket, hub: Arc<Hub>, mut setup: Option<watch::Receiver<Option<SetupInfo>>>) {
    log::info!("display connected");
    let hello = ServerMsg::Hello { protocol: PROTOCOL_VERSION, server_version: env!("CARGO_PKG_VERSION").into() };
    let mut rx = hub.subscribe();
    let mut display = hub.subscribe_display();
    let mut alerts = hub.subscribe_alerts();
    let first = rx.borrow_and_update().clone();
    let look = display.borrow_and_update().clone();
    if !send(&mut socket, &hello).await
        || !send(&mut socket, &display_msg(&look, setup.as_ref())).await
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
                if !send(&mut socket, &display_msg(&look, setup.as_ref())).await {
                    break;
                }
            }
            changed = async { match setup.as_mut() { Some(s) => s.changed().await, None => std::future::pending().await } } => {
                if changed.is_err() {
                    setup = None;
                    continue;
                }
                let look = display.borrow().clone();
                if !send(&mut socket, &display_msg(&look, setup.as_ref())).await {
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
        Access::Setup => Some((StatusCode::FORBIDDEN, "finish setup first: open /setup")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_displays_get_the_setup_code() {
        let info = SetupInfo { code: "123456".into(), urls: vec![] };
        let (_tx, rx) = watch::channel(Some(info.clone()));
        let look = DisplayState {
            config: Default::default(),
            screen_off: false,
            utc_offset: None,
            setup: Some(info.clone()), // never trusted from the hub
        };
        let ServerMsg::Display(local) = display_msg(&look, Some(&rx)) else { panic!() };
        assert_eq!(local.setup, Some(info));
        let ServerMsg::Display(remote) = display_msg(&look, None) else { panic!() };
        assert_eq!(remote.setup, None);
    }
}
