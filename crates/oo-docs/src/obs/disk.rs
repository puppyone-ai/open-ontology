//! Disk guard (10 §4.12, G7): below `DISK_LOW_BYTES` free on the volume of
//! the content store, hashing and parsing stop. Files wait in `stored` or
//! `queued` with `blocked_reason = disk_low` (an event says so), health shows
//! `degraded` with `DOCS_ENGINE_DISK_LOW`, and everything resumes by itself
//! when space is freed. Nothing is lost: the jobs are not failed, they wait.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use sqlx::PgPool;

use crate::health::{DISK_LOW_BYTES, DiskProbe};
use crate::jobs::FailureClass;
use crate::jobs::scheduler::{HandlerFuture, JobCtx, JobHandler, Outcome};
use crate::store::{StoreError, events};

/// The `DOCS_*` code of the pause.
pub const CODE: &str = "DOCS_ENGINE_DISK_LOW";
/// How long a reading is trusted.
const CACHE: Duration = Duration::from_secs(2);
/// More files than this change at once: one `files.bulk_changed` per drive.
const BULK_OVER: usize = 200;

/// Reads free space and says whether the pause applies.
pub struct Guard {
    probe: Arc<dyn DiskProbe>,
    root: PathBuf,
    low_below: u64,
    last: Mutex<Option<(Instant, bool)>>,
}

impl Guard {
    /// Guard for the volume holding `root`.
    pub fn new(probe: Arc<dyn DiskProbe>, root: PathBuf) -> Self {
        Self::with_threshold(probe, root, DISK_LOW_BYTES)
    }

    /// With an explicit threshold (tests).
    pub fn with_threshold(probe: Arc<dyn DiskProbe>, root: PathBuf, low_below: u64) -> Self {
        Self {
            probe,
            root,
            low_below,
            last: Mutex::new(None),
        }
    }

    /// Is free space below the threshold? Unknown counts as fine: a probe
    /// that cannot answer must not stop the engine.
    pub fn is_low(&self) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, low)) = *last
            && at.elapsed() < CACHE
        {
            return low;
        }
        let low = self
            .probe
            .free_bytes(&self.root)
            .is_some_and(|f| f < self.low_below);
        *last = Some((Instant::now(), low));
        low
    }

    /// Forget the cached reading (tests that change the free space).
    pub fn recheck(&self) {
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// Wraps a handler: while the disk is low the job is put back without
/// counting an attempt.
pub struct Guarded<H> {
    /// The real handler.
    pub inner: H,
    /// The guard.
    pub guard: Arc<Guard>,
}

impl<H: JobHandler> JobHandler for Guarded<H> {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            if self.guard.is_low() {
                return Outcome::failed(FailureClass::Busy, CODE);
            }
            self.inner.run(ctx).await
        })
    }
}

impl JobHandler for Arc<dyn JobHandler> {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        (**self).run(ctx)
    }
}

/// Put the guard in front of the handlers of `kinds` (hash, parse, extract).
pub fn guard_handlers(
    map: &mut std::collections::HashMap<crate::jobs::JobKind, Arc<dyn JobHandler>>,
    guard: &Arc<Guard>,
    kinds: &[crate::jobs::JobKind],
) {
    for kind in kinds {
        if let Some(inner) = map.remove(kind) {
            map.insert(
                *kind,
                Arc::new(Guarded {
                    inner,
                    guard: guard.clone(),
                }),
            );
        }
    }
}

/// Set `blocked_reason = disk_low` on waiting files (`low`), or clear it
/// (`!low`), with the events. Returns how many files changed.
pub async fn sync_blocked(pool: &PgPool, low: bool) -> Result<usize, StoreError> {
    let mut tx = pool.begin().await?;
    let rows: Vec<(String, String, String)> = if low {
        sqlx::query_as(
            "UPDATE _oo_docs.file SET blocked_reason = 'disk_low', revision = revision + 1 \
              WHERE id IN (SELECT id FROM _oo_docs.file \
                            WHERE state IN ('stored', 'queued') AND deleted_at IS NULL \
                              AND blocked_reason IS NULL ORDER BY id LIMIT 5000 FOR UPDATE) \
          RETURNING id::text, drive_id::text, state",
        )
        .fetch_all(&mut *tx)
        .await?
    } else {
        sqlx::query_as(
            "UPDATE _oo_docs.file SET blocked_reason = NULL, revision = revision + 1 \
              WHERE id IN (SELECT id FROM _oo_docs.file WHERE blocked_reason = 'disk_low' \
                            ORDER BY id LIMIT 5000 FOR UPDATE) \
          RETURNING id::text, drive_id::text, state",
        )
        .fetch_all(&mut *tx)
        .await?
    };
    let n = rows.len();
    if n > 0 {
        if n > BULK_OVER {
            let mut by_drive: std::collections::BTreeMap<
                String,
                std::collections::BTreeMap<String, usize>,
            > = Default::default();
            for (_, drive, state) in &rows {
                *by_drive
                    .entry(drive.clone())
                    .or_default()
                    .entry(state.clone())
                    .or_default() += 1;
            }
            for (drive, states) in by_drive {
                let count: usize = states.values().sum();
                let payload = json!({ "drive_id": drive, "count": count, "states": states });
                events::append_event(
                    &mut tx,
                    "files.bulk_changed",
                    Some(&drive),
                    None,
                    &payload.to_string(),
                )
                .await?;
            }
        } else {
            for (file, drive, state) in &rows {
                let payload = json!({
                    "file_id": file, "from": state, "to": state,
                    "code": if low { json!(CODE) } else { json!(null) },
                })
                .to_string();
                events::append_event(
                    &mut tx,
                    "file.state_changed",
                    Some(drive),
                    Some(file),
                    &payload,
                )
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Free(Arc<AtomicU64>);

    impl DiskProbe for Free {
        fn free_bytes(&self, _: &std::path::Path) -> Option<u64> {
            Some(self.0.load(Ordering::Relaxed))
        }
    }

    #[test]
    fn low_below_the_threshold_and_cached_briefly() {
        let free = Arc::new(AtomicU64::new(10));
        let g = Guard::with_threshold(Arc::new(Free(free.clone())), "/".into(), 100);
        assert!(g.is_low());
        free.store(1_000, Ordering::Relaxed);
        assert!(g.is_low(), "a reading is trusted for a moment");
        g.recheck();
        assert!(!g.is_low());
    }

    #[test]
    fn an_unknown_reading_never_pauses_the_engine() {
        struct Unknown;
        impl DiskProbe for Unknown {
            fn free_bytes(&self, _: &std::path::Path) -> Option<u64> {
                None
            }
        }
        assert!(!Guard::new(Arc::new(Unknown), "/".into()).is_low());
    }
}
