//! The data-provider plugin interface.
//!
//! A provider fetches scoreboards from one upstream (ESPN first) and returns
//! games in the normalized [`crate::sports`] schema. Polling, caching and
//! backoff are the server's job, so providers stay small: fetch + normalize.
//!
//! The trait uses boxed futures so providers can be stored as
//! `Box<dyn DataProvider>` without pulling an async runtime into this crate.

use std::future::Future;
use std::pin::Pin;

use crate::sports::standings::Standings;
use crate::sports::{Game, LeagueId, Sport};
use crate::weather::{Place, Units, Weather, WeatherAlert};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A league a provider can serve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeagueInfo {
    pub id: LeagueId,
    pub sport: Sport,
    /// Human-readable name, e.g. "NFL", "Premier League".
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("league {0:?} is not supported by this provider")]
    UnknownLeague(String),
    #[error("request failed: {0}")]
    Http(String),
    #[error("upstream returned HTTP {0}")]
    Status(u16),
    #[error("could not parse response: {0}")]
    Parse(String),
    #[error("{0} is not available from this provider")]
    Unsupported(String),
}

/// A scoreboard fetch: the games that parsed, plus a note for each upstream
/// entry that was skipped because it was malformed. One bad game never hides
/// the rest of the league.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scoreboard {
    pub games: Vec<Game>,
    pub skipped: Vec<String>,
}

pub trait DataProvider: Send + Sync {
    /// Short id used in game ids, e.g. `espn`.
    fn id(&self) -> &'static str;

    fn leagues(&self) -> Vec<LeagueInfo>;

    /// Today's scoreboard for `league`.
    fn scoreboard<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>>;

    /// Current standings for `league`, if the provider has them.
    fn standings<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Standings, ProviderError>> {
        Box::pin(async move { Err(ProviderError::Unsupported(format!("{league} standings"))) })
    }
}

/// A source of weather forecasts and place search.
pub trait WeatherProvider: Send + Sync {
    /// Current conditions and a few days of forecast at `place`.
    fn forecast<'a>(&'a self, place: &'a Place, units: Units) -> BoxFuture<'a, Result<Weather, ProviderError>>;

    /// Places matching a search like "Kansas City", best first.
    fn search<'a>(&'a self, query: &'a str) -> BoxFuture<'a, Result<Vec<Place>, ProviderError>>;
}

/// A source of official weather alerts for a place.
pub trait WeatherAlertsProvider: Send + Sync {
    /// Alerts in effect at `place`. [`ProviderError::Unsupported`] when the
    /// place is outside the provider's coverage.
    fn active<'a>(&'a self, place: &'a Place) -> BoxFuture<'a, Result<Vec<WeatherAlert>, ProviderError>>;
}
