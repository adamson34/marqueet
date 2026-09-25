//! End to end: a provider that scores a touchdown on its second poll; the
//! display's WebSocket receives the takeover alert.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use marqueet_core::alert::AlertLevel;
use marqueet_core::protocol::ServerMsg;
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::settings::Settings;
use marqueet_core::sports::fixtures::mock_games;
use marqueet_core::sports::{LeagueId, Play, Sport};
use marqueet_server::Policy;
use tokio_tungstenite::tungstenite::Message;

#[derive(Default)]
struct ScoringProvider {
    calls: AtomicU32,
}

impl DataProvider for ScoringProvider {
    fn id(&self) -> &'static str {
        "scoring"
    }

    fn leagues(&self) -> Vec<LeagueInfo> {
        vec![LeagueInfo { id: LeagueId::new("nfl"), sport: Sport::Football, name: "NFL".into() }]
    }

    fn scoreboard<'a>(&'a self, _league: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let mut games: Vec<_> = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
            if call >= 1 {
                let kc_buf = &mut games[0];
                kc_buf.home.score = Some(28);
                kc_buf.last_play = Some(Play {
                    id: "td".into(),
                    text: "Rico Castellano 12 yd run".into(),
                    type_text: Some("Rushing Touchdown".into()),
                    team: Some(kc_buf.home.team.id.clone()),
                    score_value: Some(6),
                    athletes: vec![],
                });
            }
            Ok(Scoreboard { games, skipped: vec![] })
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn touchdown_reaches_the_display_as_a_takeover() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Poll every 50 ms so the test sees two snapshots quickly.
    let policy = Policy { live: Duration::from_millis(50), ..Policy::default() };
    tokio::spawn(marqueet_server::run(
        listener,
        marqueet_server::Providers {
            scores: Arc::new(ScoringProvider::default()),
            weather: None,
            weather_alerts: None,
            fantasy: None,
        },
        Settings { leagues: vec![LeagueId::new("nfl")], ..Settings::default() },
        policy,
        None,
        marqueet_server::AdminOptions::default(),
        std::future::pending(),
    ));

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();
    let alert = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Text(t) = ws.next().await.unwrap().unwrap()
                && let ServerMsg::Alert(a) = ServerMsg::from_json(&t).unwrap()
            {
                return a;
            }
        }
    })
    .await
    .expect("a touchdown alert");

    assert_eq!(alert.level, AlertLevel::Takeover);
    assert_eq!(alert.segment_id.as_deref(), Some("mock:nfl:1"));
    let t = alert.takeover.as_ref().unwrap();
    assert_eq!(t.headline, "TOUCHDOWN");
    assert_eq!(t.play.as_deref(), Some("Rico Castellano 12 yd run"));
    let score = t.score.as_ref().unwrap();
    assert_eq!((score.home.1, score.scoring_home), (28, true));

    // Only once, even though the provider keeps returning 28.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let body = reqwest_lite(addr, "/api/alerts").await;
    let recent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(recent.as_array().unwrap().len(), 1);
}

async fn reqwest_lite(addr: std::net::SocketAddr, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut raw = String::new();
    tcp.read_to_string(&mut raw).await.unwrap();
    raw.split("\r\n\r\n").nth(1).unwrap().to_owned()
}

/// A tornado warning appears on the second check.
#[derive(Default)]
struct StormyNws {
    calls: AtomicU32,
}

impl marqueet_core::provider::WeatherAlertsProvider for StormyNws {
    fn active<'a>(
        &'a self,
        _place: &'a marqueet_core::weather::Place,
    ) -> BoxFuture<'a, Result<Vec<marqueet_core::weather::WeatherAlert>, ProviderError>> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(vec![]);
            }
            Ok(vec![marqueet_core::weather::WeatherAlert {
                id: "urn:tornado".into(),
                event: "Tornado Warning".into(),
                severity: marqueet_core::weather::Severity::Extreme,
                immediate: true,
                area: "Jackson, MO".into(),
                ends: Some(Utc::now() + chrono::Duration::minutes(40)),
                sender: "NWS Kansas City".into(),
            }])
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tornado_warning_leads_the_ticker_and_takes_over() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let policy = Policy {
        slow_tick: Duration::from_millis(50),
        weather_alerts_every: Duration::from_millis(100),
        ..Policy::default()
    };
    let mut settings = Settings { leagues: vec![LeagueId::new("nfl")], ..Settings::default() };
    settings.weather.place = Some(marqueet_core::weather::mock_weather(Utc::now()).place);
    tokio::spawn(marqueet_server::run(
        listener,
        marqueet_server::Providers {
            scores: Arc::new(ScoringProvider::default()),
            weather: None,
            weather_alerts: Some(Arc::new(StormyNws::default())),
            fantasy: None,
        },
        settings,
        policy,
        None,
        marqueet_server::AdminOptions::default(),
        std::future::pending(),
    ));

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();
    let (mut led, mut takeover) = (false, None);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !(led && takeover.is_some()) {
            if let Message::Text(t) = ws.next().await.unwrap().unwrap() {
                match ServerMsg::from_json(&t).unwrap() {
                    ServerMsg::Content(c) => {
                        led |= c.ticker.first().is_some_and(|s| s.id == "weather-alert:urn:tornado")
                    }
                    ServerMsg::Alert(a) if a.source == "weather" => takeover = Some(a),
                    _ => {}
                }
            }
        }
    })
    .await
    .expect("warning on the ticker and as a takeover");
    let a = takeover.unwrap();
    assert_eq!(a.level, AlertLevel::Takeover);
    assert_eq!(a.takeover.unwrap().headline, "TORNADO WARNING");
}
