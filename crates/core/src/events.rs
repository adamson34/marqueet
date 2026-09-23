//! Event engine: compares two snapshots of the same game and reports what
//! happened in between. Pure.
//!
//! Providers poll every ~12 s, so one snapshot gap can hide several plays (a
//! touchdown and its extra point usually arrive together as +7). The engine
//! therefore classifies each team's score change using the provider's last
//! play when it names a scoring play, and the size of the change otherwise.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::alert::{Alert, AlertLevel, ScoreLine, Takeover};
use crate::sports::{Game, GameStatus, HomeAway, Sport};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Touchdown,
    FieldGoal,
    Safety,
    ExtraPoint,
    TwoPoint,
    HomeRun,
    GrandSlam,
    /// Baseball runs that weren't a home run.
    Runs,
    Goal,
    /// A score change we can't classify more precisely.
    Score,
    /// A score went down (review, stat correction). Never alerted.
    ScoreCorrection,
    Final,
}

impl EventKind {
    fn slug(self) -> &'static str {
        match self {
            EventKind::Touchdown => "touchdown",
            EventKind::FieldGoal => "field_goal",
            EventKind::Safety => "safety",
            EventKind::ExtraPoint => "extra_point",
            EventKind::TwoPoint => "two_point",
            EventKind::HomeRun => "home_run",
            EventKind::GrandSlam => "grand_slam",
            EventKind::Runs => "runs",
            EventKind::Goal => "goal",
            EventKind::Score => "score",
            EventKind::ScoreCorrection => "correction",
            EventKind::Final => "final",
        }
    }

    /// Big plays take over the widget area; everything else flashes the ticker.
    pub fn level(self) -> Option<AlertLevel> {
        match self {
            EventKind::Touchdown | EventKind::HomeRun | EventKind::GrandSlam | EventKind::Goal => {
                Some(AlertLevel::Takeover)
            }
            EventKind::FieldGoal
            | EventKind::Safety
            | EventKind::ExtraPoint
            | EventKind::TwoPoint
            | EventKind::Runs
            | EventKind::Score
            | EventKind::Final => Some(AlertLevel::Flash),
            EventKind::ScoreCorrection => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEvent {
    /// Deterministic: `<game id>:<kind>:<away>-<home>`. The same moment seen
    /// twice (or after a restart) produces the same id, so it can be deduped.
    pub id: String,
    pub kind: EventKind,
    /// Scoring side, when the event belongs to one team.
    pub side: Option<HomeAway>,
    pub points: i32,
    /// (away, home) after the event.
    pub score: (u16, u16),
}

fn event_id(game: &Game, kind: EventKind, score: (u16, u16)) -> String {
    format!("{}:{}:{}-{}", game.id.0, kind.slug(), score.0, score.1)
}

fn play_says(game: &Game, words: &[&str]) -> bool {
    game.last_play.as_ref().is_some_and(|p| {
        let hay = format!("{} {}", p.type_text.as_deref().unwrap_or(""), p.text).to_lowercase();
        words.iter().any(|w| hay.contains(w))
    })
}

fn classify(sport: Sport, next: &Game, side: HomeAway, delta: i32) -> EventKind {
    if delta < 0 {
        return EventKind::ScoreCorrection;
    }
    // Prefer the play text when the last play belongs to the scoring team.
    let scorer = &next.competitor(side).team.id;
    let play_is_theirs = next.last_play.as_ref().and_then(|p| p.team.as_ref()).is_none_or(|t| t == scorer);
    match sport {
        Sport::Football => {
            if play_is_theirs {
                if play_says(next, &["touchdown"]) && delta >= 6 {
                    return EventKind::Touchdown;
                }
                if play_says(next, &["field goal"]) && delta == 3 {
                    return EventKind::FieldGoal;
                }
            }
            match delta {
                6..=8 => EventKind::Touchdown,
                3 => EventKind::FieldGoal,
                2 if play_says(next, &["two-point", "2pt", "two point"]) => EventKind::TwoPoint,
                2 => EventKind::Safety,
                1 => EventKind::ExtraPoint,
                d if d >= 9 => EventKind::Touchdown,
                _ => EventKind::Score,
            }
        }
        Sport::Baseball => {
            if play_is_theirs && play_says(next, &["home run", "homer", "grand slam"]) {
                if delta == 4 { EventKind::GrandSlam } else { EventKind::HomeRun }
            } else {
                EventKind::Runs
            }
        }
        Sport::Hockey | Sport::Soccer => EventKind::Goal,
        Sport::Basketball => EventKind::Score,
    }
}

/// Events between two snapshots of one game. Returns nothing unless both
/// snapshots are fresh and describe the same game.
pub fn detect(prev: &Game, next: &Game) -> Vec<GameEvent> {
    if prev.id != next.id || prev.stale || next.stale {
        return Vec::new();
    }
    let mut out = Vec::new();
    let score = (next.away.score.unwrap_or(0), next.home.score.unwrap_or(0));
    // Basketball scores change constantly; only its final is worth an alert.
    if next.sport != Sport::Basketball {
        for side in [HomeAway::Away, HomeAway::Home] {
            let (Some(before), Some(after)) = (prev.competitor(side).score, next.competitor(side).score) else {
                continue;
            };
            let delta = i32::from(after) - i32::from(before);
            if delta == 0 {
                continue;
            }
            let kind = classify(next.sport, next, side, delta);
            out.push(GameEvent { id: event_id(next, kind, score), kind, side: Some(side), points: delta, score });
        }
    }
    if prev.status != GameStatus::Final && next.status == GameStatus::Final && prev.status.is_live() {
        out.push(GameEvent {
            id: event_id(next, EventKind::Final, score),
            kind: EventKind::Final,
            side: None,
            points: 0,
            score,
        });
    }
    out
}

/// Events across two scoreboards, matching games by id. New games (no
/// previous snapshot) produce nothing.
pub fn detect_all(prev: &[Game], next: &[Game]) -> Vec<(GameEvent, Game)> {
    next.iter()
        .filter_map(|n| prev.iter().find(|p| p.id == n.id).map(|p| (p, n)))
        .flat_map(|(p, n)| detect(p, n).into_iter().map(move |e| (e, n.clone())))
        .collect()
}

fn headline(kind: EventKind, points: i32) -> String {
    match kind {
        EventKind::Touchdown => "TOUCHDOWN".into(),
        EventKind::FieldGoal => "FIELD GOAL".into(),
        EventKind::Safety => "SAFETY".into(),
        EventKind::ExtraPoint => "EXTRA POINT".into(),
        EventKind::TwoPoint => "TWO-POINT".into(),
        EventKind::HomeRun => "HOME RUN".into(),
        EventKind::GrandSlam => "GRAND SLAM".into(),
        EventKind::Runs if points == 1 => "RUN SCORES".into(),
        EventKind::Runs => format!("{points} RUNS SCORE"),
        EventKind::Goal => "GOAL".into(),
        EventKind::Score | EventKind::ScoreCorrection => "SCORE".into(),
        EventKind::Final => "FINAL".into(),
    }
}

/// Builds the alert for an event, or `None` for events that shouldn't alert.
pub fn alert(event: &GameEvent, game: &Game, now: DateTime<Utc>) -> Option<Alert> {
    let level = event.kind.level()?;
    let (away, home) = (&game.away.team, &game.home.team);
    let detail = format!("{} {}  {} {}", away.abbreviation, event.score.0, home.abbreviation, event.score.1);
    let scorer = event.side.map(|s| game.competitor(s));
    let colors = scorer.map(|c| (c.team.colors.primary, c.team.colors.secondary.unwrap_or(c.team.colors.primary)));
    let title = headline(event.kind, event.points);
    let takeover = (level == AlertLevel::Takeover).then(|| {
        let clock = match (&game.clock.clock, game.clock.period_label.as_str()) {
            (Some(c), label) if !label.is_empty() => format!("{label} {c}"),
            (None, label) if !label.is_empty() => label.to_owned(),
            _ => String::new(),
        };
        let team = scorer.map(|c| c.team.display_name.to_uppercase()).unwrap_or_default();
        let kicker = if clock.is_empty() { team } else { format!("{team} · {}", clock.to_uppercase()) };
        let play_team = game.last_play.as_ref().and_then(|p| p.team.as_ref());
        let play = game
            .last_play
            .as_ref()
            .filter(|_| scorer.is_some_and(|c| play_team.is_none_or(|t| *t == c.team.id)))
            .map(|p| p.text.clone())
            .filter(|t| !t.is_empty());
        Takeover {
            kicker,
            headline: title.clone(),
            play,
            score: ScoreLine {
                away: (away.abbreviation.clone(), event.score.0),
                home: (home.abbreviation.clone(), event.score.1),
                scoring_home: event.side == Some(HomeAway::Home),
            },
            note: None,
        }
    });
    Some(Alert {
        id: event.id.clone(),
        level,
        source: "sports".into(),
        segment_id: Some(game.id.0.clone()),
        title,
        detail: Some(detail),
        colors,
        takeover,
        created_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_games;
    use crate::sports::{Play, TeamId};

    fn game(id: &str) -> Game {
        mock_games(Utc::now()).into_iter().find(|g| g.id.0 == id).unwrap()
    }

    fn with_scores(mut g: Game, away: u16, home: u16) -> Game {
        g.away.score = Some(away);
        g.home.score = Some(home);
        g
    }

    fn play(g: &mut Game, team: &TeamId, type_text: &str, text: &str) {
        g.last_play = Some(Play {
            id: "p".into(),
            text: text.into(),
            type_text: Some(type_text.into()),
            team: Some(team.clone()),
            score_value: None,
            athletes: vec![],
        });
    }

    fn kinds(prev: &Game, next: &Game) -> Vec<(EventKind, Option<HomeAway>, i32)> {
        detect(prev, next).into_iter().map(|e| (e.kind, e.side, e.points)).collect()
    }

    #[test]
    fn football_by_score_delta() {
        let g = game("mock:nfl:1"); // KC 17 @ BUF 21
        for (delta, kind) in [
            (7, EventKind::Touchdown),
            (6, EventKind::Touchdown),
            (8, EventKind::Touchdown),
            (3, EventKind::FieldGoal),
            (2, EventKind::Safety),
            (1, EventKind::ExtraPoint),
            (10, EventKind::Touchdown),
            (4, EventKind::Score),
        ] {
            let next = with_scores(g.clone(), 17, 21 + delta);
            assert_eq!(kinds(&g, &next), vec![(kind, Some(HomeAway::Home), i32::from(delta))], "+{delta}");
        }
    }

    #[test]
    fn football_play_text_breaks_ties() {
        let g = game("mock:nfl:1");
        let mut next = with_scores(g.clone(), 17, 23);
        let buf = next.home.team.id.clone();
        play(&mut next, &buf, "Two-Point Conversion", "Allen pass to Kincaid, two-point attempt succeeds");
        assert_eq!(kinds(&g, &next), vec![(EventKind::TwoPoint, Some(HomeAway::Home), 2)]);
    }

    #[test]
    fn baseball_home_runs_and_runs() {
        let g = game("mock:mlb:1"); // LAD 3 @ CHC 2
        let mut hr = with_scores(g.clone(), 4, 2);
        let lad = hr.away.team.id.clone();
        play(&mut hr, &lad, "Home Run", "Ohtani homered to right (412 ft)");
        assert_eq!(kinds(&g, &hr), vec![(EventKind::HomeRun, Some(HomeAway::Away), 1)]);

        let mut slam = with_scores(g.clone(), 7, 2);
        play(&mut slam, &lad, "Home Run", "Freeman hit a grand slam");
        assert_eq!(kinds(&g, &slam)[0].0, EventKind::GrandSlam);

        let runs = with_scores(g.clone(), 5, 2);
        assert_eq!(kinds(&g, &runs), vec![(EventKind::Runs, Some(HomeAway::Away), 2)]);
    }

    #[test]
    fn someone_elses_home_run_text_does_not_count() {
        let g = game("mock:mlb:1");
        let mut next = with_scores(g.clone(), 4, 2);
        let chc = next.home.team.id.clone();
        play(&mut next, &chc, "Home Run", "CHC homered earlier");
        assert_eq!(kinds(&g, &next)[0].0, EventKind::Runs);
    }

    #[test]
    fn goals_corrections_and_basketball() {
        let nhl = game("mock:nhl:1");
        assert_eq!(kinds(&nhl, &with_scores(nhl.clone(), 2, 2)), vec![(EventKind::Goal, Some(HomeAway::Away), 1)]);
        assert_eq!(
            kinds(&nhl, &with_scores(nhl.clone(), 1, 1)),
            vec![(EventKind::ScoreCorrection, Some(HomeAway::Home), -1)]
        );
        let wnba = game("mock:wnba:1");
        assert!(detect(&wnba, &with_scores(wnba.clone(), 80, 81)).is_empty(), "no alerts per basket");
    }

    #[test]
    fn final_whistle() {
        let live = game("mock:nfl:1");
        let mut done = live.clone();
        done.status = GameStatus::Final;
        assert_eq!(kinds(&live, &done), vec![(EventKind::Final, None, 0)]);
        // A game that was never seen live (e.g. first poll after it ended) doesn't alert.
        let mut sched = live.clone();
        sched.status = GameStatus::Scheduled;
        assert!(detect(&sched, &done).is_empty());
    }

    #[test]
    fn stale_or_mismatched_snapshots_are_ignored() {
        let g = game("mock:nfl:1");
        let mut next = with_scores(g.clone(), 17, 28);
        next.stale = true;
        assert!(detect(&g, &next).is_empty());
        assert!(detect(&game("mock:nfl:2"), &with_scores(g, 17, 28)).is_empty());
    }

    #[test]
    fn ids_are_deterministic() {
        let g = game("mock:nfl:1");
        let next = with_scores(g.clone(), 17, 28);
        let a = detect(&g, &next);
        let b = detect(&g, &next);
        assert_eq!(a[0].id, "mock:nfl:1:touchdown:17-28");
        assert_eq!(a, b);
    }

    #[test]
    fn detect_all_matches_by_id_and_skips_new_games() {
        let prev: Vec<Game> = mock_games(Utc::now()).into_iter().filter(|g| g.id.0 != "mock:nhl:1").collect();
        let mut next = mock_games(Utc::now());
        for g in &mut next {
            if g.id.0 == "mock:nhl:1" || g.id.0 == "mock:epl:1" {
                g.home.score = g.home.score.map(|s| s + 1);
            }
        }
        let events = detect_all(&prev, &next);
        assert_eq!(events.len(), 1, "NHL game is new, only EPL counts");
        assert_eq!(events[0].1.id.0, "mock:epl:1");
    }

    #[test]
    fn takeover_alert_contents() {
        let g = game("mock:nfl:1");
        let mut next = with_scores(g.clone(), 17, 28);
        let buf = next.home.team.id.clone();
        play(&mut next, &buf, "Rushing Touchdown", "Josh Allen 12 yd run");
        let ev = detect(&g, &next).remove(0);
        let a = alert(&ev, &next, Utc::now()).unwrap();
        assert_eq!(a.level, AlertLevel::Takeover);
        assert_eq!(a.segment_id.as_deref(), Some("mock:nfl:1"));
        assert_eq!(a.detail.as_deref(), Some("KC 17  BUF 28"));
        let t = a.takeover.unwrap();
        assert_eq!(t.kicker, "BUFFALO BILLS · Q3 4:32");
        assert_eq!(t.headline, "TOUCHDOWN");
        assert_eq!(t.play.as_deref(), Some("Josh Allen 12 yd run"));
        assert_eq!(t.score, ScoreLine { away: ("KC".into(), 17), home: ("BUF".into(), 28), scoring_home: true });
        assert_eq!(a.colors.unwrap().0, next.home.team.colors.primary);
    }

    #[test]
    fn flashes_have_no_takeover_and_corrections_have_no_alert() {
        let g = game("mock:nfl:1");
        let fg = detect(&g, &with_scores(g.clone(), 20, 21)).remove(0);
        let a = alert(&fg, &g, Utc::now()).unwrap();
        assert_eq!((a.level, a.title.as_str()), (AlertLevel::Flash, "FIELD GOAL"));
        assert!(a.takeover.is_none());
        let fix = detect(&g, &with_scores(g.clone(), 17, 20)).remove(0);
        assert!(alert(&fix, &g, Utc::now()).is_none());
    }
}
