//! Defense against DNS rebinding: every request must name this device in its
//! `Host` header (an IP address, `localhost`, or the device's own name), and
//! a browser may only open the display WebSocket from one of those pages.
//!
//! Without this, a web page at `evil.example` that re-resolves to 127.0.0.1
//! could reach the admin page from a browser on the device, where loopback
//! needs no login. An IP literal can't be rebound, so IPs always pass.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::header::{HOST, ORIGIN};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The names this device answers to, besides IP addresses.
#[derive(Clone, Debug)]
pub struct AllowedHosts {
    names: Vec<String>,
}

/// `host[:port]` → `host` (keeping IPv6 brackets off).
fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

impl AllowedHosts {
    /// `localhost`, the device's `hostname` and `hostname.local`, and any
    /// `extra` names (a reverse proxy's, say).
    pub fn new(hostname: Option<&str>, extra: &[String]) -> AllowedHosts {
        let mut names = vec!["localhost".to_owned()];
        if let Some(h) = hostname.map(str::to_lowercase).filter(|h| !h.is_empty()) {
            names.push(format!("{h}.local"));
            names.push(h);
        }
        names.extend(extra.iter().map(|n| n.trim().trim_end_matches('.').to_lowercase()).filter(|n| !n.is_empty()));
        AllowedHosts { names }
    }

    /// True for a `Host` value naming this device. A missing header (old
    /// HTTP/1.0 clients) passes: browsers always send one.
    pub fn allows(&self, host: Option<&str>) -> bool {
        let Some(host) = host else { return true };
        let name = strip_port(host.trim()).trim_end_matches('.').to_lowercase();
        name.parse::<IpAddr>().is_ok() || self.names.contains(&name)
    }

    /// True when a browser's `Origin` is a page on this device (or absent,
    /// as from the display and scripts).
    pub fn allows_origin(&self, origin: Option<&str>) -> bool {
        let Some(origin) = origin else { return true };
        let host = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
        host.is_some_and(|h| !h.contains('/') && self.allows(Some(h)))
    }
}

/// Middleware: 421 for any request whose `Host` isn't this device.
pub async fn check(State(hosts): State<Arc<AllowedHosts>>, request: Request, next: Next) -> Response {
    let host = request.headers().get(HOST).map(|h| h.to_str().unwrap_or("\u{0}"));
    if !hosts.allows(host) {
        log::warn!("refused a request for host {:?}", host.unwrap_or_default());
        return (StatusCode::MISDIRECTED_REQUEST, "this Marqueet doesn't answer to that name").into_response();
    }
    let origin = request.headers().get(ORIGIN).map(|h| h.to_str().unwrap_or("\u{0}"));
    if request.uri().path() == "/ws" && !hosts.allows_origin(origin) {
        return (StatusCode::FORBIDDEN, "cross-site WebSocket refused").into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> AllowedHosts {
        AllowedHosts::new(Some("Marqueet"), &["ticker.example.com".into()])
    }

    #[test]
    fn this_device_by_ip_or_name() {
        let h = hosts();
        for ok in [
            "marqueet.local:7878",
            "MARQUEET.local",
            "marqueet",
            "localhost:7878",
            "127.0.0.1:7878",
            "192.168.1.20",
            "[::1]:7878",
            "[fe80::1]",
            "marqueet.local.",
            "ticker.example.com",
        ] {
            assert!(h.allows(Some(ok)), "{ok}");
        }
        assert!(h.allows(None), "no Host header");
    }

    #[test]
    fn other_names_are_refused() {
        let h = hosts();
        for bad in ["evil.example:7878", "marqueet.local.evil.example", "localhost.evil", "", "marqueet.lan", "x"] {
            assert!(!h.allows(Some(bad)), "{bad}");
        }
    }

    #[test]
    fn websocket_origins() {
        let h = hosts();
        assert!(h.allows_origin(None), "the display sends none");
        assert!(h.allows_origin(Some("http://marqueet.local:7878")));
        assert!(h.allows_origin(Some("http://127.0.0.1:7878")));
        assert!(!h.allows_origin(Some("https://evil.example")));
        assert!(!h.allows_origin(Some("null")));
        assert!(!h.allows_origin(Some("http://marqueet.local/../evil")));
    }
}
