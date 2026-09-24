//! Shared state: settings, the store, the latest display content and look,
//! and the pollers that keep them fresh.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use marqueet_core::alert::{Alert, AlertLevel};
use marqueet_core::events;
use marqueet_core::fantasy::{self, FantasyLeagueInfo, FantasyTeamInfo, FantasyUser, Matchup};
use marqueet_core::feeds::{self, AlertPost, FeedPost};
use marqueet_core::protocol::{Content, DisplayState};
use marqueet_core::provider::{
    DataProvider, FantasyProvider, LeagueInfo, ProviderError, WeatherAlertsProvider, WeatherProvider,
};
use marqueet_core::settings::{FantasyLeague, MAX_FANTASY};
use marqueet_core::settings::{Settings, TakeoverPolicy};
use marqueet_core::sports::ticker::FormatOptions;
use marqueet_core::sports::{Game, HomeAway, LeagueId, TeamId};
use marqueet_core::team_art::{self, Image, TeamArt, TeamArtMap};
use marqueet_core::weather::Place;
use tokio::sync::{Notify, broadcast, watch};
use tokio::task::JoinHandle;

use crate::admin::auth::{ct_eq, new_token};
use crate::content;
use crate::schedule::{Policy, backoff, jittered, next_poll};
use crate::settings_store::SettingsStore;
use crate::store::Store;
use crate::tz;

/// Every logo, by the key displays look it up under.
fn logo_set(art: &TeamArtMap) -> BTreeMap<String, Image> {
    art.iter().filter_map(|(team, a)| Some((team_art::logo_key(team), a.logo.clone()?))).collect()
}

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
    fantasy_provider: Mutex<Option<Arc<dyn FantasyProvider>>>,
    pollers: Mutex<HashMap<LeagueId, JoinHandle<()>>>,
    quiet_hours_ticker: Mutex<Option<JoinHandle<()>>>,
    slow_poller: Mutex<Option<JoinHandle<()>>>,
    weather_provider: Mutex<Option<Arc<dyn WeatherProvider>>>,
    /// Wakes the slow poller (settings changed).
    wake: Notify,
    /// Feed API tokens by feed name.
    feed_tokens: Mutex<HashMap<String, String>>,
    /// Last alert and last takeover per feed, for rate limits.
    feed_alerts: Mutex<HashMap<String, AlertTimes>>,
    /// Team colors and logos people added.
    team_art: Mutex<TeamArtMap>,
    /// Their logos, for displays.
    logos: watch::Sender<Arc<BTreeMap<String, Image>>>,
}

/// A feed's last alert and last takeover.
type AlertTimes = (DateTime<Utc>, Option<DateTime<Utc>>);

/// Most teams with custom colors or logos. At 128 px, all their logos are
/// about 16 MB for each display to receive, in chunks.
pub const MAX_TEAM_ART: usize = 250;

/// Most feeds a device will hold.
pub const MAX_FEEDS: usize = 20;
/// Least time between alerts from one feed, and between its takeovers.
const FEED_ALERT_GAP: chrono::TimeDelta = chrono::TimeDelta::seconds(5);
const FEED_TAKEOVER_GAP: chrono::TimeDelta = chrono::TimeDelta::seconds(30);

/// Why a feed alert wasn't sent.
#[derive(Debug, PartialEq, Eq)]
pub enum FeedAlertError {
    Invalid(String),
    /// Too soon after the last one; retry after this many seconds.
    TooSoon(i64),
}

/// A feed as listed by `/api/feeds` and the admin page (no token).
#[derive(Clone, Debug, serde::Serialize)]
pub struct FeedInfo {
    pub name: String,
    pub segments: usize,
    pub crawl: usize,
    /// When the current content expires; `None` when there is none.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Where the hub gets its data.
#[derive(Clone)]
pub struct Providers {
    pub scores: Arc<dyn DataProvider>,
    /// Optional: without it the weather widget says so.
    pub weather: Option<Arc<dyn WeatherProvider>>,
    /// Optional: official severe weather alerts.
    pub weather_alerts: Option<Arc<dyn WeatherAlertsProvider>>,
    /// Optional: fantasy football.
    pub fantasy: Option<Arc<dyn FantasyProvider>>,
}

/// A fantasy account's leagues and their teams, for the admin page.
#[derive(Clone, Debug)]
pub struct FantasySearch {
    pub user: FantasyUser,
    pub leagues: Vec<(FantasyLeagueInfo, Vec<FantasyTeamInfo>)>,
}

impl std::fmt::Debug for Providers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Providers")
            .field("scores", &self.scores.id())
            .field("weather", &self.weather.is_some())
            .field("weather_alerts", &self.weather_alerts.is_some())
            .field("fantasy", &self.fantasy.as_ref().map(|f| f.id()))
            .finish()
    }
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

fn format_options(settings: &Settings) -> FormatOptions {
    let now = Utc::now();
    FormatOptions { tz: tz::offset(settings, now), now }
}

fn display_state(settings: &Settings) -> DisplayState {
    let now = Utc::now();
    let local = now.with_timezone(&tz::offset(settings, now)).time();
    DisplayState {
        config: settings.display.clone(),
        screen_off: settings.screen_off_at(local),
        utc_offset: tz::configured_offset(settings, now).map(|o| o.local_minus_utc()),
        setup: None,
    }
}

/// A flash when a followed fantasy team takes or loses the lead.
fn lead_change(before: Option<&Matchup>, after: Option<&Matchup>) -> Option<Alert> {
    let margin = |m: &Matchup| Some(m.me.points - m.opponent.as_ref()?.points);
    let (was, now) = (margin(before?)?, margin(after?)?);
    let after = after?;
    let took = was <= 0.0 && now > 0.0;
    if !(took || (was > 0.0 && now <= 0.0)) {
        return None;
    }
    let title = if took { "TAKES THE LEAD" } else { "LOSES THE LEAD" };
    Some(Alert {
        id: format!("fantasy:{}:{}:{}:{}", after.league_id, after.me.roster_id, title, after.fetched_at.timestamp()),
        level: AlertLevel::Flash,
        source: "fantasy".into(),
        segment_id: Some(fantasy::segment_id(&after.league_id, after.me.roster_id)),
        title: format!("{} {title}", fantasy::led_name(&after.me.name, 14)),
        detail: Some(format!(
            "{} to {}",
            fantasy::points(after.me.points),
            after.opponent.as_ref().map_or(0.0, |o| o.points)
        )),
        colors: None,
        takeover: None,
        created_at: after.fetched_at,
    })
}

/// Applies the takeover policy: big plays by non-favorites (or all, when
/// takeovers are off) are downgraded to a ticker flash. A play by one of my
/// fantasy starters counts as a favorite's.
fn apply_takeover_policy(
    mut alert: Alert,
    game: &Game,
    side: Option<HomeAway>,
    settings: &Settings,
    my_starter: bool,
) -> Alert {
    if alert.level != AlertLevel::Takeover {
        return alert;
    }
    let allowed = match settings.takeovers {
        TakeoverPolicy::All => true,
        TakeoverPolicy::Off => false,
        TakeoverPolicy::Favorites => {
            my_starter || side.is_some_and(|s| settings.is_favorite(&game.competitor(s).team.id))
        }
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
        let art = db.as_ref().and_then(|d| d.team_art().ok()).unwrap_or_default();
        let (content, _) =
            watch::channel(Arc::new(content::build_with(&store, &format_options(&settings), &settings, &art)));
        let (display, _) = watch::channel(Arc::new(display_state(&settings)));
        let (alerts, _) = broadcast::channel(64);
        let tokens = db.as_ref().and_then(|d| d.feed_tokens().ok()).unwrap_or_default().into_iter().collect();
        let (logos, _) = watch::channel(Arc::new(logo_set(&art)));
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
            fantasy_provider: Mutex::new(None),
            pollers: Mutex::default(),
            quiet_hours_ticker: Mutex::default(),
            slow_poller: Mutex::default(),
            weather_provider: Mutex::default(),
            wake: Notify::new(),
            feed_tokens: Mutex::new(tokens),
            feed_alerts: Mutex::default(),
            team_art: Mutex::new(art),
            logos,
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

    pub fn subscribe_logos(&self) -> watch::Receiver<Arc<BTreeMap<String, Image>>> {
        self.logos.subscribe()
    }

    /// Team colors and logos people added.
    pub fn team_art(&self) -> TeamArtMap {
        lock(&self.team_art).clone()
    }

    /// Adds or replaces art for teams (saving it), then updates displays.
    /// Refused when it would go past [`MAX_TEAM_ART`] teams.
    pub fn set_team_art(&self, entries: Vec<(TeamId, TeamArt)>) -> Result<(), String> {
        {
            let art = lock(&self.team_art);
            let new = entries.iter().filter(|(t, _)| !art.contains_key(t)).count();
            if art.len() + new > MAX_TEAM_ART {
                return Err(format!(
                    "Marqueet keeps colors and logos for up to {MAX_TEAM_ART} teams; that would make {}. \
                     Remove some first.",
                    art.len() + new
                ));
            }
        }
        if let Some(db) = &self.db {
            for (team, art) in &entries {
                db.set_team_art(team, art).map_err(|e| format!("couldn't save: {e}"))?;
            }
        }
        lock(&self.team_art).extend(entries);
        self.team_art_changed();
        Ok(())
    }

    /// Forgets a team's art.
    pub fn remove_team_art(&self, team: &TeamId) -> Result<(), String> {
        if let Some(db) = &self.db {
            db.remove_team_art(team).map_err(|e| format!("couldn't save: {e}"))?;
        }
        lock(&self.team_art).remove(team);
        self.team_art_changed();
        Ok(())
    }

    fn team_art_changed(&self) {
        let set = logo_set(&lock(&self.team_art));
        self.logos.send_if_modified(|current| {
            let changed = **current != set;
            *current = Arc::new(set);
            changed
        });
        let store = self.store();
        self.publish(&store);
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
        let art = lock(&self.team_art).clone();
        let next = content::build_with(store, &format_options(&settings), &settings, &art);
        self.content.send_if_modified(|current| {
            // Only what the display shows counts as a change; a fresh
            // `updated_at` alone is stored without waking subscribers.
            let same_view = current.ticker == next.ticker
                && current.crawl == next.crawl
                && current.crawl_label == next.crawl_label
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
        if let Some(zone) = &settings.time_zone {
            tz::validate(zone)?;
        }
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
            let keep: Vec<_> = settings.fantasy.iter().map(|f| (f.league_id.clone(), f.roster_id)).collect();
            store.retain_fantasy(&keep);
            self.publish(&store);
        }
        self.refresh_display();
        self.sync_pollers();
        self.wake.notify_one();
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
        let matchups: Vec<Matchup> = settings
            .fantasy
            .iter()
            .filter_map(|f| store.fantasy(&(f.league_id.clone(), f.roster_id))?.matchup.clone())
            .collect();
        // Alerts use people's own team colors too.
        let art = lock(&self.team_art).clone();
        let mut shown = games.clone();
        team_art::recolor(&mut shown, &art);
        let found: Vec<Alert> = prev
            .map(|mut prev| {
                team_art::recolor(&mut prev, &art);
                events::detect_all(&prev, &shown)
            })
            .unwrap_or_default()
            .iter()
            .filter_map(|(event, game)| {
                let mut alert = events::alert(event, game, now)?;
                let athletes = game.last_play.as_ref().map_or(&[][..], |p| p.athletes.as_slice());
                let note = fantasy::takeover_note(&matchups, athletes);
                let mine = note.as_ref().is_some_and(|(_, _, mine)| *mine);
                if let (Some(t), Some((label, value, _))) = (alert.takeover.as_mut(), note) {
                    t.note = Some((label, value));
                }
                Some(apply_takeover_policy(alert, game, event.side, &settings, mine))
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
    pub fn start(self: &Arc<Self>, providers: Providers) {
        *lock(&self.provider) = Some(Arc::clone(&providers.scores));
        lock(&self.weather_provider).clone_from(&providers.weather);
        lock(&self.fantasy_provider).clone_from(&providers.fantasy);
        self.sync_pollers();
        let hub = Arc::clone(self);
        let ticker = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(20)).await;
                hub.refresh_display();
                hub.prune_feeds();
            }
        });
        *lock(&self.quiet_hours_ticker) = Some(ticker);
        let hub = Arc::clone(self);
        *lock(&self.slow_poller) = Some(tokio::spawn(async move { hub.poll_slow(providers).await }));
    }

    /// Creates a feed and returns its API token.
    pub fn create_feed(&self, name: &str) -> Result<String, String> {
        if !feeds::valid_name(name) {
            return Err(format!("feed name {name:?}: use 1-32 of a-z, 0-9, - and _"));
        }
        let mut tokens = lock(&self.feed_tokens);
        if tokens.contains_key(name) {
            return Err(format!("a feed called {name:?} already exists"));
        }
        if tokens.len() >= MAX_FEEDS {
            return Err(format!("at most {MAX_FEEDS} feeds"));
        }
        let token = new_token().ok_or("couldn't generate a token")?;
        if let Some(db) = &self.db {
            db.set_feed_token(name, &token).map_err(|e| e.to_string())?;
        }
        tokens.insert(name.to_owned(), token.clone());
        log::info!("feed {name}: created");
        Ok(token)
    }

    /// Deletes a feed, its token and its content.
    pub fn revoke_feed(&self, name: &str) -> Result<(), String> {
        if lock(&self.feed_tokens).remove(name).is_none() {
            return Err(format!("no feed called {name:?}"));
        }
        if let Some(db) = &self.db {
            db.remove_feed_token(name).map_err(|e| e.to_string())?;
        }
        self.clear_feed(name);
        log::info!("feed {name}: revoked");
        Ok(())
    }

    /// Feed names with their tokens, for the admin page.
    pub fn feed_tokens(&self) -> Vec<(String, String)> {
        let mut out: Vec<_> = lock(&self.feed_tokens).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        out.sort();
        out
    }

    /// True when `token` is `name`'s token.
    pub fn feed_authorized(&self, name: &str, token: &str) -> bool {
        lock(&self.feed_tokens).get(name).is_some_and(|t| ct_eq(t.as_bytes(), token.as_bytes()))
    }

    pub fn feeds(&self) -> Vec<FeedInfo> {
        let now = Utc::now();
        let store = self.store();
        self.feed_tokens()
            .into_iter()
            .map(|(name, _)| {
                let live = store.custom_feed(&name).filter(|f| f.expires_at > now);
                FeedInfo {
                    segments: live.map_or(0, |f| f.segments.len()),
                    crawl: live.map_or(0, |f| f.crawl.len()),
                    expires_at: live.map(|f| f.expires_at),
                    name,
                }
            })
            .collect()
    }

    /// Replaces a feed's content.
    pub fn post_feed(&self, name: &str, post: &FeedPost) -> Result<FeedInfo, String> {
        let feed = feeds::accept(name, post, Utc::now())?;
        let info = FeedInfo {
            name: name.to_owned(),
            segments: feed.segments.len(),
            crawl: feed.crawl.len(),
            expires_at: Some(feed.expires_at),
        };
        let mut store = self.store();
        store.set_custom_feed(feed);
        self.publish(&store);
        Ok(info)
    }

    pub fn clear_feed(&self, name: &str) {
        let mut store = self.store();
        if store.remove_custom_feed(name) {
            self.publish(&store);
        }
    }

    fn prune_feeds(&self) {
        let mut store = self.store();
        if store.prune_custom_feeds(Utc::now()) {
            self.publish(&store);
        }
    }

    /// Sends a flash or takeover from a feed, within its rate limits. With
    /// takeovers turned off, a takeover is shown as a flash.
    pub fn feed_alert(&self, name: &str, post: &AlertPost) -> Result<Alert, FeedAlertError> {
        let now = Utc::now();
        let feed = self.store().custom_feed(name).filter(|f| f.expires_at > now).cloned();
        let settings = self.settings();
        let mut alert = feeds::alert(name, post, feed.as_ref(), now).map_err(FeedAlertError::Invalid)?;
        if settings.takeovers == TakeoverPolicy::Off {
            alert.level = AlertLevel::Flash;
            alert.takeover = None;
        }
        {
            let mut limits = lock(&self.feed_alerts);
            let (last, last_takeover) = limits.get(name).copied().unwrap_or((now - FEED_TAKEOVER_GAP, None));
            let wait = |since: DateTime<Utc>, gap: chrono::TimeDelta| (since + gap - now).num_seconds().max(1);
            if now - last < FEED_ALERT_GAP {
                return Err(FeedAlertError::TooSoon(wait(last, FEED_ALERT_GAP)));
            }
            let takeover = alert.level == AlertLevel::Takeover;
            if takeover && let Some(t) = last_takeover.filter(|t| now - *t < FEED_TAKEOVER_GAP) {
                return Err(FeedAlertError::TooSoon(wait(t, FEED_TAKEOVER_GAP)));
            }
            limits.insert(name.to_owned(), (now, if takeover { Some(now) } else { last_takeover }));
        }
        self.send_alert(alert.clone(), &settings);
        Ok(alert)
    }

    /// Fetches the followed fantasy teams' matchups that are due: every 30 s
    /// while NFL games are live, else every 10 minutes.
    async fn poll_fantasy(&self, provider: &dyn FantasyProvider, settings: &Settings) {
        let minutes = |d: Duration| chrono::Duration::from_std(d).unwrap_or(chrono::Duration::minutes(10));
        for f in &settings.fantasy {
            let key = (f.league_id.clone(), f.roster_id);
            let every = if self.store().nfl_live() { self.policy.fantasy_live } else { self.policy.fantasy_idle };
            if !self.store().fantasy_due(&key, Utc::now(), minutes(every), minutes(self.policy.fantasy_retry)) {
                continue;
            }
            let result = provider.matchup(&f.league_id, f.roster_id).await;
            match &result {
                Ok(m) => log::debug!(
                    "fantasy {}: {:.1} - {:.1}",
                    f.team,
                    m.me.points,
                    m.opponent.as_ref().map_or(0.0, |o| o.points)
                ),
                Err(e) => log::warn!("fantasy {} ({}): {e}", f.team, f.league),
            }
            let flash = {
                let mut store = self.store();
                let before = store.fantasy(&key).and_then(|f| f.matchup.clone());
                store.record_fantasy(key.clone(), result, Utc::now());
                self.publish(&store);
                let after = store.fantasy(&key).and_then(|f| f.matchup.clone());
                lead_change(before.as_ref(), after.as_ref())
            };
            if let Some(alert) = flash {
                self.send_alert(alert, settings);
            }
        }
    }

    /// Looks up a fantasy account and its leagues' teams, for the admin page.
    pub async fn find_fantasy(&self, username: &str) -> Result<FantasySearch, String> {
        let provider = lock(&self.fantasy_provider).clone().ok_or("fantasy isn't available on this server")?;
        let user = provider.find_user(username).await.map_err(|e| e.to_string())?;
        let mut leagues = Vec::new();
        for league in provider.leagues(&user.id).await.map_err(|e| e.to_string())?.into_iter().take(12) {
            let teams = provider.teams(&league.id).await.map_err(|e| e.to_string())?;
            leagues.push((league, teams));
        }
        Ok(FantasySearch { user, leagues })
    }

    /// Follows a fantasy team (checked against the provider).
    pub async fn add_fantasy(
        self: &Arc<Self>,
        league_id: &str,
        league_name: &str,
        roster_id: u32,
    ) -> Result<(), String> {
        let provider = lock(&self.fantasy_provider).clone().ok_or("fantasy isn't available on this server")?;
        let teams = provider.teams(league_id).await.map_err(|e| e.to_string())?;
        let team = teams.into_iter().find(|t| t.roster_id == roster_id).ok_or("that team isn't in the league")?;
        let mut settings = self.settings();
        if settings.fantasy.iter().any(|f| f.league_id == league_id && f.roster_id == roster_id) {
            return Err(format!("already following {}", team.name));
        }
        if settings.fantasy.len() >= MAX_FANTASY {
            return Err(format!("at most {MAX_FANTASY} fantasy teams"));
        }
        settings.fantasy.push(FantasyLeague {
            provider: provider.id().into(),
            league_id: league_id.into(),
            roster_id,
            league: league_name.trim().chars().take(60).collect(),
            team: team.name,
        });
        self.apply_settings(settings).map(|_| ())
    }

    pub fn remove_fantasy(self: &Arc<Self>, league_id: &str, roster_id: u32) -> Result<(), String> {
        let mut settings = self.settings();
        let before = settings.fantasy.len();
        settings.fantasy.retain(|f| !(f.league_id == league_id && f.roster_id == roster_id));
        if settings.fantasy.len() == before {
            return Err("not following that team".into());
        }
        self.apply_settings(settings).map(|_| ())
    }

    /// Sends a non-game alert (weather, feeds) to displays once. With
    /// takeovers off it's shown as a flash.
    fn send_alert(&self, mut alert: Alert, settings: &Settings) -> bool {
        if settings.takeovers == TakeoverPolicy::Off {
            alert.level = AlertLevel::Flash;
            alert.takeover = None;
        }
        let shared = Arc::new(alert);
        if !lock(&self.history).insert(&shared) {
            return false;
        }
        log::info!("{}: {} ({})", shared.source, shared.title, shared.detail.as_deref().unwrap_or(""));
        let _ = self.alerts.send(shared);
        true
    }

    /// Places matching `query`, for the admin page's location field.
    pub async fn search_places(&self, query: &str) -> Result<Vec<Place>, String> {
        let provider = lock(&self.weather_provider).clone().ok_or("weather isn't available on this server")?;
        provider.search(query).await.map_err(|e| e.to_string())
    }

    /// Fetches standings and weather when they're due, forever. Wakes early
    /// when settings change.
    async fn poll_slow(self: Arc<Self>, providers: Providers) {
        let minutes =
            |d: Duration, fallback| chrono::Duration::from_std(d).unwrap_or(chrono::Duration::minutes(fallback));
        let (every, retry) = (minutes(self.policy.standings_every, 30), minutes(self.policy.standings_retry, 10));
        let (wx_every, wx_retry) = (minutes(self.policy.weather_every, 15), minutes(self.policy.weather_retry, 5));
        let alerts_every = minutes(self.policy.weather_alerts_every, 2);
        let provider = providers.scores;
        loop {
            let settings = self.settings();
            if let (Some(weather), Some(place)) = (&providers.weather, &settings.weather.place)
                && settings.wants_weather()
                && self.store().weather_due(place, settings.weather.units, Utc::now(), wx_every, wx_retry)
            {
                let result = weather.forecast(place, settings.weather.units).await;
                if let Err(e) = &result {
                    log::warn!("weather for {}: {e}", place.name);
                }
                let mut store = self.store();
                store.record_weather(result, Utc::now());
                self.publish(&store);
            }
            if let (Some(nws), Some(place)) = (&providers.weather_alerts, &settings.weather.place)
                && settings.weather.alerts
                && self.store().weather_alerts_due(place, Utc::now(), alerts_every)
            {
                let result = nws.active(place).await;
                match &result {
                    Ok(list) => log::debug!("weather alerts for {}: {}", place.name, list.len()),
                    Err(ProviderError::Unsupported(e)) => log::info!("{}: no {e}", place.name),
                    Err(e) => log::warn!("weather alerts for {}: {e}", place.name),
                }
                let new = {
                    let mut store = self.store();
                    let new = store.record_weather_alerts(place, result, Utc::now());
                    self.publish(&store);
                    new
                };
                let tz = format_options(&settings).tz;
                for alert in new.iter().filter_map(|a| a.to_alert(tz, Utc::now())) {
                    self.send_alert(alert, &settings);
                }
            }
            // Bound first: the store lock mustn't be held across the fetch.
            let teams_due = self.store().teams_due(Utc::now(), chrono::Duration::hours(24), retry);
            for league in teams_due {
                let result = provider.teams(&league).await;
                if let Err(e) = &result
                    && !matches!(e, ProviderError::Unsupported(_))
                {
                    log::warn!("{league}: teams: {e}");
                }
                self.store().record_teams(&league, result, Utc::now());
            }
            if let Some(fantasy) = &providers.fantasy {
                self.poll_fantasy(fantasy.as_ref(), &settings).await;
            }
            let due = self.store().standings_due(Utc::now(), every, retry);
            for league in due {
                let result = provider.standings(&league).await;
                match &result {
                    Ok(s) => log::debug!("{league}: standings, {} groups", s.groups.len()),
                    Err(ProviderError::Unsupported(_)) => log::info!("{league}: no standings from this provider"),
                    Err(e) => log::warn!("{league}: standings: {e}"),
                }
                let mut store = self.store();
                store.record_standings(&league, result, Utc::now());
                self.publish(&store);
            }
            tokio::select! {
                () = tokio::time::sleep(self.policy.slow_tick) => {}
                () = self.wake.notified() => {}
            }
        }
    }

    /// Stops all background tasks.
    pub fn stop(&self) {
        for (_, task) in lock(&self.pollers).drain() {
            task.abort();
        }
        if let Some(task) = lock(&self.quiet_hours_ticker).take() {
            task.abort();
        }
        if let Some(task) = lock(&self.slow_poller).take() {
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
mod lead_tests {
    use super::*;

    #[test]
    fn flashes_only_when_the_lead_changes() {
        let m = |me: f32, them: f32| {
            let mut m = fantasy::mock_matchup(Utc::now());
            m.me.points = me;
            m.opponent.as_mut().unwrap().points = them;
            m
        };
        let took = lead_change(Some(&m(80.0, 90.0)), Some(&m(95.0, 90.0))).unwrap();
        assert_eq!(took.title, "HAIL MARY BROS TAKES THE LEAD");
        assert_eq!(took.segment_id.as_deref(), Some("fantasy:1000000000000000001:2"));
        assert!(lead_change(Some(&m(95.0, 90.0)), Some(&m(88.0, 90.0))).unwrap().title.ends_with("LOSES THE LEAD"));
        assert!(lead_change(Some(&m(95.0, 90.0)), Some(&m(99.0, 90.0))).is_none(), "still ahead");
        assert!(lead_change(None, Some(&m(99.0, 90.0))).is_none(), "first fetch");
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

    #[test]
    fn team_art_is_capped() {
        use marqueet_core::team_art::TeamArt;
        let hub = hub();
        let art = |i: usize| (TeamId(format!("t{i}")), TeamArt { label: String::new(), colors: None, logo: None });
        hub.set_team_art((0..MAX_TEAM_ART).map(art).collect()).unwrap();
        let err = hub.set_team_art(vec![art(MAX_TEAM_ART)]).unwrap_err();
        assert!(err.contains("250"), "{err}");
        hub.set_team_art(vec![art(0)]).unwrap();
        assert_eq!(hub.team_art().len(), MAX_TEAM_ART, "replacing one is fine");
        hub.remove_team_art(&TeamId("t0".into())).unwrap();
        hub.set_team_art(vec![art(MAX_TEAM_ART)]).unwrap();
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
    fn my_fantasy_starter_scoring_takes_over_with_a_note() {
        use marqueet_core::sports::{Athlete, Play};
        let m = fantasy::mock_matchup(Utc::now());
        let settings = Settings {
            leagues: vec![nfl()],
            takeovers: TakeoverPolicy::Favorites,
            fantasy: vec![FantasyLeague {
                provider: "sleeper".into(),
                league_id: m.league_id.clone(),
                roster_id: m.me.roster_id,
                league: "Office League".into(),
                team: "Hail Mary Bros".into(),
            }],
            ..Settings::default()
        };
        let hub = hub_with(settings);
        hub.store().record_fantasy((m.league_id.clone(), m.me.roster_id), Ok(m), Utc::now());
        let mut rx = hub.subscribe_alerts();
        let games = nfl_games();
        hub.record_success(&nfl(), games.clone());
        let mut scored = games;
        scored[0].home.score = Some(28);
        scored[0].last_play = Some(Play {
            id: "td".into(),
            text: "Rico Castellano 12 yd run".into(),
            type_text: Some("Rushing Touchdown".into()),
            team: Some(scored[0].home.team.id.clone()),
            score_value: Some(6),
            athletes: vec![Athlete { id: "9000001".into(), name: "Rico Castellano".into() }],
        });
        hub.record_success(&nfl(), scored);
        let alert = rx.try_recv().unwrap();
        assert_eq!(alert.level, AlertLevel::Takeover, "BUF isn't a favorite, but Castellano is my starter");
        let note = alert.takeover.as_ref().unwrap().note.clone();
        assert_eq!(note, Some(("YOUR STARTER".into(), "R. CASTELLANO  24.1 PTS".into())));
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
