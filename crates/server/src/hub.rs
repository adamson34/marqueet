//! Shared state: settings, the store, the latest display content and look,
//! and the pollers that keep them fresh.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{Local, Utc};
use marqueet_core::alert::{Alert, AlertLevel};
use marqueet_core::events;
use marqueet_core::protocol::{Content, DisplayState};
use marqueet_core::provider::{DataProvider, LeagueInfo};
use marqueet_core::settings::{Settings, TakeoverPolicy};
use marqueet_core::sports::ticker::FormatOptions;
use marqueet_core::sports::{Game, HomeAway, LeagueId};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::content;
use crate::schedule::{Policy, backoff, jittered, next_poll};
use crate::settings_store::SettingsStore;
use crate::store::Store;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding a lock can't corrupt this plain data; keep going.
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct Hub {
    store: Mutex<Store>,
    settings: Mutex<Settings>,
    db: Option<SettingsStore>,
    content: watch::Sender<Arc<Content>>,
    display: watch::Sender<Arc<DisplayState>>,
    alerts: broadcast::Sender<Arc<Alert>>,
    history: Mutex<AlertHistory>,
    policy: Policy,
    provider: Mutex<Option<Arc<dyn DataProvider>>>,
    pollers: Mutex<HashMap<LeagueId, JoinHandle<()>>>,
    quiet_hours_ticker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub").field("settings", &self.settings).field("policy", &self.policy).finish_non_exhaustive()
    }
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

fn display_state(settings: &Settings) -> DisplayState {
    DisplayState { config: settings.display.clone(), screen_off: settings.screen_off_at(Local::now().time()) }
}

/// Applies the takeover policy: big plays by non-favorites (or all, when
/// takeovers are off) are downgraded to a ticker flash.
fn apply_takeover_policy(mut alert: Alert, game: &Game, side: Option<HomeAway>, settings: &Settings) -> Alert {
    if alert.level != AlertLevel::Takeover {
        return alert;
    }
    let allowed = match settings.takeovers {
        TakeoverPolicy::All => true,
        TakeoverPolicy::Off => false,
        TakeoverPolicy::Favorites => side.is_some_and(|s| settings.is_favorite(&game.competitor(s).team.id)),
    };
    if !allowed {
        alert.level = AlertLevel::Flash;
        alert.takeover = None;
    }
    alert
}

impl Hub {
    /// A hub following `settings`. With `db`, setting changes are saved.
    pub fn new(settings: Settings, policy: Policy, db: Option<SettingsStore>) -> Arc<Hub> {
        let settings = settings.sanitized();
        let store = Store::new(settings.leagues.clone(), policy.stale_after_failures);
        let (content, _) = watch::channel(Arc::new(content::build(&store, &format_options(), &settings)));
        let (display, _) = watch::channel(Arc::new(display_state(&settings)));
        let (alerts, _) = broadcast::channel(64);
        Arc::new(Hub {
            store: Mutex::new(store),
            settings: Mutex::new(settings),
            db,
            content,
            display,
            alerts,
            history: Mutex::default(),
            policy,
            provider: Mutex::new(None),
            pollers: Mutex::default(),
            quiet_hours_ticker: Mutex::default(),
        })
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        lock(&self.store)
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// Leagues the provider can fetch; empty before [`Hub::start`].
    pub fn supported_leagues(&self) -> Vec<LeagueInfo> {
        lock(&self.provider).as_ref().map(|p| p.leagues()).unwrap_or_default()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Content>> {
        self.content.subscribe()
    }

    pub fn subscribe_display(&self) -> watch::Receiver<Arc<DisplayState>> {
        self.display.subscribe()
    }

    pub fn subscribe_alerts(&self) -> broadcast::Receiver<Arc<Alert>> {
        self.alerts.subscribe()
    }

    /// Most recent alerts, newest first.
    pub fn recent_alerts(&self) -> Vec<Arc<Alert>> {
        lock(&self.history).recent.iter().cloned().collect()
    }

    pub fn current(&self) -> Arc<Content> {
        self.content.borrow().clone()
    }

    /// Runs `f` with read access to the store (for the JSON API).
    pub fn with_store<T>(&self, f: impl FnOnce(&Store) -> T) -> T {
        f(&self.store())
    }

    fn publish(&self, store: &Store) {
        let settings = self.settings();
        let next = content::build(store, &format_options(), &settings);
        self.content.send_if_modified(|current| {
            // Only what the display shows counts as a change; a fresh
            // `updated_at` alone is stored without waking subscribers.
            let same_view = current.ticker == next.ticker
                && current.crawl == next.crawl
                && current.widgets == next.widgets
                && current.status.live_games == next.status.live_games
                && current.status.stale_leagues == next.status.stale_leagues;
            *current = Arc::new(next);
            !same_view
        });
    }

    /// Recomputes the display look (e.g. quiet hours starting or ending).
    pub fn refresh_display(&self) {
        let next = display_state(&lock(&self.settings));
        self.display.send_if_modified(|current| {
            let changed = **current != next;
            if changed {
                *current = Arc::new(next);
            }
            changed
        });
    }

    /// Validates, saves and applies new settings: starts/stops pollers for
    /// added/removed leagues and republishes content and the display look.
    pub fn apply_settings(self: &Arc<Self>, settings: Settings) -> Result<Settings, String> {
        let settings = settings.sanitized();
        if let Some(provider) = lock(&self.provider).as_ref() {
            let supported = provider.leagues();
            if let Some(bad) = settings.leagues.iter().find(|l| !supported.iter().any(|s| &s.id == *l)) {
                return Err(format!("unknown league {bad:?}"));
            }
        }
        if let Some(db) = &self.db {
            db.save(&settings).map_err(|e| e.to_string())?;
        }
        *lock(&self.settings) = settings.clone();
        {
            let mut store = self.store();
            store.set_leagues(settings.leagues.clone());
            self.publish(&store);
        }
        self.refresh_display();
        self.sync_pollers();
        log::info!(
            "settings updated: leagues {}",
            settings.leagues.iter().map(LeagueId::as_str).collect::<Vec<_>>().join(",")
        );
        Ok(settings)
    }

    /// Stores fresh games, publishes content, sends any alerts, and returns
    /// how long to wait before polling again.
    pub fn record_success(&self, league: &LeagueId, games: Vec<Game>) -> Duration {
        let now = Utc::now();
        let settings = self.settings();
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
            .filter_map(|(event, game)| {
                events::alert(event, game, now).map(|a| apply_takeover_policy(a, game, event.side, &settings))
            })
            .collect();
        store.record_success(league, games, now);
        self.publish(&store);
        drop(store);
        for alert in found {
            let alert = Arc::new(alert);
            if lock(&self.history).insert(&alert) {
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

    /// Starts polling with `provider` (one task per league) plus a ticker
    /// that keeps quiet hours up to date.
    pub fn start(self: &Arc<Self>, provider: Arc<dyn DataProvider>) {
        *lock(&self.provider) = Some(provider);
        self.sync_pollers();
        let hub = Arc::clone(self);
        let ticker = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(20)).await;
                hub.refresh_display();
            }
        });
        *lock(&self.quiet_hours_ticker) = Some(ticker);
    }

    /// Stops all background tasks.
    pub fn stop(&self) {
        for (_, task) in lock(&self.pollers).drain() {
            task.abort();
        }
        if let Some(task) = lock(&self.quiet_hours_ticker).take() {
            task.abort();
        }
    }

    /// Makes the running pollers match the configured leagues.
    fn sync_pollers(self: &Arc<Self>) {
        let Some(provider) = lock(&self.provider).clone() else { return };
        let wanted = self.store().leagues().to_vec();
        let mut pollers = lock(&self.pollers);
        pollers.retain(|league, task| {
            let keep = wanted.contains(league);
            if !keep {
                task.abort();
                log::info!("{league}: stopped polling");
            }
            keep
        });
        for league in wanted {
            if pollers.contains_key(&league) {
                continue;
            }
            let hub = Arc::clone(self);
            let provider = Arc::clone(&provider);
            let l = league.clone();
            pollers.insert(league, tokio::spawn(async move { hub.poll_forever(provider, l).await }));
        }
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

    fn nfl() -> LeagueId {
        LeagueId::new("nfl")
    }

    fn hub_with(settings: Settings) -> Arc<Hub> {
        Hub::new(settings, Policy::default(), None)
    }

    fn hub() -> Arc<Hub> {
        hub_with(Settings { leagues: vec![nfl()], ..Settings::default() })
    }

    fn nfl_games() -> Vec<Game> {
        mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect()
    }

    #[test]
    fn content_is_published_only_when_it_changes() {
        let hub = hub();
        let mut rx = hub.subscribe();
        rx.mark_unchanged();
        let games = nfl_games();
        let delay = hub.record_success(&nfl(), games.clone());
        assert_eq!(delay, Policy::default().live, "KC-BUF is live");
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();
        hub.record_success(&nfl(), games);
        assert!(!rx.has_changed().unwrap(), "same games, no new message");
        assert_eq!(hub.record_failure(&nfl(), "boom".into()), Policy::default().backoff_base);
    }

    #[test]
    fn score_changes_send_one_alert_each_and_first_snapshot_sends_none() {
        let hub = hub();
        let mut rx = hub.subscribe_alerts();
        let games = nfl_games();
        hub.record_success(&nfl(), games.clone());
        assert!(rx.try_recv().is_err(), "first snapshot: nothing to compare");

        let mut scored = games.clone();
        scored[0].home.score = Some(28);
        hub.record_success(&nfl(), scored.clone());
        let alert = rx.try_recv().unwrap();
        assert_eq!(alert.title, "TOUCHDOWN");
        assert_eq!(alert.id, "mock:nfl:1:touchdown:17-28");

        // Score bounces back and forth (e.g. review then re-award): the same
        // moment is not announced twice.
        hub.record_success(&nfl(), games);
        hub.record_success(&nfl(), scored);
        assert!(rx.try_recv().is_err());
        assert_eq!(hub.recent_alerts().len(), 1);
    }

    #[test]
    fn no_alerts_across_a_stale_gap() {
        let hub = Hub::new(
            Settings { leagues: vec![nfl()], ..Settings::default() },
            Policy { stale_after_failures: 1, ..Policy::default() },
            None,
        );
        let mut rx = hub.subscribe_alerts();
        let games = nfl_games();
        hub.record_success(&nfl(), games.clone());
        hub.record_failure(&nfl(), "down".into());
        let mut scored = games;
        scored[0].home.score = Some(35);
        hub.record_success(&nfl(), scored);
        assert!(rx.try_recv().is_err(), "we didn't see that touchdown happen");
    }

    fn touchdown_level(policy: TakeoverPolicy, favorites: Vec<marqueet_core::sports::TeamId>) -> AlertLevel {
        let hub = hub_with(Settings { leagues: vec![nfl()], takeovers: policy, favorites, ..Settings::default() });
        let mut rx = hub.subscribe_alerts();
        let games = nfl_games();
        hub.record_success(&nfl(), games.clone());
        let mut scored = games;
        scored[0].home.score = Some(28); // BUF touchdown
        hub.record_success(&nfl(), scored);
        rx.try_recv().unwrap().level
    }

    #[test]
    fn takeover_policy_downgrades_to_flash() {
        let buf = nfl_games()[0].home.team.id.clone();
        let kc = nfl_games()[0].away.team.id.clone();
        assert_eq!(touchdown_level(TakeoverPolicy::All, vec![]), AlertLevel::Takeover);
        assert_eq!(touchdown_level(TakeoverPolicy::Off, vec![buf.clone()]), AlertLevel::Flash);
        assert_eq!(touchdown_level(TakeoverPolicy::Favorites, vec![buf]), AlertLevel::Takeover);
        assert_eq!(touchdown_level(TakeoverPolicy::Favorites, vec![kc]), AlertLevel::Flash, "their score, not ours");
    }

    #[test]
    fn applying_settings_saves_them_and_updates_leagues_and_display() {
        let hub = Hub::new(Settings::default(), Policy::default(), Some(SettingsStore::in_memory().unwrap()));
        let mut display = hub.subscribe_display();
        display.mark_unchanged();
        let next = Settings {
            leagues: vec![LeagueId::new("MLB"), nfl()],
            display: marqueet_core::config::DisplayConfig {
                led_color: marqueet_core::Rgb::GREEN,
                ..Default::default()
            },
            ..Settings::default()
        };
        let saved = hub.apply_settings(next).unwrap();
        assert_eq!(saved.leagues, vec![LeagueId::new("mlb"), nfl()], "sanitized");
        assert_eq!(hub.with_store(|s| s.leagues().to_vec()), saved.leagues);
        assert!(display.has_changed().unwrap());
        assert_eq!(display.borrow().config.led_color, marqueet_core::Rgb::GREEN);
        assert_eq!(hub.db.as_ref().unwrap().load().unwrap().unwrap(), saved);
    }

    #[test]
    fn rng_is_in_unit_range() {
        let mut r = Rng::seeded("nfl");
        assert!((0..1000).map(|_| r.unit()).all(|u| (0.0..1.0).contains(&u)));
    }
}
