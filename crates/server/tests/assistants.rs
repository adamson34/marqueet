//! End to end: an assistant runs the sign through `/mcp` and `/api/tools`
//! (from the device itself, which needs no key; keys are tested in auth).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::settings::Settings;
use marqueet_core::sports::fixtures::mock_games;
use marqueet_core::sports::{LeagueId, Sport};
use marqueet_server::Policy;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Serves the mock NFL games.
struct FakeProvider;

impl DataProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn leagues(&self) -> Vec<LeagueInfo> {
        vec![LeagueInfo { id: LeagueId::new("nfl"), sport: Sport::Football, name: "NFL".into() }]
    }

    fn scoreboard<'a>(&'a self, _: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async move {
            let games = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
            Ok(Scoreboard { games, ..Scoreboard::default() })
        })
    }
}

async fn start() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let settings = Settings { leagues: vec![LeagueId::new("nfl")], ..Settings::default() };
    let providers = marqueet_server::Providers {
        scores: Arc::new(FakeProvider),
        weather: None,
        weather_alerts: None,
        fantasy: None,
    };
    tokio::spawn(marqueet_server::run(
        listener,
        providers,
        settings,
        Policy::default(),
        None,
        marqueet_server::AdminOptions::default(),
        std::future::pending(),
    ));
    addr
}

/// A raw HTTP/1.1 request; returns status and body.
async fn request(addr: std::net::SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> (u16, String) {
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n{headers}\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut raw = String::new();
    tcp.read_to_string(&mut raw).await.unwrap();
    let status = raw.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b).to_owned();
    (status, body)
}

async fn rpc(addr: std::net::SocketAddr, id: u32, method: &str, params: Value) -> Value {
    let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let (status, body) = request(addr, "POST", "/mcp", "", &msg.to_string()).await;
    assert_eq!(status, 200, "{body}");
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["id"], id);
    answer
}

async fn tool(addr: std::net::SocketAddr, name: &str, arguments: Value) -> Value {
    let answer = rpc(addr, 9, "tools/call", json!({ "name": name, "arguments": arguments })).await;
    answer["result"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_assistant_sets_up_a_teams_celebration_over_mcp() {
    let addr = start().await;

    let init = rpc(addr, 1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {} })).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "marqueet");
    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(request(addr, "POST", "/mcp", "", &note.to_string()).await.0, 202);

    let list = rpc(addr, 2, "tools/list", json!({})).await;
    let names: Vec<&str> =
        list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"set_team_art") && names.contains(&"update_settings"), "{names:?}");

    // Wait for the first poll's teams.
    let mut teams = Value::Null;
    for _ in 0..50 {
        // A list has no structured content; it comes as JSON text.
        let listed = tool(addr, "list_teams", json!({ "league": "nfl" })).await;
        teams = serde_json::from_str(listed["content"][0]["text"].as_str().unwrap()).unwrap();
        if teams.as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let team = teams[0]["id"].as_str().expect("teams arrive").to_owned();

    let art = tool(
        addr,
        "set_team_art",
        json!({
            "team": team,
            "colors": { "primary": "#95103e", "secondary": "#95bfe5" },
            "words": { "touchdown": { "headline": "up the team", "line": "what a drive" } },
            "art": {
                "pixels": { "palette": { "R": "#95103e", "B": "#95bfe5" }, "frames": [["RB", "BR"], ["BR", "RB"]] },
                "frame_ms": 80, "placement": "intro"
            }
        }),
    )
    .await;
    assert_eq!(art["isError"], false, "{art}");
    let saved = &art["structuredContent"];
    assert_eq!(saved["colors"]["primary"], "#95103e");
    assert_eq!(saved["words"]["touchdown"]["headline"], "UP THE TEAM");
    assert_eq!((saved["art"]["frames"].as_u64(), saved["art"]["placement"].as_str()), (Some(2), Some("intro")));
    let listed = tool(addr, "list_teams", json!({ "league": "nfl" })).await;
    assert!(listed["content"][0]["text"].as_str().unwrap().contains("\"art\": true"));

    let played = tool(addr, "test_takeover", json!({ "kind": "touchdown", "team": team })).await;
    assert_eq!(played["isError"], false, "{played}");

    let patched = tool(addr, "update_settings", json!({ "patch": { "show_odds": true } })).await;
    assert_eq!(patched["structuredContent"]["show_odds"], true);
    let (_, settings) = request(addr, "GET", "/api/settings", "", "").await;
    assert_eq!(serde_json::from_str::<Value>(&settings).unwrap()["show_odds"], true);

    // Mistakes come back for the assistant to read; unknown tools are protocol errors.
    let bad = tool(addr, "set_team_art", json!({ "team": "nobody" })).await;
    assert_eq!(bad["isError"], true);
    assert!(bad["content"][0]["text"].as_str().unwrap().contains("list_teams"));
    let unknown = rpc(addr, 3, "tools/call", json!({ "name": "launch_rockets" })).await;
    assert_eq!(unknown["error"]["code"], -32602);
    assert_eq!(rpc(addr, 4, "bogus", json!({})).await["error"]["code"], -32601);
    assert_eq!(request(addr, "GET", "/mcp", "", "").await.0, 405);
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_also_answer_plain_http() {
    let addr = start().await;
    let (status, list) = request(addr, "GET", "/api/tools", "", "").await;
    assert_eq!(status, 200);
    assert!(serde_json::from_str::<Value>(&list).unwrap().as_array().unwrap().len() > 10);

    let (status, body) =
        request(addr, "POST", "/api/tools/update_settings", "", r#"{"patch":{"show_odds":true}}"#).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["show_odds"], true);

    let (status, body) =
        request(addr, "POST", "/api/tools/update_settings", "", r#"{"patch":{"show_odds":"yes"}}"#).await;
    assert_eq!(status, 400);
    assert!(body.contains("error"));
    assert_eq!(request(addr, "POST", "/api/tools/launch_rockets", "", "{}").await.0, 404);
    assert_eq!(request(addr, "POST", "/api/tools/status", "", "not json").await.0, 400);

    // A key can't make keys or change the password, even through the forms.
    let key = "Authorization: Bearer mqk_anything\r\n";
    for path in ["/admin/api-keys", "/admin/api-keys/revoke", "/admin/password"] {
        assert_eq!(request(addr, "POST", path, key, "name=more").await.0, 403, "{path}");
    }

    // A page on another site can't drive it through a browser.
    let cross = "Origin: http://evil.example\r\n";
    assert_eq!(request(addr, "POST", "/api/tools/status", cross, "{}").await.0, 403);
    assert_eq!(request(addr, "POST", "/mcp", cross, "{}").await.0, 403);
}
