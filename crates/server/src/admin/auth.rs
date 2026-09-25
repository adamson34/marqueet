//! Who may change settings.
//!
//! - From the device itself (loopback): always, no login. The kiosk has no
//!   keyboard, and anyone on the device already controls it.
//! - From the network, before an admin password exists (first boot): only
//!   the setup page, which needs the one-time 6-digit code shown on the
//!   device's screen. Setting the password there logs you in and retires the
//!   code.
//! - Guessing is throttled (see [`Throttle`]): one code or password check at
//!   a time across the whole server, and a growing wait after failures, so
//!   the 6-digit code takes months to brute-force and a login flood can't
//!   pin the CPU. Wrong guesses don't change the code, so they can't lock
//!   the owner out of setup either.
//! - From the network afterwards: after logging in. The session is an
//!   HttpOnly, SameSite=Strict cookie holding a random token; sessions live
//!   in memory, so a restart logs everyone out.
//! - A password from `MARQUEET_ADMIN_PASSWORD` / `--admin-password` wins over
//!   the stored one (headless installs), and there is no setup mode.
//! - Any form POST whose `Origin` names another site is refused, so a web
//!   page open on the device can't submit the admin form behind your back.
//!
//! The stored password is a PBKDF2 hash ([`super::password`]); checking or
//! setting it is slow on purpose, so callers run [`Auth::login`] and
//! [`Auth::finish_setup`] on a blocking thread.

use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use axum::http::header::{COOKIE, HOST, ORIGIN};
use marqueet_core::protocol::SetupInfo;
use tokio::sync::watch;

use super::password;
use crate::settings_store::SettingsStore;

pub const COOKIE_NAME: &str = "marqueet_session";
const MAX_SESSIONS: usize = 16;
/// Failures are forgotten after this long without another.
const FAILURE_MEMORY: Duration = Duration::from_secs(15 * 60);
/// The longest wait between guesses.
const MAX_WAIT: Duration = Duration::from_secs(60);

/// Wait before the next guess after `failures` recent failures: a second
/// for the first few (typos), then doubling up to a minute.
pub fn wait_after(failures: u32) -> Duration {
    match failures {
        0 => Duration::ZERO,
        1..=3 => Duration::from_secs(1),
        n => Duration::from_secs(1u64 << (n - 3).min(6)).min(MAX_WAIT),
    }
}

#[derive(Debug, Default)]
struct ThrottleState {
    /// A check is running.
    busy: bool,
    failures: u32,
    last_failure: Option<Instant>,
    next_allowed: Option<Instant>,
}

/// One setup-code or password check at a time, server-wide, with a growing
/// wait after failures (shared by `/setup` and `/login`).
#[derive(Debug, Default)]
pub struct Throttle {
    state: Mutex<ThrottleState>,
}

impl Throttle {
    fn state(&self) -> MutexGuard<'_, ThrottleState> {
        lock(&self.state)
    }

    /// Starts a check, or says how long to wait first.
    fn begin(&self, now: Instant) -> Result<(), Duration> {
        let mut s = self.state();
        if s.last_failure.is_some_and(|t| now.duration_since(t) > FAILURE_MEMORY) {
            *s = ThrottleState { busy: s.busy, ..ThrottleState::default() };
        }
        if s.busy {
            return Err(Duration::from_secs(1));
        }
        if let Some(next) = s.next_allowed.filter(|n| *n > now) {
            return Err(next - now);
        }
        s.busy = true;
        Ok(())
    }

    fn end(&self) {
        self.state().busy = false;
    }

    fn record(&self, ok: bool, now: Instant) {
        let mut s = self.state();
        if ok {
            s.failures = 0;
            s.next_allowed = None;
        } else {
            s.failures += 1;
            s.last_failure = Some(now);
            s.next_allowed = Some(now + wait_after(s.failures));
        }
    }
}

/// A running check; dropping it lets the next one start.
#[derive(Debug)]
pub struct Attempt(Arc<Auth>);

impl Drop for Attempt {
    fn drop(&mut self) {
        self.0.throttle.end();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Granted,
    /// Remote, password set, not logged in.
    NeedsLogin,
    /// Remote, and no password yet: only the setup page is open.
    Setup,
}

#[derive(Debug)]
enum Credential {
    /// First boot: waiting for setup.
    None,
    /// From the environment or command line.
    Given(String),
    /// A stored PBKDF2 hash.
    Stored(String),
}

#[derive(Debug)]
struct SetupCode {
    code: String,
}

/// Why setup didn't complete.
#[derive(Debug, PartialEq, Eq)]
pub enum SetupError {
    /// The device already has a password.
    Done,
    /// Wrong code (the code on screen stays the same).
    WrongCode,
    Invalid(String),
    Internal(String),
}

pub struct Auth {
    credential: Mutex<Credential>,
    sessions: Mutex<VecDeque<String>>,
    setup: Mutex<Option<SetupCode>>,
    /// What the first-boot screen shows; `None` once set up.
    setup_info: watch::Sender<Option<SetupInfo>>,
    urls: Vec<String>,
    db: Option<SettingsStore>,
    throttle: Throttle,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth").field("setup_pending", &self.setup_pending()).finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A uniformly random 6-digit code.
fn new_code() -> Option<String> {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).ok()?;
        let n = u32::from_le_bytes(b);
        // Reject the top sliver so every code is equally likely.
        if n < u32::MAX - u32::MAX % 1_000_000 {
            return Some(format!("{:06}", n % 1_000_000));
        }
    }
}

impl Auth {
    /// `given`: a password from the environment. `db`: where a password set
    /// at first boot lives. `urls`: where the setup page can be reached.
    pub fn new(given: Option<String>, db: Option<SettingsStore>, urls: Vec<String>) -> Auth {
        let credential = match given.filter(|p| !p.is_empty()) {
            Some(p) => Credential::Given(p),
            None => match db.as_ref().and_then(|d| d.admin_password_hash().ok().flatten()) {
                Some(hash) => Credential::Stored(hash),
                None => Credential::None,
            },
        };
        let (setup_info, _) = watch::channel(None);
        let auth = Auth {
            credential: Mutex::new(credential),
            sessions: Mutex::default(),
            setup: Mutex::new(None),
            setup_info,
            urls,
            db,
            throttle: Throttle::default(),
        };
        if auth.setup_pending() {
            auth.new_setup_code();
        }
        auth
    }

    /// True until an admin password exists.
    pub fn setup_pending(&self) -> bool {
        matches!(*lock(&self.credential), Credential::None)
    }

    /// True when logging in from the network is possible.
    pub fn has_password(&self) -> bool {
        !self.setup_pending()
    }

    fn new_setup_code(&self) {
        let code = new_code();
        *lock(&self.setup) = code.clone().map(|code| SetupCode { code });
        match code {
            // The screen only shows setup when the page is reachable from
            // another device.
            Some(code) if !self.urls.is_empty() => {
                log::info!("setup code {code}: open {} and enter it", self.urls[0]);
                self.setup_info.send_replace(Some(SetupInfo { code, urls: self.urls.clone() }));
            }
            Some(_) => {}
            None => log::error!("no randomness for a setup code"),
        }
    }

    /// The first-boot screen's content, for displays on the device.
    pub fn subscribe_setup(&self) -> watch::Receiver<Option<SetupInfo>> {
        self.setup_info.subscribe()
    }

    fn sessions(&self) -> MutexGuard<'_, VecDeque<String>> {
        lock(&self.sessions)
    }

    fn new_session(&self) -> Option<String> {
        let token = new_token()?;
        let mut sessions = self.sessions();
        sessions.push_back(token.clone());
        while sessions.len() > MAX_SESSIONS {
            sessions.pop_front();
        }
        Some(token)
    }

    pub fn check(&self, peer: IpAddr, headers: &HeaderMap) -> Access {
        if is_local(peer) {
            return Access::Granted;
        }
        if self.setup_pending() {
            return Access::Setup;
        }
        match cookie(headers, COOKIE_NAME) {
            Some(token) if self.sessions().iter().any(|s| ct_eq(s.as_bytes(), token.as_bytes())) => Access::Granted,
            _ => Access::NeedsLogin,
        }
    }

    /// Starts a code or password check, or says how long to wait before
    /// trying again. Hold the [`Attempt`] until the check is done.
    pub fn begin_attempt(self: &Arc<Self>) -> Result<Attempt, Duration> {
        self.throttle.begin(Instant::now())?;
        Ok(Attempt(Arc::clone(self)))
    }

    /// A new session token if `attempt` is the password. Slow (hashing).
    pub fn login(&self, attempt: &str) -> Option<String> {
        let ok = match &*lock(&self.credential) {
            Credential::None => false,
            Credential::Given(p) => ct_eq(p.as_bytes(), attempt.as_bytes()),
            Credential::Stored(hash) => password::verify(attempt, hash),
        };
        self.throttle.record(ok, Instant::now());
        if ok { self.new_session() } else { None }
    }

    /// First boot: checks the code, stores `new_password` and returns a
    /// session. Slow (hashing).
    pub fn finish_setup(&self, code: &str, new_password: &str) -> Result<String, SetupError> {
        self.finish_setup_with(code, new_password, password::ITERATIONS)
    }

    pub(crate) fn finish_setup_with(
        &self,
        code: &str,
        new_password: &str,
        iterations: u32,
    ) -> Result<String, SetupError> {
        {
            let setup = lock(&self.setup);
            let Some(current) = setup.as_ref() else { return Err(SetupError::Done) };
            let right = ct_eq(current.code.as_bytes(), code.trim().as_bytes());
            drop(setup);
            self.throttle.record(right, Instant::now());
            if !right {
                log::warn!("setup: wrong code entered");
                return Err(SetupError::WrongCode);
            }
        }
        password::check_rules(new_password).map_err(SetupError::Invalid)?;
        let hash = password::hash_with(new_password, iterations)
            .ok_or_else(|| SetupError::Internal("couldn't hash the password".into()))?;
        if let Some(db) = &self.db {
            db.set_admin_password_hash(Some(&hash)).map_err(|e| SetupError::Internal(e.to_string()))?;
        }
        {
            let mut credential = lock(&self.credential);
            if !matches!(*credential, Credential::None) {
                return Err(SetupError::Done);
            }
            *credential = Credential::Stored(hash);
        }
        *lock(&self.setup) = None;
        self.setup_info.send_replace(None);
        log::info!("setup complete: admin password created");
        self.new_session().ok_or_else(|| SetupError::Internal("couldn't start a session".into()))
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
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub(crate) fn new_token() -> Option<String> {
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

    fn given(p: &str) -> Auth {
        Auth::new(Some(p.into()), None, urls())
    }

    fn urls() -> Vec<String> {
        vec!["http://marqueet.local:7878/setup".into()]
    }

    #[test]
    fn device_needs_no_login() {
        let auth = Auth::new(None, None, vec![]);
        assert_eq!(auth.check("127.0.0.1".parse().unwrap(), &HeaderMap::new()), Access::Granted);
        assert_eq!(auth.check("::1".parse().unwrap(), &HeaderMap::new()), Access::Granted);
        assert_eq!(auth.check("::ffff:127.0.0.1".parse().unwrap(), &HeaderMap::new()), Access::Granted);
    }

    #[test]
    fn first_boot_sends_the_network_to_setup() {
        let auth = Auth::new(None, None, urls());
        assert_eq!(auth.check(LAN, &HeaderMap::new()), Access::Setup);
        let info = auth.subscribe_setup().borrow().clone().unwrap();
        assert_eq!(info.code.len(), 6);
        assert!(info.code.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(info.urls, ["http://marqueet.local:7878/setup"]);
        assert_eq!(Auth::new(Some(String::new()), None, urls()).check(LAN, &HeaderMap::new()), Access::Setup);
        assert!(Auth::new(None, None, vec![]).subscribe_setup().borrow().is_none(), "loopback-only: nothing to show");
        assert!(given("hunter22").subscribe_setup().borrow().is_none(), "no setup with a given password");
    }

    #[test]
    fn setup_takes_the_code_then_retires_it() {
        let db = SettingsStore::in_memory().unwrap();
        let auth = Auth::new(None, Some(db.clone()), urls());
        let code = auth.subscribe_setup().borrow().clone().unwrap().code;
        let wrong = if code == "000000" { "111111" } else { "000000" };
        assert_eq!(auth.finish_setup_with(wrong, "long enough", 1000), Err(SetupError::WrongCode));
        assert!(matches!(auth.finish_setup_with(&code, "short", 1000), Err(SetupError::Invalid(_))));
        let token = auth.finish_setup_with(&code, "long enough", 1000).unwrap();
        let with = headers(&[("cookie", &format!("{COOKIE_NAME}={token}"))]);
        assert_eq!(auth.check(LAN, &with), Access::Granted, "setting up logs you in");
        assert_eq!(auth.check(LAN, &HeaderMap::new()), Access::NeedsLogin);
        assert!(auth.subscribe_setup().borrow().is_none(), "the screen stops showing a code");
        assert_eq!(auth.finish_setup_with(&code, "another one", 1000), Err(SetupError::Done));
        assert!(auth.login("long enough").is_some() && auth.login("long enougH").is_none());
        // A restart keeps the password.
        let again = Auth::new(None, Some(db), vec![]);
        assert!(!again.setup_pending() && again.login("long enough").is_some());
    }

    #[test]
    fn wrong_codes_never_change_the_code_on_screen() {
        let auth = Auth::new(None, None, urls());
        let rx = auth.subscribe_setup();
        let first = rx.borrow().clone().unwrap().code;
        let wrong = if first == "000000" { "111111" } else { "000000" };
        for _ in 0..20 {
            assert_eq!(auth.finish_setup_with(wrong, "long enough", 1000), Err(SetupError::WrongCode));
        }
        assert_eq!(rx.borrow().clone().unwrap().code, first, "an attacker can't swap it out from under the owner");
        assert!(auth.finish_setup_with(&first, "long enough", 1000).is_ok());
    }

    #[test]
    fn waits_grow_after_failures() {
        let secs: Vec<u64> = (0..12).map(|n| wait_after(n).as_secs()).collect();
        assert_eq!(secs, [0, 1, 1, 1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }

    #[test]
    fn one_check_at_a_time_and_failures_slow_the_next() {
        let t = Throttle::default();
        let now = Instant::now();
        assert!(t.begin(now).is_ok());
        assert!(t.begin(now).is_err(), "a second check waits for the first");
        t.end();
        for _ in 0..5 {
            t.record(false, now);
        }
        assert_eq!(t.begin(now), Err(Duration::from_secs(4)));
        assert!(t.begin(now + Duration::from_secs(4)).is_ok());
        t.end();
        t.record(true, now + Duration::from_secs(4));
        assert!(t.begin(now + Duration::from_secs(4)).is_ok(), "success clears the wait");
        t.end();
        for _ in 0..9 {
            t.record(false, now);
        }
        let later = now + FAILURE_MEMORY + Duration::from_secs(1);
        assert!(t.begin(later).is_ok(), "failures are forgotten after a quiet spell");
    }

    #[test]
    fn attempts_release_when_dropped() {
        let auth = Arc::new(given("pw"));
        let a = auth.begin_attempt().unwrap();
        assert!(auth.begin_attempt().is_err());
        drop(a);
        assert!(auth.begin_attempt().is_ok());
    }

    #[test]
    fn login_issues_a_session() {
        let auth = given("hunter22");
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
        let auth = given("pw");
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
