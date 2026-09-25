//! Live feed from `marqueet-server` over WebSocket.
//!
//! A background thread owns the connection and hands events to the render
//! loop through a channel, so the display needs no async runtime. It
//! reconnects forever with backoff (0.5 s doubling to 10 s); the scene keeps
//! showing the last content while disconnected.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use marqueet_core::protocol::{PROTOCOL_VERSION, ServerMsg};
use tungstenite::Message;

pub const DEFAULT_URL: &str = "ws://127.0.0.1:7878/ws";

#[derive(Debug, Clone, PartialEq)]
pub enum FeedEvent {
    Connected,
    Message(ServerMsg),
    Disconnected(String),
}

#[derive(Debug)]
pub struct LiveFeed {
    rx: Receiver<FeedEvent>,
}

impl LiveFeed {
    pub fn connect(url: &str) -> LiveFeed {
        let (tx, rx) = mpsc::channel();
        let url = url.to_owned();
        let spawned = thread::Builder::new().name("marqueet-feed".into()).spawn(move || run(&url, &tx));
        if let Err(e) = spawned {
            log::error!("could not start feed thread: {e}");
        }
        LiveFeed { rx }
    }

    /// Events received since the last call; never blocks.
    pub fn drain(&self) -> Vec<FeedEvent> {
        self.rx.try_iter().collect()
    }
}

const RETRY_MIN: Duration = Duration::from_millis(500);
const RETRY_MAX: Duration = Duration::from_secs(10);

/// Runs until the receiving side (the scene) is dropped.
fn run(url: &str, tx: &Sender<FeedEvent>) {
    let mut delay = RETRY_MIN;
    loop {
        let reason = match tungstenite::connect(url) {
            Ok((mut ws, _)) => {
                delay = RETRY_MIN;
                if tx.send(FeedEvent::Connected).is_err() {
                    return;
                }
                loop {
                    match ws.read() {
                        Ok(Message::Text(text)) => match ServerMsg::from_json(&text) {
                            Ok(msg) => {
                                if let ServerMsg::Hello { protocol, server_version } = &msg
                                    && *protocol != PROTOCOL_VERSION
                                {
                                    log::warn!(
                                        "server {server_version} speaks protocol {protocol}, display expects {PROTOCOL_VERSION}"
                                    );
                                }
                                if tx.send(FeedEvent::Message(msg)).is_err() {
                                    return;
                                }
                            }
                            Err(e) => log::warn!("ignoring unreadable message from server: {e}"),
                        },
                        Ok(Message::Close(_)) => break "server closed the connection".to_owned(),
                        Ok(_) => {}
                        Err(e) => break e.to_string(),
                    }
                }
            }
            Err(e) => e.to_string(),
        };
        if tx.send(FeedEvent::Disconnected(reason)).is_err() {
            return;
        }
        thread::sleep(delay);
        delay = (delay * 2).min(RETRY_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::protocol::{Content, FeedStatus};
    use std::net::TcpListener;
    use std::time::Instant;

    fn content() -> ServerMsg {
        ServerMsg::Content(Content {
            ticker: vec![],
            crawl: vec![],
            crawl_label: None,
            status: FeedStatus::default(),
            widgets: vec![],
        })
    }

    /// Collects events until `pred` matches or 5 s pass.
    fn wait_for(feed: &LiveFeed, events: &mut Vec<FeedEvent>, pred: impl Fn(&[FeedEvent]) -> bool) {
        let start = Instant::now();
        while !pred(events) {
            assert!(start.elapsed() < Duration::from_secs(5), "timed out; got {events:?}");
            events.extend(feed.drain());
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn receives_messages_and_reconnects_after_the_server_drops() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/ws", listener.local_addr().unwrap());
        thread::spawn(move || {
            for (i, stream) in listener.incoming().take(2).enumerate() {
                let mut ws = tungstenite::accept(stream.unwrap()).unwrap();
                let hello = ServerMsg::Hello { protocol: PROTOCOL_VERSION, server_version: format!("t{i}") };
                ws.send(Message::Text(hello.to_json().into())).unwrap();
                ws.send(Message::Text(content().to_json().into())).unwrap();
                if i == 0 {
                    // First connection: hang up so the client must reconnect.
                    let _ = ws.close(None);
                    let _ = ws.flush();
                } else {
                    thread::sleep(Duration::from_secs(2));
                }
            }
        });

        let feed = LiveFeed::connect(&url);
        let mut events = Vec::new();
        wait_for(&feed, &mut events, |e| e.iter().filter(|x| **x == FeedEvent::Connected).count() >= 2);
        assert_eq!(events[0], FeedEvent::Connected);
        assert!(matches!(events[1], FeedEvent::Message(ServerMsg::Hello { .. })));
        assert_eq!(events[2], FeedEvent::Message(content()));
        assert!(events.iter().any(|e| matches!(e, FeedEvent::Disconnected(_))), "{events:?}");
    }

    #[test]
    fn unreachable_server_reports_disconnected_and_keeps_trying() {
        // Port 9 (discard) is essentially never listening on loopback.
        let feed = LiveFeed::connect("ws://127.0.0.1:9/ws");
        let mut events = Vec::new();
        wait_for(&feed, &mut events, |e| e.iter().filter(|x| matches!(x, FeedEvent::Disconnected(_))).count() >= 2);
    }
}
