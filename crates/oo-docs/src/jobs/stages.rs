//! The L2 pipeline stages after ingest: parse (md/txt), extract_rules and
//! publish, plus the pump that feeds them (10 §4.1, §4.2, §4.9).
//!
//! The hash stage leaves a file in `queued`. The [`pump`] looks at the file
//! table every few hundred milliseconds and enqueues the next stage for every
//! file that has none in flight: `queued -> parse`, `parsed -> extract_rules`,
//! and a debounced `publish` when something is `indexed` or a fact waits for
//! its retraction to be published. The rows in `file` are the source of truth,
//! so a lost wake-up or a restarted engine only delays work, never loses it.
//! The pump also moves files whose stage job failed for good into the failure
//! state the table allows (`parse_failed`, `extract_partial`).
//!
//! Every stage finishes its job and writes its state transition in one
//! transaction (4.1.3); the stage products (CAS object, `parse_result`,
//! drafts) are idempotent, so a rerun after a crash adds nothing.
//!
//! L2 simplifications (documented in the card report):
//! * md/txt are parsed in the engine process on a blocking thread, not in the
//!   `oo-docs-parse` child (a parser panic is classified as a crash and
//!   retried; process isolation returns with the binary formats);
//! * parse job keys carry the file version (`...:v<version_id>`) next to the
//!   blob, so a file that returns to an earlier content is not stuck behind a
//!   finished key; a finished `parse_result` of the same blob is reused
//!   without parsing again (4.1.2: parse once per blob).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use super::scheduler::{HandlerFuture, JobCtx, JobHandler, Outcome};
use super::{FailureClass, JobKind, JobStatus, NewJob};
use crate::api::progress::{Bus, Progress};
use crate::cas::{Cas, CasError, CasKind};
use crate::extract::rules::RULESET_VERSION;
use crate::ingest::files::{self, Extra};
use crate::ingest::{DriveRef, SourceFactory};
use crate::parse::{self, LOCAL_PARSER_VERSION, ParseError};
use crate::publish::run::{PublishConfig, PublishOutcome, publish};
use crate::publish::write::{DocumentInput, index_document};
use crate::state::{self, FileState, Transition, TransitionError};
use crate::store::jobs as store;
use oo_connectors::objectstore::{DocumentSource, ReadExpect};
use oo_core::docir::DocIr;

/// Priority of a new file's stage jobs; manual retry 50; reprocessing after an
/// upgrade 900 (4.1.7, D23).
const PRIORITY_NEW: i16 = 100;
const PRIORITY_OUTDATED: i16 = 900;

/// Debounce window of the `publish` job (4.9).
pub const PUBLISH_WINDOW_MS: i64 = 2_000;
/// Pump period.
pub const PUMP_TICK: Duration = Duration::from_millis(250);
const PUMP_BATCH: i64 = 200;

/// Code of a failure caused by lock contention, which a handler may retry at
/// once (the stages are idempotent) instead of waiting out the job backoff.
const CONTENDED: &str = "DOCS_ENGINE_DB_CONTENDED";

/// Deadlock, serialization failure or lock timeout, from the text of a driver
/// or store error (`sqlstate=40P01`, `40001`, `55P03`).
fn is_contention(text: &str) -> bool {
    ["sqlstate=40P01", "sqlstate=40001", "sqlstate=55P03"]
        .iter()
        .any(|s| text.contains(s))
        || text.contains("deadlock detected")
        || text.contains("could not serialize")
        || text.contains("lock timeout")
}

fn db_fail(e: impl std::fmt::Display) -> Outcome {
    // The driver text can echo values (a unique violation names the key), so
    // only the SQLSTATE, a fixed vocabulary, goes to the log.
    let text = e.to_string();
    let state = text
        .split("sqlstate=")
        .nth(1)
        .map(|r| r.chars().take(5).collect::<String>())
        .filter(|s| s.len() == 5 && s.bytes().all(|b| b.is_ascii_alphanumeric()));
    let code = state.map_or_else(|| "DB".to_owned(), |s| format!("sqlstate={s}"));
    crate::obs::emit(
        "warn",
        "stage.db_failed",
        &crate::obs::Fields {
            code: Some(&code),
            ..crate::obs::Fields::default()
        },
    );
    if is_contention(&text) {
        return Outcome::failed(FailureClass::Transient, CONTENDED);
    }
    Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_DB_UNAVAILABLE")
}

/// Runs a stage handler again, a few times and quickly, when it lost a lock
/// race. Every stage is idempotent, so a rerun is safe; anything else is the
/// handler's own outcome.
struct Contended<H>(H);

impl<H: JobHandler> JobHandler for Contended<H> {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let mut tries = 0u64;
            loop {
                let out = self.0.run(ctx.clone()).await;
                let again = matches!(&out, Outcome::Failed { code, .. } if code == CONTENDED);
                tries += 1;
                if !again || tries >= 6 || ctx.should_stop() {
                    return match out {
                        Outcome::Failed { class, code } if code == CONTENDED => {
                            Outcome::failed(class, "DOCS_ENGINE_DB_UNAVAILABLE")
                        }
                        other => other,
                    };
                }
                tokio::time::sleep(Duration::from_millis(120 * tries)).await;
            }
        })
    }
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

/// A file version a stage works on.
struct Subject {
    file_id: String,
    version_id: String,
    drive_id: String,
    raw_key: Vec<u8>,
    path: String,
    state: FileState,
    b3: String,
    size: i64,
    format: String,
}

type SubjectRow = (String, String, Vec<u8>, String, String, i64, Option<String>);

/// Load the job's file version; `None` when the version is no longer the
/// file's current one (stale job) or the file is gone.
async fn load_subject(ctx: &JobCtx) -> Result<Option<Subject>, sqlx::Error> {
    let (Some(file_id), Some(version_id)) = (ctx.job.file_id.clone(), ctx.job.version_id.clone())
    else {
        return Ok(None);
    };
    let row: Option<SubjectRow> = sqlx::query_as(
        "SELECT f.state, f.drive_id::text, f.raw_key, fv.path_at_version, fv.b3, b.size, b.format \
               FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
               JOIN _oo_docs.blob b ON b.b3 = fv.b3 \
              WHERE f.id = $1::uuid AND f.current_version_id = $2::uuid \
                AND f.deleted_at IS NULL AND fv.superseded_at IS NULL AND fv.deleted_at IS NULL",
    )
    .bind(&file_id)
    .bind(&version_id)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(row.map(
        |(state, drive_id, raw_key, path, b3, size, format)| Subject {
            file_id,
            version_id,
            drive_id,
            raw_key,
            path,
            state: FileState::parse(&state).unwrap_or(FileState::Deleted),
            b3,
            size,
            format: format.unwrap_or_default(),
        },
    ))
}

async fn move_file(
    pool: &PgPool,
    s: &Subject,
    from: FileState,
    to: FileState,
    code: Option<&str>,
    retryable: bool,
    attempt: i32,
) -> Result<(), TransitionError> {
    match state::transition(
        pool,
        &Transition {
            file_id: &s.file_id,
            expect_from: Some(from),
            to,
            code,
            retryable,
            version_id: Some(&s.version_id),
            attempt: Some(attempt),
            detail_json: None,
        },
    )
    .await
    {
        Ok(_) | Err(TransitionError::Conflict { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Finish the transaction of a stage that already called `complete`.
async fn finish(
    tx: sqlx::Transaction<'static, sqlx::Postgres>,
    r: Result<bool, String>,
) -> Outcome {
    match r {
        Ok(true) => match tx.commit().await {
            Ok(()) => Outcome::Completed,
            Err(e) => db_fail(e),
        },
        Ok(false) => {
            let _ = tx.rollback().await;
            Outcome::Completed // lost lease: result discarded
        }
        Err(e) => {
            let _ = tx.rollback().await;
            db_fail(e)
        }
    }
}

fn stage_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- parse

/// `parse` handler for md and txt.
pub struct ParseHandler<F: SourceFactory> {
    pool: PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
    progress: Option<Bus>,
}

struct ParsedIr {
    ir_b3: String,
    parser_id: String,
    parser_version: String,
    profile: String,
    blocks: i32,
}

async fn cached_parse(pool: &PgPool, b3: &str) -> Result<Option<ParsedIr>, sqlx::Error> {
    let row: Option<(String, String, String, String, Option<i32>)> = sqlx::query_as(
        "SELECT ir_b3, parser_id, parser_version, profile_sha256, block_count \
           FROM _oo_docs.parse_result \
          WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
            AND parser_id IN ('oo-md', 'oo-text') AND parser_version = $2 \
          ORDER BY finished_at DESC NULLS LAST LIMIT 1",
    )
    .bind(b3)
    .bind(LOCAL_PARSER_VERSION)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(ir_b3, parser_id, parser_version, profile, blocks)| ParsedIr {
            ir_b3,
            parser_id,
            parser_version,
            profile,
            blocks: blocks.unwrap_or(0),
        },
    ))
}

impl<F: SourceFactory> ParseHandler<F> {
    async fn open_source(&self, s: &Subject) -> Result<F::Source, Outcome> {
        let row: Option<(String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT backend, config::text, status, sync_generation, max_files::bigint \
               FROM _oo_docs.drive WHERE id = $1::uuid",
        )
        .bind(&s.drive_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_fail)?;
        let Some((backend, config, status, sync_generation, max_files)) = row else {
            return Err(Outcome::Done);
        };
        if matches!(status.as_str(), "deleting" | "deleted") {
            return Err(Outcome::Done);
        }
        let drive = DriveRef {
            id: s.drive_id.clone(),
            backend,
            config: serde_json::from_str(&config).unwrap_or_default(),
            status,
            sync_generation,
            max_files,
        };
        self.factory
            .open(&drive)
            .map_err(|e| Outcome::failed(FailureClass::Permanent, e.code()))
    }

    async fn parse(&self, ctx: &JobCtx, s: &Subject) -> Result<ParsedIr, Outcome> {
        #[cfg(feature = "failpoints")]
        if crate::failpoints::hits("parse_error", &s.b3) {
            return Err(Outcome::failed(
                FailureClass::Transient,
                "DOCS_PARSE_TIMEOUT",
            ));
        }
        if let Some(hit) = cached_parse(&self.pool, &s.b3).await.map_err(db_fail)? {
            return Ok(hit);
        }
        let source = self.open_source(s).await?;
        let len = u64::try_from(s.size).unwrap_or(0);
        let bytes = match source
            .read(&s.raw_key, 0, len, &ReadExpect::default())
            .await
        {
            Ok(b) => b,
            // The file went away or moved: a later hash makes a new version.
            Err(oo_connectors::objectstore::DocSourceError::NotFound) => {
                return Err(Outcome::Done);
            }
            Err(e) => {
                let class = match e {
                    oo_connectors::objectstore::DocSourceError::PermissionDenied
                    | oo_connectors::objectstore::DocSourceError::AuthFailed => {
                        FailureClass::Permanent
                    }
                    _ => FailureClass::Transient,
                };
                return Err(Outcome::failed(class, e.code()));
            }
        };
        if ctx.should_stop() {
            return Err(Outcome::Cancelled);
        }
        self.report_start(s, &bytes).await;
        let b3 = s.b3.clone();
        let md = s.format == "md";
        #[cfg(feature = "failpoints")]
        if let Some(ms) = crate::failpoints::sleep_ms("parse_window_mid", &b3) {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        let parsed = tokio::task::spawn_blocking(move || {
            #[cfg(feature = "failpoints")]
            if crate::failpoints::hits("parse_crash", &b3) {
                panic!("failpoint parse_crash");
            }
            if blake3::hash(&bytes).to_hex().as_str() != b3 {
                return None;
            }
            Some(if md {
                parse::markdown::parse_markdown(&bytes)
            } else {
                parse::text::parse_text(&bytes)
            })
        })
        .await;
        let parsed = match parsed {
            Err(_) => return Err(Outcome::failed(FailureClass::Crash, "DOCS_PARSE_CRASH")),
            // The bytes are not the ones hashed: the file changed since.
            Ok(None) => {
                return Err(Outcome::failed(
                    FailureClass::Transient,
                    "DOCS_SOURCE_CHANGED_DURING_READ",
                ));
            }
            Ok(Some(r)) => r,
        };
        let n = match parsed {
            Ok(n) => n,
            Err(e) => return Err(self.parse_failed(ctx, s, &e).await),
        };
        let put = self
            .cas
            .put(CasKind::Ir, n.canonical.as_bytes())
            .map_err(|e| Outcome::failed(FailureClass::Transient, e.code()))?;
        let parser_id = serde_json::to_value(n.ir.parser.id)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(ParsedIr {
            ir_b3: put.b3,
            parser_id,
            parser_version: n.ir.parser.version.clone(),
            profile: n.ir.parser.profile_sha256.clone(),
            blocks: i32::try_from(n.ir.blocks.len()).unwrap_or(i32::MAX),
        })
    }

    /// The `file.progress` event and the `progress` column at the start of the
    /// parse of a text file: the unit is the line, the total the number of lines.
    /// Shown by `FileSummary.progress` until the next state change clears it.
    async fn report_start(&self, s: &Subject, bytes: &[u8]) {
        let total = bytes.iter().filter(|b| **b == b'\n').count() as i64 + 1;
        let p = Progress {
            file_id: s.file_id.clone(),
            drive_id: Some(s.drive_id.clone()),
            unit: "line",
            done: 0,
            total: Some(total),
        };
        let _ = sqlx::query(
            "UPDATE _oo_docs.file SET progress = $2::jsonb WHERE id = $1::uuid AND state = 'parsing'",
        )
        .bind(&s.file_id)
        .bind(p.column().to_string())
        .execute(&self.pool)
        .await;
        if let Some(bus) = &self.progress {
            bus.emit(p);
        }
    }

    /// A parse that cannot succeed: the file becomes `parse_failed` and the
    /// job finishes in the same commit.
    async fn parse_failed(&self, ctx: &JobCtx, s: &Subject, e: &ParseError) -> Outcome {
        let mut tx = match begin_stage(&self.pool).await {
            Ok(t) => t,
            Err(e) => return db_fail(e),
        };
        let r = async {
            match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
                .await
                .map_err(stage_err)?
                .status
            {
                JobStatus::Succeeded => {}
                _ => return Ok(false),
            }
            files::to_state(
                &mut tx,
                &s.file_id,
                FileState::Parsing,
                FileState::ParseFailed,
                Extra {
                    code: Some(e.code()),
                    retryable: e.retryable(),
                    version_id: Some(&s.version_id),
                    detail_json: None,
                },
            )
            .await
            .map_err(stage_err)?;
            Ok::<bool, String>(true)
        }
        .await;
        finish(tx, r).await
    }
}

impl<F: SourceFactory> JobHandler for ParseHandler<F> {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let mut s = match load_subject(&ctx).await {
                Ok(Some(s)) => s,
                Ok(None) => return Outcome::Done,
                Err(e) => return db_fail(e),
            };
            if !matches!(
                s.state,
                FileState::Queued | FileState::Parsing | FileState::Outdated
            ) {
                return Outcome::Done;
            }
            if ctx.should_stop() {
                return Outcome::Cancelled;
            }
            if matches!(s.state, FileState::Queued | FileState::Outdated)
                && let Err(e) = move_file(
                    &self.pool,
                    &s,
                    s.state,
                    FileState::Parsing,
                    None,
                    false,
                    ctx.job.attempt,
                )
                .await
            {
                return db_fail(e);
            }
            s.state = FileState::Parsing;
            let done = match self.parse(&ctx, &s).await {
                Ok(d) => d,
                Err(outcome) => return outcome,
            };
            let mut tx = match begin_stage(&self.pool).await {
                Ok(t) => t,
                Err(e) => return db_fail(e),
            };
            let r = async {
                match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
                    .await
                    .map_err(stage_err)?
                    .status
                {
                    JobStatus::Succeeded => {}
                    _ => return Ok(false),
                }
                sqlx::query(
                    "INSERT INTO _oo_docs.parse_result \
                       (b3, parser_id, parser_version, profile_sha256, status, ir_b3, block_count, finished_at) \
                     VALUES ($1, $2, $3, $4, 'complete', $5, $6, clock_timestamp()) ON CONFLICT DO NOTHING",
                )
                .bind(&s.b3)
                .bind(&done.parser_id)
                .bind(&done.parser_version)
                .bind(&done.profile)
                .bind(&done.ir_b3)
                .bind(done.blocks)
                .execute(&mut *tx)
                .await
                .map_err(stage_err)?;
                match state::transition_in_tx(
                    &mut tx,
                    &Transition {
                        file_id: &s.file_id,
                        expect_from: Some(FileState::Parsing),
                        to: FileState::Parsed,
                        code: None,
                        retryable: false,
                        version_id: Some(&s.version_id),
                        attempt: Some(ctx.job.attempt),
                        detail_json: None,
                    },
                )
                .await
                {
                    Ok(_) | Err(TransitionError::Conflict { .. } | TransitionError::NotFound) => {}
                    Err(e) => return Err(stage_err(e)),
                }
                Ok::<bool, String>(true)
            }
            .await;
            finish(tx, r).await
        })
    }
}

// -------------------------------------------------------- extract_rules

/// `extract_rules` handler: rules -> drafts -> `indexed`.
pub struct ExtractHandler {
    pool: PgPool,
    cas: Arc<Cas>,
}

impl ExtractHandler {
    async fn load_ir(&self, b3: &str) -> Result<Option<DocIr>, Outcome> {
        let ir_b3: Option<String> = sqlx::query_scalar(
            "SELECT ir_b3 FROM _oo_docs.parse_result \
              WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
              ORDER BY finished_at DESC NULLS LAST LIMIT 1",
        )
        .bind(b3)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_fail)?;
        let Some(ir_b3) = ir_b3 else { return Ok(None) };
        match self.cas.get(CasKind::Ir, &ir_b3) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| Outcome::failed(FailureClass::Permanent, "DOCS_PARSE_IR_INVALID")),
            Err(CasError::NotFound) => Ok(None),
            Err(e) => Err(Outcome::failed(FailureClass::Transient, e.code())),
        }
    }
}

impl JobHandler for ExtractHandler {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let s = match load_subject(&ctx).await {
                Ok(Some(s)) => s,
                Ok(None) => return Outcome::Done,
                Err(e) => return db_fail(e),
            };
            if !matches!(s.state, FileState::Parsed | FileState::Extracting) {
                return Outcome::Done;
            }
            if s.state == FileState::Parsed
                && let Err(e) = move_file(
                    &self.pool,
                    &s,
                    FileState::Parsed,
                    FileState::Extracting,
                    None,
                    false,
                    ctx.job.attempt,
                )
                .await
            {
                return db_fail(e);
            }
            #[cfg(feature = "failpoints")]
            if crate::failpoints::hits("extract_error", &s.b3) {
                return Outcome::failed(FailureClass::Permanent, "DOCS_EXTRACT_FAILPOINT");
            }
            let ir = match self.load_ir(&s.b3).await {
                Ok(Some(ir)) => ir,
                // The IR object is gone (CAS GC or corruption): parse again.
                Ok(None) => {
                    return Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_CAS_MISSING");
                }
                Err(o) => return o,
            };
            if ctx.should_stop() {
                return Outcome::Cancelled;
            }
            {
                let mut conn = match self.pool.acquire().await {
                    Ok(c) => c,
                    Err(e) => return db_fail(e),
                };
                let doc = DocumentInput {
                    version_id: &s.version_id,
                    b3: &s.b3,
                    path: &s.path,
                    ir: &ir,
                };
                if let Err(e) = index_document(&mut conn, &doc).await {
                    return db_fail(e);
                }
            }
            let mut tx = match begin_stage(&self.pool).await {
                Ok(t) => t,
                Err(e) => return db_fail(e),
            };
            let r = async {
                match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
                    .await
                    .map_err(stage_err)?
                    .status
                {
                    JobStatus::Succeeded => {}
                    _ => return Ok(false),
                }
                // Evidence of earlier versions of this file goes now that the
                // new version has asserted what still holds; facts left
                // without live evidence are retracted (4.3.5, D22).
                let old: Vec<String> = sqlx::query_scalar(
                    "SELECT id::text FROM _oo_docs.file_version \
                      WHERE file_id = $1::uuid AND id <> $2::uuid AND superseded_at IS NOT NULL \
                        AND EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                                     WHERE e.version_id = _oo_docs.file_version.id \
                                       AND e.withdrawn_at IS NULL) \
                      ORDER BY created_at, id",
                )
                .bind(&s.file_id)
                .bind(&s.version_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(stage_err)?;
                for v in &old {
                    files::withdraw_evidence(&mut tx, v, "file_superseded")
                        .await
                        .map_err(stage_err)?;
                }
                match state::transition_in_tx(
                    &mut tx,
                    &Transition {
                        file_id: &s.file_id,
                        expect_from: Some(FileState::Extracting),
                        to: FileState::Indexed,
                        code: None,
                        retryable: false,
                        version_id: Some(&s.version_id),
                        attempt: Some(ctx.job.attempt),
                        detail_json: None,
                    },
                )
                .await
                {
                    Ok(_) | Err(TransitionError::Conflict { .. } | TransitionError::NotFound) => {}
                    Err(e) => return Err(stage_err(e)),
                }
                Ok::<bool, String>(true)
            }
            .await;
            finish(tx, r).await
        })
    }
}

// -------------------------------------------------------------- publish

/// `publish` handler: one ontology version from the eligible drafts, then
/// every `indexed` file that has nothing left to publish becomes `published`.
pub struct PublishHandler {
    pool: PgPool,
    cas: Arc<Cas>,
}

impl PublishHandler {
    async fn promote(&self, n: Option<i64>) -> Result<u32, TransitionError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT f.id::text, fv.id::text FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
              WHERE f.state = 'indexed' AND f.deleted_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                                  JOIN _oo_docs.fact ft ON ft.id = e.target_id \
                                 WHERE e.target_kind = 'fact' AND e.version_id = fv.id \
                                   AND e.withdrawn_at IS NULL AND ft.status = 'draft') \
                AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                                  JOIN _oo_docs.alias a ON a.id = e.target_id \
                                 WHERE e.target_kind = 'alias' AND e.version_id = fv.id \
                                   AND e.withdrawn_at IS NULL AND a.status = 'draft') \
              ORDER BY 1 LIMIT 500",
        )
        .fetch_all(&self.pool)
        .await?;
        let detail = serde_json::json!({ "ontology_version": n }).to_string();
        let mut done = 0;
        for (file_id, version_id) in rows {
            match state::transition(
                &self.pool,
                &Transition {
                    file_id: &file_id,
                    expect_from: Some(FileState::Indexed),
                    to: FileState::Published,
                    code: None,
                    retryable: false,
                    version_id: Some(&version_id),
                    attempt: None,
                    detail_json: Some(&detail),
                },
            )
            .await
            {
                Ok(_) => done += 1,
                Err(TransitionError::Conflict { .. } | TransitionError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }
}

impl JobHandler for PublishHandler {
    fn run(&self, _ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let mut conn = match self.pool.acquire().await {
                Ok(c) => c,
                Err(e) => return db_fail(e),
            };
            let outcome = publish(&mut conn, &self.cas, &PublishConfig::default()).await;
            drop(conn);
            let n = match outcome {
                Ok(PublishOutcome::Published(v)) => {
                    crate::obs::emit(
                        "info",
                        "publish.version",
                        &crate::obs::Fields {
                            count: Some(("ontology_version", v.n)),
                            ..crate::obs::Fields::default()
                        },
                    );
                    Some(v.n)
                }
                Ok(PublishOutcome::NoChange { .. }) => None,
                Ok(PublishOutcome::Busy) => {
                    return Outcome::failed(FailureClass::Busy, "DOCS_ENGINE_BUSY");
                }
                Err(_) => return Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_INTERNAL"),
            };
            let n = match n {
                Some(n) => Some(n),
                None => sqlx::query_scalar("SELECT max(n) FROM _oo_docs.ontology_version")
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(None),
            };
            match self.promote(n).await {
                Ok(_) => Outcome::Done,
                Err(_) => Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_INTERNAL"),
            }
        })
    }
}

/// Handlers for `parse`, `extract_rules` and `publish`.
pub fn handlers<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    handlers_with_progress(pool, factory, cas, None)
}

/// [`handlers`] with the bus the API streams `file.progress` from.
pub fn handlers_with_progress<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
    progress: Option<Bus>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    let mut map: HashMap<JobKind, Arc<dyn JobHandler>> = HashMap::new();
    map.insert(
        JobKind::Parse,
        Arc::new(Contended(ParseHandler {
            pool: pool.clone(),
            factory,
            cas: cas.clone(),
            progress,
        })),
    );
    map.insert(
        JobKind::ExtractRules,
        Arc::new(Contended(ExtractHandler {
            pool: pool.clone(),
            cas: cas.clone(),
        })),
    );
    map.insert(
        JobKind::Publish,
        Arc::new(Contended(PublishHandler {
            pool: pool.clone(),
            cas,
        })),
    );
    map
}

/// Concurrency of the stage kinds: publish is a single writer (4.9).
pub fn default_concurrency() -> HashMap<JobKind, usize> {
    HashMap::from([
        (JobKind::Parse, 4),
        (JobKind::ExtractRules, 2),
        (JobKind::Publish, 1),
    ])
}

// ----------------------------------------------------------------- pump

/// Enqueue a stage job. A key whose job ended cancelled (a newer fingerprint
/// or a hash of the same content passed by) is replaced by `#r<n>`, up to
/// four times, so a cancelled job never strands a file.
async fn enqueue_stage(
    conn: &mut sqlx::PgConnection,
    kind: JobKind,
    key: &str,
    drive_id: &str,
    file_id: &str,
    version_id: &str,
    priority: i16,
) -> Result<bool, crate::store::StoreError> {
    let mut job = NewJob::new(kind, key);
    job.drive_id = Some(drive_id);
    job.file_id = Some(file_id);
    job.version_id = Some(version_id);
    job.priority = priority;
    let first = store::enqueue(conn, &job).await?.status;
    if first == JobStatus::Enqueued {
        return Ok(true);
    }
    // `AlreadyDone` with the file still waiting for this stage means the file
    // was sent back through it (reprocessing after an upgrade, manual retry):
    // the finished job is history, a new one is due.
    if !matches!(first, JobStatus::Terminal | JobStatus::AlreadyDone) {
        return Ok(false);
    }
    // Only cancellations are capped (a cancel loop must not spin); a job
    // that failed and was sent back by a retry or an upgrade is not.
    let (used, cancelled): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE state = 'cancelled') FROM _oo_docs.job \
          WHERE dedupe_key = $1 OR dedupe_key LIKE $1 || '#r%'",
    )
    .bind(key)
    .fetch_one(&mut *conn)
    .await?;
    if cancelled > 4 {
        return Ok(false);
    }
    let retry_key = format!("{key}#r{used}");
    let mut job = NewJob::new(kind, &retry_key);
    job.drive_id = Some(drive_id);
    job.file_id = Some(file_id);
    job.version_id = Some(version_id);
    job.priority = priority;
    Ok(store::enqueue(conn, &job).await?.status == JobStatus::Enqueued)
}

/// One pump pass: enqueue what is due. Returns the number of jobs enqueued.
pub async fn pump_once(pool: &PgPool) -> Result<u32, crate::store::StoreError> {
    pump_once_with(pool, false).await
}

/// [`pump_once`]; `paused` (disk low) enqueues no parse or extract job.
pub async fn pump_once_with(pool: &PgPool, paused: bool) -> Result<u32, crate::store::StoreError> {
    let mut conn = pool.acquire().await?;
    let mut enqueued = 0;
    if paused {
        return publish_due(&mut conn).await;
    }

    // parsing -> queued: a failed attempt that will be retried puts the file
    // back in the queue with the reason and `retryable` (4.2).
    requeue_retrying(pool).await;

    // queued / parsing / outdated -> parse. A blob whose parse was
    // quarantined fails every path that carries it, until a parse of that
    // content has succeeded (4.1.6).
    let rows: Vec<(String, String, String, String, bool, bool, String)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text, f.drive_id::text, fv.b3, \
                EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'parse' AND j.file_id = f.id \
                          AND j.version_id = f.current_version_id \
                          AND j.state IN ('failed', 'quarantined') AND j.updated_at >= f.state_since), \
                (EXISTS (SELECT 1 FROM _oo_docs.job j \
                           JOIN _oo_docs.file_version qv ON qv.id = j.version_id \
                          WHERE j.kind = 'parse' AND j.state = 'quarantined' AND qv.b3 = fv.b3) \
                 AND NOT EXISTS (SELECT 1 FROM _oo_docs.parse_result p \
                                  WHERE p.b3 = fv.b3 AND p.status = 'complete' \
                                    AND p.ir_b3 IS NOT NULL)), \
                f.state \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state IN ('queued', 'parsing', 'outdated') AND f.deleted_at IS NULL \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'parse' AND j.file_id = f.id \
                              AND j.version_id = f.current_version_id AND j.state IN ('queued', 'running')) \
          ORDER BY f.state_since, f.id LIMIT $1",
    )
    .bind(PUMP_BATCH)
    .fetch_all(&mut *conn)
    .await?;
    for (file_id, version_id, drive_id, b3, failed, inherited, state) in rows {
        if failed {
            fail_file(pool, &file_id, &version_id, FileState::ParseFailed, None).await;
            continue;
        }
        if inherited {
            let code = Some("DOCS_PARSE_QUARANTINED");
            fail_file(pool, &file_id, &version_id, FileState::ParseFailed, code).await;
            continue;
        }
        let key = format!("parse:{b3}:local@{LOCAL_PARSER_VERSION}:v{version_id}");
        let priority = if state == "outdated" {
            PRIORITY_OUTDATED
        } else {
            PRIORITY_NEW
        };
        if enqueue_stage(
            &mut conn,
            JobKind::Parse,
            &key,
            &drive_id,
            &file_id,
            &version_id,
            priority,
        )
        .await?
        {
            enqueued += 1;
        }
    }

    // parsed / extracting -> extract_rules
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text, f.drive_id::text, pr.ir_b3, \
                EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_rules' AND j.file_id = f.id \
                          AND j.version_id = f.current_version_id \
                          AND j.state IN ('failed', 'quarantined') AND j.updated_at >= f.state_since) \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
           JOIN LATERAL (SELECT ir_b3 FROM _oo_docs.parse_result p \
                          WHERE p.b3 = fv.b3 AND p.status = 'complete' AND p.ir_b3 IS NOT NULL \
                          ORDER BY p.finished_at DESC NULLS LAST LIMIT 1) pr ON true \
          WHERE f.state IN ('parsed', 'extracting') AND f.deleted_at IS NULL \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_rules' AND j.file_id = f.id \
                              AND j.version_id = f.current_version_id AND j.state IN ('queued', 'running')) \
          ORDER BY f.state_since, f.id LIMIT $1",
    )
    .bind(PUMP_BATCH)
    .fetch_all(&mut *conn)
    .await?;
    for (file_id, version_id, drive_id, ir_b3, failed) in rows {
        if failed {
            fail_file(pool, &file_id, &version_id, FileState::ExtractPartial, None).await;
            continue;
        }
        let key = format!("rules:{version_id}:{ir_b3}:{RULESET_VERSION}");
        if enqueue_stage(
            &mut conn,
            JobKind::ExtractRules,
            &key,
            &drive_id,
            &file_id,
            &version_id,
            PRIORITY_NEW,
        )
        .await?
        {
            enqueued += 1;
        }
    }

    enqueued += publish_due(&mut conn).await?;
    Ok(enqueued)
}

/// A stage whose job failed for good: move the file to its failure state,
/// through the intermediate state the table requires (`queued -> parsing ->
/// parse_failed`, `parsed -> extracting -> extract_partial`).
async fn fail_file(
    pool: &PgPool,
    file_id: &str,
    version_id: &str,
    to: FileState,
    forced_code: Option<&str>,
) {
    let last: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT last_error, state FROM _oo_docs.job \
          WHERE file_id = $1::uuid AND version_id = $2::uuid AND kind IN ('parse', 'extract_rules') \
            AND state IN ('failed', 'quarantined') ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(file_id)
    .bind(version_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    // A quarantined blob (poison pill) reads the same wherever it shows up;
    // only a manual retry takes it out of `parse_failed`.
    let code = match last {
        _ if forced_code.is_some() => forced_code.unwrap_or_default().to_owned(),
        Some((_, state)) if state == "quarantined" => "DOCS_PARSE_QUARANTINED".to_owned(),
        Some((Some(code), _)) => code,
        _ => "DOCS_ENGINE_INTERNAL".to_owned(),
    };
    let current: Option<String> =
        sqlx::query_scalar("SELECT state FROM _oo_docs.file WHERE id = $1::uuid")
            .bind(file_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let mut from = current.as_deref().and_then(FileState::parse);
    let via = match (from, to) {
        (Some(FileState::Queued | FileState::Outdated), FileState::ParseFailed) => {
            Some(FileState::Parsing)
        }
        (Some(FileState::Parsed), FileState::ExtractPartial) => Some(FileState::Extracting),
        _ => None,
    };
    if let Some(mid) = via {
        let hop = Transition {
            file_id,
            expect_from: from,
            to: mid,
            code: None,
            retryable: false,
            version_id: Some(version_id),
            attempt: None,
            detail_json: None,
        };
        if state::transition(pool, &hop).await.is_err() {
            return;
        }
        from = Some(mid);
    }
    let _ = state::transition(
        pool,
        &Transition {
            file_id,
            expect_from: from,
            to,
            code: Some(&code),
            retryable: true,
            version_id: Some(version_id),
            attempt: None,
            detail_json: None,
        },
    )
    .await;
}

/// indexed files, duplicates and unpublished retractions -> publish (debounced).
async fn publish_due(conn: &mut sqlx::PgConnection) -> Result<u32, crate::store::StoreError> {
    let mut enqueued = 0;
    // indexed files and unpublished retractions -> publish (debounced)
    let due: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.file f WHERE f.state = 'indexed' AND f.deleted_at IS NULL \
                         AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'publish' \
                                          AND j.created_at >= f.state_since \
                                          AND j.state IN ('queued', 'running', 'succeeded'))) \
             OR EXISTS (SELECT 1 FROM _oo_docs.file f JOIN _oo_docs.file_version fv \
                          ON fv.id = f.current_version_id \
                         WHERE f.state = 'duplicate' AND f.deleted_at IS NULL \
                           AND fv.created_at > COALESCE((SELECT max(published_at) \
                                 FROM _oo_docs.ontology_version), '-infinity'::timestamptz) \
                           AND EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                                        WHERE e.version_id = fv.id AND e.withdrawn_at IS NULL) \
                           AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'publish' \
                                            AND j.created_at >= fv.created_at \
                                            AND j.state IN ('queued', 'running', 'succeeded'))) \
             OR EXISTS (SELECT 1 FROM _oo_docs.fact ft WHERE ft.status = 'retracted' \
                         AND ft.retracted_in IS NULL AND ft.published_in IS NOT NULL \
                         AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'publish' \
                                          AND j.created_at >= ft.retracted_at \
                                          AND j.state IN ('queued', 'running', 'succeeded')))",
    )
    .fetch_one(&mut *conn)
    .await?;
    if due {
        let now_ms: i64 =
            sqlx::query_scalar("SELECT (extract(epoch FROM clock_timestamp()) * 1000)::bigint")
                .fetch_one(&mut *conn)
                .await?;
        let start = now_ms - now_ms.rem_euclid(PUBLISH_WINDOW_MS);
        let key = format!("publish:{start}");
        let mut job = NewJob::new(JobKind::Publish, &key);
        job.run_after_ms = (start + PUBLISH_WINDOW_MS - now_ms).max(0);
        if store::enqueue(&mut *conn, &job).await?.status == JobStatus::Enqueued {
            enqueued += 1;
        }
    }
    Ok(enqueued)
}

/// `parsing -> queued` for files whose parse job failed an attempt and waits
/// for its backoff (job `queued` again with an error and `attempt > 0`): the
/// state shows "waiting to retry" with the code, `retryable = true` (4.2).
async fn requeue_retrying(pool: &PgPool) {
    let rows: Vec<(String, String, String, i32)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text, j.last_error, j.attempt \
           FROM _oo_docs.file f \
           JOIN _oo_docs.job j ON j.kind = 'parse' AND j.file_id = f.id \
                              AND j.version_id = f.current_version_id \
          WHERE f.state = 'parsing' AND f.deleted_at IS NULL \
            AND j.state = 'queued' AND j.attempt > 0 AND j.last_error IS NOT NULL \
          LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (file_id, version_id, code, attempt) in rows {
        let _ = state::transition(
            pool,
            &Transition {
                file_id: &file_id,
                expect_from: Some(FileState::Parsing),
                to: FileState::Queued,
                code: Some(&code),
                retryable: true,
                version_id: Some(&version_id),
                attempt: Some(attempt),
                detail_json: None,
            },
        )
        .await;
    }
}

/// Files whose result was made by an older parser or ruleset become
/// `outdated`; the pump then reprocesses them at low priority and the old
/// facts stay valid meanwhile (4.2, D23). Returns how many were marked.
pub async fn mark_outdated(pool: &PgPool) -> Result<u32, crate::store::StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state IN ('published', 'extract_partial', 'parse_failed') AND f.deleted_at IS NULL \
            AND fv.duplicate_of IS NULL \
            AND EXISTS (SELECT 1 FROM _oo_docs.parse_result p WHERE p.b3 = fv.b3 \
                         AND p.status = 'complete' AND p.parser_id IN ('oo-md', 'oo-text')) \
            AND (NOT EXISTS (SELECT 1 FROM _oo_docs.parse_result p WHERE p.b3 = fv.b3 \
                              AND p.status = 'complete' AND p.parser_id IN ('oo-md', 'oo-text') \
                              AND p.parser_version = $1) \
                 OR NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_rules' \
                                 AND j.version_id = fv.id AND j.state = 'succeeded' \
                                 AND j.dedupe_key LIKE '%:' || $2 || '%')) \
          ORDER BY f.id LIMIT 500",
    )
    .bind(LOCAL_PARSER_VERSION)
    .bind(RULESET_VERSION)
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for (file_id, version_id) in rows {
        let done = state::transition(
            pool,
            &Transition {
                file_id: &file_id,
                expect_from: None,
                to: FileState::Outdated,
                code: None,
                retryable: false,
                version_id: Some(&version_id),
                attempt: None,
                detail_json: None,
            },
        )
        .await;
        match done {
            Ok(_) => n += 1,
            Err(TransitionError::Conflict { .. } | TransitionError::NotFound) => {}
            Err(TransitionError::Illegal { .. }) => {}
            Err(TransitionError::Store(e)) => return Err(e),
            Err(TransitionError::UnknownState(_)) => {}
        }
    }
    Ok(n)
}

type RetryRow = (String, Option<String>, Option<String>, Option<String>, bool);

/// What `retry_file` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryResult {
    /// The file is queued again.
    Queued,
    /// The file is not in a state a retry applies to.
    NotRetryable(String),
    /// No such file.
    NotFound,
}

/// Manual retry (4.2): `parse_failed` and `extract_partial` go back to
/// `queued` with the attempt counter cleared (a new job; the old failed or
/// quarantined one stays as history) at the manual priority 50. `safe`
/// asks for the degraded parser where the format has one; md and txt have
/// none, so it behaves like `normal`.
pub async fn retry_file(
    pool: &PgPool,
    file_id: &str,
    _safe: bool,
) -> Result<RetryResult, crate::store::StoreError> {
    let mut tx = pool.begin().await?;
    let row: Option<RetryRow> = sqlx::query_as(
        "SELECT f.state, f.current_version_id::text, f.drive_id::text, \
                (SELECT fv.b3 FROM _oo_docs.file_version fv WHERE fv.id = f.current_version_id), \
                f.deleted_at IS NOT NULL \
           FROM _oo_docs.file f WHERE f.id = $1::uuid FOR UPDATE",
    )
    .bind(file_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((state_text, version_id, drive_id, b3, deleted)) = row else {
        return Ok(RetryResult::NotFound);
    };
    let b3 = b3.unwrap_or_default();
    let (Some(version_id), Some(drive_id)) = (version_id, drive_id) else {
        return Ok(RetryResult::NotRetryable(state_text));
    };
    if deleted || !matches!(state_text.as_str(), "parse_failed" | "extract_partial") {
        return Ok(RetryResult::NotRetryable(state_text));
    }
    state::transition_in_tx(
        &mut tx,
        &Transition {
            file_id,
            expect_from: FileState::parse(&state_text),
            to: FileState::Queued,
            code: None,
            retryable: false,
            version_id: Some(&version_id),
            attempt: None,
            detail_json: Some("{\"manual\":true}"),
        },
    )
    .await
    .map_err(|e| match e {
        TransitionError::Store(s) => s,
        other => crate::store::StoreError::Protocol(other.to_string()),
    })?;
    let stamp: i64 =
        sqlx::query_scalar("SELECT (extract(epoch FROM clock_timestamp()) * 1000000)::bigint")
            .fetch_one(&mut *tx)
            .await?;
    let key = format!("parse:{b3}:local@{LOCAL_PARSER_VERSION}:v{version_id}#m{stamp}");
    let mut job = NewJob::new(JobKind::Parse, &key);
    job.drive_id = Some(&drive_id);
    job.file_id = Some(file_id);
    job.version_id = Some(&version_id);
    job.priority = 50;
    store::enqueue(&mut tx, &job).await?;
    tx.commit().await?;
    Ok(RetryResult::Queued)
}

/// Run the pump until `stop` flips. `wake` is poked after every pass that
/// enqueued something, so a scheduler sharing it claims at once.
pub fn spawn_pump(pool: PgPool, wake: Arc<Notify>, stop: watch::Receiver<bool>) -> JoinHandle<()> {
    spawn_pump_guarded(pool, wake, stop, None)
}

/// [`spawn_pump`] with the disk guard: while the disk is low no parse or
/// extract job is enqueued and waiting files say `blocked_reason = disk_low`.
pub fn spawn_pump_guarded(
    pool: PgPool,
    wake: Arc<Notify>,
    mut stop: watch::Receiver<bool>,
    guard: Option<Arc<crate::obs::disk::Guard>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut was_low = false;
        loop {
            if *stop.borrow() {
                return;
            }
            let low = guard.as_ref().is_some_and(|g| g.is_low());
            if guard.is_some() && (low || was_low) {
                // New files keep arriving while low; on the way back, clear.
                let _ = crate::obs::disk::sync_blocked(&pool, low).await;
            }
            was_low = low;
            if let Ok(n) = pump_once_with(&pool, low).await
                && n > 0
            {
                wake.notify_waiters();
            }
            tokio::select! {
                _ = tokio::time::sleep(PUMP_TICK) => {}
                _ = stop.changed() => {}
            }
        }
    })
}
