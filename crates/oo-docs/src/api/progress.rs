//! `file.progress`: the transient event (10 §8.5). No sequence number, not
//! stored, not replayed; at most one per file per second.
//!
//! Stages call [`Bus::emit`]; every open event stream subscribes. A client
//! that needs the current value reads `FileSummary.progress`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::broadcast;

/// Minimum time between two events of one file.
pub const MIN_INTERVAL: Duration = Duration::from_secs(1);
const CAPACITY: usize = 256;
/// Rate limit entries kept before old ones are dropped.
const MAX_TRACKED: usize = 4096;

/// One progress report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// File id.
    pub file_id: String,
    /// Drive id.
    pub drive_id: Option<String>,
    /// `page`, `sheet`, `line` or `batch`.
    pub unit: &'static str,
    /// Units done.
    pub done: i64,
    /// Units in all, when known.
    pub total: Option<i64>,
}

impl Progress {
    /// The `Event` object (`type`, `at`, `drive_id`, `data`), without `seq`.
    pub fn event(&self, at: &str) -> Value {
        json!({
            "type": "file.progress",
            "at": at,
            "drive_id": self.drive_id,
            "data": {
                "file_id": self.file_id,
                "unit": self.unit,
                "done": self.done,
                "total": self.total,
            },
        })
    }

    /// The value for `file.progress` (the column behind `FileSummary.progress`).
    pub fn column(&self) -> Value {
        json!({ "unit": self.unit, "done": self.done, "total": self.total })
    }
}

/// The SSE frame of a transient event: `event:` and `data:`, no `id:`.
pub fn frame(event: &Value) -> String {
    format!("event: file.progress\ndata: {event}\n\n")
}

/// Fan out to every stream.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Arc<Progress>>,
    last: Arc<Mutex<HashMap<String, Instant>>>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    /// A bus with no subscribers.
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(CAPACITY).0,
            last: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// A new stream's receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Progress>> {
        self.tx.subscribe()
    }

    /// Publish unless this file reported less than a second ago. Returns
    /// whether it was sent.
    pub fn emit(&self, p: Progress) -> bool {
        self.emit_at(p, Instant::now())
    }

    /// [`Bus::emit`] with the clock given (tests).
    pub fn emit_at(&self, p: Progress, now: Instant) -> bool {
        {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(t) = last.get(&p.file_id)
                && now.saturating_duration_since(*t) < MIN_INTERVAL
            {
                return false;
            }
            if last.len() >= MAX_TRACKED {
                last.retain(|_, t| now.saturating_duration_since(*t) < MIN_INTERVAL);
            }
            last.insert(p.file_id.clone(), now);
        }
        // No receivers is fine: nobody is listening.
        let _ = self.tx.send(Arc::new(p));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(file: &str, done: i64) -> Progress {
        Progress {
            file_id: file.into(),
            drive_id: Some("d".into()),
            unit: "page",
            done,
            total: Some(10),
        }
    }

    #[test]
    fn one_event_per_file_per_second() {
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let t0 = Instant::now();
        assert!(bus.emit_at(p("a", 1), t0));
        assert!(!bus.emit_at(p("a", 2), t0 + Duration::from_millis(900)));
        assert!(
            bus.emit_at(p("b", 1), t0 + Duration::from_millis(10)),
            "per file"
        );
        assert!(bus.emit_at(p("a", 3), t0 + Duration::from_millis(1000)));
        let got: Vec<i64> = std::iter::from_fn(|| rx.try_recv().ok().map(|e| e.done)).collect();
        assert_eq!(got, [1, 1, 3]);
    }

    #[test]
    fn the_frame_has_no_id_and_the_event_no_seq() {
        let e = p("a", 3).event("2026-11-17T00:00:00Z");
        assert!(e.get("seq").is_none());
        assert_eq!(e["data"]["total"], 10);
        let f = frame(&e);
        assert!(f.starts_with("event: file.progress\ndata: {"));
        assert!(!f.contains("\nid:") && !f.starts_with("id:"));
        assert_eq!(f.matches('\n').count(), 3);
    }

    #[test]
    fn emitting_without_listeners_is_harmless() {
        assert!(Bus::new().emit(p("a", 1)));
    }
}
