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

use crate::fantasy::{FantasyLeagueInfo, FantasyTeamInfo, FantasyUser, Matchup};
use crate::sports::standings::Standings;
use crate::sports::summary::GameSummary;
use crate::sports::{Game, GameId, LeagueId, Sport, TeamId};
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

/// A team in a league, for picking favorites.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TeamInfo {
    pub id: TeamId,
    pub abbreviation: String,
    /// "Buffalo Blizzard"
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
    #[error("{0} not found")]
    NotFound(String),
}

/// A scoreboard fetch: the games that parsed, plus a note for each upstream
/// entry that was skipped because it was malformed. One bad game never hides
/// the rest of the league.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scoreboard {
    /// The provider's own list: every game, or for college, the featured ones.
    pub games: Vec<Game>,
    pub skipped: Vec<String>,
    /// The rest of a big division (about 70 college football games on a
    /// Saturday). Too many for the ticker, so [`Scoreboard::games_for`] keeps
    /// only the ones someone follows or picked to watch.
    pub more: Vec<Game>,
}

impl Scoreboard {
    /// The games to show: the main list, plus any from [`Scoreboard::more`]
    /// with one of `favorites` playing or that is the `watched` game.
    pub fn games_for(self, favorites: &[TeamId], watched: Option<&GameId>) -> Vec<Game> {
        let mut games = self.games;
        for g in self.more {
            let wanted = watched == Some(&g.id) || [&g.home, &g.away].iter().any(|c| favorites.contains(&c.team.id));
            if wanted && !games.iter().any(|o| o.id == g.id) {
                games.push(g);
            }
        }
        games
    }
}

pub trait DataProvider: Send + Sync {
    /// Short id used in game ids, e.g. `espn`.
    fn id(&self) -> &'static str;

    fn leagues(&self) -> Vec<LeagueInfo>;

    /// Today's scoreboard for `league`.
    fn scoreboard<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>>;

    /// `league`'s scoreboard for another day (to fill in a postseason's
    /// earlier games), if the provider can look back.
    fn scoreboard_on<'a>(
        &'a self,
        league: &'a LeagueId,
        day: chrono::NaiveDate,
    ) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async move { Err(ProviderError::Unsupported(format!("{league} scoreboard for {day}"))) })
    }

    /// Current standings for `league`, if the provider has them.
    fn standings<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Standings, ProviderError>> {
        Box::pin(async move { Err(ProviderError::Unsupported(format!("{league} standings"))) })
    }

    /// Every team in `league`, if the provider can list them.
    fn teams<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Vec<TeamInfo>, ProviderError>> {
        Box::pin(async move { Err(ProviderError::Unsupported(format!("{league} teams"))) })
    }

    /// Details for one game (team stats, leaders, scoring plays), fetched
    /// only for the spotlighted game.
    fn summary<'a>(&'a self, game: &'a Game) -> BoxFuture<'a, Result<GameSummary, ProviderError>> {
        Box::pin(async move { Err(ProviderError::Unsupported(format!("summary for {}", game.id.0))) })
    }

    /// A team logo (image bytes) from a URL this provider put in its own
    /// data, for owners who turned provider logos on. Providers refuse any
    /// other host.
    fn logo<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Vec<u8>, ProviderError>> {
        Box::pin(async move { Err(ProviderError::Unsupported(format!("logo {url}"))) })
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

/// A fantasy football platform (Sleeper first).
pub trait FantasyProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Looks up an account by username.
    fn find_user<'a>(&'a self, username: &'a str) -> BoxFuture<'a, Result<FantasyUser, ProviderError>>;

    /// The user's leagues this season.
    fn leagues<'a>(&'a self, user_id: &'a str) -> BoxFuture<'a, Result<Vec<FantasyLeagueInfo>, ProviderError>>;

    /// The teams in a league.
    fn teams<'a>(&'a self, league_id: &'a str) -> BoxFuture<'a, Result<Vec<FantasyTeamInfo>, ProviderError>>;

    /// This week's matchup for a team, with live points.
    fn matchup<'a>(&'a self, league_id: &'a str, roster_id: u32) -> BoxFuture<'a, Result<Matchup, ProviderError>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_games;

    #[test]
    fn extra_games_show_only_for_favorites_or_the_watched_game() {
        let all = mock_games(chrono::Utc::now());
        let (main, more) = (all[..2].to_vec(), all[2..5].to_vec());
        let board = |more: Vec<Game>| Scoreboard { games: main.clone(), skipped: vec![], more };
        let ids = |games: Vec<Game>| games.into_iter().map(|g| g.id).collect::<Vec<_>>();

        assert_eq!(ids(board(more.clone()).games_for(&[], None)), ids(main.clone()), "nobody follows them");

        let fav = more[0].away.team.id.clone();
        let with_fav = ids(board(more.clone()).games_for(std::slice::from_ref(&fav), None));
        assert_eq!(with_fav.len(), 3);
        assert_eq!(with_fav[2], more[0].id);

        let watched = ids(board(more.clone()).games_for(&[], Some(&more[2].id)));
        assert_eq!(watched.last(), Some(&more[2].id));

        let dup = ids(board(vec![main[0].clone()]).games_for(&[main[0].home.team.id.clone()], None));
        assert_eq!(dup, ids(main), "a game on both lists shows once");
    }
}
