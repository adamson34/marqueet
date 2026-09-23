//! Who may change settings.
//!
//! - From the device itself (loopback): always, no login. The kiosk has no
//!   keyboard and the first-boot flow doesn't exist yet.
//! - From the network: only when the server was started with an admin
//!   password, after logging in. The session is an HttpOnly, SameSite=Strict
//!   cookie holding a random token; sessions live in memory, so a restart
//!   logs everyone out.
//! - Any form POST whose `Origin` names another site is refused, so a web
//!   page open on the device can't submit the admin form behind your back.

use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Mutex;

use axum::http::HeaderMap;
use axum::http::header::{COOKIE, HOST, ORIGIN};

pub const COOKIE_NAME: &str = "marqueet_session";
const MAX_SESSIONS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Granted,
    /// Remote, password set, not logged in.
    NeedsLogin,
    /// Remote and no password configured.
    Forbidden,
}

pub struct Auth {
    password: Option<String>,
    sessions: Mutex<VecDeque<String>>,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth").field("password_set", &self.password.is_some()).finish_non_exhaustive()
    }
}

impl Auth {
    pub fn new(password: Option<String>) -> Auth {
        Auth { password: password.filter(|p| !p.is_empty()), sessions: Mutex::default() }
    }

    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, VecDeque<String>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn check(&self, peer: IpAddr, headers: &HeaderMap) -> Access {
        if is_local(peer) {
            return Access::Granted;
        }
        if self.password.is_none() {
            return Access::Forbidden;
        }
        match cookie(headers, COOKIE_NAME) {
            Some(token) if self.sessions().iter().any(|s| ct_eq(s.as_bytes(), token.as_bytes())) => Access::Granted,
            _ => Access::NeedsLogin,
        }
    }

    /// A new session token if `attempt` is the password.
    pub fn login(&self, attempt: &str) -> Option<String> {
        let password = self.password.as_deref()?;
        if !ct_eq(password.as_bytes(), attempt.as_bytes()) {
            return None;
        }
        let token = new_token()?;
        let mut sessions = self.sessions();
        sessions.push_back(token.clone());
        while sessions.len() > MAX_SESSIONS {
            sessions.pop_front();
        }
        Some(token)
    }

    pub fn logout(&self, headers: &HeaderMap) {
        if let Some(token) = cookie(headers, COOKIE_NAME) {
            self.sessions().retain(|s| !ct_eq(s.as_bytes(), token.as_bytes()));
        }
    }
}

/// Loopback, including IPv4-mapped IPv6 (`::ffff:127.0.0.1`).
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

/// False when the request says it came from a page on another origin.
/// Requests without `Origin` (curl, older browsers on same-origin GETs) pass;
/// the session cookie is SameSite=Strict, which covers those browsers.
pub fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN) else { return true };
    let (Ok(origin), Some(Ok(host))) = (origin.to_str(), headers.get(HOST).map(|h| h.to_str())) else {
        return false;
    };
    let origin_host = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
    origin_host == Some(host)
}

pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

pub fn session_cookie(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict")
}

pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// Compares without an early exit, so timing doesn't reveal how many leading
/// bytes matched. (Length can differ observably; that's fine.)
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    if let Err(e) = getrandom::fill(&mut bytes) {
        log::error!("no randomness for a session token: {e}");
        return None;
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const LAN: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 20));

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn device_needs_no_login() {
        let auth = Auth::new(None);
        assert_eq!(auth.check("127.0.0.1".parse().unwrap(), &HeaderMap::new()), Access::Granted);
        assert_eq!(auth.check("::1".parse().unwrap(), &HeaderMap::new()), Access::Granted);
        assert_eq!(auth.check("::ffff:127.0.0.1".parse().unwrap(), &HeaderMap::new()), Access::Granted);
    }

    #[test]
    fn network_is_closed_without_a_password() {
        assert_eq!(Auth::new(None).check(LAN, &HeaderMap::new()), Access::Forbidden);
        assert_eq!(Auth::new(Some(String::new())).check(LAN, &HeaderMap::new()), Access::Forbidden);
    }

    #[test]
    fn login_issues_a_session() {
        let auth = Auth::new(Some("hunter22".into()));
        assert_eq!(auth.check(LAN, &HeaderMap::new()), Access::NeedsLogin);
        assert_eq!(auth.login("hunter2"), None);
        assert_eq!(auth.login("hunter222"), None);
        let token = auth.login("hunter22").unwrap();
        assert_eq!(token.len(), 64);
        let with = |t: &str| headers(&[("cookie", &format!("theme=dark; {COOKIE_NAME}={t}"))]);
        assert_eq!(auth.check(LAN, &with(&token)), Access::Granted);
        assert_eq!(auth.check(LAN, &with("forged")), Access::NeedsLogin);
        auth.logout(&with(&token));
        assert_eq!(auth.check(LAN, &with(&token)), Access::NeedsLogin);
    }

    #[test]
    fn old_sessions_are_evicted() {
        let auth = Auth::new(Some("pw".into()));
        let first = auth.login("pw").unwrap();
        for _ in 0..MAX_SESSIONS {
            auth.login("pw").unwrap();
        }
        let h = headers(&[("cookie", &format!("{COOKIE_NAME}={first}"))]);
        assert_eq!(auth.check(LAN, &h), Access::NeedsLogin);
    }

    #[test]
    fn cross_site_posts_are_refused() {
        assert!(same_origin(&headers(&[("host", "marqueet.local:7878")])));
        assert!(same_origin(&headers(&[("host", "marqueet.local:7878"), ("origin", "http://marqueet.local:7878")])));
        assert!(!same_origin(&headers(&[("host", "127.0.0.1:7878"), ("origin", "https://evil.example")])));
        assert!(!same_origin(&headers(&[("host", "127.0.0.1:7878"), ("origin", "null")])));
        assert!(!same_origin(&headers(&[("origin", "http://127.0.0.1:7878")])));
    }
}
