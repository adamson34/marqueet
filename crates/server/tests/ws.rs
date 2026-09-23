//! End to end: real server on a random port, fake provider, real WebSocket
//! client and HTTP request.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use marqueet_core::protocol::{Content, PROTOCOL_VERSION, ServerMsg};
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::sports::fixtures::mock_games;
use marqueet_core::sports::ticker::segment_text;
use marqueet_core::sports::{LeagueId, Sport};
use marqueet_server::Policy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

/// Serves mock NFL games; MLB always fails.
struct FakeProvider;

impl DataProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn leagues(&self) -> Vec<LeagueInfo> {
        vec![LeagueInfo { id: LeagueId::new("nfl"), sport: Sport::Football, name: "NFL".into() }]
    }

    fn scoreboard<'a>(&'a self, league: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async move {
            match league.as_str() {
                "nfl" => Ok(Scoreboard {
                    games: mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect(),
                    skipped: vec![],
                }),
                _ => Err(ProviderError::Status(503)),
            }
        })
    }
}

async fn start() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let leagues = vec![LeagueId::new("nfl"), LeagueId::new("mlb")];
    tokio::spawn(marqueet_server::run(
        listener,
        Arc::new(FakeProvider),
        leagues,
        Policy::default(),
        std::future::pending(),
    ));
    addr
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_msg(ws: &mut Ws) -> ServerMsg {
    loop {
        if let Message::Text(t) = ws.next().await.expect("stream ended").unwrap() {
            return ServerMsg::from_json(&t).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn display_gets_hello_then_live_content_and_api_reports_health() {
    let addr = start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();

    match next_msg(&mut ws).await {
        ServerMsg::Hello { protocol, .. } => assert_eq!(protocol, PROTOCOL_VERSION),
        other => panic!("expected hello, got {other:?}"),
    }

    // The first content may be "LOADING SCORES" if the poll hasn't landed yet.
    let content: Content = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMsg::Content(c) = next_msg(&mut ws).await
                && c.ticker.iter().any(|s| s.id == "league:nfl")
            {
                return c;
            }
        }
    })
    .await
    .expect("content with NFL games");
    let texts: Vec<String> = content.ticker.iter().map(segment_text).collect();
    assert_eq!(texts[0], "NFL");
    assert!(texts.iter().any(|t| t.starts_with("KC/BUF")), "{texts:?}");
    assert_eq!(content.status.live_games, 1);

    // HTTP API: raw request, no client dependency needed.
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(b"GET /api/games HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut raw = String::new();
    tcp.read_to_string(&mut raw).await.unwrap();
    let body = raw.split("\r\n\r\n").nth(1).unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    let leagues = json["leagues"].as_array().unwrap();
    assert_eq!(leagues[0]["id"], "nfl");
    assert_eq!(leagues[0]["games"].as_array().unwrap().len(), 4);
    assert_eq!(leagues[1]["id"], "mlb");
    assert!(leagues[1]["failures"].as_u64().unwrap() >= 1);
    assert_eq!(leagues[1]["last_error"], "upstream returned HTTP 503");

    ws.close(None).await.unwrap();
}
