//! Drive deletion (10 §8.3 `DELETE`): `deleting` -> `deleted`.
//!
//! `DELETE` flips the status and cancels pending jobs; this reaper then
//! deletes the Drive's files one transaction each (withdrawing evidence, 4.3.5)
//! and finally marks the Drive `deleted` and emits `drive.deleted`.

use std::time::Duration;

use serde_json::json;
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::error::ApiError;
use crate::ingest::files;
use crate::store::events;

/// Files deleted per Drive per pass.
const PER_PASS: i64 = 200;

/// One pass over every `deleting` Drive. Returns how many Drives finished.
pub async fn reap_once(pool: &PgPool) -> Result<usize, ApiError> {
    let drives: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.drive WHERE status = 'deleting' ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    let mut finished = 0;
    for drive in drives {
        let live: Vec<String> = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.file WHERE drive_id = $1::uuid AND deleted_at IS NULL \
             ORDER BY id LIMIT $2",
        )
        .bind(&drive)
        .bind(PER_PASS)
        .fetch_all(pool)
        .await?;
        for id in &live {
            let mut tx = pool.begin().await?;
            files::delete_file(&mut tx, id, None, None).await?;
            tx.commit().await?;
        }
        if live.len() as i64 == PER_PASS {
            continue; // more next pass
        }
        let running: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.job WHERE drive_id = $1::uuid AND state = 'running'",
        )
        .bind(&drive)
        .fetch_one(pool)
        .await?;
        if running > 0 {
            continue; // let running jobs reach a checkpoint
        }
        let mut tx = pool.begin().await?;
        let done = sqlx::query(
            "UPDATE _oo_docs.drive SET status = 'deleted', revision = revision + 1, \
             updated_at = clock_timestamp() WHERE id = $1::uuid AND status = 'deleting'",
        )
        .bind(&drive)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if done == 1 {
            let bulk = json!({ "drive_id": drive, "count": live.len(), "states": { "deleted": live.len() } });
            events::append_event(
                &mut tx,
                "files.bulk_changed",
                Some(&drive),
                None,
                &bulk.to_string(),
            )
            .await?;
            let payload = json!({ "drive_id": drive }).to_string();
            events::append_event(
                &mut tx,
                "drive.deleted",
                Some(&drive),
                Some(&drive),
                &payload,
            )
            .await?;
            finished += 1;
        }
        tx.commit().await?;
    }
    Ok(finished)
}

/// Run [`reap_once`] and idempotency housekeeping until `stop`.
pub fn spawn(pool: PgPool, every: Duration, mut stop: watch::Receiver<bool>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticks = 0u64;
        loop {
            if *stop.borrow() {
                return;
            }
            let _ = reap_once(&pool).await;
            // Expired shares (D28): the credential dies and the daemon sees the event.
            let _ = crate::share::lifecycle::expire_due(&pool).await;
            ticks += 1;
            if ticks.is_multiple_of(600)
                && let Ok(mut c) = pool.acquire().await
            {
                let _ = super::idem::purge_expired(&mut c).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                _ = stop.changed() => {}
            }
        }
    })
}
