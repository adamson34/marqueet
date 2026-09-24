//! Last known games per league, with failure tracking. Pure.
//!
//! A failed fetch never wipes data: the last good games are kept and, after a
//! few consecutive failures, flagged stale so the display can say so.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use marqueet_core::fantasy::Matchup;
use marqueet_core::feeds::Feed;
use marqueet_core::protocol::FeedStatus;
use marqueet_core::provider::{ProviderError, TeamInfo};
use marqueet_core::sports::standings::Standings;
use marqueet_core::sports::{Game, LeagueId};
use marqueet_core::weather::{Place, Units, Weather, WeatherAlert};
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

/// Official weather alerts for the configured place.
#[derive(Clone, Debug, Default)]
pub struct WeatherAlertsFeed {
    pub place: Option<Place>,
    pub alerts: Vec<WeatherAlert>,
    pub last_attempt: Option<DateTime<Utc>>,
    /// The place is outside the provider's coverage; don't ask again.
    pub unsupported: bool,
    pub last_error: Option<String>,
}

/// A league's team list (for picking favorites).
#[derive(Clone, Debug, Default)]
pub struct TeamsFeed {
    pub teams: Vec<TeamInfo>,
    pub last_attempt: Option<DateTime<Utc>>,
    pub last_failed: bool,
    pub unsupported: bool,
}

/// The last fetch of one fantasy team's matchup.
#[derive(Clone, Debug, Default)]
pub struct FantasyFeed {
    pub matchup: Option<Matchup>,
    pub last_attempt: Option<DateTime<Utc>>,
    pub last_failed: bool,
    pub last_error: Option<String>,
}

/// (league id, roster id)
pub type FantasyKey = (String, u32);

#[derive(Debug)]
pub struct Store {
    /// Display order of leagues.
    order: Vec<LeagueId>,
    feeds: HashMap<LeagueId, LeagueFeed>,
    standings: HashMap<LeagueId, StandingsFeed>,
    weather: WeatherFeed,
    weather_alerts: WeatherAlertsFeed,
    fantasy: HashMap<FantasyKey, FantasyFeed>,
    teams: HashMap<LeagueId, TeamsFeed>,
    /// Content pushed through the feed API, by feed name.
    custom: BTreeMap<String, Feed>,
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
            weather_alerts: WeatherAlertsFeed::default(),
            fantasy: HashMap::new(),
            teams: HashMap::new(),
            custom: BTreeMap::new(),
            stale_after: stale_after.max(1),
        }
    }

    /// Changes the followed leagues (and their order). Data for leagues that
    /// stay is kept; dropped leagues are forgotten.
    pub fn set_leagues(&mut self, order: Vec<LeagueId>) {
        self.feeds.retain(|l, _| order.contains(l));
        self.standings.retain(|l, _| order.contains(l));
        self.teams.retain(|l, _| order.contains(l));
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

    /// True when alerts for `place` should be fetched: a new place, or the
    /// last fetch was at least `every` ago. Never for a place the provider
    /// doesn't cover.
    pub fn weather_alerts_due(&self, place: &Place, now: DateTime<Utc>, every: chrono::Duration) -> bool {
        let f = &self.weather_alerts;
        if f.place.as_ref() != Some(place) {
            return true;
        }
        !f.unsupported && f.last_attempt.is_none_or(|t| now - t >= every)
    }

    /// Records a fetch and returns the alerts that weren't there before. An
    /// update to an alert (same event from the same office) isn't new.
    pub fn record_weather_alerts(
        &mut self,
        place: &Place,
        result: Result<Vec<WeatherAlert>, ProviderError>,
        now: DateTime<Utc>,
    ) -> Vec<WeatherAlert> {
        let f = &mut self.weather_alerts;
        if f.place.as_ref() != Some(place) {
            *f = WeatherAlertsFeed { place: Some(place.clone()), ..WeatherAlertsFeed::default() };
        }
        f.last_attempt = Some(now);
        match result {
            Ok(list) => {
                let seen = |a: &WeatherAlert| {
                    f.alerts.iter().any(|o| o.id == a.id || (o.event == a.event && o.sender == a.sender))
                };
                let new = list.iter().filter(|a| !seen(a)).cloned().collect();
                f.alerts = list;
                f.last_error = None;
                new
            }
            Err(ProviderError::Unsupported(_)) => {
                f.unsupported = true;
                f.alerts.clear();
                Vec::new()
            }
            Err(e) => {
                f.last_error = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// Alerts in effect at `place` now.
    pub fn weather_alerts_for(&self, place: &Place, now: DateTime<Utc>) -> Vec<&WeatherAlert> {
        let f = &self.weather_alerts;
        if f.place.as_ref() != Some(place) {
            return Vec::new();
        }
        f.alerts.iter().filter(|a| a.ends.is_none_or(|e| e > now)).collect()
    }

    /// Followed leagues whose team lists should be fetched: never tried,
    /// older than `every`, or failed at least `retry` ago.
    pub fn teams_due(&self, now: DateTime<Utc>, every: chrono::Duration, retry: chrono::Duration) -> Vec<LeagueId> {
        self.order
            .iter()
            .filter(|l| match self.teams.get(*l) {
                None => true,
                Some(f) if f.unsupported => false,
                Some(f) => f.last_attempt.is_none_or(|t| now - t >= if f.last_failed { retry } else { every }),
            })
            .cloned()
            .collect()
    }

    pub fn record_teams(
        &mut self,
        league: &LeagueId,
        result: Result<Vec<TeamInfo>, ProviderError>,
        now: DateTime<Utc>,
    ) {
        let f = self.teams.entry(league.clone()).or_default();
        f.last_attempt = Some(now);
        f.last_failed = result.is_err();
        match result {
            Ok(teams) => f.teams = teams,
            Err(ProviderError::Unsupported(_)) => f.unsupported = true,
            Err(_) => {}
        }
    }

    /// A league's teams (empty until fetched).
    pub fn teams(&self, league: &LeagueId) -> &[TeamInfo] {
        self.teams.get(league).map_or(&[], |f| f.teams.as_slice())
    }

    /// True while any NFL game is live (fantasy points are moving).
    pub fn nfl_live(&self) -> bool {
        self.feeds.iter().any(|(l, f)| l.as_str() == "nfl" && f.games.iter().any(|g| g.status.is_live()))
    }

    /// True when a fantasy team's matchup should be fetched: never tried,
    /// older than `every`, or failed at least `retry` ago.
    pub fn fantasy_due(
        &self,
        key: &FantasyKey,
        now: DateTime<Utc>,
        every: chrono::Duration,
        retry: chrono::Duration,
    ) -> bool {
        match self.fantasy.get(key) {
            None => true,
            Some(f) => f.last_attempt.is_none_or(|t| now - t >= if f.last_failed { retry } else { every }),
        }
    }

    pub fn record_fantasy(&mut self, key: FantasyKey, result: Result<Matchup, ProviderError>, now: DateTime<Utc>) {
        let f = self.fantasy.entry(key).or_default();
        f.last_attempt = Some(now);
        f.last_failed = result.is_err();
        match result {
            Ok(m) => {
                f.matchup = Some(m);
                f.last_error = None;
            }
            Err(e) => f.last_error = Some(e.to_string()),
        }
    }

    pub fn fantasy(&self, key: &FantasyKey) -> Option<&FantasyFeed> {
        self.fantasy.get(key)
    }

    /// Forgets teams that are no longer followed.
    pub fn retain_fantasy(&mut self, keep: &[FantasyKey]) {
        self.fantasy.retain(|k, _| keep.contains(k));
    }

    pub fn set_custom_feed(&mut self, feed: Feed) {
        self.custom.insert(feed.name.clone(), feed);
    }

    /// Returns true if the feed had content.
    pub fn remove_custom_feed(&mut self, name: &str) -> bool {
        self.custom.remove(name).is_some()
    }

    pub fn custom_feed(&self, name: &str) -> Option<&Feed> {
        self.custom.get(name)
    }

    /// Unexpired custom feeds, by name.
    pub fn custom_feeds(&self, now: DateTime<Utc>) -> impl Iterator<Item = &Feed> {
        self.custom.values().filter(move |f| f.expires_at > now)
    }

    /// Drops expired feeds; true if any were.
    pub fn prune_custom_feeds(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.custom.len();
        self.custom.retain(|_, f| f.expires_at > now);
        self.custom.len() != before
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
    fn team_lists_refresh_daily() {
        let t0 = Utc::now();
        let (every, retry) = (chrono::Duration::hours(24), chrono::Duration::minutes(10));
        let mut s = Store::new(vec![l("nfl"), l("ucl")], 3);
        assert_eq!(s.teams_due(t0, every, retry).len(), 2);
        let blizzard = TeamInfo {
            id: marqueet_core::sports::TeamId("espn:nfl:2".into()),
            abbreviation: "BUF".into(),
            name: "Buffalo Blizzard".into(),
        };
        s.record_teams(&l("nfl"), Ok(vec![blizzard]), t0);
        s.record_teams(&l("ucl"), Err(ProviderError::Unsupported("ucl teams".into())), t0);
        assert!(s.teams_due(t0 + chrono::Duration::hours(1), every, retry).is_empty());
        assert_eq!(s.teams_due(t0 + chrono::Duration::hours(24), every, retry), vec![l("nfl")]);
        assert_eq!(s.teams(&l("nfl"))[0].abbreviation, "BUF");
        assert!(s.teams(&l("mlb")).is_empty());
    }

    #[test]
    fn fantasy_polls_fast_only_when_asked() {
        let t0 = Utc::now();
        let key: FantasyKey = ("123".into(), 1);
        let mut s = Store::new(vec![l("nfl")], 3);
        let (every, retry) = (chrono::Duration::seconds(30), chrono::Duration::minutes(2));
        assert!(s.fantasy_due(&key, t0, every, retry));
        s.record_fantasy(key.clone(), Err(ProviderError::Status(500)), t0);
        assert!(!s.fantasy_due(&key, t0 + chrono::Duration::seconds(30), every, retry), "retry is slower");
        assert!(s.fantasy_due(&key, t0 + chrono::Duration::minutes(2), every, retry));
        assert_eq!(s.fantasy(&key).unwrap().last_error.as_deref(), Some("upstream returned HTTP 500"));
        s.retain_fantasy(&[]);
        assert!(s.fantasy(&key).is_none());
        assert!(!s.nfl_live());
        s.record_success(&l("nfl"), mock_games(t0).into_iter().filter(|g| g.league.as_str() == "nfl").collect(), t0);
        assert!(s.nfl_live());
    }

    #[test]
    fn weather_alerts_fire_once_and_respect_coverage() {
        use marqueet_core::weather::{Severity, mock_weather};
        let t0 = Utc::now();
        let every = chrono::Duration::minutes(2);
        let mut s = Store::new(vec![l("nfl")], 3);
        let kc = mock_weather(t0).place;
        let warning = |id: &str| WeatherAlert {
            id: id.into(),
            event: "Tornado Warning".into(),
            severity: Severity::Extreme,
            immediate: true,
            area: "Jackson, MO".into(),
            ends: Some(t0 + chrono::Duration::minutes(45)),
            sender: "NWS Kansas City".into(),
        };
        assert!(s.weather_alerts_due(&kc, t0, every));
        assert_eq!(s.record_weather_alerts(&kc, Ok(vec![warning("a")]), t0).len(), 1, "new");
        assert!(!s.weather_alerts_due(&kc, t0 + chrono::Duration::minutes(1), every));
        assert!(s.record_weather_alerts(&kc, Ok(vec![warning("a")]), t0).is_empty(), "same alert");
        assert!(s.record_weather_alerts(&kc, Ok(vec![warning("a-update")]), t0).is_empty(), "an update isn't new");
        assert_eq!(s.weather_alerts_for(&kc, t0).len(), 1);
        assert!(s.weather_alerts_for(&kc, t0 + chrono::Duration::hours(1)).is_empty(), "ended");
        s.record_weather_alerts(&kc, Err(ProviderError::Status(500)), t0);
        assert_eq!(s.weather_alerts_for(&kc, t0).len(), 1, "a failure keeps what we had");

        let oslo = Place { name: "Oslo, Norway".into(), latitude: 59.9, longitude: 10.7 };
        assert!(s.weather_alerts_for(&oslo, t0).is_empty() && s.weather_alerts_due(&oslo, t0, every));
        s.record_weather_alerts(&oslo, Err(ProviderError::Unsupported("outside the US".into())), t0);
        assert!(!s.weather_alerts_due(&oslo, t0 + chrono::Duration::hours(5), every), "not covered: stop asking");
    }

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
        assert_eq!(s.status().live_games, 2, "KC-BUF and LA-CHI are live");
    }
}
