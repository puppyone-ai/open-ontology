//! The `hash` job (10 §4.3.2, 4.1.2): settle, stream-hash, write the blob and
//! the version, move the file out of `stored`.
//!
//! The write set is built in two halves so that a transaction taking several
//! file locks (a rename) can do all row writes before its first event:
//! [`prepare_version`] writes `blob`, `file_version` and the `file` columns;
//! [`apply_plan`] runs the state transitions (each appends its event).

use std::time::{SystemTime, UNIX_EPOCH};

use oo_connectors::objectstore::fs::is_settled;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::{DocSourceError, EntryKind, Fingerprint, ReadExpect};
use sqlx::{PgConnection, PgPool};

use super::files::{self, Extra, FileRow};
use super::sniff::{self, Format, Verdict};
use super::{HashableSource, IngestError, IngestLimits, SourceFactory};
use crate::jobs::scheduler::{HandlerFuture, JobCtx, JobHandler, Outcome};
use crate::jobs::{FailureClass, JobStatus};
use crate::state::FileState;
use crate::store::jobs as store;

/// Bytes read for sniffing.
pub const SNIFF_HEAD: u64 = 64 * 1024;

/// What `prepare_version` decided; `apply_plan` executes it.
#[derive(Clone, Debug)]
pub struct VersionPlan {
    /// File id.
    pub file_id: String,
    /// New version id (`None` when the blob did not change).
    pub version_id: Option<String>,
    /// State to leave through `superseded`/`deleted` -> `stored`; `None` for a
    /// brand new file that is still `stored` without a version.
    pub restart_from: Option<FileState>,
    /// Transitions after `stored`, in order.
    pub tail: Vec<(FileState, Option<&'static str>)>,
    /// Version this one duplicates.
    pub duplicate_of: Option<String>,
}

impl VersionPlan {
    /// State the file ends in.
    pub fn final_state(&self) -> FileState {
        self.tail.last().map_or(FileState::Stored, |t| t.0)
    }
}

/// Everything about a hashed object that the version rows need.
pub struct HashedObject<'a> {
    /// Digests and length.
    pub digest: &'a DualDigest,
    /// Fingerprint at hash time.
    pub fp: &'a Fingerprint,
    /// First bytes of the object.
    pub head: &'a [u8],
}

fn mtime_i64(fp: &Fingerprint) -> Option<i64> {
    fp.last_modified_ns
        .map(|v| v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

/// Write the blob, the version and the `file` columns for a hashed object.
/// The caller holds the transaction, has completed its job and has locked the
/// file row (`lock_file`). `rename_of` makes the new version point at the old
/// file's version so the evidence can be carried over.
pub async fn prepare_version(
    conn: &mut PgConnection,
    row: &FileRow,
    state_now: FileState,
    deleted: bool,
    obj: &HashedObject<'_>,
    limits: &IngestLimits,
    rename_of: Option<&str>,
) -> Result<VersionPlan, IngestError> {
    let ext = sniff::extension(&row.path);
    let sniffed = sniff::sniff(obj.head, obj.digest.len, &ext);
    let encrypted = sniffed.verdict == Verdict::Encrypted;
    sqlx::query(
        "INSERT INTO _oo_docs.blob (b3, sha256, size, format, encrypted, has_macros) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (b3) DO UPDATE SET format = COALESCE(_oo_docs.blob.format, EXCLUDED.format)",
    )
    .bind(&obj.digest.b3)
    .bind(&obj.digest.sha256)
    .bind(obj.digest.len as i64)
    .bind(sniffed.format.as_str())
    .bind(encrypted)
    .bind(sniffed.has_macros)
    .execute(&mut *conn)
    .await?;

    // Same content as the current version: only the fingerprint moves.
    let current: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT f.current_version_id::text, cv.b3 FROM _oo_docs.file f \
         LEFT JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
         WHERE f.id = $1::uuid",
    )
    .bind(&row.id)
    .fetch_optional(&mut *conn)
    .await?;
    let (cur_vid, cur_b3) = current.unwrap_or((None, None));
    let fp_json =
        serde_json::to_string(obj.fp).map_err(|e| IngestError::Protocol(e.to_string()))?;
    if !deleted && cur_vid.is_some() && cur_b3.as_deref() == Some(obj.digest.b3.as_str()) {
        write_fp_columns(conn, &row.id, obj.fp).await?;
        sqlx::query("UPDATE _oo_docs.file_version SET fp = $2::jsonb WHERE id = $1::uuid")
            .bind(cur_vid.as_deref())
            .bind(&fp_json)
            .execute(&mut *conn)
            .await?;
        return Ok(VersionPlan {
            file_id: row.id.clone(),
            version_id: None,
            restart_from: None,
            tail: Vec::new(),
            duplicate_of: None,
        });
    }

    if let Some(old) = &cur_vid {
        sqlx::query(
            "UPDATE _oo_docs.file_version SET superseded_at = clock_timestamp() \
             WHERE id = $1::uuid AND superseded_at IS NULL AND deleted_at IS NULL",
        )
        .bind(old)
        .execute(&mut *conn)
        .await?;
    }

    // Another live file already finished this content: duplicate (4.2).
    let duplicate_of: Option<String> = match rename_of {
        Some(v) => Some(v.to_owned()),
        None => {
            sqlx::query_scalar(
                "SELECT fv.id::text FROM _oo_docs.file_version fv \
                 JOIN _oo_docs.file f ON f.id = fv.file_id \
                 WHERE fv.b3 = $1 AND fv.file_id <> $2::uuid AND fv.superseded_at IS NULL \
                 AND fv.deleted_at IS NULL AND f.deleted_at IS NULL \
                 AND f.state IN ('published', 'indexed', 'extract_partial') \
                 ORDER BY fv.created_at, fv.id LIMIT 1",
            )
            .bind(&obj.digest.b3)
            .bind(&row.id)
            .fetch_optional(&mut *conn)
            .await?
        }
    };
    let new_vid: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.file_version (id, file_id, b3, path_at_version, fp, duplicate_of) \
         VALUES (gen_random_uuid(), $1::uuid, $2, $3, $4::jsonb, $5::uuid) RETURNING id::text",
    )
    .bind(&row.id)
    .bind(&obj.digest.b3)
    .bind(&row.path)
    .bind(&fp_json)
    .bind(duplicate_of.as_deref())
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.file SET current_version_id = $2::uuid, deleted_at = NULL, \
         blocked_reason = NULL, progress = NULL WHERE id = $1::uuid",
    )
    .bind(&row.id)
    .bind(&new_vid)
    .execute(&mut *conn)
    .await?;
    write_fp_columns(conn, &row.id, obj.fp).await?;

    let restart_from = if cur_vid.is_none() && !deleted && state_now == FileState::Stored {
        None
    } else {
        Some(if deleted {
            FileState::Deleted
        } else {
            state_now
        })
    };
    let mut tail: Vec<(FileState, Option<&'static str>)> = Vec::new();
    // Rename of a finished file keeps its result: `duplicate_of` the old
    // version. Content equality alone only counts for a live finished twin.
    let finished_twin = duplicate_of.is_some();
    match sniffed.verdict {
        Verdict::Encrypted => {
            tail.push((FileState::Queued, None));
            tail.push((FileState::Encrypted, Some("DOCS_PARSE_ENCRYPTED")));
        }
        Verdict::Unsupported(code) => {
            tail.push((FileState::Queued, None));
            tail.push((FileState::Unsupported, Some(code)));
        }
        Verdict::Parseable if !parser_exists(limits, sniffed.format) => {
            tail.push((FileState::Queued, None));
            tail.push((
                FileState::Unsupported,
                Some("DOCS_PARSE_UNSUPPORTED_FORMAT"),
            ));
        }
        Verdict::Parseable if finished_twin => tail.push((FileState::Duplicate, None)),
        Verdict::Parseable => tail.push((FileState::Queued, None)),
    }
    Ok(VersionPlan {
        file_id: row.id.clone(),
        version_id: Some(new_vid),
        restart_from,
        tail,
        duplicate_of,
    })
}

fn parser_exists(limits: &IngestLimits, format: Format) -> bool {
    limits.parsers.contains(&format)
}

async fn write_fp_columns(
    conn: &mut PgConnection,
    file_id: &str,
    fp: &Fingerprint,
) -> Result<(), IngestError> {
    sqlx::query(
        "UPDATE _oo_docs.file SET fp_size = $2, fp_etag = $3, fp_mtime_ns = $4, \
         fp_version_id = $5, fp_ino = $6 WHERE id = $1::uuid",
    )
    .bind(file_id)
    .bind(fp.size as i64)
    .bind(fp.etag.as_deref())
    .bind(mtime_i64(fp))
    .bind(fp.version_id.as_deref())
    .bind(fp.fs_ino.map(|i| i as i64))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Run the state transitions of a plan (events are appended here).
pub async fn apply_plan(conn: &mut PgConnection, plan: &VersionPlan) -> Result<(), IngestError> {
    if let Some(from) = plan.restart_from {
        files::restart_as_stored(conn, &plan.file_id, from, None).await?;
    }
    let mut prev = FileState::Stored;
    for (to, code) in &plan.tail {
        files::to_state(
            conn,
            &plan.file_id,
            prev,
            *to,
            Extra {
                code: *code,
                version_id: plan.version_id.as_deref(),
                ..Extra::default()
            },
        )
        .await?;
        prev = *to;
    }
    Ok(())
}

/// Set the file to `too_large` without hashing (size above the limit).
/// Records the fingerprint in the `file` columns so the next listing does not
/// enqueue another hash while nothing changed.
pub async fn mark_too_large(
    conn: &mut PgConnection,
    row: &FileRow,
    state_now: FileState,
    deleted: bool,
    fp: &Fingerprint,
) -> Result<(), IngestError> {
    if state_now == FileState::TooLarge && !deleted {
        write_fp_columns(conn, &row.id, fp).await?;
        return Ok(());
    }
    if let Some(vid) = &row.version_id {
        sqlx::query(
            "UPDATE _oo_docs.file_version SET superseded_at = clock_timestamp() \
             WHERE id = $1::uuid AND superseded_at IS NULL AND deleted_at IS NULL",
        )
        .bind(vid)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query(
        "UPDATE _oo_docs.file SET current_version_id = NULL, deleted_at = NULL WHERE id = $1::uuid",
    )
    .bind(&row.id)
    .execute(&mut *conn)
    .await?;
    write_fp_columns(conn, &row.id, fp).await?;
    let restart = !(state_now == FileState::Stored && row.version_id.is_none() && !deleted);
    if restart {
        let from = if deleted {
            FileState::Deleted
        } else {
            state_now
        };
        files::restart_as_stored(conn, &row.id, from, None).await?;
    }
    files::to_state(
        conn,
        &row.id,
        FileState::Stored,
        FileState::TooLarge,
        Extra::code(Some("DOCS_SOURCE_TOO_LARGE")),
    )
    .await?;
    Ok(())
}

async fn now_ns() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}

fn classify(e: &DocSourceError) -> Outcome {
    let code = e.code();
    match e {
        DocSourceError::AuthFailed
        | DocSourceError::InvalidConfig(_)
        | DocSourceError::Protocol(_) => Outcome::failed(FailureClass::Permanent, code),
        _ => Outcome::failed(FailureClass::Transient, code),
    }
}

fn db_fail(e: impl std::fmt::Display) -> Outcome {
    let _ = e.to_string();
    Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_DB_UNAVAILABLE")
}

async fn begin_stage(
    pool: &PgPool,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL statement_timeout = '15s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Settle and unstable bookkeeping: `Busy` retries do not count an attempt;
/// after `unstable_limit` of them one failure is counted (4.3.2).
async fn unstable(ctx: &JobCtx, limits: &IngestLimits) -> Outcome {
    let seen = ctx
        .job
        .checkpoint
        .as_deref()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v.get("unstable").and_then(|n| n.as_i64()))
        .unwrap_or(0)
        + 1;
    if seen >= limits.unstable_limit {
        // Count it, and start over next time.
        if let Ok(mut c) = ctx.pool.acquire().await {
            let _ = store::save_checkpoint(
                &mut c,
                &ctx.job.id,
                &ctx.owner,
                ctx.job.generation,
                "{\"unstable\":0}",
            )
            .await;
        }
        return Outcome::failed(FailureClass::Transient, "DOCS_SOURCE_UNSTABLE");
    }
    if let Ok(mut c) = ctx.pool.acquire().await {
        let _ = store::save_checkpoint(
            &mut c,
            &ctx.job.id,
            &ctx.owner,
            ctx.job.generation,
            &format!("{{\"unstable\":{seen}}}"),
        )
        .await;
    }
    Outcome::failed(FailureClass::Busy, "DOCS_SOURCE_UNSTABLE")
}

/// Complete a job whose subject is gone or irrelevant.
async fn nothing_to_do(ctx: &JobCtx) -> Outcome {
    let _ = ctx;
    Outcome::Done
}

async fn load_row(
    pool: &PgPool,
    file_id: &str,
) -> Result<Option<(FileRow, String, bool)>, IngestError> {
    let mut conn = pool.acquire().await.map_err(IngestError::from)?;
    let drive: Option<String> =
        sqlx::query_scalar("SELECT drive_id::text FROM _oo_docs.file WHERE id = $1::uuid")
            .bind(file_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(drive) = drive else { return Ok(None) };
    let rows = files::load_files(&mut conn, &drive).await?;
    let found = rows.into_iter().find(|r| r.id == file_id);
    Ok(found.map(|r| {
        let deleted = r.deleted;
        (r, drive, deleted)
    }))
}

/// Run one hash job against `source`.
pub async fn run_hash<S: HashableSource>(
    ctx: &JobCtx,
    source: &S,
    limits: &IngestLimits,
) -> Outcome {
    let Some(file_id) = ctx.job.file_id.clone() else {
        return Outcome::failed(FailureClass::Permanent, "DOCS_ENGINE_INTERNAL");
    };
    let (row, drive_id, deleted) = match load_row(&ctx.pool, &file_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return nothing_to_do(ctx).await,
        Err(e) => return db_fail(e),
    };
    // A deleted row is not skipped: a job queued by a reappearance revives it,
    // and a stale job on a really deleted file finds nothing at the source.
    let _ = deleted;
    let state_now = FileState::parse(&row.state).unwrap_or(FileState::Stored);
    if ctx.should_stop() {
        return Outcome::Cancelled;
    }

    let first = match source.stat(&row.raw_key).await {
        Ok(Some(m)) if m.kind == EntryKind::File => m,
        Ok(_) => return nothing_to_do(ctx).await,
        Err(DocSourceError::NotFound) => return nothing_to_do(ctx).await,
        Err(DocSourceError::PermissionDenied) => {
            return permission_denied(ctx, &row, state_now).await;
        }
        Err(e) => return classify(&e),
    };
    let fp1 = first.fingerprint();
    if fp1.size > limits.size_limit(&row.path) {
        let mut tx = match begin_stage(&ctx.pool).await {
            Ok(t) => t,
            Err(e) => return db_fail(e),
        };
        let r = async {
            match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
                .await?
                .status
            {
                JobStatus::Succeeded => {}
                _ => return Ok(false),
            }
            files::cancel_file_jobs(&mut tx, &file_id, Some(&ctx.job.id), false).await?;
            if files::lock_file(&mut tx, &file_id).await?.is_none() {
                return Ok(true);
            }
            mark_too_large(&mut tx, &row, state_now, false, &fp1).await?;
            Ok::<bool, IngestError>(true)
        }
        .await;
        return finish_tx(tx, r).await;
    }

    // Recently written: wait, then compare size and mtime (4.3.2).
    let age = now_ns().await - first.last_modified_ns.unwrap_or(0);
    if age < limits.settle.as_nanos() as i128 {
        tokio::time::sleep(limits.settle).await;
        if ctx.should_stop() {
            return Outcome::Cancelled;
        }
        match source.stat(&row.raw_key).await {
            Ok(Some(second)) if is_settled(&fp1, &second.fingerprint()) => {}
            Ok(Some(_)) => return unstable(ctx, limits).await,
            Ok(None) | Err(DocSourceError::NotFound) => return nothing_to_do(ctx).await,
            Err(e) => return classify(&e),
        }
    }

    let (digest, meta) = match source.hash_object(&row.raw_key).await {
        Ok(v) => v,
        Err(DocSourceError::Changed) => return unstable(ctx, limits).await,
        Err(DocSourceError::NotFound) => return nothing_to_do(ctx).await,
        Err(DocSourceError::PermissionDenied) => {
            return permission_denied(ctx, &row, state_now).await;
        }
        Err(e) => return classify(&e),
    };
    let head = match source
        .read(
            &row.raw_key,
            0,
            digest.len.min(SNIFF_HEAD),
            &ReadExpect::default(),
        )
        .await
    {
        Ok(h) => h,
        Err(DocSourceError::Changed) => return unstable(ctx, limits).await,
        Err(DocSourceError::NotFound) => return nothing_to_do(ctx).await,
        Err(e) => return classify(&e),
    };
    if ctx.should_stop() {
        return Outcome::Cancelled;
    }
    let fp = meta.fingerprint();
    let obj = HashedObject {
        digest: &digest,
        fp: &fp,
        head: &head,
    };

    let mut tx = match begin_stage(&ctx.pool).await {
        Ok(t) => t,
        Err(e) => return db_fail(e),
    };
    let r = async {
        match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
            .await?
            .status
        {
            JobStatus::Succeeded => {}
            _ => return Ok(false), // lost lease: discard
        }
        files::cancel_file_jobs(&mut tx, &file_id, Some(&ctx.job.id), false).await?;
        let Some((st, del)) = files::lock_file(&mut tx, &file_id).await? else {
            return Ok(true);
        };
        let st = FileState::parse(&st).unwrap_or(FileState::Stored);
        let _ = &drive_id;
        let plan = prepare_version(&mut tx, &row, st, del, &obj, limits, None).await?;
        apply_plan(&mut tx, &plan).await?;
        Ok::<bool, IngestError>(true)
    }
    .await;
    finish_tx(tx, r).await
}

async fn finish_tx(
    tx: sqlx::Transaction<'static, sqlx::Postgres>,
    r: Result<bool, IngestError>,
) -> Outcome {
    match r {
        Ok(true) => match tx.commit().await {
            Ok(()) => Outcome::Completed,
            Err(e) => db_fail(e),
        },
        Ok(false) => {
            let _ = tx.rollback().await;
            Outcome::Completed // lost lease: nothing to finish, result discarded
        }
        Err(e) => {
            let _ = tx.rollback().await;
            db_fail(e)
        }
    }
}

/// Unreadable file: a new (`stored`) file becomes `parse_failed` with
/// `DOCS_SOURCE_PERMISSION_DENIED, retryable=true`; every reconcile tries it
/// again once (4.3.2). Files that already have a state keep it.
async fn permission_denied(ctx: &JobCtx, row: &FileRow, state_now: FileState) -> Outcome {
    let mut tx = match begin_stage(&ctx.pool).await {
        Ok(t) => t,
        Err(e) => return db_fail(e),
    };
    let r = async {
        match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
            .await?
            .status
        {
            JobStatus::Succeeded => {}
            _ => return Ok(false),
        }
        if files::lock_file(&mut tx, &row.id).await?.is_some() && state_now == FileState::Stored {
            files::to_state(
                &mut tx,
                &row.id,
                FileState::Stored,
                FileState::ParseFailed,
                Extra {
                    code: Some("DOCS_SOURCE_PERMISSION_DENIED"),
                    retryable: true,
                    ..Extra::default()
                },
            )
            .await?;
        }
        Ok::<bool, IngestError>(true)
    }
    .await;
    finish_tx(tx, r).await
}

/// Handler for `hash` jobs.
pub struct HashHandler<F: SourceFactory> {
    pub(crate) pool: PgPool,
    pub(crate) factory: std::sync::Arc<F>,
    pub(crate) limits: std::sync::Arc<IngestLimits>,
}

impl<F: SourceFactory> JobHandler for HashHandler<F> {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let Some(drive_id) = ctx.job.drive_id.clone() else {
                return Outcome::failed(FailureClass::Permanent, "DOCS_ENGINE_INTERNAL");
            };
            let drive = match super::reconcile::load_drive(&self.pool, &drive_id).await {
                Ok(Some(d)) => d,
                Ok(None) => return Outcome::Done,
                Err(e) => return db_fail(e),
            };
            if matches!(drive.status.as_str(), "deleting" | "deleted") {
                return Outcome::Done;
            }
            let source = match self.factory.open(&drive) {
                Ok(s) => s,
                Err(e) => return classify(&e),
            };
            run_hash(&ctx, &source, &self.limits).await
        })
    }
}
