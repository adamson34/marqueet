//! ESPN scoreboard provider.
//!
//! ESPN's `site.api.espn.com` scoreboard endpoints are **unofficial and
//! undocumented**; they can change or disappear. This crate keeps the blast
//! radius small: lenient models ([`model`]), a pure normalizer tested against
//! saved responses ([`normalize`]), and a thin HTTP client.
//!
//! Requests identify themselves honestly (`marqueet/<version> (+repo url)`).
//! Spoofing a browser user agent gets blocked by ESPN's CDN anyway.

pub mod leagues;
mod model;
pub mod normalize;

use std::time::Duration;

use chrono::Utc;
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::sports::LeagueId;

pub const DEFAULT_BASE_URL: &str = "https://site.api.espn.com/apis/site/v2/sports";
pub const USER_AGENT: &str =
    concat!("marqueet/", env!("CARGO_PKG_VERSION"), " (+https://github.com/adamson34/marqueet)");

#[derive(Debug, Clone)]
pub struct EspnProvider {
    client: reqwest::Client,
    base_url: String,
}

impl EspnProvider {
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    /// Points the provider at another server (tests, mirrors).
    pub fn with_base_url(base_url: &str) -> Result<Self, ProviderError> {
        // rustls needs a process-wide crypto provider; ignore "already installed".
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        Ok(Self { client, base_url: base_url.trim_end_matches('/').to_owned() })
    }

    async fn fetch(&self, league: &LeagueId) -> Result<Scoreboard, ProviderError> {
        let def = leagues::find(league.as_str()).ok_or_else(|| ProviderError::UnknownLeague(league.to_string()))?;
        let url = format!("{}/{}/scoreboard", self.base_url, def.path);
        let resp = self.client.get(&url).send().await.map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ProviderError::Status(status.as_u16()));
        }
        let body = resp.text().await.map_err(|e| ProviderError::Http(e.to_string()))?;
        let board = normalize::normalize(&body, def, Utc::now())?;
        for note in &board.skipped {
            log::warn!("skipped malformed ESPN entry: {note}");
        }
        Ok(board)
    }
}

impl DataProvider for EspnProvider {
    fn id(&self) -> &'static str {
        normalize::PROVIDER
    }

    fn leagues(&self) -> Vec<LeagueInfo> {
        leagues::infos()
    }

    fn scoreboard<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(self.fetch(league))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_is_honest() {
        assert!(USER_AGENT.starts_with("marqueet/"));
        assert!(!USER_AGENT.contains("Mozilla"));
    }

    #[tokio::test]
    async fn unknown_league_is_an_error_without_a_request() {
        let p = EspnProvider::with_base_url("http://127.0.0.1:9").unwrap();
        let err = p.scoreboard(&LeagueId::new("curling")).await.unwrap_err();
        assert_eq!(err, ProviderError::UnknownLeague("curling".into()));
    }

    /// Hits the real ESPN API; run with `cargo test -p marqueet-provider-espn -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_scoreboards_parse() {
        let p = EspnProvider::new().unwrap();
        for league in ["nfl", "mlb", "nhl", "epl"] {
            let board = p.scoreboard(&LeagueId::new(league)).await.unwrap();
            assert!(board.skipped.is_empty(), "{league}: {:?}", board.skipped);
            println!("{league}: {} games", board.games.len());
        }
    }
}
