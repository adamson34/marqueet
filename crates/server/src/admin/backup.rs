//! "Backup" on the admin page: download the settings and team art as one
//! file, or restore one (see `crate::backup`).

use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::Utc;

use super::deny;
use super::multipart;
use super::teams::{check_post, failed, form_fields};
use crate::backup::{self, MAX_BACKUP_BYTES};
use crate::web::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/backup.json", get(download))
        .route("/admin/backup/restore", post(restore).layer(DefaultBodyLimit::max(MAX_BACKUP_BYTES + 1024 * 1024)))
}

/// `GET /admin/backup.json`: the settings and team art, as a download.
async fn download(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = deny(state.auth.check(peer.ip(), &headers)) {
        return denied;
    }
    let hub = state.hub.clone();
    // Encoding every logo and animation as PNG takes a moment.
    let json = tokio::task::spawn_blocking(move || {
        serde_json::to_string_pretty(&backup::make(hub.settings(), &hub.team_art())).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let name = format!("marqueet-backup-{}.json", Utc::now().format("%Y-%m-%d"));
    let headers = [
        (CONTENT_TYPE, "application/json".to_owned()),
        (CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        (CACHE_CONTROL, "no-store".to_owned()),
    ];
    (headers, json).into_response()
}

/// `POST /admin/backup/restore`: a backup file, uploaded.
async fn restore(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = check_post(&state, peer, &headers) {
        return denied;
    }
    let (hub, request_headers) = (state.hub.clone(), headers.clone());
    let result = tokio::task::spawn_blocking(move || {
        let fields = form_fields(&request_headers, &body)?;
        let file =
            multipart::get(&fields, "backup").filter(|f| !f.data.is_empty()).ok_or("choose a backup file first")?;
        backup::restore(&hub, backup::read(&file.data)?)
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match result {
        Ok(()) => (StatusCode::SEE_OTHER, [(LOCATION, "/admin?restored")]).into_response(),
        Err(e) => failed(&state, peer, &headers, e),
    }
}
