//! End to end: favorites and team art saved under ids the provider no longer
//! gives out move to the current ids when the server starts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use marqueet_core::Rgb;
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::settings::Settings;
use marqueet_core::sports::{LeagueId, Sport, TeamColors, TeamId};
use marqueet_core::team_art::TeamArt;
use marqueet_server::{Policy, SettingsStore};

/// Renamed its teams from `fake:old:N` to `fake:club:N`.
struct Renamed;

impl DataProvider for Renamed {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn leagues(&self) -> Vec<LeagueInfo> {
        vec![LeagueInfo { id: LeagueId::new("epl"), sport: Sport::Soccer, name: "League".into() }]
    }

    fn scoreboard<'a>(&'a self, _: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async { Ok(Scoreboard::default()) })
    }

    fn canonical_team(&self, team: &TeamId) -> Option<TeamId> {
        team.0.strip_prefix("fake:old:").map(|n| TeamId(format!("fake:club:{n}")))
    }
}

fn art(label: &str) -> TeamArt {
    TeamArt {
        label: label.into(),
        colors: Some(TeamColors { primary: Rgb::new(0x67, 0x0e, 0x36), secondary: None }),
        logo: None,
        words: Default::default(),
        art: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn old_team_ids_move_to_the_current_ones_at_startup() {
    let db = SettingsStore::in_memory().unwrap();
    let (old, new, kept) =
        (TeamId("fake:old:362".into()), TeamId("fake:club:362".into()), TeamId("fake:club:1".into()));
    db.set_team_art(&old, &art("Old")).unwrap();
    db.set_team_art(&TeamId("fake:old:1".into()), &art("Stale")).unwrap();
    db.set_team_art(&kept, &art("Current")).unwrap();
    let settings =
        Settings { leagues: vec![LeagueId::new("epl")], favorites: vec![old.clone()], ..Settings::default() };
    db.save(&settings).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let providers =
        marqueet_server::Providers { scores: Arc::new(Renamed), weather: None, weather_alerts: None, fantasy: None };
    tokio::spawn(marqueet_server::run(
        listener,
        providers,
        settings,
        Policy::default(),
        Some(db.clone()),
        marqueet_server::AdminOptions::default(),
        std::future::pending(),
    ));

    for _ in 0..50 {
        if db.load().unwrap().is_some_and(|s| s.favorites == std::slice::from_ref(&new)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(db.load().unwrap().unwrap().favorites, std::slice::from_ref(&new), "favorite moved and saved");
    let saved = db.team_art().unwrap();
    assert_eq!(saved.get(&new).map(|a| a.label.as_str()), Some("Old"), "art moved");
    assert_eq!(saved.get(&kept).map(|a| a.label.as_str()), Some("Current"), "art already under the new id wins");
    assert!(!saved.keys().any(|t| t.0.starts_with("fake:old:")), "old ids gone: {saved:?}");
}
