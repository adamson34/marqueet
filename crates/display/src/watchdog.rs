//! Restarts a frozen display. The app loop beats a heartbeat on every turn; a
//! thread checks it, and if the loop hasn't turned for a while (stuck in a
//! loop, a hung GPU call) it exits so the service manager (snapd or systemd,
//! both set to restart) brings the display back, instead of leaving a frozen
//! picture on the TV.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// How long the loop may stop before the display restarts.
pub const LIMIT: Duration = Duration::from_secs(30);

/// Beaten by the app loop.
#[derive(Clone, Debug)]
pub struct Heartbeat {
    start: Instant,
    /// Milliseconds after `start` of the last beat.
    last: Arc<AtomicU64>,
}

impl Heartbeat {
    pub fn beat(&self) {
        self.last.store(millis(self.start.elapsed()), Ordering::Relaxed);
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Starts watching: `on_stall` runs (once) when no beat has come for `limit`.
pub fn watch(limit: Duration, on_stall: impl FnOnce(Duration) + Send + 'static) -> Heartbeat {
    let heart = Heartbeat { start: Instant::now(), last: Arc::new(AtomicU64::new(0)) };
    let watched = heart.clone();
    let spawned = thread::Builder::new().name("marqueet-watchdog".into()).spawn(move || {
        let every = (limit / 4).max(Duration::from_millis(10));
        loop {
            thread::sleep(every);
            let quiet = millis(watched.start.elapsed()).saturating_sub(watched.last.load(Ordering::Relaxed));
            if quiet >= millis(limit) {
                on_stall(Duration::from_millis(quiet));
                return;
            }
        }
    });
    if let Err(e) = spawned {
        log::warn!("no display watchdog: {e}");
    }
    heart
}

/// The display's watchdog: logs and exits so the service restarts it.
pub fn start() -> Heartbeat {
    watch(LIMIT, |quiet| {
        log::error!("display stuck for {}s; exiting so it restarts", quiet.as_secs());
        std::process::exit(70);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn flag() -> (Arc<AtomicBool>, impl FnOnce(Duration) + Send + 'static) {
        let fired = Arc::new(AtomicBool::new(false));
        let f = fired.clone();
        (fired, move |_| f.store(true, Ordering::SeqCst))
    }

    #[test]
    fn a_silent_loop_trips_it() {
        let (fired, on_stall) = flag();
        let _heart = watch(Duration::from_millis(40), on_stall);
        thread::sleep(Duration::from_millis(300));
        assert!(fired.load(Ordering::SeqCst));
    }

    #[test]
    fn a_beating_loop_does_not() {
        let (fired, on_stall) = flag();
        let heart = watch(Duration::from_millis(200), on_stall);
        for _ in 0..30 {
            heart.beat();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!fired.load(Ordering::SeqCst));
    }
}
