//! Gate process liveness. Observational only: never an authorization input.
//!
//! A background task stamps a heartbeat every few seconds. The audit snapshot
//! reports uptime and heartbeat age so the dashboard can say whether the gate's
//! own runtime is running, separately from the router checkpoint (which comes
//! from the router, not the gate). The heartbeat stays in-process and is not
//! published to the bus: a liveness event on the checkpoint chain would move the
//! chain head that IACs anchor to.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub const HEARTBEAT_INTERVAL_SECS: u64 = 5;
/// The heartbeat is stale after three missed intervals.
pub const HEARTBEAT_STALE_SECS: i64 = 3 * HEARTBEAT_INTERVAL_SECS as i64;

#[derive(Debug)]
pub struct Liveness {
    started_at: i64,
    last_beat: AtomicI64,
}

impl Default for Liveness {
    fn default() -> Self {
        Self::new(now_secs())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivenessReport {
    pub started_at: i64,
    pub uptime_secs: i64,
    pub heartbeat_age_secs: i64,
    /// False once the heartbeat is older than three intervals.
    pub heartbeat_ok: bool,
}

impl Liveness {
    pub fn new(now: i64) -> Self {
        Self {
            started_at: now,
            last_beat: AtomicI64::new(now),
        }
    }

    pub fn beat(&self, now: i64) {
        self.last_beat.store(now, Ordering::Relaxed);
    }

    pub fn report(&self, now: i64) -> LivenessReport {
        // A clock that moved backwards is reported as stale, never as fresh.
        let raw_age = now - self.last_beat.load(Ordering::Relaxed);
        let ok = (0..=HEARTBEAT_STALE_SECS).contains(&raw_age);
        LivenessReport {
            started_at: self.started_at,
            uptime_secs: now.saturating_sub(self.started_at).max(0),
            heartbeat_age_secs: raw_age.max(0),
            heartbeat_ok: ok,
        }
    }
}

pub fn spawn_heartbeat(liveness: Arc<Liveness>) {
    tokio::spawn(async move {
        let mut tick =
            tokio::time::interval(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        loop {
            tick.tick().await;
            liveness.beat(now_secs());
        }
    });
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_heartbeat_is_ok() {
        let l = Liveness::new(1_000);
        l.beat(1_010);
        let r = l.report(1_012);
        assert!(r.heartbeat_ok);
        assert_eq!((r.uptime_secs, r.heartbeat_age_secs), (12, 2));
    }

    #[test]
    fn stale_after_three_missed_intervals() {
        let l = Liveness::new(1_000);
        assert!(l.report(1_000 + HEARTBEAT_STALE_SECS).heartbeat_ok);
        assert!(!l.report(1_000 + HEARTBEAT_STALE_SECS + 1).heartbeat_ok);
    }

    #[test]
    fn clock_going_backwards_is_not_ok() {
        let l = Liveness::new(1_000);
        l.beat(2_000);
        let r = l.report(1_500);
        assert!(!r.heartbeat_ok);
        assert_eq!(r.heartbeat_age_secs, 0);
    }

    #[test]
    fn beat_resets_age() {
        let l = Liveness::new(0);
        assert!(!l.report(100).heartbeat_ok);
        l.beat(100);
        assert!(l.report(101).heartbeat_ok);
    }
}
