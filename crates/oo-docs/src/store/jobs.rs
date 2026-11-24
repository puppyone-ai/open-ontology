//! Wrappers over the `_oo_docs` job functions (10 §3.2, §4.1).
//!
//! Every call takes the caller's connection, so a stage can run `complete`
//! in the same transaction as its own writes. Nothing here commits. Ids and
//! JSON travel as text because this crate does not enable the sqlx `uuid`,
//! `json` or time features.

use sqlx::{PgConnection, Row, postgres::PgRow};

use super::StoreError;
use crate::jobs::{Job, JobResult, JobStatus, NewJob};

macro_rules! cols {
    () => {
        "status, job_id::text AS job_id, kind, state, attempt, generation, lease_owner, crash_count, cancel_requested, last_error, checkpoint::text AS checkpoint, drive_id::text AS drive_id, file_id::text AS file_id, version_id::text AS version_id, dedupe_key, priority, max_attempts"
    };
}

fn decode(row: &PgRow) -> Result<JobResult, StoreError> {
    let status = JobStatus::parse(&row.try_get::<String, _>("status")?);
    let Some(id) = row.try_get::<Option<String>, _>("job_id")? else {
        return Ok(JobResult { status, job: None });
    };
    let job = Job {
        id,
        kind: row.try_get("kind")?,
        state: row.try_get("state")?,
        attempt: row.try_get("attempt")?,
        generation: row.try_get("generation")?,
        lease_owner: row.try_get("lease_owner")?,
        crash_count: row.try_get("crash_count")?,
        cancel_requested: row.try_get("cancel_requested")?,
        last_error: row.try_get("last_error")?,
        checkpoint: row.try_get("checkpoint")?,
        drive_id: row.try_get("drive_id")?,
        file_id: row.try_get("file_id")?,
        version_id: row.try_get("version_id")?,
        dedupe_key: row.try_get("dedupe_key")?,
        priority: row.try_get("priority")?,
        max_attempts: row.try_get("max_attempts")?,
    };
    Ok(JobResult {
        status,
        job: Some(job),
    })
}

/// `enqueue`: `Enqueued`, `Replayed` (same key, live), `AlreadyDone`
/// (same key, succeeded) or `Terminal` (same key, failed/cancelled/
/// quarantined; the caller adds `retry#n` to the key).
pub async fn enqueue(conn: &mut PgConnection, job: &NewJob<'_>) -> Result<JobResult, StoreError> {
    let max_attempts = job
        .max_attempts
        .unwrap_or_else(|| job.kind.policy().max_attempts);
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.enqueue(NULL::uuid, $1, $2::uuid, $3::uuid, $4::uuid, $5, \
         $6::smallint, $7::integer, clock_timestamp() + $8::bigint * interval '1 millisecond')"
    );
    let row = sqlx::query(sql)
        .bind(job.kind.as_str())
        .bind(job.drive_id)
        .bind(job.file_id)
        .bind(job.version_id)
        .bind(job.dedupe_key)
        .bind(job.priority)
        .bind(max_attempts)
        .bind(job.run_after_ms)
        .fetch_one(&mut *conn)
        .await?;
    decode(&row)
}

/// `claim`: lease up to `limit` due jobs of `kinds` for `owner`. Expired
/// leases are returned to the queue first.
pub async fn claim(
    conn: &mut PgConnection,
    kinds: &[&str],
    owner: &str,
    limit: i32,
) -> Result<Vec<Job>, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.claim($1::text[], $2, $3)"
    );
    let kinds: Vec<String> = kinds.iter().map(|k| (*k).to_string()).collect();
    let rows = sqlx::query(sql)
        .bind(kinds)
        .bind(owner)
        .bind(limit)
        .fetch_all(&mut *conn)
        .await?;
    rows.iter()
        .map(|r| {
            decode(r)?
                .job
                .ok_or_else(|| StoreError::Protocol("claim returned an empty row".into()))
        })
        .collect()
}

/// `heartbeat`: `Extended` (check `job.cancel_requested`), `LostLease` or
/// `Terminal`.
pub async fn heartbeat(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.heartbeat($1::uuid, $2, $3)"
    );
    {
        let row = sqlx::query(sql)
            .bind(id)
            .bind(owner)
            .bind(generation)
            .fetch_one(&mut *conn)
            .await?;
        decode(&row)
    }
}

/// `complete`: `Succeeded`, `LostLease` or `Terminal`.
pub async fn complete(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.complete($1::uuid, $2, $3)"
    );
    {
        let row = sqlx::query(sql)
            .bind(id)
            .bind(owner)
            .bind(generation)
            .fetch_one(&mut *conn)
            .await?;
        decode(&row)
    }
}

/// `fail`: `Failed`, `LostLease` or `Terminal`.
pub async fn fail(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
    code: &str,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.fail($1::uuid, $2, $3, $4)"
    );
    {
        let row = sqlx::query(sql)
            .bind(id)
            .bind(owner)
            .bind(generation)
            .bind(code)
            .fetch_one(&mut *conn)
            .await?;
        decode(&row)
    }
}

/// `quarantine`: `Quarantined`, `LostLease` or `Terminal`.
pub async fn quarantine(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
    code: &str,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.quarantine($1::uuid, $2, $3, $4)"
    );
    {
        let row = sqlx::query(sql)
            .bind(id)
            .bind(owner)
            .bind(generation)
            .bind(code)
            .fetch_one(&mut *conn)
            .await?;
        decode(&row)
    }
}

/// `retry`: `RetryScheduled`, `Exhausted` (attempt cap, now failed),
/// `Quarantined` (crash limit), `LostLease` or `Terminal`. The server clamps
/// `delay_ms` to the per kind cap. `count_attempt = false` gives the attempt
/// back; `crash = true` counts toward quarantine.
#[allow(clippy::too_many_arguments)]
pub async fn retry(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
    code: &str,
    delay_ms: i64,
    count_attempt: bool,
    crash: bool,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.retry($1::uuid, $2, $3, $4, $5, $6, $7, $8::integer)"
    );
    let row = sqlx::query(sql)
        .bind(id)
        .bind(owner)
        .bind(generation)
        .bind(code)
        .bind(delay_ms)
        .bind(count_attempt)
        .bind(crash)
        .bind(crate::jobs::CRASH_QUARANTINE_LIMIT)
        .fetch_one(&mut *conn)
        .await?;
    decode(&row)
}

/// `ack_cancel`: the worker saw `cancel_requested` and stops. `Cancelled`,
/// `NotRequested` (nobody asked), `LostLease` or `Terminal`.
pub async fn ack_cancel(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.ack_cancel($1::uuid, $2, $3)"
    );
    let row = sqlx::query(sql)
        .bind(id)
        .bind(owner)
        .bind(generation)
        .fetch_one(&mut *conn)
        .await?;
    decode(&row)
}

/// `cancel`: `Cancelled` (was queued), `CancelRequested` (running),
/// `Terminal` or `NotFound`.
pub async fn cancel(conn: &mut PgConnection, id: &str) -> Result<JobResult, StoreError> {
    let sql = concat!("SELECT ", cols!(), " FROM _oo_docs.cancel($1::uuid)");
    {
        let row = sqlx::query(sql).bind(id).fetch_one(&mut *conn).await?;
        decode(&row)
    }
}

/// `save_checkpoint`: `Saved` or `LostLease`. `checkpoint_json` is JSON text.
pub async fn save_checkpoint(
    conn: &mut PgConnection,
    id: &str,
    owner: &str,
    generation: i64,
    checkpoint_json: &str,
) -> Result<JobResult, StoreError> {
    let sql = concat!(
        "SELECT ",
        cols!(),
        " FROM _oo_docs.save_checkpoint($1::uuid, $2, $3, $4::jsonb)"
    );
    {
        let row = sqlx::query(sql)
            .bind(id)
            .bind(owner)
            .bind(generation)
            .bind(checkpoint_json)
            .fetch_one(&mut *conn)
            .await?;
        decode(&row)
    }
}

/// Read one job without locking (the runtime role has SELECT).
pub async fn get(conn: &mut PgConnection, id: &str) -> Result<Option<Job>, StoreError> {
    let sql = "SELECT 'found'::text AS status, id::text AS job_id, kind, state, attempt, generation, \
         lease_owner, crash_count, cancel_requested, last_error, checkpoint::text AS checkpoint, \
         drive_id::text AS drive_id, file_id::text AS file_id, version_id::text AS version_id, \
         dedupe_key, priority, max_attempts FROM _oo_docs.job WHERE id = $1::uuid";
    match sqlx::query(sql).bind(id).fetch_optional(&mut *conn).await? {
        Some(row) => Ok(decode(&row)?.job),
        None => Ok(None),
    }
}
