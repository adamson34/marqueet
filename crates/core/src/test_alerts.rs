//! Test takeovers, for the admin page: see what a touchdown, a home run or a
//! weather warning looks like on this screen without waiting for one. Each
//! replays the last real takeover of its kind when there is one; otherwise
//! it's built from a game on today's scoreboard (a chosen team's, if one is
//! picked) or, failing that, from the made-up demo games.

use chrono::{DateTime, Duration, FixedOffset, Utc};

use crate::alert::{Alert, AlertLevel, Takeover};
use crate::events::{self, EventKind, GameEvent};
use crate::sports::fixtures::mock_games;
use crate::sports::{Game, HomeAway, Sport, TeamId};
use crate::team_art::{TeamArtMap, apply_words};
use crate::weather::{Severity, WeatherAlert};

/// A kind of takeover that can be tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestKind {
    Touchdown,
    HomeRun,
    GrandSlam,
    HockeyGoal,
    SoccerGoal,
    WeatherWarning,
    FeedMessage,
}

impl TestKind {
    pub const ALL: [TestKind; 7] = [
        TestKind::Touchdown,
        TestKind::HomeRun,
        TestKind::GrandSlam,
        TestKind::HockeyGoal,
        TestKind::SoccerGoal,
        TestKind::WeatherWarning,
        TestKind::FeedMessage,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TestKind::Touchdown => "touchdown",
            TestKind::HomeRun => "home_run",
            TestKind::GrandSlam => "grand_slam",
            TestKind::HockeyGoal => "hockey_goal",
            TestKind::SoccerGoal => "soccer_goal",
            TestKind::WeatherWarning => "weather",
            TestKind::FeedMessage => "feed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TestKind::Touchdown => "Touchdown",
            TestKind::HomeRun => "Home run",
            TestKind::GrandSlam => "Grand slam",
            TestKind::HockeyGoal => "Hockey goal",
            TestKind::SoccerGoal => "Soccer goal",
            TestKind::WeatherWarning => "Weather warning",
            TestKind::FeedMessage => "Message from a feed",
        }
    }

    pub fn from_id(id: &str) -> Option<TestKind> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The sport and scoring event, for the game takeovers.
    fn game_event(self) -> Option<(Sport, EventKind, u16)> {
        match self {
            TestKind::Touchdown => Some((Sport::Football, EventKind::Touchdown, 6)),
            TestKind::HomeRun => Some((Sport::Baseball, EventKind::HomeRun, 1)),
            TestKind::GrandSlam => Some((Sport::Baseball, EventKind::GrandSlam, 4)),
            TestKind::HockeyGoal => Some((Sport::Hockey, EventKind::Goal, 1)),
            TestKind::SoccerGoal => Some((Sport::Soccer, EventKind::Goal, 1)),
            TestKind::WeatherWarning | TestKind::FeedMessage => None,
        }
    }
}

/// The last real takeover of `kind` among `recent` (newest first).
fn last_real<'a>(kind: TestKind, recent: &'a [Alert], games: &[Game]) -> Option<&'a Alert> {
    let sport_of = |a: &Alert| {
        let segment = a.segment_id.as_deref()?;
        games.iter().find(|g| g.id.0 == segment).map(|g| g.sport)
    };
    recent.iter().filter(|a| a.level == AlertLevel::Takeover && a.takeover.is_some()).find(|a| match kind {
        TestKind::WeatherWarning => a.source == "weather",
        TestKind::FeedMessage => a.source.starts_with("feed:"),
        _ => {
            let Some((sport, event, _)) = kind.game_event() else { return false };
            let headline = events::headline(event, 1);
            a.source == "sports" && a.title == headline && sport_of(a).is_none_or(|s| s == sport)
        }
    })
}

/// A test takeover of `kind`: the last real one, else one built from today's
/// games (with `team` scoring, when picked). Errors say what to change.
pub fn test_alert(
    kind: TestKind,
    recent: &[Alert],
    games: &[Game],
    team: Option<&TeamId>,
    art: &TeamArtMap,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Result<Alert, String> {
    let mut alert = match (team, last_real(kind, recent, games)) {
        (None, Some(real)) => real.clone(),
        _ => sample(kind, games, team, art, tz, now)?,
    };
    alert.id = format!("test:{}:{}", kind.id(), now.timestamp_millis());
    alert.level = AlertLevel::Takeover;
    alert.created_at = now;
    Ok(alert)
}

fn sample(
    kind: TestKind,
    games: &[Game],
    team: Option<&TeamId>,
    art: &TeamArtMap,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Result<Alert, String> {
    let Some((sport, event, points)) = kind.game_event() else {
        return Ok(match kind {
            TestKind::WeatherWarning => sample_weather(tz, now),
            _ => sample_feed(now),
        });
    };
    let mock = mock_games(now);
    let game = match team {
        Some(t) => {
            let g = games
                .iter()
                .find(|g| &g.home.team.id == t || &g.away.team.id == t)
                .ok_or("That team has no game on today's scoreboard; pick another or leave it on Automatic.")?;
            if g.sport != sport {
                return Err(format!(
                    "{} is for {} teams; that team plays another sport.",
                    kind.label(),
                    sport_name(sport)
                ));
            }
            g.clone()
        }
        None => games
            .iter()
            .filter(|g| g.sport == sport)
            .max_by_key(|g| (g.status.is_live(), g.home.score.is_some()))
            .or_else(|| mock.iter().find(|g| g.sport == sport))
            .cloned()
            .ok_or("no sample game")?,
    };
    let side = match team {
        Some(t) if &game.away.team.id == t => HomeAway::Away,
        _ => HomeAway::Home,
    };
    let (mut away, mut home) = (game.away.score.unwrap_or(0), game.home.score.unwrap_or(0));
    match side {
        HomeAway::Home => home += points,
        HomeAway::Away => away += points,
    }
    let mut game = game;
    game.last_play = None;
    let e =
        GameEvent { id: String::new(), kind: event, side: Some(side), points: i32::from(points), score: (away, home) };
    let mut alert = events::alert(&e, &game, now).ok_or("no alert for that event")?;
    if let Some(t) = alert.takeover.as_mut() {
        t.play = Some("A test: this is how it will look".into());
    }
    // The team's own words, when it has some for this play.
    apply_words(&mut alert, art, &game.competitor(side).team.id);
    Ok(alert)
}

fn sport_name(sport: Sport) -> &'static str {
    match sport {
        Sport::Football => "football",
        Sport::Baseball => "baseball",
        Sport::Basketball => "basketball",
        Sport::Hockey => "hockey",
        Sport::Soccer => "soccer",
    }
}

fn sample_weather(tz: FixedOffset, now: DateTime<Utc>) -> Alert {
    let warning = WeatherAlert {
        id: "test".into(),
        event: "Severe Thunderstorm Warning".into(),
        severity: Severity::Severe,
        immediate: true,
        area: "Your area (a test)".into(),
        ends: Some(now + Duration::minutes(45)),
        sender: "National Weather Service".into(),
    };
    warning.to_alert(tz, now).unwrap_or_else(|| sample_feed(now))
}

fn sample_feed(now: DateTime<Utc>) -> Alert {
    Alert {
        id: String::new(),
        level: AlertLevel::Takeover,
        source: "feed:test".into(),
        segment_id: None,
        title: "YOUR MESSAGE".into(),
        detail: None,
        colors: None,
        takeover: Some(Takeover {
            kicker: "FROM A FEED".into(),
            headline: "YOUR MESSAGE".into(),
            play: Some("Scripts can send takeovers through the feed API".into()),
            score: None,
            note: None,
            art: Default::default(),
        }),
        created_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    #[test]
    fn every_kind_has_a_takeover_with_no_history_or_games() {
        let now = Utc::now();
        for kind in TestKind::ALL {
            let a = test_alert(kind, &[], &[], None, &TeamArtMap::new(), tz(), now).unwrap();
            assert_eq!(a.level, AlertLevel::Takeover, "{kind:?}");
            assert!(a.takeover.is_some(), "{kind:?}");
            assert!(a.id.starts_with(&format!("test:{}:", kind.id())), "a fresh id each time");
            assert_eq!(TestKind::from_id(kind.id()), Some(kind));
        }
        let hr = test_alert(TestKind::HomeRun, &[], &[], None, &TeamArtMap::new(), tz(), now).unwrap();
        assert_eq!(hr.title, "HOME RUN");
    }

    #[test]
    fn the_last_real_one_is_replayed() {
        let now = Utc::now();
        let games = mock_games(now);
        let mut real = test_alert(TestKind::Touchdown, &[], &games, None, &TeamArtMap::new(), tz(), now).unwrap();
        real.id = "espn:nfl:1:touchdown:7-0".into();
        real.detail = Some("the real one".into());
        let a =
            test_alert(TestKind::Touchdown, std::slice::from_ref(&real), &games, None, &TeamArtMap::new(), tz(), now)
                .unwrap();
        assert_eq!(a.detail.as_deref(), Some("the real one"));
        assert_ne!(a.id, real.id, "a new id, so it isn't deduped");
        let hr =
            test_alert(TestKind::HomeRun, std::slice::from_ref(&real), &games, None, &TeamArtMap::new(), tz(), now)
                .unwrap();
        assert_eq!(hr.title, "HOME RUN", "a touchdown isn't replayed for a home run");
    }

    #[test]
    fn a_picked_team_scores() {
        let now = Utc::now();
        let games = mock_games(now);
        let g = games.iter().find(|g| g.sport == Sport::Football).unwrap();
        let away = &g.away.team;
        let a = test_alert(TestKind::Touchdown, &[], &games, Some(&away.id), &TeamArtMap::new(), tz(), now).unwrap();
        let t = a.takeover.unwrap();
        assert!(t.kicker.starts_with(&away.display_name.to_uppercase()));
        assert!(!t.score.unwrap().scoring_home);
        let words =
            [("touchdown".to_owned(), crate::team_art::TakeoverWords { headline: "ROAD TD!".into(), line: None })];
        let art: TeamArtMap = [(
            away.id.clone(),
            crate::team_art::TeamArt {
                label: String::new(),
                colors: None,
                logo: None,
                words: words.into(),
                art: Default::default(),
            },
        )]
        .into();
        let own = test_alert(TestKind::Touchdown, &[], &games, Some(&away.id), &art, tz(), now).unwrap();
        assert_eq!(own.takeover.unwrap().headline, "ROAD TD!", "the team's own words");
        let err =
            test_alert(TestKind::HomeRun, &[], &games, Some(&away.id), &TeamArtMap::new(), tz(), now).unwrap_err();
        assert!(err.contains("baseball"), "{err}");
    }
}
