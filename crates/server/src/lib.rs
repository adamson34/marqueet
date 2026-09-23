//! Marqueet server: polls data providers on an adaptive schedule, keeps the
//! last good data when an upstream fails, and pushes ready-to-render ticker
//! content to the display over a local WebSocket.
//!
//! - [`schedule`]: pure polling policy (fast when live, slow when idle, backoff).
//! - [`store`]: pure per-league cache with failure tracking and staleness.
//! - [`content`]: pure games → ticker/crawl segments.
//! - [`hub`]: shared state plus one polling task per league.
//! - [`web`]: `/ws`, `/api/games`, `/healthz`.

pub mod content;
pub mod hub;
pub mod schedule;
pub mod store;
pub mod web;

use std::future::Future;
use std::sync::Arc;

use marqueet_core::provider::DataProvider;
use marqueet_core::sports::LeagueId;
use tokio::net::TcpListener;

pub use hub::Hub;
pub use schedule::Policy;

/// Starts pollers for `leagues` and serves until `shutdown` resolves.
pub async fn run(
    listener: TcpListener,
    provider: Arc<dyn DataProvider>,
    leagues: Vec<LeagueId>,
    policy: Policy,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let hub = Hub::new(leagues, policy);
    let pollers = hub.spawn_pollers(provider);
    let result = axum::serve(listener, web::router(hub)).with_graceful_shutdown(shutdown).await;
    for p in pollers {
        p.abort();
    }
    result
}
