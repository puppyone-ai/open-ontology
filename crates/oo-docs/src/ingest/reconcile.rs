//! Generation mark-sweep reconcile (10 §4.3.4) and targeted sync (G11, D47).
//!
//! ```text
//! g = drive.sync_generation + 1            (committed only on a complete sync)
//! list all pages -> upsert file rows (last_seen_generation = g),
//!                   changed or new -> hash job
//! complete listing only:
//!     missing = live files with last_seen_generation < g
//!     mass delete (> 100 and > 50% of live, or empty root) -> held
//!     rename (same b3 appeared this generation) | deleted (evidence withdrawn)
//! ```
//!
//! Formal: FM3. The sweep needs a listing whose last page said `complete`;
//! a failed or partial listing still records what it saw but never deletes.
//! A targeted sync never reaches the sweep and never moves the generation.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use oo_connectors::objectstore::fs::is_settled;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::path::{PathStatus, annotate, path_fold};
use oo_connectors::objectstore::{
    DocPath, DocSourceError, EntryKind, Fingerprint, ListCursor, ObjectMeta, ReadExpect,
};
use sqlx::pool::PoolConnection;
use sqlx::{PgPool, Postgres, Row};

use super::files::{self, Extra, FileRow, fp_matches};
use super::hashjob::{HashedObject, SNIFF_HEAD, apply_plan, mark_too_large, prepare_version};
use super::{
    DriveRef, HashableSource, IngestError, IngestLimits, SourceFactory, SyncFlavor, Trigger,
    fp_sha256, hex,
};
use crate::jobs::{JobKind, JobStatus, NewJob};
use crate::state::FileState;
use crate::store::{events, jobs as store};

const MAX_ENTRIES: usize = 1_000_000;
const MAX_PAGES: i32 = 100_000;

/// Final status of one sync run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// Listing complete, sweep done, generation committed.
    Complete,
    /// Listing failed or incomplete: nothing was swept.
    Failed,
    /// Mass delete held for confirmation: nothing was swept.
    Held,
}

impl SyncStatus {
    /// `drive_sync.status` text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Held => "held",
        }
    }
}

/// Result of a sync run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncReport {
    /// Generation this run worked on.
    pub generation: i64,
    /// Outcome.
    pub status: SyncStatus,
    /// Entries listed.
    pub listed: i64,
    /// Pages read.
    pub pages: i32,
    /// New files.
    pub added: i32,
    /// Changed files (hash enqueued, or oversize/symlink conversion).
    pub changed: i32,
    /// Files deleted.
    pub removed: i32,
    /// Renames detected.
    pub renamed: i32,
    /// `DOCS_SOURCE_*` code of a failure or hold.
    pub error_code: Option<String>,
    /// Size of the held sweep.
    pub would_delete: Option<i64>,
    /// Hash jobs enqueued.
    pub hash_enqueued: i32,
}

/// Result of a targeted sync.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetedReport {
    /// Paths looked at.
    pub checked: usize,
    /// New files created.
    pub added: usize,
    /// Known files whose fingerprint changed.
    pub changed: usize,
    /// Known files left alone.
    pub unchanged: usize,
    /// Paths that do not exist now; only a full reconcile deletes (G11).
    pub maybe_deleted: Vec<String>,
    /// Paths rejected (invalid, hidden, not a file).
    pub skipped: Vec<String>,
    /// Hash jobs enqueued.
    pub hash_enqueued: usize,
}

/// Read a Drive.
pub async fn load_drive(pool: &PgPool, drive_id: &str) -> Result<Option<DriveRef>, IngestError> {
    let row = sqlx::query(
        "SELECT id::text AS id, backend, config::text AS config, status, sync_generation, \
         max_files FROM _oo_docs.drive WHERE id = $1::uuid",
    )
    .bind(drive_id)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let config: String = r.try_get("config")?;
    Ok(Some(DriveRef {
        id: r.try_get("id")?,
        backend: r.try_get("backend")?,
        config: serde_json::from_str(&config).unwrap_or(serde_json::Value::Null),
        status: r.try_get("status")?,
        sync_generation: r.try_get("sync_generation")?,
        max_files: i64::from(r.try_get::<i32, _>("max_files")?),
    }))
}

// ---------------------------------------------------------------------------
// enqueue
// ---------------------------------------------------------------------------

/// What `enqueue_sync` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnqueuedSync {
    /// A new job.
    Enqueued {
        /// Generation the job will work on.
        generation: i64,
        /// Job id.
        job_id: String,
    },
    /// A sync job of this Drive is already queued or running.
    Running {
        /// Generation in progress.
        generation: i64,
    },
    /// The last attempt for this generation was held and not confirmed.
    Held {
        /// Held generation.
        generation: i64,
        /// Files the sweep would delete.
        would_delete: i64,
    },
}

/// Files a held sweep of `generation` would delete.
pub async fn would_delete(
    pool: &PgPool,
    drive_id: &str,
    generation: i64,
) -> Result<i64, IngestError> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.file WHERE drive_id = $1::uuid AND deleted_at IS NULL \
         AND last_seen_generation < $2",
    )
    .bind(drive_id)
    .bind(generation)
    .fetch_one(pool)
    .await?)
}

/// Enqueue a sync job for a Drive. Idempotent per generation and flavor
/// (4.1.2); a `confirm` for another generation than the held one is
/// [`IngestError::ConfirmMismatch`].
pub async fn enqueue_sync(
    pool: &PgPool,
    drive_id: &str,
    flavor: SyncFlavor,
) -> Result<EnqueuedSync, IngestError> {
    let drive = load_drive(pool, drive_id)
        .await?
        .ok_or(IngestError::DriveNotFound)?;
    if drive.status != "active" {
        return Err(IngestError::DriveNotRunnable(drive.status));
    }
    let g = drive.sync_generation + 1;
    let held: Option<String> = sqlx::query_scalar(
        "SELECT status FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid AND generation = $2",
    )
    .bind(drive_id)
    .bind(g)
    .fetch_optional(pool)
    .await?;
    let is_held = held.as_deref() == Some("held");
    if let Some(c) = flavor.confirm {
        if !is_held || c != g {
            return Err(IngestError::ConfirmMismatch { generation: g });
        }
    } else if is_held {
        return Ok(EnqueuedSync::Held {
            generation: g,
            would_delete: would_delete(pool, drive_id, g).await?,
        });
    }
    let live: Option<String> = sqlx::query_scalar(
        "SELECT dedupe_key FROM _oo_docs.job WHERE kind = 'sync' AND drive_id = $1::uuid \
         AND state IN ('queued', 'running') LIMIT 1",
    )
    .bind(drive_id)
    .fetch_optional(pool)
    .await?;
    if live.is_some() {
        return Ok(EnqueuedSync::Running { generation: g });
    }
    let base = flavor.dedupe_key(drive_id, g);
    let mut conn = pool.acquire().await?;
    for n in 0..=6 {
        let key = if n == 0 {
            base.clone()
        } else {
            format!("{base}#retry{n}")
        };
        let mut job = NewJob::new(JobKind::Sync, &key);
        job.drive_id = Some(drive_id);
        job.priority = if flavor.trigger == Trigger::Manual {
            50
        } else {
            100
        };
        let res = store::enqueue(&mut conn, &job).await?;
        match res.status {
            JobStatus::Enqueued => {
                let job_id = res.job.map(|j| j.id).unwrap_or_default();
                return Ok(EnqueuedSync::Enqueued {
                    generation: g,
                    job_id,
                });
            }
            JobStatus::Replayed => return Ok(EnqueuedSync::Running { generation: g }),
            _ => continue,
        }
    }
    Ok(EnqueuedSync::Running { generation: g })
}

// ---------------------------------------------------------------------------
// the per drive lock
// ---------------------------------------------------------------------------

struct DriveLock {
    conn: PoolConnection<Postgres>,
    key: String,
}

impl DriveLock {
    async fn acquire(pool: &PgPool, drive_id: &str) -> Result<Self, IngestError> {
        let mut conn = pool.acquire().await?;
        // Ends the session (and so the lock) even if this value is dropped
        // without `release`, e.g. by a panic.
        conn.close_on_drop();
        let key = format!("oo_docs.sync:{drive_id}");
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1, 0))")
            .bind(&key)
            .fetch_one(&mut *conn)
            .await?;
        if !got {
            return Err(IngestError::SyncRunning);
        }
        Ok(Self { conn, key })
    }

    async fn release(mut self) {
        let _ = sqlx::query("SELECT pg_advisory_unlock(hashtextextended($1, 0))")
            .bind(&self.key)
            .fetch_one(&mut *self.conn)
            .await;
    }
}

// ---------------------------------------------------------------------------
// drive_sync bookkeeping
// ---------------------------------------------------------------------------

const SYNC_JSON: &str = "jsonb_build_object('generation', generation, 'status', status, \
    'trigger', trigger, 'added', added, 'changed', changed, 'removed', removed, \
    'renamed', renamed, 'error_code', error_code, \
    'started_at', to_char(started_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
    'finished_at', CASE WHEN finished_at IS NULL THEN NULL ELSE \
       to_char(finished_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END)";

async fn sync_event(
    conn: &mut sqlx::PgConnection,
    kind: &str,
    drive_id: &str,
    generation: i64,
    extra: Option<(&str, i64)>,
) -> Result<(), IngestError> {
    let sql = format!(
        "SELECT jsonb_build_object('drive_id', drive_id::text, 'sync', {SYNC_JSON})::text \
         FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid AND generation = $2"
    );
    let payload: String = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(drive_id)
        .bind(generation)
        .fetch_one(&mut *conn)
        .await?;
    let payload = match extra {
        Some((k, v)) => {
            let mut value: serde_json::Value =
                serde_json::from_str(&payload).map_err(|e| IngestError::Protocol(e.to_string()))?;
            value[k] = serde_json::json!(v);
            value.to_string()
        }
        None => payload,
    };
    events::append_event(conn, kind, Some(drive_id), None, &payload).await?;
    Ok(())
}

async fn start_sync(
    pool: &PgPool,
    drive_id: &str,
    g: i64,
    trigger: Trigger,
) -> Result<(), IngestError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO _oo_docs.drive_sync (drive_id, generation, trigger, status) \
         VALUES ($1::uuid, $2, $3, 'running') \
         ON CONFLICT (drive_id, generation) DO UPDATE SET trigger = EXCLUDED.trigger, \
           status = 'running', listed = 0, pages = 0, added = 0, changed = 0, removed = 0, \
           renamed = 0, error_code = NULL, started_at = clock_timestamp(), finished_at = NULL",
    )
    .bind(drive_id)
    .bind(g)
    .bind(trigger.as_str())
    .execute(&mut *tx)
    .await?;
    sync_event(&mut tx, "drive.sync_started", drive_id, g, None).await?;
    tx.commit().await?;
    Ok(())
}

async fn finish_sync(
    pool: &PgPool,
    drive_id: &str,
    report: &SyncReport,
) -> Result<(), IngestError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE _oo_docs.drive_sync SET status = $3, listed = $4, pages = $5, added = $6, \
         changed = $7, removed = $8, renamed = $9, error_code = $10, \
         finished_at = clock_timestamp() WHERE drive_id = $1::uuid AND generation = $2",
    )
    .bind(drive_id)
    .bind(report.generation)
    .bind(report.status.as_str())
    .bind(report.listed)
    .bind(report.pages)
    .bind(report.added)
    .bind(report.changed)
    .bind(report.removed)
    .bind(report.renamed)
    .bind(report.error_code.as_deref())
    .execute(&mut *tx)
    .await?;
    if report.status == SyncStatus::Complete {
        sqlx::query(
            "UPDATE _oo_docs.drive SET sync_generation = $2, updated_at = clock_timestamp() \
             WHERE id = $1::uuid AND sync_generation < $2",
        )
        .bind(drive_id)
        .bind(report.generation)
        .execute(&mut *tx)
        .await?;
    }
    let kind = match report.status {
        SyncStatus::Complete => "drive.sync_completed",
        SyncStatus::Failed => "drive.sync_failed",
        SyncStatus::Held => "drive.sync_held",
    };
    let extra = report.would_delete.map(|n| ("would_delete", n));
    sync_event(&mut tx, kind, drive_id, report.generation, extra).await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// listing
// ---------------------------------------------------------------------------

struct Listing {
    entries: Vec<ObjectMeta>,
    pages: i32,
    complete: bool,
    error: Option<DocSourceError>,
}

async fn list_all<S: HashableSource>(source: &S, page_size: u32, cancel: &AtomicBool) -> Listing {
    let mut out = Listing {
        entries: Vec::new(),
        pages: 0,
        complete: false,
        error: None,
    };
    let mut cursor: Option<ListCursor> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return out;
        }
        match source.list(cursor.as_ref(), page_size).await {
            Ok(page) => {
                out.pages += 1;
                out.entries.extend(page.entries);
                match page.next {
                    Some(next) => {
                        cursor = Some(next);
                        if out.entries.len() > MAX_ENTRIES || out.pages > MAX_PAGES {
                            return out; // incomplete: never sweep a runaway listing
                        }
                    }
                    None => {
                        out.complete = page.complete;
                        return out;
                    }
                }
            }
            Err(e) => {
                out.error = Some(e);
                return out;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

/// Open the source for a Drive and run a sync. A source that cannot be opened
/// fails the generation with its `DOCS_SOURCE_*` code (root missing -> no
/// sweep).
pub async fn run_sync<F: SourceFactory>(
    pool: &PgPool,
    factory: &F,
    drive_id: &str,
    flavor: SyncFlavor,
    limits: &IngestLimits,
    cancel: &AtomicBool,
) -> Result<SyncReport, IngestError> {
    let drive = load_drive(pool, drive_id)
        .await?
        .ok_or(IngestError::DriveNotFound)?;
    match factory.open(&drive) {
        Ok(source) => run_sync_source(pool, &drive, &source, flavor, limits, cancel).await,
        Err(e) => {
            let lock = DriveLock::acquire(pool, drive_id).await?;
            let r = async {
                check_runnable(&drive)?;
                let g = drive.sync_generation + 1;
                start_sync(pool, drive_id, g, flavor.trigger).await?;
                let code = match &e {
                    DocSourceError::InvalidConfig(m) if m.contains("root") => {
                        "DOCS_SOURCE_ROOT_MISSING"
                    }
                    other => other.code(),
                };
                let report = SyncReport {
                    generation: g,
                    status: SyncStatus::Failed,
                    listed: 0,
                    pages: 0,
                    added: 0,
                    changed: 0,
                    removed: 0,
                    renamed: 0,
                    error_code: Some(code.to_owned()),
                    would_delete: None,
                    hash_enqueued: 0,
                };
                finish_sync(pool, drive_id, &report).await?;
                Ok(report)
            }
            .await;
            lock.release().await;
            r
        }
    }
}

fn check_runnable(drive: &DriveRef) -> Result<(), IngestError> {
    if drive.status == "active" {
        Ok(())
    } else {
        Err(IngestError::DriveNotRunnable(drive.status.clone()))
    }
}

/// Run one full reconcile of `drive` against `source`.
pub async fn run_sync_source<S: HashableSource>(
    pool: &PgPool,
    drive: &DriveRef,
    source: &S,
    flavor: SyncFlavor,
    limits: &IngestLimits,
    cancel: &AtomicBool,
) -> Result<SyncReport, IngestError> {
    let lock = DriveLock::acquire(pool, &drive.id).await?;
    let out = async {
        // Re-read under the lock: a sync that finished a moment ago moved it.
        let fresh = load_drive(pool, &drive.id)
            .await?
            .ok_or(IngestError::DriveNotFound)?;
        check_runnable(&fresh)?;
        let g = fresh.sync_generation + 1;
        if flavor.confirm.is_some_and(|c| c != g) {
            return Err(IngestError::ConfirmMismatch { generation: g });
        }
        start_sync(pool, &fresh.id, g, flavor.trigger).await?;
        match sync_core(pool, &fresh, g, source, flavor, limits, cancel).await {
            Ok(report) => {
                finish_sync(pool, &fresh.id, &report).await?;
                Ok(report)
            }
            Err(e) => {
                // Record the failure best effort; the job layer retries.
                let report = SyncReport {
                    generation: g,
                    status: SyncStatus::Failed,
                    listed: 0,
                    pages: 0,
                    added: 0,
                    changed: 0,
                    removed: 0,
                    renamed: 0,
                    error_code: Some("DOCS_ENGINE_DB_UNAVAILABLE".to_owned()),
                    would_delete: None,
                    hash_enqueued: 0,
                };
                let _ = finish_sync(pool, &fresh.id, &report).await;
                Err(e)
            }
        }
    }
    .await;
    lock.release().await;
    out
}

struct NewFile {
    id: String,
    path: String,
    meta: ObjectMeta,
}

async fn hash_backlog(pool: &PgPool) -> Result<i64, IngestError> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.job WHERE kind = 'hash' AND state = 'queued'",
    )
    .fetch_one(pool)
    .await?)
}

async fn sync_core<S: HashableSource>(
    pool: &PgPool,
    drive: &DriveRef,
    g: i64,
    source: &S,
    flavor: SyncFlavor,
    limits: &IngestLimits,
    cancel: &AtomicBool,
) -> Result<SyncReport, IngestError> {
    let mut report = SyncReport {
        generation: g,
        status: SyncStatus::Failed,
        listed: 0,
        pages: 0,
        added: 0,
        changed: 0,
        removed: 0,
        renamed: 0,
        error_code: None,
        would_delete: None,
        hash_enqueued: 0,
    };
    let listing = list_all(source, limits.page_size, cancel).await;
    report.pages = listing.pages;
    let entries: Vec<ObjectMeta> = listing
        .entries
        .into_iter()
        .filter(|e| matches!(e.kind, EntryKind::File | EntryKind::Symlink))
        .collect();
    report.listed = entries.len() as i64;
    let full = listing.complete && listing.error.is_none() && !cancel.load(Ordering::Relaxed);

    let existing_rows = {
        let mut conn = pool.acquire().await?;
        files::load_files(&mut conn, &drive.id).await?
    };
    let live_before = existing_rows.iter().filter(|r| !r.deleted).count();
    let existing: HashMap<String, &FileRow> =
        existing_rows.iter().map(|r| (hex(&r.raw_key), r)).collect();

    let statuses = annotate(&entries);
    let mut budget_open = hash_backlog(pool).await? <= limits.hash_backlog_pause;
    // Room for new files under max_files (4.3.4): first N by path bytes.
    let mut room = (drive.max_files - live_before as i64).max(0);
    let mut new_order: Vec<usize> = (0..entries.len())
        .filter(|i| {
            !existing.contains_key(&hex(&entries[*i].raw_key))
                && entries[*i].kind == EntryKind::File
                && matches!(statuses[*i], PathStatus::Ok | PathStatus::FoldConflict)
        })
        .collect();
    new_order.sort_by(|a, b| {
        let pa = entries[*a].path.as_ref().map(|p| p.as_str().as_bytes());
        let pb = entries[*b].path.as_ref().map(|p| p.as_str().as_bytes());
        pa.cmp(&pb)
    });
    let mut over_limit: HashSet<usize> = HashSet::new();
    for i in new_order {
        if room > 0 {
            room -= 1;
        } else {
            over_limit.insert(i);
        }
    }

    let mut listed_keys: HashSet<String> = HashSet::with_capacity(entries.len());
    let mut created: Vec<NewFile> = Vec::new();
    let mut pending_hash: Vec<(String, Fingerprint)> = Vec::new(); // new files, enqueued last
    let mut oversize: Vec<(&FileRow, Fingerprint)> = Vec::new();
    let mut symlinked: Vec<&FileRow> = Vec::new();

    for (chunk_no, chunk) in entries.chunks(500).enumerate() {
        let base = chunk_no * 500;
        let mut touch_ids: Vec<String> = Vec::new();
        let mut enqueue: Vec<(String, Fingerprint)> = Vec::new();
        let mut news: Vec<(usize, &ObjectMeta)> = Vec::new();
        for (off, meta) in chunk.iter().enumerate() {
            let idx = base + off;
            let key = hex(&meta.raw_key);
            listed_keys.insert(key.clone());
            let Some(row) = existing.get(&key) else {
                news.push((idx, meta));
                continue;
            };
            let fp = meta.fingerprint();
            if row.deleted {
                // Reappeared: touch and hash; the hash job revives the row.
                touch_ids.push(row.id.clone());
                if meta.kind == EntryKind::File {
                    enqueue.push((row.id.clone(), fp));
                    report.changed += 1;
                }
                continue;
            }
            touch_ids.push(row.id.clone());
            if meta.kind == EntryKind::Symlink {
                if row.state != "unsupported" {
                    symlinked.push(row);
                }
                continue;
            }
            if matches!(statuses[idx], PathStatus::Invalid | PathStatus::Collision) {
                continue;
            }
            let changed = flavor.deep
                || match &row.fp {
                    Some(stored) => !fp_matches(&fp, stored),
                    None => row.state == "stored",
                };
            let stuck_permission = row.state == "parse_failed"
                && row.state_code.as_deref() == Some("DOCS_SOURCE_PERMISSION_DENIED");
            if changed || stuck_permission {
                report.changed += 1;
                if fp.size > limits.size_limit(&row.path) {
                    oversize.push((row, fp));
                } else {
                    enqueue.push((row.id.clone(), fp));
                }
            }
        }

        // One transaction per chunk: bulk touch first, job rows next, events
        // (new files) last, so the event_head lock is taken after every other
        // lock this transaction needs (4.1.3).
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        files::touch(&mut tx, &touch_ids, g).await?;
        if budget_open {
            for (id, fp) in &enqueue {
                files::enqueue_hash(&mut tx, &drive.id, id, fp, g, 100).await?;
                report.hash_enqueued += 1;
            }
        }
        for (idx, meta) in news {
            let status = statuses[idx];
            let (path, fold, verdict) = new_file_identity(meta, status);
            let verdict = if verdict.is_none() && over_limit.contains(&idx) {
                Some((FileState::Unsupported, "DOCS_SOURCE_DRIVE_FILE_LIMIT"))
            } else if verdict.is_none() && meta.size > limits.size_limit(&path) {
                Some((FileState::TooLarge, "DOCS_SOURCE_TOO_LARGE"))
            } else {
                verdict
            };
            let Some(id) =
                files::insert_file(&mut tx, &drive.id, &meta.raw_key, &path, &fold, g).await?
            else {
                continue; // a concurrent creator won
            };
            report.added += 1;
            match verdict {
                Some((to, code)) => {
                    files::to_state(&mut tx, &id, FileState::Stored, to, Extra::code(Some(code)))
                        .await?;
                    // Remember the fingerprint so the next listing is quiet.
                    sqlx::query(
                        "UPDATE _oo_docs.file SET fp_size = $2, fp_mtime_ns = $3, fp_etag = $4, \
                         fp_version_id = $5, fp_ino = $6 WHERE id = $1::uuid",
                    )
                    .bind(&id)
                    .bind(meta.size as i64)
                    .bind(
                        meta.last_modified_ns
                            .map(|v| v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64),
                    )
                    .bind(meta.etag.as_deref())
                    .bind(meta.version_id.as_deref())
                    .bind(meta.fs_ino.map(|i| i as i64))
                    .execute(&mut *tx)
                    .await?;
                }
                None => {
                    pending_hash.push((id.clone(), meta.fingerprint()));
                    created.push(NewFile {
                        id,
                        path,
                        meta: meta.clone(),
                    });
                }
            }
        }
        tx.commit().await?;
        if budget_open && (report.hash_enqueued as i64) > limits.hash_backlog_pause {
            budget_open = false;
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
    }

    // Oversize and symlink conversions: one transaction each, they lock a
    // pre-existing file row and then append events.
    for (row, fp) in oversize {
        let mut tx = pool.begin().await?;
        if let Some((st, del)) = files::lock_file(&mut tx, &row.id).await? {
            let st = FileState::parse(&st).unwrap_or(FileState::Stored);
            mark_too_large(&mut tx, row, st, del, &fp).await?;
        }
        tx.commit().await?;
    }
    for row in symlinked {
        let mut tx = pool.begin().await?;
        if let Some((st, del)) = files::lock_file(&mut tx, &row.id).await? {
            let st = FileState::parse(&st).unwrap_or(FileState::Stored);
            if st != FileState::Unsupported && !del {
                files::cancel_file_jobs(&mut tx, &row.id, None, true).await?;
                files::restart_as_stored(&mut tx, &row.id, st, row.version_id.as_deref()).await?;
                files::to_state(
                    &mut tx,
                    &row.id,
                    FileState::Stored,
                    FileState::Unsupported,
                    Extra::code(Some("DOCS_SOURCE_SYMLINK_SKIPPED")),
                )
                .await?;
            }
        }
        tx.commit().await?;
    }

    let mut renamed_new: HashSet<String> = HashSet::new();
    if !full {
        report.error_code = Some(match &listing.error {
            Some(e) => e.code().to_owned(),
            None => "DOCS_SOURCE_LIST_INCOMPLETE".to_owned(),
        });
        report.status = SyncStatus::Failed;
    } else {
        // Sweep set from the database, not from the snapshot: files that a
        // targeted sync created meanwhile carry generation g already.
        let missing = load_missing(pool, &drive.id, g).await?;
        let mass = (missing.len() > limits.mass_delete_min && missing.len() * 2 > live_before)
            || (entries.is_empty() && live_before > 0 && !missing.is_empty());
        if mass && flavor.confirm != Some(g) {
            report.status = SyncStatus::Held;
            report.would_delete = Some(missing.len() as i64);
            report.error_code = Some("DOCS_SOURCE_MASS_DELETE_HELD".to_owned());
        } else {
            let renames = detect_renames(pool, source, &missing, &created, limits).await?;
            let mut gone: Vec<&Missing> = Vec::new();
            for m in &missing {
                match renames.iter().find(|r| r.old_id == m.id) {
                    Some(r) => {
                        if apply_rename(pool, drive, m, r, &created, limits).await? {
                            report.renamed += 1;
                            renamed_new.insert(r.new_id.clone());
                        } else {
                            gone.push(m);
                        }
                    }
                    None => gone.push(m),
                }
            }
            for m in gone {
                let mut tx = pool.begin().await?;
                sqlx::query("SET LOCAL lock_timeout = '5s'")
                    .execute(&mut *tx)
                    .await?;
                if files::delete_file(&mut tx, &m.id, None, None).await? {
                    report.removed += 1;
                }
                tx.commit().await?;
            }
            // Renames are not additions.
            report.added -= renamed_new.len() as i32;
            report.status = SyncStatus::Complete;
            let total = report.added + report.changed + report.removed + report.renamed;
            if total > 200 {
                bulk_changed(pool, &drive.id, &report).await?;
            }
        }
    }

    // New files that were not renamed get their hash job now (after rename
    // detection, so a renamed file is not hashed twice).
    if budget_open {
        for chunk in pending_hash.chunks(500) {
            let mut tx = pool.begin().await?;
            for (id, fp) in chunk {
                if renamed_new.contains(id) {
                    continue;
                }
                files::enqueue_hash(&mut tx, &drive.id, id, fp, g, 100).await?;
                report.hash_enqueued += 1;
            }
            tx.commit().await?;
        }
    }
    Ok(report)
}

/// Identity of a new file row: `(path, fold, forced verdict)`. Paths that do
/// not exist as such (invalid, collision) get a synthetic unique path so the
/// row can exist at all.
fn new_file_identity(
    meta: &ObjectMeta,
    status: PathStatus,
) -> (String, String, Option<(FileState, &'static str)>) {
    let synthetic = |prefix: &str| {
        let digest = hex(blake3::hash(&meta.raw_key).as_bytes());
        format!("{prefix}/{}", &digest[..32])
    };
    match (status, meta.path.as_ref()) {
        (PathStatus::Invalid, _) | (_, None) => {
            let p = synthetic("_invalid");
            (
                p.clone(),
                p,
                Some((FileState::Unsupported, "DOCS_SOURCE_PATH_INVALID")),
            )
        }
        (PathStatus::Collision, _) => {
            let p = synthetic("_collision");
            (
                p.clone(),
                p,
                Some((FileState::Unsupported, "DOCS_SOURCE_PATH_COLLISION")),
            )
        }
        (_, Some(p)) => {
            let verdict = (meta.kind == EntryKind::Symlink)
                .then_some((FileState::Unsupported, "DOCS_SOURCE_SYMLINK_SKIPPED"));
            (p.as_str().to_owned(), path_fold(p), verdict)
        }
    }
}

// ---------------------------------------------------------------------------
// sweep helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Missing {
    id: String,
    path: String,
    state: String,
    version_id: Option<String>,
    b3: Option<String>,
    size: Option<u64>,
}

async fn load_missing(pool: &PgPool, drive_id: &str, g: i64) -> Result<Vec<Missing>, IngestError> {
    let rows = sqlx::query(
        "SELECT f.id::text AS id, f.path, f.state, cv.id::text AS vid, cv.b3, \
         COALESCE((cv.fp->>'size')::bigint, f.fp_size) AS size \
         FROM _oo_docs.file f LEFT JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
         WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND f.last_seen_generation < $2 \
         ORDER BY f.path",
    )
    .bind(drive_id)
    .bind(g)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(Missing {
                id: r.try_get("id")?,
                path: r.try_get("path")?,
                state: r.try_get("state")?,
                version_id: r.try_get("vid")?,
                b3: r.try_get("b3")?,
                size: r
                    .try_get::<Option<i64>, _>("size")?
                    .map(|s| s.max(0) as u64),
            })
        })
        .collect()
}

struct RenamePair {
    old_id: String,
    new_id: String,
    digest: DualDigest,
    fp: Fingerprint,
    head: Vec<u8>,
}

/// Match missing files with new paths of this run by content: only new files
/// of the same size as a missing one are hashed (bounded by
/// `rename_candidate_cap`), the match is the BLAKE3.
async fn detect_renames<S: HashableSource>(
    _pool: &PgPool,
    source: &S,
    missing: &[Missing],
    created: &[NewFile],
    limits: &IngestLimits,
) -> Result<Vec<RenamePair>, IngestError> {
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, n) in created.iter().enumerate() {
        by_size.entry(n.meta.size).or_default().push(i);
    }
    let mut hashed: HashMap<usize, Option<(DualDigest, Fingerprint, Vec<u8>)>> = HashMap::new();
    let mut used: HashSet<usize> = HashSet::new();
    let mut budget = limits.rename_candidate_cap;
    let mut out = Vec::new();
    for m in missing {
        let (Some(b3), Some(size)) = (&m.b3, m.size) else {
            continue;
        };
        let Some(cands) = by_size.get(&size) else {
            continue;
        };
        for &ci in cands {
            if used.contains(&ci) {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(slot) = hashed.entry(ci) {
                if budget == 0 {
                    continue;
                }
                budget -= 1;
                let n = &created[ci];
                let got = match source.hash_object(&n.meta.raw_key).await {
                    Ok((digest, meta)) => {
                        let head = source
                            .read(
                                &n.meta.raw_key,
                                0,
                                digest.len.min(SNIFF_HEAD),
                                &ReadExpect::default(),
                            )
                            .await
                            .ok();
                        head.map(|h| (digest, meta.fingerprint(), h))
                    }
                    Err(_) => None,
                };
                slot.insert(got);
            }
            if let Some(Some((digest, fp, head))) = hashed.get(&ci)
                && &digest.b3 == b3
            {
                used.insert(ci);
                out.push(RenamePair {
                    old_id: m.id.clone(),
                    new_id: created[ci].id.clone(),
                    digest: digest.clone(),
                    fp: fp.clone(),
                    head: head.clone(),
                });
                break;
            }
        }
    }
    Ok(out)
}

/// One rename in one transaction (4.3.2): the old file is deleted and its
/// evidence withdrawn, the new file gets its version, and `file.renamed` is
/// the last event. The new version points at the old one (`duplicate_of`)
/// when the old file had a finished result, so evidence can move across.
async fn apply_rename(
    pool: &PgPool,
    drive: &DriveRef,
    old: &Missing,
    pair: &RenamePair,
    created: &[NewFile],
    limits: &IngestLimits,
) -> Result<bool, IngestError> {
    let Some(new) = created.iter().find(|n| n.id == pair.new_id) else {
        return Ok(false);
    };
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    // Both rows locked before the first event; id order avoids a lock cycle.
    let mut ids = [old.id.as_str(), new.id.as_str()];
    ids.sort_unstable();
    for id in ids {
        if files::lock_file(&mut tx, id).await?.is_none() {
            tx.rollback().await?;
            return Ok(false);
        }
    }
    let new_rows = files::load_files(&mut tx, &drive.id).await?;
    let Some(new_row) = new_rows.iter().find(|r| r.id == new.id) else {
        tx.rollback().await?;
        return Ok(false);
    };
    if new_row.deleted || new_row.state != "stored" || new_row.version_id.is_some() {
        tx.rollback().await?;
        return Ok(false); // a hash job got there first: plain delete + add
    }
    let Some(prepared) = files::prepare_delete(&mut tx, &old.id).await? else {
        tx.rollback().await?;
        return Ok(false);
    };
    let finished = matches!(
        old.state.as_str(),
        "published" | "indexed" | "extract_partial"
    );
    let obj = HashedObject {
        digest: &pair.digest,
        fp: &pair.fp,
        head: &pair.head,
    };
    let plan = prepare_version(
        &mut tx,
        new_row,
        FileState::Stored,
        false,
        &obj,
        limits,
        if finished {
            old.version_id.as_deref()
        } else {
            None
        },
    )
    .await?;
    let detail = serde_json::json!({ "renamed_to": new.path }).to_string();
    files::finish_delete(&mut tx, &prepared, None, Some(&detail)).await?;
    apply_plan(&mut tx, &plan).await?;
    files::append_renamed(&mut tx, &drive.id, &new.id, &old.path, &new.path).await?;
    tx.commit().await?;
    Ok(true)
}

async fn bulk_changed(
    pool: &PgPool,
    drive_id: &str,
    report: &SyncReport,
) -> Result<(), IngestError> {
    let payload = serde_json::json!({
        "drive_id": drive_id,
        "count": report.added + report.changed + report.removed + report.renamed,
        "states": {
            "stored": report.added,
            "deleted": report.removed,
        },
    })
    .to_string();
    let mut conn = pool.acquire().await?;
    events::append_event(
        &mut conn,
        "files.bulk_changed",
        Some(drive_id),
        None,
        &payload,
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// targeted sync
// ---------------------------------------------------------------------------

/// G11 / D47: look at `paths` (and their parents) only. New or changed files
/// are queued like in a full sync. It never moves `sync_generation`, never
/// writes a `drive_sync` row and never deletes: a path that is gone is only
/// reported, the next full reconcile confirms the deletion.
pub async fn targeted_sync<S: HashableSource>(
    pool: &PgPool,
    drive: &DriveRef,
    source: &S,
    paths: &[String],
    limits: &IngestLimits,
) -> Result<TargetedReport, IngestError> {
    check_runnable(drive)?;
    let mut report = TargetedReport::default();
    // A file created now must not look "unseen" to a sweep that is running:
    // use the highest generation any sync of this drive has started.
    let seen: i64 = sqlx::query_scalar(
        "SELECT GREATEST(d.sync_generation, COALESCE((SELECT max(generation) \
         FROM _oo_docs.drive_sync WHERE drive_id = d.id), 0)) FROM _oo_docs.drive d \
         WHERE d.id = $1::uuid",
    )
    .bind(&drive.id)
    .fetch_one(pool)
    .await?;

    let mut seen_paths: HashSet<&str> = HashSet::new();
    let mut parents: HashSet<String> = HashSet::new();
    let mut valid: Vec<(&String, DocPath)> = Vec::new();
    for p in paths {
        report.checked += 1;
        if !seen_paths.insert(p.as_str()) {
            continue;
        }
        match DocPath::new(p) {
            Ok(dp) if !oo_connectors::objectstore::path::is_skipped_path(p) => {
                if let Some(parent) = dp.parent() {
                    parents.insert(parent.as_str().to_owned());
                }
                valid.push((p, dp));
            }
            _ => report.skipped.push(p.clone()),
        }
    }
    // Parents: only existence is of interest; a missing parent means every
    // path under it is "maybe deleted", reported below via its own stat.
    for parent in &parents {
        let _ = source.stat(parent.as_bytes()).await;
    }

    let existing_rows = {
        let mut conn = pool.acquire().await?;
        files::load_files(&mut conn, &drive.id).await?
    };
    let existing: HashMap<String, &FileRow> =
        existing_rows.iter().map(|r| (hex(&r.raw_key), r)).collect();
    let budget_open = hash_backlog(pool).await? <= limits.hash_backlog_pause;

    for (orig, dp) in valid {
        let raw = dp.as_str().as_bytes().to_vec();
        let meta = match source.stat(&raw).await {
            Ok(Some(m)) if m.kind == EntryKind::File => m,
            Ok(Some(_)) => {
                report.skipped.push(orig.clone());
                continue;
            }
            Ok(None) | Err(DocSourceError::NotFound) => {
                report.maybe_deleted.push(orig.clone());
                continue;
            }
            Err(_) => {
                report.skipped.push(orig.clone());
                continue;
            }
        };
        let fp = meta.fingerprint();
        let key = hex(&raw);
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        match existing.get(&key) {
            Some(row) => {
                files::touch(&mut tx, std::slice::from_ref(&row.id), seen).await?;
                let changed = row.deleted
                    || match &row.fp {
                        Some(stored) => !fp_matches(&fp, stored),
                        None => row.state == "stored",
                    };
                if changed {
                    report.changed += 1;
                    if fp.size > limits.size_limit(&row.path) {
                        // Needs its own transaction: it locks the row and appends events.
                        tx.commit().await?;
                        let mut tx2 = pool.begin().await?;
                        if let Some((st, del)) = files::lock_file(&mut tx2, &row.id).await? {
                            let st = FileState::parse(&st).unwrap_or(FileState::Stored);
                            mark_too_large(&mut tx2, row, st, del, &fp).await?;
                        }
                        tx2.commit().await?;
                        continue;
                    }
                    if budget_open {
                        files::enqueue_hash(&mut tx, &drive.id, &row.id, &fp, seen, 50).await?;
                        report.hash_enqueued += 1;
                    }
                } else {
                    report.unchanged += 1;
                }
            }
            None => {
                let status = if meta.path.is_some() {
                    PathStatus::Ok
                } else {
                    PathStatus::Invalid
                };
                let (path, fold, verdict) = new_file_identity(&meta, status);
                let verdict = verdict.or_else(|| {
                    (meta.size > limits.size_limit(&path))
                        .then_some((FileState::TooLarge, "DOCS_SOURCE_TOO_LARGE"))
                });
                if let Some(id) =
                    files::insert_file(&mut tx, &drive.id, &raw, &path, &fold, seen).await?
                {
                    report.added += 1;
                    match verdict {
                        Some((to, code)) => {
                            files::to_state(
                                &mut tx,
                                &id,
                                FileState::Stored,
                                to,
                                Extra::code(Some(code)),
                            )
                            .await?;
                        }
                        None if budget_open => {
                            files::enqueue_hash(&mut tx, &drive.id, &id, &fp, seen, 50).await?;
                            report.hash_enqueued += 1;
                        }
                        None => {}
                    }
                }
            }
        }
        tx.commit().await?;
    }
    let _ = (is_settled, fp_sha256); // shared helpers used by the hash job
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::objectstore::path::normalize_raw_key;

    fn meta(raw: &[u8], kind: EntryKind) -> ObjectMeta {
        ObjectMeta {
            raw_key: raw.to_vec(),
            path: normalize_raw_key(raw),
            kind,
            size: 1,
            etag: None,
            last_modified_ns: Some(1),
            version_id: None,
            crc64: None,
            fs_ino: None,
            fs_ctime_ns: None,
        }
    }

    #[test]
    fn identities_of_special_entries_are_unique_and_valid_paths() {
        let a = new_file_identity(&meta(&[b'a', 0xFF], EntryKind::File), PathStatus::Invalid);
        let b = new_file_identity(&meta(&[b'b', 0xFF], EntryKind::File), PathStatus::Invalid);
        assert_ne!(a.0, b.0);
        assert!(a.0.starts_with("_invalid/") && a.0.len() < 64);
        assert_eq!(
            a.2,
            Some((FileState::Unsupported, "DOCS_SOURCE_PATH_INVALID"))
        );
        let c = new_file_identity(&meta(b"x.md", EntryKind::File), PathStatus::Collision);
        assert!(c.0.starts_with("_collision/"));
        assert_eq!(
            c.2,
            Some((FileState::Unsupported, "DOCS_SOURCE_PATH_COLLISION"))
        );
        let ok = new_file_identity(&meta(b"Dir/X.md", EntryKind::File), PathStatus::Ok);
        assert_eq!(
            (ok.0.as_str(), ok.1.as_str(), ok.2),
            ("Dir/X.md", "dir/x.md", None)
        );
        let link = new_file_identity(&meta(b"l.md", EntryKind::Symlink), PathStatus::Ok);
        assert_eq!(
            link.2,
            Some((FileState::Unsupported, "DOCS_SOURCE_SYMLINK_SKIPPED"))
        );
    }

    #[test]
    fn every_code_used_exists_in_the_contract() {
        let raw = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../contracts/docs-api/errors.v1.json"),
        )
        .unwrap();
        for code in [
            "DOCS_SOURCE_PATH_INVALID",
            "DOCS_SOURCE_PATH_COLLISION",
            "DOCS_SOURCE_SYMLINK_SKIPPED",
            "DOCS_SOURCE_DRIVE_FILE_LIMIT",
            "DOCS_SOURCE_TOO_LARGE",
            "DOCS_SOURCE_PERMISSION_DENIED",
            "DOCS_SOURCE_ROOT_MISSING",
            "DOCS_SOURCE_MASS_DELETE_HELD",
            "DOCS_SOURCE_LIST_INCOMPLETE",
            "DOCS_SOURCE_UNSTABLE",
            "DOCS_ENGINE_DB_UNAVAILABLE",
            "DOCS_ENGINE_INTERNAL",
        ] {
            assert!(raw.contains(&format!("\"{code}\"")), "{code}");
        }
    }
}
