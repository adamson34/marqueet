//! Last known games per league, with failure tracking. Pure.
//!
//! A failed fetch never wipes data: the last good games are kept and, after a
//! few consecutive failures, flagged stale so the display can say so.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use marqueet_core::protocol::FeedStatus;
use marqueet_core::sports::{Game, LeagueId};
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct LeagueFeed {
    pub games: Vec<Game>,
    pub last_success: Option<DateTime<Utc>>,
    /// Consecutive failures since the last success.
    pub failures: u32,
    pub last_error: Option<String>,
}

#[derive(Debug)]
pub struct Store {
    /// Display order of leagues.
    order: Vec<LeagueId>,
    feeds: HashMap<LeagueId, LeagueFeed>,
    stale_after: u32,
}

impl Store {
    pub fn new(order: Vec<LeagueId>, stale_after: u32) -> Self {
        let feeds = order.iter().map(|l| (l.clone(), LeagueFeed::default())).collect();
        Store { order, feeds, stale_after: stale_after.max(1) }
    }

    /// Changes the followed leagues (and their order). Data for leagues that
    /// stay is kept; dropped leagues are forgotten.
    pub fn set_leagues(&mut self, order: Vec<LeagueId>) {
        self.feeds.retain(|l, _| order.contains(l));
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

    /// True until any league has fetched successfully once.
    pub fn is_empty_startup(&self) -> bool {
        self.feeds.values().all(|f| f.last_success.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::sports::fixtures::mock_games;

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
