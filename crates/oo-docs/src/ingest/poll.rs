//! Poll planner (10 §4.3.2 "轮询", "深度校验"): decides when a Drive gets its
//! next sync job.
//!
//! Every `interval` an active local Drive gets a `poll` sync; a Drive on an object store
//! gets one every `object_interval` (`sync.s3_poll_seconds`, 10 §3.2 config). When its last sync
//! took longer than 10 s the interval for that Drive becomes six times the
//! duration. Once a week (when enabled) the sync is an `audit` that rehashes
//! everything. Held Drives are skipped until someone confirms.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::{PgPool, Row};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use super::reconcile::{EnqueuedSync, enqueue_sync};
use super::{SyncFlavor, Trigger};

/// Planner settings.
#[derive(Clone, Debug)]
pub struct PollConfig {
    /// `sync.fs_poll_seconds`.
    pub interval: Duration,
    /// `sync.s3_poll_seconds`: the interval of Drives on an object store.
    pub object_interval: Duration,
    /// `sync.weekly_hash_audit`.
    pub weekly_audit: bool,
    /// How often the planner looks at the Drives.
    pub tick: Duration,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            object_interval: Duration::from_secs(300),
            weekly_audit: true,
            tick: Duration::from_secs(1),
        }
    }
}

/// Interval for a Drive whose last sync took `last_duration`: at least the
/// base, and six times a sync that exceeded 10 s.
pub fn adaptive_interval(base: Duration, last_duration: Option<Duration>) -> Duration {
    match last_duration {
        Some(d) if d > Duration::from_secs(10) => base.max(d * 6),
        _ => base,
    }
}

/// Run the planner until `stop` flips. `wake` is poked after each enqueue.
pub fn spawn(
    pool: PgPool,
    cfg: PollConfig,
    wake: Arc<Notify>,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut next_due: HashMap<String, Instant> = HashMap::new();
        let mut first = true;
        loop {
            if *stop.borrow() {
                return;
            }
            let _ = tick(&pool, &cfg, &mut next_due, first, &wake).await;
            first = false;
            tokio::select! {
                _ = tokio::time::sleep(cfg.tick) => {}
                _ = stop.changed() => {}
            }
        }
    })
}

async fn tick(
    pool: &PgPool,
    cfg: &PollConfig,
    next_due: &mut HashMap<String, Instant>,
    first: bool,
    wake: &Notify,
) -> Result<(), super::IngestError> {
    let rows = sqlx::query(
        "SELECT d.id::text AS id, d.backend AS backend, \
         (SELECT extract(epoch FROM (s.finished_at - s.started_at))::float8 \
            FROM _oo_docs.drive_sync s WHERE s.drive_id = d.id AND s.status = 'complete' \
            ORDER BY s.generation DESC LIMIT 1) AS last_secs, \
         NOT EXISTS (SELECT 1 FROM _oo_docs.drive_sync a WHERE a.drive_id = d.id \
            AND a.trigger = 'audit' AND a.status = 'complete' \
            AND a.started_at > clock_timestamp() - interval '7 days') AS audit_due \
         FROM _oo_docs.drive d WHERE d.status = 'active'",
    )
    .fetch_all(pool)
    .await?;
    let now = Instant::now();
    for r in rows {
        let id: String = r.try_get("id")?;
        if next_due.get(&id).is_some_and(|t| *t > now) {
            continue;
        }
        let last: Option<f64> = r.try_get("last_secs")?;
        let audit_due: bool = r.try_get("audit_due")?;
        let backend: String = r.try_get("backend")?;
        let local = backend == "fs";
        // The weekly deep audit rehashes a tree; an object store has no such pass (its ETag is the fingerprint).
        let flavor = if local && cfg.weekly_audit && audit_due && !first {
            SyncFlavor {
                trigger: Trigger::Audit,
                deep: true,
                confirm: None,
            }
        } else if first {
            SyncFlavor {
                trigger: Trigger::Startup,
                ..SyncFlavor::POLL
            }
        } else {
            SyncFlavor::POLL
        };
        let base = if local {
            cfg.interval
        } else {
            cfg.object_interval
        };
        let interval = adaptive_interval(base, last.map(Duration::from_secs_f64));
        next_due.insert(id.clone(), now + interval);
        if let Ok(EnqueuedSync::Enqueued { .. }) = enqueue_sync(pool, &id, flavor).await {
            wake.notify_waiters();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_grows_with_slow_syncs() {
        let base = Duration::from_secs(30);
        assert_eq!(adaptive_interval(base, None), base);
        assert_eq!(adaptive_interval(base, Some(Duration::from_secs(10))), base);
        assert_eq!(
            adaptive_interval(base, Some(Duration::from_secs(20))),
            Duration::from_secs(120)
        );
        assert_eq!(
            adaptive_interval(base, Some(Duration::from_secs(11))),
            Duration::from_secs(66)
        );
    }
}
