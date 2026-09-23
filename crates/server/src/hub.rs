//! Shared state: the store, the latest display content, and the pollers
//! that keep them fresh.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{Local, Utc};
use marqueet_core::alert::Alert;
use marqueet_core::events;
use marqueet_core::protocol::Content;
use marqueet_core::provider::DataProvider;
use marqueet_core::sports::LeagueId;
use marqueet_core::sports::ticker::FormatOptions;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::content;
use crate::schedule::{Policy, backoff, jittered, next_poll};
use crate::store::Store;

#[derive(Debug)]
pub struct Hub {
    store: Mutex<Store>,
    content: watch::Sender<Arc<Content>>,
    alerts: broadcast::Sender<Arc<Alert>>,
    history: Mutex<AlertHistory>,
    policy: Policy,
}

/// Recently sent alerts, for dedupe and `/api/alerts`.
#[derive(Debug, Default)]
struct AlertHistory {
    seen: HashSet<String>,
    order: VecDeque<String>,
    recent: VecDeque<Arc<Alert>>,
}

impl AlertHistory {
    const SEEN_CAP: usize = 1000;
    const RECENT_CAP: usize = 50;

    /// Records `alert`; false if an alert with the same id was already sent.
    fn insert(&mut self, alert: &Arc<Alert>) -> bool {
        if !self.seen.insert(alert.id.clone()) {
            return false;
        }
        self.order.push_back(alert.id.clone());
        if self.order.len() > Self::SEEN_CAP
            && let Some(old) = self.order.pop_front()
        {
            self.seen.remove(&old);
        }
        self.recent.push_front(Arc::clone(alert));
        self.recent.truncate(Self::RECENT_CAP);
        true
    }
}

fn format_options() -> FormatOptions {
    FormatOptions { tz: *Local::now().offset(), now: Utc::now() }
}

impl Hub {
    pub fn new(leagues: Vec<LeagueId>, policy: Policy) -> Arc<Hub> {
        let store = Store::new(leagues, policy.stale_after_failures);
        let (content, _) = watch::channel(Arc::new(content::build(&store, &format_options())));
        let (alerts, _) = broadcast::channel(64);
        Arc::new(Hub { store: Mutex::new(store), content, alerts, history: Mutex::default(), policy })
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        // A panic while holding the lock can't corrupt plain data; keep going.
        self.store.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Content>> {
        self.content.subscribe()
    }

    pub fn subscribe_alerts(&self) -> broadcast::Receiver<Arc<Alert>> {
        self.alerts.subscribe()
    }

    /// Most recent alerts, newest first.
    pub fn recent_alerts(&self) -> Vec<Arc<Alert>> {
        self.history.lock().unwrap_or_else(|p| p.into_inner()).recent.iter().cloned().collect()
    }

    pub fn current(&self) -> Arc<Content> {
        self.content.borrow().clone()
    }

    /// Runs `f` with read access to the store (for the JSON API).
    pub fn with_store<T>(&self, f: impl FnOnce(&Store) -> T) -> T {
        f(&self.store())
    }

    fn publish(&self, store: &Store) {
        let next = content::build(store, &format_options());
        self.content.send_if_modified(|current| {
            // Only what the display shows counts as a change; a fresh
            // `updated_at` alone is stored without waking subscribers.
            let same_view = current.ticker == next.ticker
                && current.crawl == next.crawl
                && current.status.live_games == next.status.live_games
                && current.status.stale_leagues == next.status.stale_leagues;
            *current = Arc::new(next);
            !same_view
        });
    }

    /// Stores fresh games, publishes content, sends any alerts, and returns
    /// how long to wait before polling again.
    pub fn record_success(&self, league: &LeagueId, games: Vec<marqueet_core::sports::Game>) -> Duration {
        let now = Utc::now();
        let mut store = self.store();
        let delay = next_poll(&games, now, &self.policy);
        // Only compare against a fresh previous snapshot: after a restart or a
        // stale stretch, a jump in score isn't a play we watched happen.
        let prev =
            store.feed(league).filter(|f| f.last_success.is_some() && !store.is_stale(league)).map(|f| f.games.clone());
        let found: Vec<Alert> = prev
            .map(|prev| events::detect_all(&prev, &games))
            .unwrap_or_default()
            .iter()
            .filter_map(|(event, game)| events::alert(event, game, now))
            .collect();
        store.record_success(league, games, now);
        self.publish(&store);
        drop(store);
        for alert in found {
            let alert = Arc::new(alert);
            if self.history.lock().unwrap_or_else(|p| p.into_inner()).insert(&alert) {
                log::info!("{league}: {} ({})", alert.title, alert.detail.as_deref().unwrap_or(""));
                // No displays connected is fine.
                let _ = self.alerts.send(alert);
            }
        }
        delay
    }

    /// Records a failure (keeping old data) and returns the backoff delay.
    pub fn record_failure(&self, league: &LeagueId, error: String) -> Duration {
        let mut store = self.store();
        let failures = store.record_failure(league, error);
        self.publish(&store);
        backoff(failures, &self.policy)
    }

    /// Starts one polling task per league.
    pub fn spawn_pollers(self: &Arc<Self>, provider: Arc<dyn DataProvider>) -> Vec<JoinHandle<()>> {
        let leagues = self.store().leagues().to_vec();
        leagues
            .into_iter()
            .map(|league| {
                let hub = Arc::clone(self);
                let provider = Arc::clone(&provider);
                tokio::spawn(async move { hub.poll_forever(provider, league).await })
            })
            .collect()
    }

    async fn poll_forever(self: Arc<Self>, provider: Arc<dyn DataProvider>, league: LeagueId) {
        let mut rng = Rng::seeded(league.as_str());
        loop {
            let delay = match provider.scoreboard(&league).await {
                Ok(board) => {
                    for note in &board.skipped {
                        log::warn!("{league}: skipped {note}");
                    }
                    let delay = self.record_success(&league, board.games);
                    log::debug!("{league}: ok, next poll in {delay:?}");
                    delay
                }
                Err(e) => {
                    let delay = self.record_failure(&league, e.to_string());
                    log::warn!("{league}: {e}; retrying in {delay:?}");
                    delay
                }
            };
            tokio::time::sleep(jittered(delay, rng.unit())).await;
        }
    }
}

/// Tiny xorshift PRNG for jitter; not worth a dependency.
#[derive(Debug)]
struct Rng(u64);

impl Rng {
    fn seeded(salt: &str) -> Rng {
        let nanos = u64::from(Utc::now().timestamp_subsec_nanos());
        let salt =
            salt.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
        Rng((nanos ^ salt) | 1)
    }

    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::sports::fixtures::mock_games;

    #[test]
    fn content_is_published_only_when_it_changes() {
        let hub = Hub::new(vec![LeagueId::new("nfl")], Policy::default());
        let mut rx = hub.subscribe();
        rx.mark_unchanged();
        let games: Vec<_> = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
        let delay = hub.record_success(&LeagueId::new("nfl"), games.clone());
        assert_eq!(delay, Policy::default().live, "KC-BUF is live");
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();
        hub.record_success(&LeagueId::new("nfl"), games);
        assert!(!rx.has_changed().unwrap(), "same games, no new message");
        assert_eq!(hub.record_failure(&LeagueId::new("nfl"), "boom".into()), Policy::default().backoff_base);
    }

    #[test]
    fn score_changes_send_one_alert_each_and_first_snapshot_sends_none() {
        let nfl = LeagueId::new("nfl");
        let hub = Hub::new(vec![nfl.clone()], Policy::default());
        let mut rx = hub.subscribe_alerts();
        let games: Vec<_> = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
        hub.record_success(&nfl, games.clone());
        assert!(rx.try_recv().is_err(), "first snapshot: nothing to compare");

        let mut scored = games.clone();
        scored[0].home.score = Some(28);
        hub.record_success(&nfl, scored.clone());
        let alert = rx.try_recv().unwrap();
        assert_eq!(alert.title, "TOUCHDOWN");
        assert_eq!(alert.id, "mock:nfl:1:touchdown:17-28");

        // Score bounces back and forth (e.g. review then re-award): the same
        // moment is not announced twice.
        hub.record_success(&nfl, games);
        hub.record_success(&nfl, scored);
        assert!(rx.try_recv().is_err());
        assert_eq!(hub.recent_alerts().len(), 1);
    }

    #[test]
    fn no_alerts_across_a_stale_gap() {
        let nfl = LeagueId::new("nfl");
        let policy = Policy { stale_after_failures: 1, ..Policy::default() };
        let hub = Hub::new(vec![nfl.clone()], policy);
        let mut rx = hub.subscribe_alerts();
        let games: Vec<_> = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
        hub.record_success(&nfl, games.clone());
        hub.record_failure(&nfl, "down".into());
        let mut scored = games;
        scored[0].home.score = Some(35);
        hub.record_success(&nfl, scored);
        assert!(rx.try_recv().is_err(), "we didn't see that touchdown happen");
    }

    #[test]
    fn rng_is_in_unit_range() {
        let mut r = Rng::seeded("nfl");
        assert!((0..1000).map(|_| r.unit()).all(|u| (0.0..1.0).contains(&u)));
    }
}
