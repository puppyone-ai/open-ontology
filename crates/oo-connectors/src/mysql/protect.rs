//! Source protection (S1-C-02): duty cycle, load pause and resource receipts.
//!
//! One engine holds one connection. Reads rest so read time stays at or
//! below `ReadBudget::duty_cycle_percent` of wall time, pause while
//! `Threads_running` is above a threshold or the free connection margin is
//! below a reserve, and give up after a bounded pause.

use std::time::{Duration, Instant};

use serde::Serialize;

/// Load thresholds. Defaults are on; they are not relaxed for speed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protection {
    /// Pause while `Threads_running` is above this.
    pub threads_running_max: u64,
    /// Pause while `max_connections - Threads_connected` is below this.
    pub connection_reserve: u64,
    pub pause_step: Duration,
    /// Total pause per gate before the read fails as `source_busy`.
    pub max_pause: Duration,
    /// Session `net_read_timeout` / `net_write_timeout`, seconds.
    pub net_timeout_s: u64,
    /// Client timeout on top of the server statement timeout.
    pub client_grace: Duration,
}

impl Default for Protection {
    fn default() -> Self {
        Self {
            threads_running_max: 16,
            connection_reserve: 5,
            pause_step: Duration::from_millis(500),
            max_pause: Duration::from_secs(120),
            net_timeout_s: 600,
            client_grace: Duration::from_secs(2),
        }
    }
}

/// Why a gate pauses, if it does.
pub fn load_verdict(
    p: &Protection,
    threads_running: Option<u64>,
    threads_connected: Option<u64>,
    max_connections: Option<u64>,
) -> Option<&'static str> {
    if threads_running.is_none() || threads_connected.is_none() || max_connections.is_none() {
        return Some("load_unavailable");
    }
    if threads_running.is_some_and(|n| n > p.threads_running_max) {
        return Some("threads_running");
    }
    if let (Some(used), Some(max)) = (threads_connected, max_connections)
        && max.saturating_sub(used) < p.connection_reserve
    {
        return Some("connection_margin");
    }
    None
}

/// Rest after `busy` so that busy / (busy + rest) ≤ `percent` / 100.
pub fn rest_after(busy: Duration, percent: u8) -> Duration {
    let p = u32::from(percent.clamp(1, 100));
    busy * (100 - p) / p
}

/// Resource receipt of one engine session (T8).
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct ProtectionStats {
    pub connects: u64,
    /// Connections held at once by this engine. Always 1 by construction.
    pub peak_connections: u64,
    pub statements: u64,
    pub busy_ms: u64,
    /// Total source-idle time, including client encoding/consumer work.
    pub rest_ms: u64,
    /// Explicit duty-cycle sleep (a subset of `rest_ms`).
    pub sleep_ms: u64,
    pub pauses: u64,
    pub pause_ms: u64,
    pub max_threads_running: u64,
    pub explain_checks: u64,
    pub client_timeouts: u64,
    pub chunks_read: u64,
    pub chunks_over_budget: u64,
    pub first_op_unix_ms: u64,
}

/// Tracks busy time and the earliest moment the next read may start.
#[derive(Clone, Debug)]
pub struct DutyCycle {
    percent: u8,
    next_allowed: Option<Instant>,
}

impl DutyCycle {
    pub fn new(percent: u8) -> Self {
        Self {
            percent: percent.clamp(1, 100),
            next_allowed: None,
        }
    }

    pub fn set_percent(&mut self, percent: u8) {
        self.percent = percent.clamp(1, 100);
    }

    /// How long to wait before the next read may start.
    pub fn wait(&self, now: Instant) -> Duration {
        self.next_allowed
            .map_or(Duration::ZERO, |t| t.saturating_duration_since(now))
    }

    /// Record a read that took `busy` and ended at `now`; returns the rest.
    pub fn record(&mut self, busy: Duration, now: Instant) -> Duration {
        let rest = rest_after(busy, self.percent);
        self.next_allowed = Some(now + rest);
        rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_cycle_rests_three_times_the_read_at_25_percent() {
        assert_eq!(
            rest_after(Duration::from_millis(500), 25),
            Duration::from_millis(1500)
        );
        assert_eq!(rest_after(Duration::from_millis(500), 100), Duration::ZERO);
        assert_eq!(
            rest_after(Duration::from_millis(10), 0),
            Duration::from_millis(990)
        );
        let mut d = DutyCycle::new(25);
        let t0 = Instant::now();
        assert_eq!(d.wait(t0), Duration::ZERO);
        d.record(Duration::from_millis(200), t0);
        assert_eq!(d.wait(t0), Duration::from_millis(600));
        assert_eq!(d.wait(t0 + Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn load_pauses_on_running_threads_or_connection_margin() {
        let p = Protection::default();
        assert_eq!(load_verdict(&p, Some(3), Some(10), Some(151)), None);
        assert_eq!(
            load_verdict(&p, Some(17), Some(10), Some(151)),
            Some("threads_running")
        );
        assert_eq!(
            load_verdict(&p, Some(2), Some(148), Some(151)),
            Some("connection_margin")
        );
        assert_eq!(load_verdict(&p, None, None, None), Some("load_unavailable"));
    }
}
