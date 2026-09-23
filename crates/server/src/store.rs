//! Last known games per league, with failure tracking. Pure.
//!
//! A failed fetch never wipes data: the last good games are kept and, after a
//! few consecutive failures, flagged stale so the display can say so.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use marqueet_core::protocol::FeedStatus;
use marqueet_core::provider::ProviderError;
use marqueet_core::sports::standings::Standings;
use marqueet_core::sports::{Game, LeagueId};
use marqueet_core::weather::{Place, Units, Weather};
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct LeagueFeed {
    pub games: Vec<Game>,
    pub last_success: Option<DateTime<Utc>>,
    /// Consecutive failures since the last success.
    pub failures: u32,
    pub last_error: Option<String>,
}

/// Standings for one league and when they were last fetched.
#[derive(Clone, Debug, Default)]
pub struct StandingsFeed {
    /// Last good standings (kept through failures).
    pub standings: Option<Standings>,
    pub last_attempt: Option<DateTime<Utc>>,
    pub last_failed: bool,
    /// The provider has no standings for this league; don't ask again.
    pub unsupported: bool,
}

/// The last weather fetch.
#[derive(Clone, Debug, Default)]
pub struct WeatherFeed {
    pub weather: Option<Weather>,
    pub last_attempt: Option<DateTime<Utc>>,
    pub last_failed: bool,
    pub last_error: Option<String>,
}

#[derive(Debug)]
pub struct Store {
    /// Display order of leagues.
    order: Vec<LeagueId>,
    feeds: HashMap<LeagueId, LeagueFeed>,
    standings: HashMap<LeagueId, StandingsFeed>,
    weather: WeatherFeed,
    stale_after: u32,
}

impl Store {
    pub fn new(order: Vec<LeagueId>, stale_after: u32) -> Self {
        let feeds = order.iter().map(|l| (l.clone(), LeagueFeed::default())).collect();
        Store {
            order,
            feeds,
            standings: HashMap::new(),
            weather: WeatherFeed::default(),
            stale_after: stale_after.max(1),
        }
    }

    /// Changes the followed leagues (and their order). Data for leagues that
    /// stay is kept; dropped leagues are forgotten.
    pub fn set_leagues(&mut self, order: Vec<LeagueId>) {
        self.feeds.retain(|l, _| order.contains(l));
        self.standings.retain(|l, _| order.contains(l));
        for l in &order {
            self.feeds.entry(l.clone()).or_default();
        }
        self.order = order;
    }

    pub fn leagues(&self) -> &[LeagueId] {
        &self.order
    }

    pub fn feed(&self, league: &LeagueId) -> Option<&LeagueFeed> {
        self.feeds.get(league)
    }

    pub fn record_success(&mut self, league: &LeagueId, games: Vec<Game>, now: DateTime<Utc>) {
        let feed = self.feeds.entry(league.clone()).or_default();
        feed.games = games;
        feed.last_success = Some(now);
        feed.failures = 0;
        feed.last_error = None;
    }

    /// Returns the consecutive failure count.
    pub fn record_failure(&mut self, league: &LeagueId, error: String) -> u32 {
        let feed = self.feeds.entry(league.clone()).or_default();
        feed.failures = feed.failures.saturating_add(1);
        feed.last_error = Some(error);
        feed.failures
    }

    /// True when the league has data but recent fetches keep failing.
    pub fn is_stale(&self, league: &LeagueId) -> bool {
        self.feeds.get(league).is_some_and(|f| f.last_success.is_some() && f.failures >= self.stale_after)
    }

    /// All games in league order, with `stale` set where it applies.
    pub fn games(&self) -> Vec<Game> {
        let mut out = Vec::new();
        for league in &self.order {
            let Some(feed) = self.feeds.get(league) else { continue };
            let stale = self.is_stale(league);
            out.extend(feed.games.iter().cloned().map(|mut g| {
                g.stale = stale;
                g
            }));
        }
        out
    }

    pub fn status(&self) -> FeedStatus {
        let games = self.games();
        FeedStatus {
            live_games: games.iter().filter(|g| g.status.is_live()).count() as u32,
            stale_leagues: self.order.iter().filter(|l| self.is_stale(l)).map(|l| l.to_string()).collect(),
            updated_at: self.feeds.values().filter_map(|f| f.last_success).max(),
        }
    }

    /// Followed leagues whose standings should be fetched now: never tried,
    /// older than `every`, or failed more than `retry` ago.
    pub fn standings_due(&self, now: DateTime<Utc>, every: chrono::Duration, retry: chrono::Duration) -> Vec<LeagueId> {
        self.order
            .iter()
            .filter(|l| match self.standings.get(*l) {
                None => true,
                Some(f) if f.unsupported => false,
                Some(f) => f.last_attempt.is_none_or(|t| now - t >= if f.last_failed { retry } else { every }),
            })
            .cloned()
            .collect()
    }

    pub fn record_standings(
        &mut self,
        league: &LeagueId,
        result: Result<Standings, ProviderError>,
        now: DateTime<Utc>,
    ) {
        let feed = self.standings.entry(league.clone()).or_default();
        feed.last_attempt = Some(now);
        feed.last_failed = result.is_err();
        match result {
            Ok(s) => feed.standings = Some(s),
            Err(ProviderError::Unsupported(_)) => feed.unsupported = true,
            Err(_) => {}
        }
    }

    /// Standings for followed leagues, in league order.
    pub fn standings(&self) -> Vec<Standings> {
        self.order.iter().filter_map(|l| self.standings.get(l)?.standings.clone()).collect()
    }

    /// Weather for `place` in `units`, if that's what was last fetched.
    pub fn weather_for(&self, place: &Place, units: Units) -> Option<&Weather> {
        self.weather.weather.as_ref().filter(|w| &w.place == place && w.units == units)
    }

    pub fn weather_feed(&self) -> &WeatherFeed {
        &self.weather
    }

    /// True when weather for `place` should be fetched: none for this place
    /// and units yet, older than `every`, or failed more than `retry` ago.
    pub fn weather_due(
        &self,
        place: &Place,
        units: Units,
        now: DateTime<Utc>,
        every: chrono::Duration,
        retry: chrono::Duration,
    ) -> bool {
        let f = &self.weather;
        if f.last_failed {
            return f.last_attempt.is_none_or(|t| now - t >= retry);
        }
        match self.weather_for(place, units) {
            None => true,
            Some(w) => now - w.fetched_at >= every,
        }
    }

    pub fn record_weather(&mut self, result: Result<Weather, ProviderError>, now: DateTime<Utc>) {
        let f = &mut self.weather;
        f.last_attempt = Some(now);
        f.last_failed = result.is_err();
        match result {
            Ok(w) => {
                f.weather = Some(w);
                f.last_error = None;
            }
            Err(e) => f.last_error = Some(e.to_string()),
        }
    }

    /// True until any league has fetched successfully once.
    pub fn is_empty_startup(&self) -> bool {
        self.feeds.values().all(|f| f.last_success.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::sports::Sport;
    use marqueet_core::sports::fixtures::mock_games;

    #[test]
    fn weather_is_fetched_for_the_current_place_only() {
        use marqueet_core::weather::mock_weather;
        let t0 = Utc::now();
        let (every, retry) = (chrono::Duration::minutes(15), chrono::Duration::minutes(5));
        let mut s = Store::new(vec![l("nfl")], 3);
        let w = mock_weather(t0);
        let kc = w.place.clone();
        assert!(s.weather_due(&kc, Units::Fahrenheit, t0, every, retry));
        s.record_weather(Ok(w), t0);
        assert!(s.weather_for(&kc, Units::Fahrenheit).is_some());
        assert!(!s.weather_due(&kc, Units::Fahrenheit, t0 + chrono::Duration::minutes(10), every, retry));
        assert!(s.weather_due(&kc, Units::Fahrenheit, t0 + chrono::Duration::minutes(15), every, retry));
        assert!(s.weather_due(&kc, Units::Celsius, t0, every, retry), "units changed");
        assert!(s.weather_for(&kc, Units::Celsius).is_none());
        let oslo = Place { name: "Oslo, Norway".into(), latitude: 59.9, longitude: 10.7 };
        assert!(s.weather_for(&oslo, Units::Fahrenheit).is_none(), "moved: old weather isn't shown");
        s.record_weather(Err(ProviderError::Status(500)), t0);
        assert!(!s.weather_due(&kc, Units::Fahrenheit, t0 + chrono::Duration::minutes(4), every, retry));
        assert!(s.weather_due(&kc, Units::Fahrenheit, t0 + chrono::Duration::minutes(5), every, retry));
        assert!(s.weather_for(&kc, Units::Fahrenheit).is_some(), "a failure keeps the last forecast");
    }

    #[test]
    fn standings_are_fetched_on_a_slow_schedule() {
        let t0 = Utc::now();
        let (every, retry) = (chrono::Duration::minutes(30), chrono::Duration::minutes(10));
        let mut s = Store::new(vec![l("nfl"), l("ncaaf"), l("mlb")], 3);
        assert_eq!(s.standings_due(t0, every, retry).len(), 3, "never fetched");
        let table =
            |league: &str| Standings { league: l(league), sport: Sport::Football, groups: vec![], fetched_at: t0 };
        s.record_standings(&l("nfl"), Ok(table("nfl")), t0);
        s.record_standings(&l("ncaaf"), Err(ProviderError::Unsupported("ncaaf standings".into())), t0);
        s.record_standings(&l("mlb"), Err(ProviderError::Status(503)), t0);
        assert!(s.standings_due(t0 + chrono::Duration::minutes(5), every, retry).is_empty());
        assert_eq!(s.standings_due(t0 + chrono::Duration::minutes(10), every, retry), vec![l("mlb")], "retry");
        assert_eq!(s.standings_due(t0 + chrono::Duration::minutes(30), every, retry), vec![l("nfl"), l("mlb")]);
        s.record_standings(&l("nfl"), Err(ProviderError::Status(500)), t0);
        assert_eq!(s.standings().len(), 1, "a failure keeps the last good standings");
        s.set_leagues(vec![l("mlb")]);
        assert!(s.standings().is_empty(), "dropped leagues are forgotten");
    }

    fn l(id: &str) -> LeagueId {
        LeagueId::new(id)
    }

    fn nfl_games() -> Vec<Game> {
        mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect()
    }

    #[test]
    fn keeps_last_good_data_through_failures_then_marks_stale() {
        let mut s = Store::new(vec![l("nfl")], 3);
        s.record_success(&l("nfl"), nfl_games(), Utc::now());
        assert_eq!(s.record_failure(&l("nfl"), "timeout".into()), 1);
        assert_eq!(s.record_failure(&l("nfl"), "timeout".into()), 2);
        assert!(!s.is_stale(&l("nfl")));
        assert_eq!(s.games().len(), 4, "data kept");
        s.record_failure(&l("nfl"), "HTTP 503".into());
        assert!(s.is_stale(&l("nfl")));
        assert!(s.games().iter().all(|g| g.stale));
        assert_eq!(s.status().stale_leagues, vec!["nfl".to_string()]);

        s.record_success(&l("nfl"), nfl_games(), Utc::now());
        assert!(!s.is_stale(&l("nfl")));
        assert!(s.games().iter().all(|g| !g.stale));
    }

    #[test]
    fn a_league_that_never_loaded_is_not_stale_just_empty() {
        let mut s = Store::new(vec![l("nfl")], 1);
        s.record_failure(&l("nfl"), "dns".into());
        assert!(!s.is_stale(&l("nfl")));
        assert!(s.games().is_empty());
        assert!(s.is_empty_startup());
    }

    #[test]
    fn changing_leagues_keeps_data_for_the_ones_that_stay() {
        let mut s = Store::new(vec![l("nfl"), l("mlb")], 3);
        s.record_success(&l("nfl"), nfl_games(), Utc::now());
        s.set_leagues(vec![l("nhl"), l("nfl")]);
        assert_eq!(s.leagues(), [l("nhl"), l("nfl")]);
        assert_eq!(s.games().len(), 4, "NFL data kept");
        assert!(s.feed(&l("mlb")).is_none());
    }

    #[test]
    fn games_follow_configured_league_order() {
        let mut s = Store::new(vec![l("mlb"), l("nfl")], 3);
        let all = mock_games(Utc::now());
        s.record_success(&l("nfl"), all.iter().filter(|g| g.league.as_str() == "nfl").cloned().collect(), Utc::now());
        s.record_success(&l("mlb"), all.iter().filter(|g| g.league.as_str() == "mlb").cloned().collect(), Utc::now());
        let leagues: Vec<String> = s.games().iter().map(|g| g.league.to_string()).collect();
        assert_eq!(leagues.first().map(String::as_str), Some("mlb"));
        assert_eq!(leagues.last().map(String::as_str), Some("nfl"));
        assert_eq!(s.status().live_games, 2, "KC-BUF and LAD-CHC are live");
    }
}
