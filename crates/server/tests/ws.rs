//! End to end: real server on a random port, fake provider, real WebSocket
//! client and HTTP request.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use marqueet_core::protocol::{Content, PROTOCOL_VERSION, ServerMsg};
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::settings::Settings;
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
        vec![
            LeagueInfo { id: LeagueId::new("nfl"), sport: Sport::Football, name: "NFL".into() },
            LeagueInfo { id: LeagueId::new("mlb"), sport: Sport::Baseball, name: "MLB".into() },
        ]
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
    start_with(marqueet_server::AdminOptions::default()).await
}

async fn start_with(admin: marqueet_server::AdminOptions) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let leagues = vec![LeagueId::new("nfl"), LeagueId::new("mlb")];
    let settings = Settings { leagues, ..Settings::default() };
    tokio::spawn(marqueet_server::run(
        listener,
        marqueet_server::Providers {
            scores: Arc::new(FakeProvider),
            weather: None,
            weather_alerts: None,
            fantasy: None,
        },
        settings,
        Policy::default(),
        None,
        admin,
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

async fn http(addr: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, String) {
    let (status, _, body) = request(addr, method, path, "Content-Type: application/json\r\n", body).await;
    (status, body)
}

/// A raw HTTP/1.1 request; returns status, head and body.
async fn request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    body: &str,
) -> (u16, String, String) {
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut raw = String::new();
    tcp.read_to_string(&mut raw).await.unwrap();
    let status = raw.split_whitespace().nth(1).unwrap().parse().unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    (status, head.to_owned(), body.to_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_page_saves_the_form_from_the_device() {
    let addr = start().await;
    let (status, head, _) = request(addr, "GET", "/", "", "").await;
    assert_eq!(status, 303);
    assert!(head.to_lowercase().contains("location: /admin"));

    let (status, head, page_html) = request(addr, "GET", "/admin", "", "").await;
    assert_eq!(status, 200);
    assert!(head.to_lowercase().contains("content-security-policy: default-src 'none'"));
    assert!(page_html.contains("name=\"league\" value=\"nfl\" checked"));
    let (status, _, css) = request(addr, "GET", "/admin/admin.css", "", "").await;
    assert_eq!(status, 200);
    assert!(css.contains("--accent"));

    let form = "Content-Type: application/x-www-form-urlencoded\r\n";
    let body = "league=mlb&order_mlb=1&league=nfl&order_nfl=2&takeovers=off&led_color=%2300ff00&widget_0=scores&widget_1=scores";
    let (status, _, _) =
        request(addr, "POST", "/admin", &format!("{form}Origin: https://evil.example\r\n"), body).await;
    assert_eq!(status, 403, "cross-site post");

    let (status, head, err) = request(addr, "POST", "/admin", &format!("{form}Origin: http://{addr}\r\n"), body).await;
    assert_eq!(status, 303, "{}", err.split("Not saved").nth(1).unwrap_or(""));
    assert!(head.to_lowercase().contains("location: /admin?saved"));
    let (_, settings) = http(addr, "GET", "/api/settings", "").await;
    let settings: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(settings["leagues"], serde_json::json!(["mlb", "nfl"]));
    assert_eq!(settings["takeovers"], "off");
    assert_eq!(settings["display"]["led_color"], "#00ff00");

    let (status, _, page_html) = request(addr, "POST", "/admin", form, "league=curling").await;
    assert_eq!(status, 400);
    assert!(page_html.contains("Not saved: unknown league"));

    let (status, _, page_html) = request(addr, "GET", "/admin?saved", "", "").await;
    assert_eq!(status, 200);
    assert!(page_html.contains("Saved."));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_api_saves_and_pushes_the_new_look_to_the_display() {
    let addr = start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();

    let (status, body) = http(addr, "GET", "/api/settings", "").await;
    assert_eq!(status, 200);
    let mut settings: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(settings["leagues"], serde_json::json!(["nfl", "mlb"]));

    settings["leagues"] = serde_json::json!(["curling"]);
    let (status, body) = http(addr, "PUT", "/api/settings", &settings.to_string()).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("unknown league"));

    settings["leagues"] = serde_json::json!(["nfl"]);
    settings["display"]["led_color"] = serde_json::json!("#00ff00");
    let (status, body) = http(addr, "PUT", "/api/settings", &settings.to_string()).await;
    assert_eq!(status, 200, "{body}");

    let look = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMsg::Display(d) = next_msg(&mut ws).await
                && d.config.led_color == marqueet_core::Rgb::new(0, 255, 0)
            {
                return d;
            }
        }
    })
    .await
    .expect("display gets the new look");
    assert!(!look.screen_off);
    let (_, body) = http(addr, "GET", "/api/settings", "").await;
    assert!(body.contains("#00ff00"));
}

#[tokio::test(flavor = "multi_thread")]
async fn feed_api_puts_script_content_on_the_ticker() {
    let addr = start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();

    // Create the feed on the admin page (from the device), read its token back.
    let form = "Content-Type: application/x-www-form-urlencoded\r\n";
    let (status, _, _) = request(addr, "POST", "/admin/feeds", form, "name=stocks").await;
    assert_eq!(status, 303);
    let (_, _, page) = request(addr, "GET", "/admin", "", "").await;
    let token = page.split("<code class=\"token\">").nth(1).unwrap().split('<').next().unwrap().to_owned();
    assert_eq!(token.len(), 64);
    let (status, _, _) = request(addr, "POST", "/admin/feeds", form, "name=stocks").await;
    assert_eq!(status, 400, "names are unique");

    let json = "Content-Type: application/json\r\n";
    let body = r#"{"segments":[{"id":"aapl","text":"AAPL 189.20","detail":"+1.2%","color":"green"}]}"#;
    let (status, _, _) =
        request(addr, "POST", "/api/feeds/stocks", &format!("{json}Authorization: Bearer nope\r\n"), body).await;
    assert_eq!(status, 401);
    let (status, _, _) =
        request(addr, "POST", "/api/feeds/other", &format!("{json}Authorization: Bearer {token}\r\n"), body).await;
    assert_eq!(status, 401, "a token only works for its own feed");
    let auth = format!("{json}Authorization: Bearer {token}\r\n");
    let (status, _, resp) = request(addr, "POST", "/api/feeds/stocks", &auth, body).await;
    assert_eq!(status, 200, "{resp}");
    let (status, _, resp) = request(addr, "POST", "/api/feeds/stocks", &auth, r#"{"segments":[{"txt":"x"}]}"#).await;
    assert_eq!(status, 400);
    assert!(resp.contains("unknown field"), "{resp}");

    let shown = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMsg::Content(c) = next_msg(&mut ws).await
                && let Some(seg) = c.ticker.iter().find(|s| s.id == "feed:stocks:aapl")
            {
                return segment_text(seg);
            }
        }
    })
    .await
    .expect("feed content reaches the display");
    assert_eq!(shown, "AAPL 189.20/+1.2%");

    let (status, _, resp) =
        request(addr, "POST", "/api/feeds/stocks/alert", &auth, r#"{"title":"AAPL +5%","level":"takeover"}"#).await;
    assert_eq!(status, 200, "{resp}");
    let alert = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMsg::Alert(a) = next_msg(&mut ws).await {
                return a;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(alert.segment_id.as_deref(), Some("feed:stocks:aapl"));
    assert_eq!(alert.takeover.unwrap().headline, "AAPL +5%");
    let (status, head, _) = request(addr, "POST", "/api/feeds/stocks/alert", &auth, r#"{"title":"again"}"#).await;
    assert_eq!(status, 429);
    assert!(head.to_lowercase().contains("retry-after"));

    let (status, _, _) = request(addr, "DELETE", "/api/feeds/stocks", &auth, "").await;
    assert_eq!(status, 204);
    let (status, _, list) = request(addr, "GET", "/api/feeds", "", "").await;
    assert_eq!(status, 200);
    assert!(list.contains("\"name\":\"stocks\"") && !list.contains(&token), "{list}");
    let (_, settings) = http(addr, "GET", "/api/settings", "").await;
    assert!(!settings.contains(&token), "tokens never appear in settings");
}

#[tokio::test(flavor = "multi_thread")]
async fn first_boot_code_shows_on_the_device_and_creates_the_password() {
    let admin =
        marqueet_server::AdminOptions { password: None, setup_urls: vec!["http://marqueet.local:7878/setup".into()] };
    let addr = start_with(admin).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();
    let setup = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMsg::Display(d) = next_msg(&mut ws).await
                && let Some(s) = d.setup
            {
                return s;
            }
        }
    })
    .await
    .expect("a display on the device gets the code");
    assert_eq!(setup.urls, ["http://marqueet.local:7878/setup"]);

    let form = "Content-Type: application/x-www-form-urlencoded\r\n";
    let (status, _, page) = request(addr, "GET", "/setup", "", "").await;
    assert_eq!(status, 200);
    assert!(page.contains("Code from the screen"));
    let wrong = if setup.code == "000000" { "111111" } else { "000000" };
    let (status, _, page) =
        request(addr, "POST", "/setup", form, &format!("code={wrong}&password=long+enough&confirm=long+enough")).await;
    assert_eq!(status, 401);
    assert!(page.contains("the one on the screen"));
    let (status, _, page) =
        request(addr, "POST", "/setup", form, &format!("code={}&password=long+enough&confirm=different", setup.code))
            .await;
    assert_eq!(status, 400);
    assert!(page.contains("passwords don"));
    let (status, head, _) =
        request(addr, "POST", "/setup", form, &format!("code={}&password=long+enough&confirm=long+enough", setup.code))
            .await;
    assert_eq!(status, 303);
    assert!(head.to_lowercase().contains("set-cookie: marqueet_session="), "logged in");

    let gone = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMsg::Display(d) = next_msg(&mut ws).await
                && d.setup.is_none()
            {
                return;
            }
        }
    })
    .await;
    assert!(gone.is_ok(), "the screen stops showing the code");
    let (status, head, _) = request(addr, "GET", "/setup", "", "").await;
    assert_eq!(status, 303);
    assert!(head.to_lowercase().contains("location: /admin"));
}

#[tokio::test(flavor = "multi_thread")]
async fn welcome_steps_save_as_you_go() {
    let addr = start().await;
    let form = "Content-Type: application/x-www-form-urlencoded\r\n";
    let (status, _, page) = request(addr, "GET", "/welcome/sports", "", "").await;
    assert_eq!(status, 200);
    assert!(page.contains("Which sports do you follow?"));

    let (status, _, page) = request(addr, "POST", "/welcome/sports", form, "").await;
    assert_eq!(status, 400);
    assert!(page.contains("Pick at least one sport"));
    let (status, head, _) = request(addr, "POST", "/welcome/sports", form, "league=nfl").await;
    assert_eq!(status, 303);
    assert!(head.to_lowercase().contains("location: /welcome/teams"));

    let (status, _, page) = request(addr, "GET", "/welcome/teams", "", "").await;
    assert_eq!(status, 200);
    assert!(page.contains("Who are your teams?"));
    let (status, head, _) = request(addr, "POST", "/welcome/teams", form, "favorite=espn%3Anfl%3A2").await;
    assert_eq!(status, 303);
    assert!(head.to_lowercase().contains("location: /welcome/town"));

    let (status, head, _) =
        request(addr, "POST", "/welcome/town", form, "location=39.10%2C+-94.58&units=celsius").await;
    assert_eq!(status, 303);
    assert!(head.to_lowercase().contains("location: /welcome/fantasy"));
    let (_, settings) = http(addr, "GET", "/api/settings", "").await;
    let settings: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(settings["leagues"], serde_json::json!(["nfl"]));
    assert_eq!(settings["favorites"], serde_json::json!(["espn:nfl:2"]));
    assert_eq!(settings["weather"]["units"], "celsius");
    assert_eq!(settings["weather"]["place"]["latitude"], 39.1);

    let (status, _, page) = request(addr, "POST", "/welcome/town", form, "location=Atlantis&units=celsius").await;
    assert_eq!(status, 400, "no weather provider in this test: the lookup fails politely");
    assert!(page.contains("Where are you?"));

    let (status, _, page) = request(addr, "GET", "/welcome/fantasy", "", "").await;
    assert_eq!(status, 200);
    assert!(page.contains("Sleeper username") && page.contains("href=\"/welcome/done\">Skip"));
    let (status, _, page) = request(addr, "GET", "/welcome/done", "", "").await;
    assert_eq!(status, 200);
    assert!(page.contains("all set"));
}
