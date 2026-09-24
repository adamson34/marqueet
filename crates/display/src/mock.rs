//! A deterministic fake feed: game clocks tick and live games score every few
//! seconds, raising flash alerts. Stands in for the server until Phase 2.

use chrono::{DateTime, Utc};
use marqueet_core::alert::Alert;
use marqueet_core::sports::{Athlete, Game, GameStatus, HomeAway, Play, Sport, fixtures};
use marqueet_core::{events, fantasy};

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
            update.alerts.extend(self.score_random(now));
            update.games_changed = true;
        }
        update
    }

    /// Scores for a random live game.
    fn score_random(&mut self, now: DateTime<Utc>) -> Vec<Alert> {
        let live: Vec<usize> =
            (0..self.games.len()).filter(|&i| self.games[i].status == GameStatus::InProgress).collect();
        let idx = live[self.rng.below(live.len() as u64) as usize];
        let side = if self.rng.below(2) == 0 { HomeAway::Home } else { HomeAway::Away };
        let roll = self.rng.below(100);
        let points = match self.games[idx].sport {
            Sport::Football if roll < 60 => 7,
            Sport::Football => 3,
            Sport::Basketball if roll < 35 => 3,
            Sport::Basketball => 2,
            Sport::Baseball if roll < 30 => 2,
            Sport::Baseball => 1,
            Sport::Hockey | Sport::Soccer => 1,
        };
        let id = self.games[idx].id.0.clone();
        self.score(&id, side, points, now)
    }

    /// Adds `points` for one side of a game with matching sample play text,
    /// then runs the real event engine on the before/after snapshots so the
    /// alerts match what live data produces. Unknown games produce nothing.
    pub fn score(&mut self, game_id: &str, side: HomeAway, points: u16, now: DateTime<Utc>) -> Vec<Alert> {
        let Some(idx) = self.games.iter().position(|g| g.id.0 == game_id) else { return Vec::new() };
        let prev = self.games[idx].clone();
        let tag = self.rng.next();
        let g = &mut self.games[idx];
        let abbr = g.competitor(side).team.abbreviation.clone();
        let (kind, text) = match (g.sport, points) {
            (Sport::Football, 6..) => ("Rushing Touchdown", format!("{abbr} touchdown, 12 yd run")),
            (Sport::Football, 3) => ("Field Goal Good", format!("{abbr} 44 yd field goal")),
            (Sport::Football, _) => ("Score", format!("{abbr} score")),
            (Sport::Basketball, 3) => ("Three Point Jumper", format!("{abbr} three-pointer")),
            (Sport::Basketball, _) => ("Layup", format!("{abbr} layup")),
            (Sport::Baseball, 2..) => ("Home Run", format!("{abbr} home run to left")),
            (Sport::Baseball, _) => ("Single", format!("{abbr} RBI single")),
            (Sport::Hockey | Sport::Soccer, _) => ("Goal", format!("{abbr} goal")),
        };
        let team = g.competitor(side).team.id.clone();
        // Buffalo's scorer is the demo fantasy team's QB, so the mock shows
        // a fantasy note on its takeovers.
        let athletes = if abbr == "BUF" && g.sport == Sport::Football {
            vec![Athlete { id: "3918298".into(), name: "Josh Allen".into() }]
        } else {
            vec![]
        };
        let c = match side {
            HomeAway::Home => &mut g.home,
            HomeAway::Away => &mut g.away,
        };
        c.score = Some(c.score.unwrap_or(0) + points);
        g.last_play = Some(Play {
            id: format!("mock-{tag}"),
            text,
            type_text: Some(kind.into()),
            team: Some(team),
            score_value: u8::try_from(points).ok(),
            athletes,
        });
        let next = g.clone();
        let matchups = [fantasy::mock_matchup(now)];
        let athletes = next.last_play.as_ref().map_or(&[][..], |p| p.athletes.as_slice());
        let note = fantasy::takeover_note(&matchups, athletes);
        events::detect(&prev, &next)
            .iter()
            .filter_map(|e| events::alert(e, &next, now))
            .map(|mut a| {
                if let (Some(t), Some((label, value, _))) = (a.takeover.as_mut(), note.clone()) {
                    t.note = Some((label, value));
                }
                a
            })
            .collect()
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
