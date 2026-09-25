//! Sleeper fantasy football (`api.sleeper.app`): a public, read-only API
//! with no key. Only league data the user points us at is read.
//!
//! Sleeper asks apps to fetch the ~15 MB player list at most once a day, so
//! a compact copy (name, position, team, ESPN id) is cached in memory and,
//! with [`Sleeper::with_cache_file`], on disk across restarts. The current
//! NFL week is cached for an hour. Parsing is pure ([`parse`]) and tested
//! against saved responses.

pub mod parse;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use marqueet_core::fantasy::{FantasyLeagueInfo, FantasyTeamInfo, FantasyUser, Matchup};
use marqueet_core::provider::{BoxFuture, FantasyProvider, ProviderError};
use serde::{Deserialize, Serialize};

use parse::{LeagueData, NflState, PlayerInfo};

pub const BASE_URL: &str = "https://api.sleeper.app/v1";
pub const USER_AGENT: &str =
    concat!("marqueet/", env!("CARGO_PKG_VERSION"), " (+https://github.com/adamson34/marqueet)");

const PLAYERS_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::hours(24);
const STATE_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::hours(1);

type Players = Arc<HashMap<String, PlayerInfo>>;

/// The on-disk player cache.
#[derive(Serialize, Deserialize)]
struct PlayersCache {
    fetched_at: DateTime<Utc>,
    players: HashMap<String, PlayerInfo>,
}

/// Usernames are letters, digits and underscores.
fn valid_username(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Sleeper ids are numeric.
fn valid_id(s: &str) -> bool {
    (1..=24).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

fn check_id(s: &str) -> Result<(), ProviderError> {
    if valid_id(s) { Ok(()) } else { Err(ProviderError::NotFound(format!("Sleeper id {s:?}"))) }
}

#[derive(Debug)]
pub struct Sleeper {
    client: reqwest::Client,
    base_url: String,
    cache_file: Option<PathBuf>,
    /// Async so only one request downloads the player list.
    players: tokio::sync::Mutex<Option<(DateTime<Utc>, Players)>>,
    state: Mutex<Option<(DateTime<Utc>, NflState)>>,
}

impl Sleeper {
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_base_url(BASE_URL)
    }

    pub fn with_base_url(base_url: &str) -> Result<Self, ProviderError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            cache_file: None,
            players: tokio::sync::Mutex::new(None),
            state: Mutex::new(None),
        })
    }

    /// Keeps the compact player list in `path` across restarts.
    pub fn with_cache_file(mut self, path: PathBuf) -> Self {
        self.cache_file = Some(path);
        self
    }

    async fn get(&self, path: &str) -> Result<String, ProviderError> {
        let resp = self
            .client
            .get(format!("{}{path}", self.base_url))
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ProviderError::Status(status.as_u16()));
        }
        resp.text().await.map_err(|e| ProviderError::Http(e.to_string()))
    }

    async fn state(&self) -> Result<NflState, ProviderError> {
        let now = Utc::now();
        if let Some((t, s)) = self.state.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
            && now - *t < STATE_MAX_AGE
        {
            return Ok(s.clone());
        }
        let state = parse::state(&self.get("/state/nfl").await?)?;
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = Some((now, state.clone()));
        Ok(state)
    }

    fn read_cache(&self) -> Option<PlayersCache> {
        let bytes = std::fs::read(self.cache_file.as_ref()?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_cache(&self, cache: &PlayersCache) {
        let Some(path) = &self.cache_file else { return };
        let written = serde_json::to_vec(cache).map_err(|e| e.to_string()).and_then(|bytes| {
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            log::warn!("couldn't save the Sleeper player cache to {}: {e}", path.display());
        }
    }

    /// The player list: from memory, the disk cache, or Sleeper (at most
    /// daily). A stale copy is used if a refresh fails.
    async fn players(&self) -> Result<Players, ProviderError> {
        let now = Utc::now();
        let mut guard = self.players.lock().await;
        if guard.is_none()
            && let Some(cache) = self.read_cache()
        {
            *guard = Some((cache.fetched_at, Arc::new(cache.players)));
        }
        if let Some((t, p)) = guard.as_ref()
            && now - *t < PLAYERS_MAX_AGE
        {
            return Ok(Arc::clone(p));
        }
        match self.get("/players/nfl").await.and_then(|body| parse::players(&body)) {
            Ok(players) => {
                log::info!("Sleeper: loaded {} players", players.len());
                let cache = PlayersCache { fetched_at: now, players };
                self.write_cache(&cache);
                let players = Arc::new(cache.players);
                *guard = Some((now, Arc::clone(&players)));
                Ok(players)
            }
            Err(e) => match guard.as_ref() {
                Some((_, p)) => {
                    log::warn!("Sleeper: player list refresh failed ({e}); using the cached one");
                    Ok(Arc::clone(p))
                }
                None => Err(e),
            },
        }
    }

    async fn fetch_user(&self, username: &str) -> Result<FantasyUser, ProviderError> {
        let name = username.trim();
        if !valid_username(name) {
            return Err(ProviderError::NotFound(format!("Sleeper user {name:?}")));
        }
        parse::user(&self.get(&format!("/user/{name}")).await?)?
            .ok_or_else(|| ProviderError::NotFound(format!("Sleeper user {name:?}")))
    }

    async fn fetch_leagues(&self, user_id: &str) -> Result<Vec<FantasyLeagueInfo>, ProviderError> {
        check_id(user_id)?;
        let state = self.state().await?;
        let season = if state.league_season.is_empty() { state.season } else { state.league_season };
        parse::leagues(&self.get(&format!("/user/{user_id}/leagues/nfl/{season}")).await?)
    }

    async fn fetch_teams(&self, league_id: &str) -> Result<Vec<FantasyTeamInfo>, ProviderError> {
        check_id(league_id)?;
        let rosters = self.get(&format!("/league/{league_id}/rosters")).await?;
        let users = self.get(&format!("/league/{league_id}/users")).await?;
        parse::teams(&rosters, &users)
    }

    async fn fetch_matchup(&self, league_id: &str, roster_id: u32) -> Result<Matchup, ProviderError> {
        check_id(league_id)?;
        let week = self.state().await?.display_week.max(1);
        let league = parse::league(&self.get(&format!("/league/{league_id}")).await?)?;
        let users = parse::users(&self.get(&format!("/league/{league_id}/users")).await?)?;
        let rosters = parse::rosters(&self.get(&format!("/league/{league_id}/rosters")).await?)?;
        let matchups = parse::matchups(&self.get(&format!("/league/{league_id}/matchups/{week}")).await?)?;
        let players = self.players().await?;
        let data =
            LeagueData { league: &league, users: &users, rosters: &rosters, matchups: &matchups, players: &players };
        parse::build_matchup(&data, roster_id, week, Utc::now())
    }
}

impl FantasyProvider for Sleeper {
    fn id(&self) -> &'static str {
        "sleeper"
    }

    fn find_user<'a>(&'a self, username: &'a str) -> BoxFuture<'a, Result<FantasyUser, ProviderError>> {
        Box::pin(self.fetch_user(username))
    }

    fn leagues<'a>(&'a self, user_id: &'a str) -> BoxFuture<'a, Result<Vec<FantasyLeagueInfo>, ProviderError>> {
        Box::pin(self.fetch_leagues(user_id))
    }

    fn teams<'a>(&'a self, league_id: &'a str) -> BoxFuture<'a, Result<Vec<FantasyTeamInfo>, ProviderError>> {
        Box::pin(self.fetch_teams(league_id))
    }

    fn matchup<'a>(&'a self, league_id: &'a str, roster_id: u32) -> BoxFuture<'a, Result<Matchup, ProviderError>> {
        Box::pin(self.fetch_matchup(league_id, roster_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parts_are_validated() {
        assert!(
            valid_username("Some_User1") && !valid_username("../etc") && !valid_username("a b") && !valid_username("")
        );
        assert!(valid_id("1257448602461032448") && !valid_id("12/34") && !valid_id("abc"));
    }

    #[tokio::test]
    async fn bad_names_never_reach_the_network() {
        let s = Sleeper::with_base_url("http://127.0.0.1:9").unwrap();
        assert!(matches!(s.find_user("../../x").await, Err(ProviderError::NotFound(_))));
        assert!(matches!(s.teams("1/../2").await, Err(ProviderError::NotFound(_))));
    }

    /// Hits the real API; run with `cargo test -p marqueet-provider-sleeper -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_lookup() {
        let s = Sleeper::new().unwrap();
        let user = s.find_user("sleeper").await.unwrap();
        println!("{user:?}");
        assert!(matches!(s.find_user("no_such_user_zz9q").await, Err(ProviderError::NotFound(_))));
        // Optionally a real matchup: SLEEPER_LEAGUE=<id> SLEEPER_ROSTER=<n>.
        if let (Ok(league), Ok(roster)) = (std::env::var("SLEEPER_LEAGUE"), std::env::var("SLEEPER_ROSTER")) {
            let cache = std::env::temp_dir().join("marqueet-sleeper-test-players.json");
            let s = s.with_cache_file(cache.clone());
            let t = std::time::Instant::now();
            let m = s.matchup(&league, roster.parse().unwrap()).await.unwrap();
            println!("week {} {:.1} vs {:?} in {:?}", m.week, m.me.points, m.opponent.map(|o| o.points), t.elapsed());
            println!("cache: {} KB", std::fs::metadata(&cache).unwrap().len() / 1024);
        }
    }
}
