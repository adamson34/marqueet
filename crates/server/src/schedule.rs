//! How often to poll a league. Pure: decided from the games we last saw.
//!
//! Polling is polite by design: about every 12 s only while games are live,
//! once a minute when a game is about to start, every few minutes on a game
//! day, and rarely otherwise. Failures back off exponentially.

use std::time::Duration;

use chrono::{DateTime, Utc};
use marqueet_core::sports::{Game, GameStatus};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Policy {
    /// While any game is live.
    pub live: Duration,
    /// When a game starts within `soon_window` (or should have started).
    pub soon: Duration,
    pub soon_window: chrono::Duration,
    /// When a game starts later today (within `today_window`).
    pub today: Duration,
    pub today_window: chrono::Duration,
    /// Nothing happening.
    pub idle: Duration,
    /// First retry after a failure; doubles each time, up to `backoff_max`.
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// Consecutive failures before a league's data is marked stale.
    pub stale_after_failures: u32,
    /// Standings refresh interval (they only change when games end).
    pub standings_every: Duration,
    /// Standings retry after a failed fetch.
    pub standings_retry: Duration,
    /// Weather refresh interval, and retry after a failure.
    pub weather_every: Duration,
    pub weather_retry: Duration,
    /// Severe weather alerts refresh interval.
    pub weather_alerts_every: Duration,
    /// Fantasy matchups: while NFL games are live, otherwise, and after a
    /// failure.
    pub fantasy_live: Duration,
    pub fantasy_idle: Duration,
    pub fantasy_retry: Duration,
    /// How often the slow poller (standings, weather, fantasy) looks for work.
    pub slow_tick: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            live: Duration::from_secs(12),
            soon: Duration::from_secs(60),
            soon_window: chrono::Duration::minutes(30),
            today: Duration::from_secs(5 * 60),
            today_window: chrono::Duration::hours(12),
            idle: Duration::from_secs(20 * 60),
            backoff_base: Duration::from_secs(15),
            backoff_max: Duration::from_secs(5 * 60),
            stale_after_failures: 3,
            standings_every: Duration::from_secs(30 * 60),
            standings_retry: Duration::from_secs(10 * 60),
            weather_every: Duration::from_secs(15 * 60),
            weather_retry: Duration::from_secs(5 * 60),
            weather_alerts_every: Duration::from_secs(2 * 60),
            fantasy_live: Duration::from_secs(30),
            fantasy_idle: Duration::from_secs(10 * 60),
            fantasy_retry: Duration::from_secs(2 * 60),
            slow_tick: Duration::from_secs(15),
        }
    }
}

/// Delay before the next successful-path poll.
pub fn next_poll(games: &[Game], now: DateTime<Utc>, p: &Policy) -> Duration {
    if games.iter().any(|g| g.status.is_live()) {
        return p.live;
    }
    let until_start = |g: &Game| (g.status == GameStatus::Scheduled).then(|| g.start_time - now);
    let starts: Vec<chrono::Duration> = games.iter().filter_map(until_start).collect();
    if starts.iter().any(|d| *d <= p.soon_window) {
        // Includes games past their start time that haven't flipped to live yet.
        p.soon
    } else if starts.iter().any(|d| *d <= p.today_window) {
        p.today
    } else {
        p.idle
    }
}

/// Delay after `failures` consecutive failures (1 = first failure).
pub fn backoff(failures: u32, p: &Policy) -> Duration {
    let exp = failures.saturating_sub(1).min(16);
    p.backoff_base.saturating_mul(1 << exp).min(p.backoff_max)
}

/// Spreads polls out by ±10% so leagues don't hit the API in lockstep.
/// `unit` is a random number in `[0, 1)`.
pub fn jittered(d: Duration, unit: f64) -> Duration {
    d.mul_f64(0.9 + 0.2 * unit.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use marqueet_core::sports::fixtures::mock_games;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap()
    }

    fn only(status: GameStatus, starts_in: chrono::Duration) -> Vec<Game> {
        let mut g = mock_games(now()).remove(0);
        g.status = status;
        g.start_time = now() + starts_in;
        vec![g]
    }

    #[test]
    fn live_games_poll_fast() {
        let p = Policy::default();
        assert_eq!(next_poll(&mock_games(now()), now(), &p), p.live);
        assert_eq!(next_poll(&only(GameStatus::Halftime, chrono::Duration::zero()), now(), &p), p.live);
    }

    #[test]
    fn upcoming_games_poll_by_proximity() {
        let p = Policy::default();
        let m = chrono::Duration::minutes;
        assert_eq!(next_poll(&only(GameStatus::Scheduled, m(10)), now(), &p), p.soon);
        assert_eq!(next_poll(&only(GameStatus::Scheduled, m(-5)), now(), &p), p.soon, "late to flip live");
        assert_eq!(next_poll(&only(GameStatus::Scheduled, m(180)), now(), &p), p.today);
        assert_eq!(next_poll(&only(GameStatus::Scheduled, m(60 * 30)), now(), &p), p.idle);
    }

    #[test]
    fn finals_and_empty_days_idle() {
        let p = Policy::default();
        assert_eq!(next_poll(&only(GameStatus::Final, chrono::Duration::hours(-3)), now(), &p), p.idle);
        assert_eq!(next_poll(&[], now(), &p), p.idle);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let p = Policy::default();
        let secs = |f| backoff(f, &p).as_secs();
        assert_eq!([secs(1), secs(2), secs(3), secs(4), secs(5), secs(6)], [15, 30, 60, 120, 240, 300]);
        assert_eq!(secs(1000), 300);
    }

    #[test]
    fn jitter_stays_within_ten_percent() {
        let d = Duration::from_secs(100);
        assert_eq!(jittered(d, 0.0), Duration::from_secs(90));
        assert_eq!(jittered(d, 0.5), Duration::from_secs(100));
        assert!(jittered(d, 0.999) < Duration::from_secs(110));
    }
}
