//! End to end: find a fantasy account on the admin page, follow a team, and
//! see its matchup polled.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use marqueet_core::fantasy::{FantasyLeagueInfo, FantasyTeam, FantasyTeamInfo, FantasyUser, Matchup};
use marqueet_core::provider::{BoxFuture, DataProvider, FantasyProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::settings::Settings;
use marqueet_core::sports::{LeagueId, Sport};
use marqueet_server::{Policy, Providers};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct NoGames;

impl DataProvider for NoGames {
    fn id(&self) -> &'static str {
        "none"
    }
    fn leagues(&self) -> Vec<LeagueInfo> {
        vec![LeagueInfo { id: LeagueId::new("nfl"), sport: Sport::Football, name: "NFL".into() }]
    }
    fn scoreboard<'a>(&'a self, _: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async { Ok(Scoreboard::default()) })
    }
}

struct FakeSleeper;

fn team(roster_id: u32, name: &str, points: f32) -> FantasyTeam {
    FantasyTeam { roster_id, name: name.into(), record: "2-0".into(), points, starters: vec![] }
}

impl FantasyProvider for FakeSleeper {
    fn id(&self) -> &'static str {
        "sleeper"
    }
    fn find_user<'a>(&'a self, username: &'a str) -> BoxFuture<'a, Result<FantasyUser, ProviderError>> {
        Box::pin(async move {
            match username {
                "example" => Ok(FantasyUser { id: "42".into(), display_name: "Example".into() }),
                _ => Err(ProviderError::NotFound(format!("Sleeper user {username:?}"))),
            }
        })
    }
    fn leagues<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<FantasyLeagueInfo>, ProviderError>> {
        Box::pin(async {
            Ok(vec![FantasyLeagueInfo {
                id: "777".into(),
                name: "Office League".into(),
                season: "2026".into(),
                teams: 2,
            }])
        })
    }
    fn teams<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<FantasyTeamInfo>, ProviderError>> {
        Box::pin(async {
            Ok(vec![
                FantasyTeamInfo { roster_id: 1, name: "Rivals".into(), owner_id: Some("9".into()) },
                FantasyTeamInfo { roster_id: 2, name: "My Team".into(), owner_id: Some("42".into()) },
            ])
        })
    }
    fn matchup<'a>(&'a self, league_id: &'a str, roster_id: u32) -> BoxFuture<'a, Result<Matchup, ProviderError>> {
        Box::pin(async move {
            Ok(Matchup {
                league_id: league_id.into(),
                league: "Office League".into(),
                week: 3,
                me: team(roster_id, "My Team", 88.42),
                opponent: Some(team(1, "Rivals", 76.2)),
                fetched_at: Utc::now(),
            })
        })
    }
}

async fn request(addr: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut raw = String::new();
    tcp.read_to_string(&mut raw).await.unwrap();
    let status = raw.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, raw.split_once("\r\n\r\n").map(|(_, b)| b.to_owned()).unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread")]
async fn follow_a_fantasy_team_from_the_admin_page() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let policy = Policy { slow_tick: Duration::from_millis(50), ..Policy::default() };
    let providers = Providers {
        scores: Arc::new(NoGames),
        weather: None,
        weather_alerts: None,
        fantasy: Some(Arc::new(FakeSleeper)),
    };
    let settings = Settings { leagues: vec![LeagueId::new("nfl")], ..Settings::default() };
    tokio::spawn(marqueet_server::run(
        listener,
        providers,
        settings,
        policy,
        None,
        marqueet_server::AdminOptions::default(),
        std::future::pending(),
    ));

    let (status, page) = request(addr, "POST", "/admin/fantasy/find", "username=nobody").await;
    assert_eq!(status, 400);
    assert!(page.contains("not found"), "unknown user is explained");

    let (status, page) = request(addr, "POST", "/admin/fantasy/find", "username=example").await;
    assert_eq!(status, 200);
    assert!(page.contains("Office League"));
    assert!(page.contains("<option value=\"2\" selected>My Team</option>"), "your own team is preselected");

    let (status, _) =
        request(addr, "POST", "/admin/fantasy/add", "league_id=777&league=Office+League&roster_id=9").await;
    assert_eq!(status, 400, "not a team in the league");
    let (status, _) =
        request(addr, "POST", "/admin/fantasy/add", "league_id=777&league=Office+League&roster_id=2").await;
    assert_eq!(status, 303);
    let (status, _) =
        request(addr, "POST", "/admin/fantasy/add", "league_id=777&league=Office+League&roster_id=2").await;
    assert_eq!(status, 400, "already following");

    let polled = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (_, page) = request(addr, "GET", "/admin", "").await;
            if page.contains("Week 3: 88.4 to 76.2") {
                return page;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("matchup polled and shown");
    assert!(polled.contains("My Team"));

    let (status, _) = request(addr, "POST", "/admin/fantasy/remove", "league_id=777&roster_id=2").await;
    assert_eq!(status, 303);
    let (_, page) = request(addr, "GET", "/admin", "").await;
    assert!(!page.contains("Week 3:"));
}
