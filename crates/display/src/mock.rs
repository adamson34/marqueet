//! A deterministic fake feed: game clocks tick and live games score every few
//! seconds, raising flash alerts. Stands in for the server until Phase 2.

use chrono::{DateTime, Utc};
use marqueet_core::alert::{Alert, AlertLevel};
use marqueet_core::sports::{Game, GameStatus, HomeAway, Sport, fixtures};

/// Small xorshift PRNG; deterministic so screenshots are reproducible.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() % 10_000) as f64 / 10_000.0 * (hi - lo)
    }
}

#[derive(Debug)]
pub struct MockFeed {
    pub games: Vec<Game>,
    /// Wall clock at the last `advance`.
    pub now: DateTime<Utc>,
    rng: Rng,
    elapsed: f64,
    next_score_at: f64,
    next_clock_tick: f64,
}

#[derive(Debug, Default)]
pub struct MockUpdate {
    pub games_changed: bool,
    pub alerts: Vec<Alert>,
}

impl MockFeed {
    pub fn new(now: DateTime<Utc>, seed: u64) -> Self {
        MockFeed {
            games: fixtures::mock_games(now),
            now,
            rng: Rng(seed.max(1)),
            elapsed: 0.0,
            next_score_at: 4.0,
            next_clock_tick: 1.0,
        }
    }

    pub fn advance(&mut self, dt: f64, now: DateTime<Utc>) -> MockUpdate {
        let mut update = MockUpdate::default();
        self.now = now;
        self.elapsed += dt;
        while self.elapsed >= self.next_clock_tick {
            self.next_clock_tick += 1.0;
            let tick = self.next_clock_tick as u64;
            for g in &mut self.games {
                if g.status == GameStatus::InProgress && tick_clock(g, tick) {
                    update.games_changed = true;
                }
            }
        }
        if self.elapsed >= self.next_score_at {
            self.next_score_at = self.elapsed + self.rng.range(6.0, 11.0);
            if let Some(alert) = self.score_random(now) {
                update.games_changed = true;
                update.alerts.push(alert);
            }
        }
        update
    }

    /// Adds points to one side of a game; returns the updated game.
    pub fn add_points(&mut self, game_id: &str, side: HomeAway, points: u16) -> Option<&Game> {
        let g = self.games.iter_mut().find(|g| g.id.0 == game_id)?;
        let c = match side {
            HomeAway::Home => &mut g.home,
            HomeAway::Away => &mut g.away,
        };
        c.score = Some(c.score.unwrap_or(0) + points);
        Some(g)
    }

    fn score_random(&mut self, now: DateTime<Utc>) -> Option<Alert> {
        let live: Vec<usize> =
            (0..self.games.len()).filter(|&i| self.games[i].status == GameStatus::InProgress).collect();
        let idx = live[self.rng.below(live.len() as u64) as usize];
        let side = if self.rng.below(2) == 0 { HomeAway::Home } else { HomeAway::Away };
        let roll = self.rng.below(100);
        let g = &mut self.games[idx];
        let (points, title) = match g.sport {
            Sport::Football if roll < 60 => (7, "TOUCHDOWN"),
            Sport::Football => (3, "FIELD GOAL"),
            Sport::Basketball if roll < 35 => (3, "THREE"),
            Sport::Basketball => (2, "BASKET"),
            Sport::Baseball if roll < 30 => (2, "HOME RUN"),
            Sport::Baseball => (1, "RUN SCORES"),
            Sport::Hockey | Sport::Soccer => (1, "GOAL"),
        };
        let c = match side {
            HomeAway::Home => &mut g.home,
            HomeAway::Away => &mut g.away,
        };
        c.score = Some(c.score.unwrap_or(0) + points);
        let colors = (c.team.colors.primary, c.team.colors.secondary.unwrap_or(c.team.colors.primary));
        let detail = format!(
            "{} {} - {} {}",
            g.away.team.abbreviation,
            g.away.score.unwrap_or(0),
            g.home.team.abbreviation,
            g.home.score.unwrap_or(0)
        );
        Some(Alert {
            id: format!("{}:{title}:{detail}", g.id.0),
            level: AlertLevel::Flash,
            source: "mock".into(),
            segment_id: Some(g.id.0.clone()),
            title: title.into(),
            detail: Some(detail),
            colors: Some(colors),
            created_at: now,
        })
    }
}

/// Counts a "M:SS" clock down by a second, or a soccer "67'" clock up by a
/// minute every 6 ticks. Returns whether anything changed.
fn tick_clock(g: &mut Game, tick: u64) -> bool {
    let Some(clock) = g.clock.clock.as_mut() else { return false };
    if let Some(min) = clock.strip_suffix('\'') {
        if !tick.is_multiple_of(6) {
            return false;
        }
        let Ok(m) = min.parse::<u32>() else { return false };
        *clock = format!("{}'", (m + 1).min(90));
        return true;
    }
    let Some((m, s)) = clock.split_once(':') else { return false };
    let (Ok(m), Ok(s)) = (m.parse::<u32>(), s.parse::<u32>()) else { return false };
    let total = (m * 60 + s).saturating_sub(1);
    if total == 0 {
        return false;
    }
    *clock = format!("{}:{:02}", total / 60, total % 60);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock_of(feed: &MockFeed, id: &str) -> Option<String> {
        feed.games.iter().find(|g| g.id.0 == id).and_then(|g| g.clock.clock.clone())
    }

    #[test]
    fn clocks_tick_down_each_second() {
        let now = Utc::now();
        let mut feed = MockFeed::new(now, 1);
        assert_eq!(clock_of(&feed, "mock:nfl:1").as_deref(), Some("4:32"));
        let u = feed.advance(1.0, now);
        assert!(u.games_changed);
        assert_eq!(clock_of(&feed, "mock:nfl:1").as_deref(), Some("4:31"));
    }

    #[test]
    fn scores_raise_flash_alerts_for_live_games() {
        let now = Utc::now();
        let mut feed = MockFeed::new(now, 42);
        let before: u32 =
            feed.games.iter().map(|g| u32::from(g.home.score.unwrap_or(0) + g.away.score.unwrap_or(0))).sum();
        let mut alerts = Vec::new();
        for _ in 0..300 {
            alerts.extend(feed.advance(0.1, now).alerts);
        }
        assert!(!alerts.is_empty(), "30s of mock play should produce a score");
        let after: u32 =
            feed.games.iter().map(|g| u32::from(g.home.score.unwrap_or(0) + g.away.score.unwrap_or(0))).sum();
        assert!(after > before);
        for a in &alerts {
            let id = a.segment_id.as_deref().unwrap();
            let g = feed.games.iter().find(|g| g.id.0 == id).unwrap();
            assert_eq!(g.status, GameStatus::InProgress);
        }
    }

    #[test]
    fn same_seed_same_story() {
        let now = Utc::now();
        let run = |seed| {
            let mut f = MockFeed::new(now, seed);
            (0..200).flat_map(|_| f.advance(0.1, now).alerts).map(|a| a.id).collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7));
    }

    #[test]
    fn soccer_minute_advances_slowly() {
        let now = Utc::now();
        let mut feed = MockFeed::new(now, 1);
        for _ in 0..6 {
            feed.advance(1.0, now);
        }
        assert_eq!(clock_of(&feed, "mock:epl:1").as_deref(), Some("68'"));
    }
}
