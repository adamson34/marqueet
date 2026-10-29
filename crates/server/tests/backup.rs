//! End to end: a backup downloaded from one device restores on another
//! (from the device itself, which needs no login).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use marqueet_core::Rgb;
use marqueet_core::provider::{BoxFuture, DataProvider, LeagueInfo, ProviderError, Scoreboard};
use marqueet_core::settings::Settings;
use marqueet_core::sports::{LeagueId, Sport, TeamColors, TeamId};
use marqueet_core::team_art::TeamArt;
use marqueet_server::{Policy, SettingsStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct NoGames;

impl DataProvider for NoGames {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn leagues(&self) -> Vec<LeagueInfo> {
        ["nfl", "mlb"]
            .into_iter()
            .map(|l| LeagueInfo { id: LeagueId::new(l), sport: Sport::Football, name: l.into() })
            .collect()
    }

    fn scoreboard<'a>(&'a self, _: &'a LeagueId) -> BoxFuture<'a, Result<Scoreboard, ProviderError>> {
        Box::pin(async { Ok(Scoreboard::default()) })
    }
}

async fn start(db: SettingsStore, settings: Settings) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let providers = marqueet_server::Providers {
        scores: Arc::new(NoGames),
        weather: None,
        weather_alerts: None,
        fantasy: None,
        cameras: None,
    };
    tokio::spawn(marqueet_server::run(
        listener,
        providers,
        settings,
        Policy::default(),
        Some(db),
        marqueet_server::AdminOptions::default(),
        std::future::pending(),
    ));
    addr
}

/// A raw HTTP/1.1 request; returns the status and body.
async fn request(addr: std::net::SocketAddr, head: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!("{head}\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    tcp.write_all(req.as_bytes()).await.unwrap();
    tcp.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    tcp.read_to_end(&mut raw).await.unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let status = String::from_utf8_lossy(&raw[..split]).split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, raw[split + 4..].to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_moves_settings_and_team_art_to_a_new_device() {
    let team = TeamId("fake:mlb:19".into());
    let old = SettingsStore::in_memory().unwrap();
    let settings = Settings {
        leagues: vec![LeagueId::new("mlb")],
        favorites: vec![team.clone()],
        time_zone: Some("America/Los_Angeles".into()),
        ..Settings::default()
    };
    old.save(&settings).unwrap();
    let colors = TeamColors { primary: Rgb::new(0, 90, 156), secondary: None };
    let art =
        TeamArt { label: "LA baseball".into(), colors: Some(colors), logo: None, words: Default::default(), art: None };
    old.set_team_art(&team, &art).unwrap();
    old.set_ha_token(Some("made-up.token")).unwrap();
    let from = start(old, settings.clone()).await;

    let (status, file) = request(from, "GET /admin/backup.json HTTP/1.1", b"").await;
    assert_eq!(status, 200);
    assert!(!String::from_utf8_lossy(&file).contains("made-up.token"), "secrets stay behind");

    let new = SettingsStore::in_memory().unwrap();
    let to = start(new.clone(), Settings::default()).await;
    let mut body = b"--x\r\nContent-Disposition: form-data; name=\"backup\"; filename=\"b.json\"\r\n\
                     Content-Type: application/json\r\n\r\n"
        .to_vec();
    body.extend_from_slice(&file);
    body.extend_from_slice(b"\r\n--x--\r\n");
    let head = "POST /admin/backup/restore HTTP/1.1\r\nContent-Type: multipart/form-data; boundary=x";
    let (status, page) = request(to, head, &body).await;
    assert_eq!(status, 303, "{}", String::from_utf8_lossy(&page));

    let restored = new.load().unwrap().unwrap();
    assert_eq!(
        (restored.leagues, restored.favorites, restored.time_zone),
        (settings.leagues, settings.favorites, settings.time_zone)
    );
    assert_eq!(new.team_art().unwrap().get(&team), Some(&art));
    assert!(new.ha_token().unwrap().is_none());

    let (status, page) = request(
        to,
        head,
        b"--x\r\nContent-Disposition: form-data; name=\"backup\"; filename=\"b.json\"\r\n\r\n{}\r\n--x--\r\n",
    )
    .await;
    assert_eq!(status, 400);
    assert!(String::from_utf8_lossy(&page).contains("Marqueet backup"));
}
