//! The pipeline stages after ingest: parse (md, txt, csv, xlsx locally; pdf,
//! docx, pptx through the sidecar), extract_rules and publish, plus the pump that feeds them (10 §4.1, §4.2, §4.9).
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
//! Routing (L3): the format the hash stage sniffed decides. md, txt, csv and
//! xlsx are parsed in the engine process on a blocking thread (a parser panic
//! is classified as a crash and retried); pdf, docx and pptx go to the Docling
//! sidecar with windows and progress. Content that is already waiting or in
//! flight under another path is not parsed again: the later paths wait in
//! `stored` and become `duplicate` (see [`settle_waiting`]).
//!
//! Simplifications:
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
use crate::extract::llm::PROMPT_VERSION;
use crate::extract::llm::config::ModelRuntime;
use crate::extract::llm::run::{Limits, LlmInput, extract_llm};
use crate::extract::rules::RULESET_VERSION;
use crate::ingest::HashableSource as _;
use crate::ingest::files::{self, Extra};
use crate::ingest::{DriveRef, SourceFactory};
use crate::parse::media::{MediaKind, MediaOptions};
use crate::parse::sidecar::{self, SidecarClient, SidecarError};
use crate::parse::{self, LOCAL_PARSER_VERSION};
use crate::publish::run::{PublishConfig, PublishOutcome, publish};
use crate::publish::write::{DocumentInput, index_extraction, index_links};
use crate::state::{self, FileState, Transition, TransitionError};
use crate::store::jobs as store;
use oo_connectors::objectstore::{DocumentSource, ReadExpect};
use oo_core::docir::{DocFormat, DocIr};

/// Reprocess jobs (`outdated` files) in flight at once: a parser or prompt upgrade never floods the pool (D23).
const REPROCESS_INFLIGHT: i64 = 1;

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
    sidecar: Option<Arc<SidecarClient>>,
    media: Option<MediaRuntime>,
}

/// The `media-parser` client and its `oo.docs/v1` `media.*` settings (10 17.1, 17.2).
#[derive(Clone)]
pub struct MediaRuntime {
    /// Client of the media sidecar.
    pub client: Arc<SidecarClient>,
    /// `media.external_ffmpeg` (Linux, D65).
    pub external_ffmpeg: Option<String>,
}

/// Audio or video by sniffed format.
fn media_kind(format: &str) -> Option<MediaKind> {
    match format {
        "audio" => Some(MediaKind::Audio),
        "video" => Some(MediaKind::Video),
        _ => None,
    }
}

/// The job kind a parse of this sniffed format is queued as (D77: media has its own pool).
pub fn parse_kind_of(format: &str) -> JobKind {
    if media_kind(format).is_some() {
        JobKind::ParseMedia
    } else {
        JobKind::Parse
    }
}

/// Why a parse attempt stopped without an IR.
enum Halt {
    /// The scheduler decides (retry, cancel, done).
    Out(Outcome),
    /// The file becomes `parse_failed` with this code.
    Failed { code: String, retryable: bool },
    /// The file goes back to `queued` and on to `to` (`unsupported` or
    /// `encrypted`), the route the table allows for a verdict found late.
    Park { to: FileState, code: &'static str },
    /// The sidecar is not reachable: the file waits in `queued`.
    Waiting,
}

impl From<Outcome> for Halt {
    fn from(o: Outcome) -> Self {
        Halt::Out(o)
    }
}

/// `blocked_reason` of a file that waits for the sidecar.
pub const SIDECAR_BLOCKED: &str = "parser_unavailable";

/// Formats the sidecar parses.
fn sidecar_format(format: &str) -> Option<DocFormat> {
    match format {
        "pdf" => Some(DocFormat::Pdf),
        "docx" => Some(DocFormat::Docx),
        "pptx" => Some(DocFormat::Pptx),
        // Standalone images go to the ocr-parser through the same client (10 17.6).
        "image" => Some(DocFormat::Image),
        _ => None,
    }
}

struct ParsedIr {
    ir_b3: String,
    parser_id: String,
    parser_version: String,
    profile: String,
    blocks: i32,
}

/// Versions the sidecars reported at start (`/v1/info`): a stored result of that parser with another version is
/// not reused, so an upgraded parser really parses again (D23). A parser not listed is reused whatever its version.
static CURRENT_PARSERS: std::sync::RwLock<Vec<(String, String)>> =
    std::sync::RwLock::new(Vec::new());

/// Record the version a sidecar parser reports now.
pub fn set_current_parser(id: &str, version: &str) {
    let mut g = CURRENT_PARSERS.write().unwrap_or_else(|p| p.into_inner());
    g.retain(|(i, _)| i != id);
    g.push((id.to_owned(), version.to_owned()));
}

/// The `(id, version)` of the parser a `/v1/info` answer describes.
pub fn parser_of_info(info: &serde_json::Value) -> Option<(String, String)> {
    // `oo.doc-parser/v1` `/v1/info` has `parser_id` and `parser_version` at the top; a parse result header nests them.
    if let (Some(id), Some(version)) = (
        info.get("parser_id").and_then(serde_json::Value::as_str),
        info.get("parser_version")
            .and_then(serde_json::Value::as_str),
    ) {
        return Some((id.to_owned(), version.to_owned()));
    }
    let p = info.get("parser")?;
    Some((
        p.get("id")?.as_str()?.to_owned(),
        p.get("version")?.as_str()?.to_owned(),
    ))
}

fn current_parsers() -> (Vec<String>, Vec<String>) {
    let g = CURRENT_PARSERS.read().unwrap_or_else(|p| p.into_inner());
    g.iter().cloned().unzip()
}

async fn cached_parse(pool: &PgPool, b3: &str) -> Result<Option<ParsedIr>, sqlx::Error> {
    let (ids, versions) = current_parsers();
    let row: Option<(String, String, String, String, Option<i32>)> = sqlx::query_as(
        "SELECT ir_b3, parser_id, parser_version, profile_sha256, block_count \
           FROM _oo_docs.parse_result p \
          WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
            AND ((parser_id IN ('oo-md', 'oo-text', 'oo-csv', 'oo-xlsx') AND parser_version = $2) \
                 OR (parser_id IN ('docling', 'paddleocr', 'media') \
                     AND (EXISTS (SELECT 1 FROM unnest($3::text[], $4::text[]) c(i, v) \
                                   WHERE c.i = p.parser_id AND c.v = p.parser_version) \
                          OR NOT EXISTS (SELECT 1 FROM unnest($3::text[]) c(i) WHERE c.i = p.parser_id)))) \
          ORDER BY finished_at DESC NULLS LAST LIMIT 1",
    )
    .bind(b3)
    .bind(LOCAL_PARSER_VERSION)
    .bind(&ids)
    .bind(&versions)
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

    async fn parse(&self, ctx: &JobCtx, s: &Subject) -> Result<ParsedIr, Halt> {
        #[cfg(feature = "failpoints")]
        if crate::failpoints::hits("parse_error", &s.b3) {
            return Err(Halt::Out(Outcome::failed(
                FailureClass::Transient,
                "DOCS_PARSE_TIMEOUT",
            )));
        }
        if let Some(hit) = cached_parse(&self.pool, &s.b3).await.map_err(db_fail)? {
            return Ok(hit);
        }
        let side = sidecar_format(&s.format);
        let media = media_kind(&s.format);
        if let Some(_kind) = media
            && let Err(limit) =
                crate::jobs::media_pool::check_size(u64::try_from(s.size).unwrap_or(0))
        {
            return Err(Halt::Park {
                to: FileState::TooLarge,
                code: limit.code(),
            });
        }
        if side.is_none()
            && media.is_none()
            && !matches!(s.format.as_str(), "md" | "txt" | "csv" | "xlsx")
        {
            return Err(Halt::Park {
                to: FileState::Unsupported,
                code: "DOCS_PARSE_UNSUPPORTED_FORMAT",
            });
        }
        let source = self.open_source(s).await?;
        let len = u64::try_from(s.size).unwrap_or(0);
        // Remote originals come through the CAS: streamed in ranges with
        // `If-Match`, verified against the hash of this version, and kept for
        // the LRU (10 4.3.3). Local files are read in place.
        let read = match source.any_source().filter(|a| a.is_remote()) {
            Some(any) => {
                match crate::ingest::download::fetch_original(
                    any,
                    &self.cas,
                    &s.raw_key,
                    Some(&s.b3),
                )
                .await
                {
                    Ok(f) => self.cas.get(CasKind::Orig, &f.put.b3).map_err(|_| {
                        oo_connectors::objectstore::DocSourceError::Io(
                            "cached original is unreadable",
                        )
                    }),
                    Err(e) => Err(e),
                }
            }
            None => {
                source
                    .read(&s.raw_key, 0, len, &ReadExpect::default())
                    .await
            }
        };
        let bytes = match read {
            Ok(b) => b,
            // The file went away or moved: a later hash makes a new version.
            Err(
                oo_connectors::objectstore::DocSourceError::NotFound
                | oo_connectors::objectstore::DocSourceError::Changed,
            ) => {
                return Err(Outcome::Done.into());
            }
            Err(e) => {
                let class = match e {
                    oo_connectors::objectstore::DocSourceError::PermissionDenied
                    | oo_connectors::objectstore::DocSourceError::AuthFailed => {
                        FailureClass::Permanent
                    }
                    _ => FailureClass::Transient,
                };
                return Err(Outcome::failed(class, e.code()).into());
            }
        };
        if ctx.should_stop() {
            return Err(Outcome::Cancelled.into());
        }
        #[cfg(feature = "failpoints")]
        if let Some(ms) = crate::failpoints::sleep_ms("parse_window_mid", &s.b3) {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        let normalized = match (side, media) {
            (Some(doc_format), _) => self.parse_sidecar(s, bytes, doc_format).await?,
            (None, Some(kind)) => self.parse_media(ctx, s, bytes, kind).await?,
            (None, None) => self.parse_local(ctx, s, bytes).await?,
        };
        let n = normalized;
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

    /// md, txt, csv and xlsx: in this process, on a blocking thread.
    async fn parse_local(
        &self,
        _ctx: &JobCtx,
        s: &Subject,
        bytes: Vec<u8>,
    ) -> Result<crate::normalize::Normalized, Halt> {
        self.report(s, "line", 0, Some(line_count(&bytes))).await;
        let b3 = s.b3.clone();
        let format = s.format.clone();
        let parsed = tokio::task::spawn_blocking(move || {
            #[cfg(feature = "failpoints")]
            if crate::failpoints::hits("parse_crash", &b3) {
                panic!("failpoint parse_crash");
            }
            if blake3::hash(&bytes).to_hex().as_str() != b3 {
                return None;
            }
            Some(match format.as_str() {
                "md" => parse::markdown::parse_markdown(&bytes),
                "csv" => parse::csv::parse_csv(&bytes),
                "xlsx" => parse::xlsx::parse_xlsx(&bytes),
                _ => parse::text::parse_text(&bytes),
            })
        })
        .await;
        match parsed {
            Err(_) => Err(Outcome::failed(FailureClass::Crash, "DOCS_PARSE_CRASH").into()),
            // The bytes are not the ones hashed: the file changed since.
            Ok(None) => Err(Outcome::failed(
                FailureClass::Transient,
                "DOCS_SOURCE_CHANGED_DURING_READ",
            )
            .into()),
            Ok(Some(Ok(n))) => Ok(n),
            Ok(Some(Err(e))) => Err(Halt::Failed {
                code: e.code().to_owned(),
                retryable: e.retryable(),
            }),
        }
    }

    /// pdf, docx and pptx: through the sidecar, windows reported as progress.
    async fn parse_sidecar(
        &self,
        s: &Subject,
        bytes: Vec<u8>,
        format: DocFormat,
    ) -> Result<crate::normalize::Normalized, Halt> {
        let Some(client) = self.sidecar.clone() else {
            // The format was admitted but no sidecar is configured.
            return Err(Halt::Park {
                to: FileState::Unsupported,
                code: "DOCS_PARSE_UNSUPPORTED_FORMAT",
            });
        };
        if blake3::hash(&bytes).to_hex().as_str() != s.b3 {
            return Err(Outcome::failed(
                FailureClass::Transient,
                "DOCS_SOURCE_CHANGED_DURING_READ",
            )
            .into());
        }
        let handle = tokio::runtime::Handle::current();
        let pool = self.pool.clone();
        let bus = self.progress.clone();
        let (file_id, drive_id) = (s.file_id.clone(), s.drive_id.clone());
        let done = tokio::task::spawn_blocking(move || {
            #[cfg(feature = "failpoints")]
            if crate::failpoints::hits("parse_crash", &blake3::hash(&bytes).to_hex()) {
                panic!("failpoint parse_crash");
            }
            client.parse_bytes(&bytes, format, &mut |p: sidecar::Progress| {
                if p.page_count > 0 {
                    handle.block_on(report_progress(
                        &pool,
                        bus.as_ref(),
                        &file_id,
                        &drive_id,
                        "page",
                        i64::from(p.pages_done),
                        Some(i64::from(p.page_count)),
                    ));
                }
            })
        })
        .await;
        match done {
            Err(_) => Err(Outcome::failed(FailureClass::Crash, "DOCS_PARSE_CRASH").into()),
            Ok(Ok(n)) => Ok(n),
            Ok(Err(e)) => Err(sidecar_halt(&e)),
        }
    }

    /// The client a format is parsed through.
    fn client_for(&self, format: &str) -> Option<Arc<SidecarClient>> {
        if media_kind(format).is_some() {
            self.media.as_ref().map(|m| m.client.clone())
        } else {
            self.sidecar.clone()
        }
    }

    /// Audio and video: through the media-parser as a long task, progress in milliseconds of the source
    /// timeline, cancelled with the job (10 17.1, 17.3).
    async fn parse_media(
        &self,
        ctx: &JobCtx,
        s: &Subject,
        bytes: Vec<u8>,
        kind: MediaKind,
    ) -> Result<crate::normalize::Normalized, Halt> {
        let Some(rt) = self.media.clone() else {
            return Err(Halt::Park {
                to: FileState::Unsupported,
                code: "DOCS_PARSE_UNSUPPORTED_FORMAT",
            });
        };
        if blake3::hash(&bytes).to_hex().as_str() != s.b3 {
            return Err(Outcome::failed(
                FailureClass::Transient,
                "DOCS_SOURCE_CHANGED_DURING_READ",
            )
            .into());
        }
        let opts = MediaOptions {
            hotwords: self.hotwords().await,
            external_ffmpeg: rt.external_ffmpeg.clone(),
            host_decode_dir: None,
            num_speakers: None,
            media_asset: crate::extract::rules::media_class::is_asset_name(&s.path),
        };
        let handle = tokio::runtime::Handle::current();
        let pool = self.pool.clone();
        let bus = self.progress.clone();
        let (file_id, drive_id) = (s.file_id.clone(), s.drive_id.clone());
        let stop = ctx.cancel_flag_arc();
        let done = tokio::task::spawn_blocking(move || {
            rt.client.parse_media(
                &bytes,
                kind,
                &opts,
                Duration::from_secs(2),
                &mut |p: crate::parse::media::MediaProgress| {
                    if p.total_ms > 0 {
                        handle.block_on(report_progress(
                            &pool,
                            bus.as_ref(),
                            &file_id,
                            &drive_id,
                            "ms",
                            i64::try_from(p.done_ms).unwrap_or(i64::MAX),
                            i64::try_from(p.total_ms).ok(),
                        ));
                    }
                    !stop.load(std::sync::atomic::Ordering::Relaxed)
                },
            )
        })
        .await;
        match done {
            Err(_) => Err(Outcome::failed(FailureClass::Crash, "DOCS_PARSE_CRASH").into()),
            Ok(Ok(n)) => Ok(n),
            Ok(Err(e)) => Err(sidecar_halt(&e)),
        }
    }

    /// Entity names and aliases of this Drive that are already published: they bias recognition only and are
    /// never written to a Pack (17.3).
    async fn hotwords(&self) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT DISTINCT n FROM (SELECT e.canonical_name AS n FROM _oo_docs.entity e \
                                       WHERE e.status = 'active' AND e.type <> 'Document' \
                                     UNION ALL SELECT a.alias FROM _oo_docs.alias a \
                                       WHERE a.status = 'published') x \
              WHERE char_length(n) BETWEEN 2 AND 32 ORDER BY n LIMIT 200",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    /// The `file.progress` event and the `progress` column: the unit is the
    /// line for text, the page for a PDF. Shown by `FileSummary.progress`
    /// until the next state change clears it.
    async fn report(&self, s: &Subject, unit: &'static str, done: i64, total: Option<i64>) {
        report_progress(
            &self.pool,
            self.progress.as_ref(),
            &s.file_id,
            &s.drive_id,
            unit,
            done,
            total,
        )
        .await;
    }

    /// A parse that cannot succeed: the file becomes `parse_failed` (or, for a
    /// verdict found late, goes back to `queued` and on to `to`) and the job
    /// finishes in the same commit.
    async fn halt(&self, ctx: &JobCtx, s: &Subject, h: Halt) -> Outcome {
        let (to, code, retryable) = match h {
            Halt::Out(o) => return o,
            Halt::Waiting => return self.wait_for_sidecar(ctx, s).await,
            Halt::Failed { code, retryable } => (FileState::ParseFailed, code, retryable),
            Halt::Park { to, code } => (to, code.to_owned(), false),
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
            if to != FileState::ParseFailed {
                files::to_state(
                    &mut tx,
                    &s.file_id,
                    FileState::Parsing,
                    FileState::Queued,
                    Extra {
                        code: Some(&code),
                        version_id: Some(&s.version_id),
                        ..Extra::default()
                    },
                )
                .await
                .map_err(stage_err)?;
            }
            files::to_state(
                &mut tx,
                &s.file_id,
                if to == FileState::ParseFailed {
                    FileState::Parsing
                } else {
                    FileState::Queued
                },
                to,
                Extra {
                    code: Some(&code),
                    retryable,
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

    /// The sidecar cannot be reached: the file waits in `queued` with
    /// `blocked_reason = parser_unavailable`, the job is retried without
    /// using an attempt (10 4.4.4).
    async fn wait_for_sidecar(&self, _ctx: &JobCtx, s: &Subject) -> Outcome {
        let back = state::transition(
            &self.pool,
            &Transition {
                file_id: &s.file_id,
                expect_from: Some(FileState::Parsing),
                to: FileState::Queued,
                code: Some("DOCS_ENGINE_UNAVAILABLE"),
                retryable: true,
                version_id: Some(&s.version_id),
                attempt: None,
                detail_json: None,
            },
        )
        .await;
        let _ = back;
        set_blocked(&self.pool, &s.file_id, Some(SIDECAR_BLOCKED)).await;
        Outcome::failed(FailureClass::Busy, "DOCS_ENGINE_UNAVAILABLE")
    }
}

/// Map a sidecar failure to what the file does (10 4.4.4).
fn sidecar_halt(e: &SidecarError) -> Halt {
    match e {
        SidecarError::Encrypted => Halt::Park {
            to: FileState::Encrypted,
            code: "DOCS_PARSE_ENCRYPTED",
        },
        SidecarError::ScannedPdf => Halt::Park {
            to: FileState::Unsupported,
            code: "DOCS_PARSE_SCANNED_PDF",
        },
        SidecarError::Unsupported(_) => Halt::Park {
            to: FileState::Unsupported,
            code: "DOCS_PARSE_UNSUPPORTED_FORMAT",
        },
        SidecarError::Crash(_) => {
            Halt::Out(Outcome::failed(FailureClass::Crash, "DOCS_PARSE_CRASH"))
        }
        SidecarError::Timeout => Halt::Out(Outcome::failed(
            FailureClass::Transient,
            "DOCS_PARSE_TIMEOUT",
        )),
        SidecarError::Oom | SidecarError::OutputLimit => Halt::Failed {
            code: "DOCS_PARSE_OOM".into(),
            retryable: false,
        },
        SidecarError::Corrupt(_) => Halt::Failed {
            code: "DOCS_PARSE_CORRUPT".into(),
            retryable: false,
        },
        SidecarError::LimitPages { .. } => Halt::Failed {
            code: "DOCS_PARSE_LIMIT_PAGES".into(),
            retryable: false,
        },
        SidecarError::Normalize(_) => Halt::Failed {
            code: "DOCS_PARSE_IR_INVALID".into(),
            retryable: true,
        },
        // Media (D65, D77): no decoder for the codec, over the size or duration limit.
        SidecarError::CodecUnavailable { .. } => Halt::Park {
            to: FileState::Unsupported,
            code: "DOCS_PARSE_CODEC_UNAVAILABLE",
        },
        SidecarError::MediaTooLong => Halt::Park {
            to: FileState::TooLarge,
            code: "DOCS_SOURCE_MEDIA_TOO_LONG",
        },
        // The host decode output was made from another version of the file: parse again.
        SidecarError::HostDecodeMismatch => Halt::Out(Outcome::failed(
            FailureClass::Transient,
            "DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH",
        )),
        SidecarError::AsrFailed => Halt::Failed {
            code: "DOCS_PARSE_ASR_FAILED".into(),
            retryable: true,
        },
        SidecarError::DiarizationFailed => Halt::Failed {
            code: "DOCS_PARSE_DIARIZATION_FAILED".into(),
            retryable: true,
        },
        SidecarError::Cancelled => Halt::Out(Outcome::Cancelled),
        // Busy, an open breaker, no answer, a hash or scratch problem: the
        // sidecar side, not this file.
        SidecarError::Busy { .. }
        | SidecarError::CircuitOpen { .. }
        | SidecarError::Unavailable(_)
        | SidecarError::Protocol(_)
        | SidecarError::BlobMissing
        | SidecarError::HashMismatch
        | SidecarError::ScratchFull
        | SidecarError::DiskLow
        | SidecarError::BadInput(_) => Halt::Waiting,
    }
}

fn line_count(bytes: &[u8]) -> i64 {
    bytes.iter().filter(|b| **b == b'\n').count() as i64 + 1
}

async fn report_progress(
    pool: &PgPool,
    bus: Option<&Bus>,
    file_id: &str,
    drive_id: &str,
    unit: &'static str,
    done: i64,
    total: Option<i64>,
) {
    let p = Progress {
        file_id: file_id.to_owned(),
        drive_id: Some(drive_id.to_owned()),
        unit,
        done,
        total,
    };
    let _ = sqlx::query(
        "UPDATE _oo_docs.file SET progress = $2::jsonb WHERE id = $1::uuid AND state = 'parsing'",
    )
    .bind(file_id)
    .bind(p.column().to_string())
    .execute(pool)
    .await;
    if let Some(bus) = bus {
        bus.emit(p);
    }
}

/// Clear a sidecar wait once the job is parsing.
async fn set_blocked_clear(pool: &PgPool, file_id: &str) {
    let _ = sqlx::query(
        "UPDATE _oo_docs.file SET blocked_reason = NULL, revision = revision + 1 \
          WHERE id = $1::uuid AND blocked_reason = 'parser_unavailable'",
    )
    .bind(file_id)
    .execute(pool)
    .await;
}

/// Set or clear `blocked_reason` on a waiting file; an event says so.
pub async fn set_blocked(pool: &PgPool, file_id: &str, reason: Option<&str>) {
    let Ok(mut tx) = pool.begin().await else {
        return;
    };
    let row: Option<(String, String)> = sqlx::query_as(
        "UPDATE _oo_docs.file SET blocked_reason = $2, revision = revision + 1 \
          WHERE id = $1::uuid AND state IN ('stored', 'queued') AND deleted_at IS NULL \
            AND blocked_reason IS DISTINCT FROM $2 \
      RETURNING drive_id::text, state",
    )
    .bind(file_id)
    .bind(reason)
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    if let Some((drive, state)) = row {
        let payload = serde_json::json!({
            "file_id": file_id, "from": state, "to": state,
            "code": reason.map(|_| "DOCS_ENGINE_UNAVAILABLE"),
        })
        .to_string();
        let _ = crate::store::events::append_event(
            &mut tx,
            "file.state_changed",
            Some(&drive),
            Some(file_id),
            &payload,
        )
        .await;
        let _ = tx.commit().await;
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
            // A sidecar format waits in `queued` while the sidecar is down.
            if (sidecar_format(&s.format).is_some() || media_kind(&s.format).is_some())
                && s.state == FileState::Queued
                && let Some(client) = self.client_for(&s.format)
                && cached_parse(&self.pool, &s.b3)
                    .await
                    .ok()
                    .flatten()
                    .is_none()
            {
                let up =
                    tokio::task::spawn_blocking(move || !client.breaker_open() && client.healthy())
                        .await
                        .unwrap_or(false);
                if !up {
                    set_blocked(&self.pool, &s.file_id, Some(SIDECAR_BLOCKED)).await;
                    return Outcome::failed(FailureClass::Busy, "DOCS_ENGINE_UNAVAILABLE");
                }
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
            if sidecar_format(&s.format).is_some() || media_kind(&s.format).is_some() {
                set_blocked_clear(&self.pool, &s.file_id).await;
            }
            let done = match self.parse(&ctx, &s).await {
                Ok(d) => d,
                Err(halt) => return self.halt(&ctx, &s, halt).await,
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
                     VALUES ($1, $2, $3, $4, 'complete', $5, $6, clock_timestamp()) \
                     ON CONFLICT (b3, parser_id, parser_version, profile_sha256) DO UPDATE \
                        SET status = 'complete', ir_b3 = EXCLUDED.ir_b3, block_count = EXCLUDED.block_count, \
                            error_code = NULL, finished_at = clock_timestamp()",
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
    model: Arc<ModelRuntime>,
}

async fn load_ir_of(pool: &PgPool, cas: &Cas, b3: &str) -> Result<Option<DocIr>, Outcome> {
    let ir_b3: Option<String> = sqlx::query_scalar(
        "SELECT ir_b3 FROM _oo_docs.parse_result \
          WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
          ORDER BY finished_at DESC NULLS LAST LIMIT 1",
    )
    .bind(b3)
    .fetch_optional(pool)
    .await
    .map_err(db_fail)?;
    let Some(ir_b3) = ir_b3 else { return Ok(None) };
    match cas.get(CasKind::Ir, &ir_b3) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| Outcome::failed(FailureClass::Permanent, "DOCS_PARSE_IR_INVALID")),
        Err(CasError::NotFound) => Ok(None),
        Err(e) => Err(Outcome::failed(FailureClass::Transient, e.code())),
    }
}

/// Does this Drive use the model right now: it is enabled on the Drive and the
/// engine has a model whose egress is up (D19: otherwise rules only).
async fn llm_wanted(pool: &PgPool, model: &ModelRuntime, drive_id: &str) -> bool {
    if model.port().is_none() {
        return false;
    }
    sqlx::query_scalar::<_, bool>("SELECT model_enabled FROM _oo_docs.drive WHERE id = $1::uuid")
        .bind(drive_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// The key of the `extract_llm` job of a file version (4.1.2; the model is
/// part of the block cache key instead, so a model switch reuses nothing).
fn llm_key(version_id: &str, b3: &str) -> String {
    format!("llm:{version_id}:{b3}:{PROMPT_VERSION}")
}

/// Evidence of earlier versions of this file goes now that the new version
/// has asserted what still holds; facts left without live evidence are
/// retracted (4.3.5, D22).
async fn withdraw_superseded(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    s: &Subject,
) -> Result<(), String> {
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
    .fetch_all(&mut **tx)
    .await
    .map_err(stage_err)?;
    for v in &old {
        files::withdraw_evidence(tx, v, "file_superseded")
            .await
            .map_err(stage_err)?;
    }
    Ok(())
}

impl ExtractHandler {
    /// D94: the class of the file. The user's override first, then the rules; the model only when the rules cannot decide and the
    /// Drive uses the model. The result is stored on the file with the reason.
    async fn classify_file(&self, s: &Subject, ir: &DocIr) -> crate::profile::Profile {
        let overrides = match self.pool.acquire().await {
            Ok(mut c) => crate::profile::load_overrides(&mut c, &s.drive_id)
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let mut class = crate::profile::classify_with(&s.path, ir, &overrides);
        // `OO_DOCS_CLASSIFY_MODEL=0` switches the model's say off (the rules' default stands), for measuring.
        if !class.decided
            && !std::env::var("OO_DOCS_CLASSIFY_MODEL").is_ok_and(|v| v == "0")
            && llm_wanted(&self.pool, &self.model, &s.drive_id).await
            && let Some(port) = self.model.port()
        {
            // The call counts against the Drive's daily limit like every model call (D19).
            let daily: Option<i32> = sqlx::query_scalar(
                "SELECT llm_daily_calls FROM _oo_docs.drive WHERE id = $1::uuid",
            )
            .bind(&s.drive_id)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();
            let granted = match (daily, self.pool.acquire().await) {
                (Some(daily), Ok(mut c)) => matches!(
                    crate::extract::llm::usage::reserve(&mut c, &s.drive_id, daily).await,
                    Ok(crate::extract::llm::usage::Reserve::Granted { .. })
                ),
                _ => false,
            };
            let asked = if granted {
                tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    crate::profile::classify_by_model(port.as_ref(), &s.path, ir),
                )
                .await
                .ok()
                .flatten()
            } else {
                None
            };
            if let Ok(mut c) = self.pool.acquire().await
                && granted
            {
                let _ = crate::extract::llm::usage::record(
                    &mut c,
                    &s.drive_id,
                    if asked.is_some() {
                        crate::extract::llm::usage::Outcome::Completed {
                            tokens_in: 0,
                            tokens_out: 0,
                        }
                    } else {
                        crate::extract::llm::usage::Outcome::Failed
                    },
                )
                .await;
            }
            if let Some(profile) = asked {
                class = crate::profile::Classified {
                    profile,
                    reason: "model",
                    decided: true,
                };
            }
        }
        if let Ok(mut c) = self.pool.acquire().await {
            let _ = crate::profile::store(&mut c, &s.file_id, &class).await;
        }
        class.profile
    }

    /// Text of the minutes file with the same stem next to a recording (`周会.m4a` and `周会.md`), when it has
    /// been parsed: the attendee list there is layer 1 of the speaker identity (D74).
    async fn sibling_minutes(&self, s: &Subject) -> Option<String> {
        let (dir, name) = s.path.rsplit_once('/').unwrap_or(("", s.path.as_str()));
        let stem = name.rsplit_once('.').map_or(name, |(st, _)| st);
        let join = |ext: &str| {
            if dir.is_empty() {
                format!("{stem}.{ext}")
            } else {
                format!("{dir}/{stem}.{ext}")
            }
        };
        let ir_b3: Option<String> = sqlx::query_scalar(
            "SELECT pr.ir_b3 FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
               JOIN _oo_docs.parse_result pr ON pr.b3 = fv.b3 AND pr.status = 'complete' \
              WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND f.path = ANY($2::text[]) \
                AND pr.ir_b3 IS NOT NULL ORDER BY f.path LIMIT 1",
        )
        .bind(&s.drive_id)
        .bind(vec![join("md"), join("txt")])
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let ir = self.load_ir_by_ir_b3(&ir_b3?)?;
        Some(
            ir.blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    fn load_ir_by_ir_b3(&self, ir_b3: &str) -> Option<DocIr> {
        let bytes = self.cas.get(CasKind::Ir, ir_b3).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    async fn load_ir(&self, b3: &str) -> Result<Option<DocIr>, Outcome> {
        load_ir_of(&self.pool, &self.cas, b3).await
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
            // D89: the profile of the file, stored for the file list and decisive for the model supplement.
            let profile = self.classify_file(&s, &ir).await;
            // Same blocks as the previous version: nothing to extract, the
            // evidence moves (D53).
            let reused = match super::reuse::reusable_version(
                &self.pool,
                &self.cas,
                &s.file_id,
                &s.version_id,
                &ir,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => return db_fail(e),
            };
            let mut quality_detail: Option<serde_json::Value> = None;
            if reused.is_none() {
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
                // A first extraction, or a reprocess that passes DOC-G10 or waits for a person (D23).
                match crate::publish::reprocess::index_gated(&mut conn, &doc).await {
                    Ok(r) => {
                        let q = match r {
                            crate::publish::reprocess::Reprocess::Fresh(s)
                            | crate::publish::reprocess::Reprocess::Applied {
                                summary: s, ..
                            }
                            | crate::publish::reprocess::Reprocess::Held { summary: s, .. } => {
                                s.quality
                            }
                        };
                        if !q.is_empty() {
                            quality_detail = Some(serde_json::json!({"quality": q.to_json()}));
                        }
                    }
                    Err(e) => return db_fail(e),
                }
                // Who is speaker A? One `speaker_identity` item per speaker label of a conversation, never blocking
                // (D74 as revised, 17.4); a recording that is not a conversation has none.
                if matches!(ir.format, DocFormat::Audio | DocFormat::Video) {
                    // Labels a person confirmed earlier keep their Person (reprocess, A8).
                    if let Err(e) =
                        crate::review::speaker::apply_confirmed(&mut conn, &ir, &s.path).await
                    {
                        return db_fail(e);
                    }
                    let minutes = self.sibling_minutes(&s).await;
                    if let Err(e) = crate::review::speaker::propose(
                        &mut conn,
                        &s.version_id,
                        &ir,
                        &s.path,
                        minutes.as_deref(),
                    )
                    .await
                    {
                        return db_fail(e);
                    }
                }
            }
            // A model that is up and a Drive that uses it: the file waits in
            // `extracting` for the `extract_llm` job, which adds its facts and
            // moves the file on. Same blocks as before (reused) need no model.
            let with_llm = reused.is_none()
                && profile.uses_model()
                && llm_wanted(&self.pool, &self.model, &s.drive_id).await;
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
                if with_llm {
                    let key = llm_key(&s.version_id, &s.b3);
                    let mut job = NewJob::new(JobKind::ExtractLlm, &key);
                    job.drive_id = Some(&s.drive_id);
                    job.file_id = Some(&s.file_id);
                    job.version_id = Some(&s.version_id);
                    job.priority = PRIORITY_NEW;
                    store::enqueue(&mut tx, &job).await.map_err(stage_err)?;
                    return Ok::<bool, String>(true);
                }
                if let Some(prev) = &reused {
                    files::copy_evidence(&mut tx, prev, &s.version_id)
                        .await
                        .map_err(stage_err)?;
                }
                withdraw_superseded(&mut tx, &s).await?;
                let detail = reused
                    .as_ref()
                    .map(|p| format!("{{\"reused_from\":\"{p}\",\"by\":\"block_ids\"}}"))
                    .or_else(|| quality_detail.as_ref().map(ToString::to_string));
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
                        detail_json: detail.as_deref(),
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

// ---------------------------------------------------------- extract_llm

/// `extract_llm` handler (10 §5.2): the model's supplement to the rules.
///
/// The rules job has indexed its drafts and left the file in `extracting`;
/// this job asks the model batch by batch (cache, caps and reservations are in
/// [`extract_llm`]), stores what it said with `origin = llm` and moves the
/// file on: `indexed`, or `extract_partial` when a batch failed after its
/// retry (the batches that worked stand). A model that is gone, a Drive that
/// switched it off, a reached cap: the file goes to `indexed` with what the
/// rules found (D19), never stuck, never failed.
pub struct ExtractLlmHandler {
    pool: PgPool,
    cas: Arc<Cas>,
    model: Arc<ModelRuntime>,
}

/// `Type:name` of the entities the Drive knows that matter for this file:
/// the ones the rules found in it, then the most asserted ones.
async fn known_entities(pool: &PgPool, version_id: &str) -> Vec<String> {
    // `Type:name`, with up to six trusted aliases as `Type:name（别名：a、b）`, so the model
    // writes a person it knows as `Kevin` the way the Drive does (extract/2).
    const LABEL: &str = "e.type || ':' || e.canonical_name || COALESCE(( \
          SELECT '（别名：' || string_agg(x.alias, '、' ORDER BY x.alias) || '）' FROM ( \
            SELECT a.alias FROM _oo_docs.alias a \
             WHERE a.entity_id = e.id AND a.status IN ('draft', 'published') \
               AND a.confidence_bp >= 7000 ORDER BY a.alias LIMIT 6) x), '')";
    let sql = format!(
        "(SELECT {LABEL} FROM _oo_docs.entity e \
           WHERE e.status = 'active' AND e.id IN ( \
                 SELECT unnest(ARRAY[f.subject_id, f.object_id]) FROM _oo_docs.evidence ev \
                   JOIN _oo_docs.fact f ON f.id = ev.target_id \
                  WHERE ev.target_kind = 'fact' AND ev.version_id = $1::uuid \
                    AND ev.withdrawn_at IS NULL) \
           ORDER BY e.canonical_name LIMIT 50) \
         UNION \
         (SELECT {LABEL} FROM _oo_docs.entity e \
           JOIN _oo_docs.fact f ON f.subject_id = e.id AND f.status = 'published' \
          WHERE e.status = 'active' AND e.type <> 'Thing' \
          GROUP BY e.id ORDER BY count(*) DESC, e.canonical_name LIMIT 50)"
    );
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
        .bind(version_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

/// `Term:name` of the Terms a rule put in the ontology (a glossary entry, a rejected or tried alternative): defined
/// by a structural position, so the model's mention of one needs no definition of its own (D96).
async fn defined_terms(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT 'Term:' || e.canonical_name FROM _oo_docs.entity e \
          WHERE e.type = 'Term' AND e.status = 'active' \
            AND EXISTS (SELECT 1 FROM _oo_docs.fact f WHERE f.origin = 'rule' AND (f.subject_id = e.id OR f.object_id = e.id) \
                          AND f.rule_id IN ('dir.terms', 'md.headings')) LIMIT 2000",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

impl JobHandler for ExtractLlmHandler {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let s = match load_subject(&ctx).await {
                Ok(Some(s)) => s,
                Ok(None) => return Outcome::Done,
                Err(e) => return db_fail(e),
            };
            if s.state != FileState::Extracting {
                return Outcome::Done;
            }
            let drive: Option<(bool, i32)> = match sqlx::query_as(
                "SELECT model_enabled, llm_daily_calls FROM _oo_docs.drive WHERE id = $1::uuid",
            )
            .bind(&s.drive_id)
            .fetch_optional(&self.pool)
            .await
            {
                Ok(d) => d,
                Err(e) => return db_fail(e),
            };
            let port = drive
                .filter(|(enabled, _)| *enabled)
                .and_then(|_| self.model.port());
            let mut to = FileState::Indexed;
            let mut code: Option<&str> = None;
            let mut detail = serde_json::json!({ "llm": "skipped" });
            let mut checkpoint: Option<String> = None;
            if let (Some(port), Some((_, daily))) = (port, drive) {
                let ir = match load_ir_of(&self.pool, &self.cas, &s.b3).await {
                    Ok(Some(ir)) => ir,
                    Ok(None) => {
                        return Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_CAS_MISSING");
                    }
                    Err(o) => return o,
                };
                if ctx.should_stop() {
                    return Outcome::Cancelled;
                }
                let calls_used = ctx
                    .job
                    .checkpoint
                    .as_deref()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
                    .and_then(|v| v["calls"].as_u64())
                    .unwrap_or(0) as u32;
                let known = known_entities(&self.pool, &s.version_id).await;
                let mut conn = match self.pool.acquire().await {
                    Ok(c) => c,
                    Err(e) => return db_fail(e),
                };
                let title = std::path::Path::new(&s.path)
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&s.path)
                    .to_owned();
                // Types a person promoted constrain the prompt (B-S9-02); a read error means none.
                let types = crate::review::promotion::constrained_types(&mut conn)
                    .await
                    .unwrap_or_default();
                // D93: a record file of a class with a confirmed structure is read against its slots only.
                let class = crate::structure::store::template_for(&mut conn, &s.drive_id, &ir)
                    .await
                    .unwrap_or(None);
                let input = LlmInput {
                    drive_id: &s.drive_id,
                    path: &s.path,
                    title: &title,
                    ir: &ir,
                    known,
                    defined: defined_terms(&self.pool).await,
                    calls_used,
                    types,
                    template: class
                        .as_ref()
                        .map(crate::structure::template::prompt_section),
                };
                let limits = Limits {
                    daily,
                    ..Limits::default()
                };
                let (mut x, stats) =
                    match extract_llm(&mut conn, &self.cas, &*port, limits, &input).await {
                        Ok(r) => r,
                        Err(e) => return db_fail(e),
                    };
                if let Some(c) = &class {
                    crate::structure::template::constrain_llm(&mut x, c);
                }
                let doc = DocumentInput {
                    version_id: &s.version_id,
                    b3: &s.b3,
                    path: &s.path,
                    ir: &ir,
                };
                // A recording the rules could not place (D74 revision): the model reads its text once. A conversation is
                // then extracted again with its speakers and asked about; anything else stays as the rules left it.
                if crate::publish::media_profile::is_media(&ir)
                    && let Ok(Some(class)) =
                        crate::publish::media_profile::ask_model(&mut conn, &*port, &ir, &title)
                            .await
                    && class.identifies_speakers()
                {
                    if let Err(e) = crate::publish::write::index_document(&mut conn, &doc).await {
                        return db_fail(e);
                    }
                    if let Err(e) =
                        crate::review::speaker::apply_confirmed(&mut conn, &ir, &s.path).await
                    {
                        return db_fail(e);
                    }
                    if let Err(e) = crate::review::speaker::propose(
                        &mut conn,
                        &s.version_id,
                        &ir,
                        &s.path,
                        None,
                    )
                    .await
                    {
                        return db_fail(e);
                    }
                }
                if let Err(e) = index_extraction(&mut conn, &doc, &x).await {
                    return db_fail(e);
                }
                drop(conn);
                checkpoint = Some(format!(
                    "{{\"calls\":{}}}",
                    u64::from(calls_used) + u64::from(stats.calls)
                ));
                if stats.failed_batches > 0 {
                    to = FileState::ExtractPartial;
                    code = Some("DOCS_EXTRACT_PARTIAL");
                }
                detail = serde_json::json!({
                    "llm": "done", "batches": stats.batches, "calls": stats.calls,
                    "cache_hits": stats.cache_hits, "failed_batches": stats.failed_batches,
                    "schema_invalid": stats.schema_invalid,
                    "quality": stats.quality.to_json(),
                    "limit": if stats.file_limited { "DOCS_EXTRACT_LLM_LIMIT_FILE" }
                        else if stats.drive_limited { "DOCS_EXTRACT_LLM_LIMIT_DRIVE" }
                        else { "" },
                });
            }
            let mut tx = match begin_stage(&self.pool).await {
                Ok(t) => t,
                Err(e) => return db_fail(e),
            };
            let detail = detail.to_string();
            let r = async {
                if let Some(cp) = &checkpoint {
                    let _ = store::save_checkpoint(
                        &mut tx,
                        &ctx.job.id,
                        &ctx.owner,
                        ctx.job.generation,
                        cp,
                    )
                    .await;
                }
                match store::complete(&mut tx, &ctx.job.id, &ctx.owner, ctx.job.generation)
                    .await
                    .map_err(stage_err)?
                    .status
                {
                    JobStatus::Succeeded => {}
                    _ => return Ok(false),
                }
                withdraw_superseded(&mut tx, &s).await?;
                match state::transition_in_tx(
                    &mut tx,
                    &Transition {
                        file_id: &s.file_id,
                        expect_from: Some(FileState::Extracting),
                        to,
                        code,
                        retryable: code.is_some(),
                        version_id: Some(&s.version_id),
                        attempt: Some(ctx.job.attempt),
                        detail_json: Some(&detail),
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
    /// File version -> hash of the names it was last linked against
    /// (`doc.link`); a pass skips a version whose names did not change.
    linked: std::sync::Mutex<HashMap<String, String>>,
    /// File version -> the amounts and dates its text states (scanned once).
    scanned: std::sync::Mutex<HashMap<String, Arc<Vec<crate::extract::anchors::Found>>>>,
    /// File version -> hash of the shared values it was last linked to (`doc.anchor`).
    anchored: std::sync::Mutex<HashMap<String, String>>,
    /// D93: aligns field labels the dictionary does not know.
    model: Arc<ModelRuntime>,
}

fn lexicon_digest(lex: &[crate::extract::links::LexEntry]) -> String {
    let mut h = blake3::Hasher::new();
    for e in lex {
        h.update(format!("{:?}\0{}\n", e.ty, e.name).as_bytes());
    }
    h.finalize().to_hex().to_string()
}

impl PublishHandler {
    /// `doc.anchor` (D16): an amount or a date that two documents state becomes one open `Thing` both
    /// mention, so a report that cites a budget figure is tied to the sheet that holds it. A value one
    /// document states ties nothing and stays out. Each version is read once; a version is written again
    /// only when the set of shared values it states changed.
    async fn anchor_pass(&self) -> Result<(), crate::store::StoreError> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT fv.id::text, fv.b3, fv.path_at_version FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
              WHERE f.state IN ('indexed', 'published', 'extract_partial') \
                AND (f.profile IS NULL OR f.profile IN ('business', 'meeting', 'table', 'media')) \
                AND f.deleted_at IS NULL AND fv.superseded_at IS NULL AND fv.deleted_at IS NULL \
              ORDER BY fv.path_at_version, fv.id LIMIT 5000",
        )
        .fetch_all(&self.pool)
        .await?;
        type Scanned<'a> = (
            &'a (String, String, String),
            Arc<Vec<crate::extract::anchors::Found>>,
        );
        let mut found: Vec<Scanned<'_>> = Vec::new();
        for r in &rows {
            let have = self.scanned.lock().ok().and_then(|m| m.get(&r.0).cloned());
            let f = match have {
                Some(f) => f,
                None => {
                    let Ok(Some(ir)) = load_ir_of(&self.pool, &self.cas, &r.1).await else {
                        continue;
                    };
                    let f = Arc::new(crate::extract::anchors::scan(&ir));
                    if let Ok(mut m) = self.scanned.lock() {
                        m.insert(r.0.clone(), f.clone());
                    }
                    f
                }
            };
            found.push((r, f));
        }
        let shared = crate::extract::anchors::shared(found.iter().map(|(_, f)| f.as_slice()));
        if shared.is_empty() {
            return Ok(());
        }
        let mut conn = self.pool.acquire().await?;
        for (r, f) in found {
            let mine: Vec<&(&'static str, String)> = shared
                .iter()
                .filter(|k| f.iter().any(|x| x.label == k.0 && x.name == k.1))
                .collect();
            if mine.is_empty() {
                continue;
            }
            let digest = format!("{mine:?}");
            let done = self.anchored.lock().ok().and_then(|m| m.get(&r.0).cloned());
            if done.as_deref() == Some(digest.as_str()) {
                continue;
            }
            let Ok(Some(ir)) = load_ir_of(&self.pool, &self.cas, &r.1).await else {
                continue;
            };
            let doc = DocumentInput {
                version_id: &r.0,
                b3: &r.1,
                path: &r.2,
                ir: &ir,
            };
            let x = crate::extract::anchors::anchor_links(&r.2, &ir, &f, &shared);
            index_extraction(&mut conn, &doc, &x).await?;
            if let Ok(mut m) = self.anchored.lock() {
                m.insert(r.0.clone(), digest);
            }
        }
        Ok(())
    }

    /// `doc.link` (10 5.1): link every document that is past the rules to the
    /// entities of the Drive it names. Entities appear in any order, so the
    /// links are made here, in front of the single publish writer, against the
    /// names known now; a version is only read again when the names changed.
    async fn link_pass(&self) -> Result<String, crate::store::StoreError> {
        let mut conn = self.pool.acquire().await?;
        let lex = crate::resolve::pg::load_lexicon(&mut conn).await?;
        if lex.is_empty() {
            return Ok(String::new());
        }
        let digest = lexicon_digest(&lex);
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT fv.id::text, fv.b3, fv.path_at_version FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
              WHERE f.state IN ('indexed', 'published', 'extract_partial') \
                AND (f.profile IS NULL OR f.profile IN ('business', 'meeting', 'table', 'media')) \
                AND f.deleted_at IS NULL AND fv.superseded_at IS NULL AND fv.deleted_at IS NULL \
              ORDER BY fv.path_at_version, fv.id LIMIT 5000",
        )
        .fetch_all(&self.pool)
        .await?;
        for (version_id, b3, path) in rows {
            let done = self
                .linked
                .lock()
                .ok()
                .and_then(|m| m.get(&version_id).cloned());
            if done.as_deref() == Some(digest.as_str()) {
                continue;
            }
            let Ok(Some(ir)) = load_ir_of(&self.pool, &self.cas, &b3).await else {
                continue;
            };
            let doc = DocumentInput {
                version_id: &version_id,
                b3: &b3,
                path: &path,
                ir: &ir,
            };
            index_links(&mut conn, &doc).await?;
            if let Ok(mut m) = self.linked.lock() {
                m.insert(version_id, digest.clone());
            }
        }
        Ok(digest)
    }

    /// The digest of the names a link pass would use now.
    async fn lexicon_digest(&self) -> Result<String, crate::store::StoreError> {
        let mut conn = self.pool.acquire().await?;
        let lex = crate::resolve::pg::load_lexicon(&mut conn).await?;
        Ok(lexicon_digest(&lex))
    }

    /// A handler over `pool` and `cas` (tests drive [`Self::promote`] directly).
    pub fn new(pool: PgPool, cas: Arc<Cas>) -> Self {
        Self {
            pool,
            cas,
            linked: std::sync::Mutex::new(HashMap::new()),
            scanned: std::sync::Mutex::new(HashMap::new()),
            anchored: std::sync::Mutex::new(HashMap::new()),
            model: ModelRuntime::disabled(),
        }
    }

    /// Move every `indexed` file that has nothing left to publish to `published`.
    pub async fn promote(&self, n: Option<i64>) -> Result<u32, TransitionError> {
        // `extract_partial` after a failed model batch publishes like `indexed`
        // (the batches that worked stand); one from a failed rules job has no
        // drafts and stays where it is. A draft that waits for a person in the
        // review queue does not hold the file back (`pending_review` counts it, 4.2); neither does a
        // fact of a new extraction that DOC-G10 holds (a `reprocess_diff` item waits). A speaker nobody has named does
        // not hold anything (D74 as revised): the statements publish under the placeholder. Nor does an alias draft: the eligible aliases
        // were published with the version this runs after, and one still a draft is a candidate that has no review item and is neither
        // published nor queued (10 6 step 6), so waiting for it would hold the file for ever (the model's cross-language alias, extract/3).
        let rows: Vec<(String, String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT f.id::text, fv.id::text, f.state FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
              WHERE (f.state = 'indexed' OR (f.state = 'extract_partial' AND EXISTS ( \
                      SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_rules' AND j.file_id = f.id \
                         AND j.version_id = f.current_version_id AND j.state = 'succeeded'))) \
                AND f.deleted_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                                  JOIN _oo_docs.fact ft ON ft.id = e.target_id \
                                  JOIN _oo_docs.entity s ON s.id = ft.subject_id \
                                 WHERE e.target_kind = 'fact' AND e.version_id = fv.id \
                                   AND e.withdrawn_at IS NULL AND ft.status = 'draft' \
                                   AND NOT {held_reprocess} \
                                   AND NOT EXISTS (SELECT 1 FROM _oo_docs.review_item r \
                                                    WHERE r.status = 'pending' \
                                                      AND r.subject->>'fact_id' = ft.id::text)) \
              ORDER BY 1 LIMIT 500",
            // A new extraction that DOC-G10 holds for a person: the old extraction stays live, so the file is
            // published (the reprocess_diff item is what waits).
            held_reprocess = crate::publish::reprocess::HELD_SQL.replace("f.fact_key", "ft.fact_key"),
        )))
        .fetch_all(&self.pool)
        .await?;
        let detail = serde_json::json!({ "ontology_version": n }).to_string();
        let mut done = 0;
        for (file_id, version_id, from) in rows {
            match state::transition(
                &self.pool,
                &Transition {
                    file_id: &file_id,
                    expect_from: FileState::parse(&from),
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
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            // Link, publish; a publish can establish entities (a model fact that
            // published), whose names the other documents then link to: go round
            // again while the names change (at most three times).
            let mut n = None;
            for _round in 0..3 {
                // A failed anchor pass only leaves the shared values unlinked this round.
                let _ = self.anchor_pass().await;
                let Ok(before) = self.link_pass().await else {
                    return Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_INTERNAL");
                };
                let mut conn = match self.pool.acquire().await {
                    Ok(c) => c,
                    Err(e) => return db_fail(e),
                };
                // One person named in two scripts becomes one entity before the version is written (a failure
                // here never blocks the publish: the pair simply stays two entities and a candidate).
                if let Ok(mut tx) = sqlx::Connection::begin(&mut *conn).await {
                    match crate::resolve::crosslang::run(&mut tx).await {
                        Ok(_) => {
                            let _ = tx.commit().await;
                        }
                        Err(_) => {
                            let _ = tx.rollback().await;
                        }
                    }
                }
                let outcome = publish(&mut conn, &self.cas, &PublishConfig::default()).await;
                drop(conn);
                match outcome {
                    Ok(PublishOutcome::Published(v)) => {
                        crate::obs::emit(
                            "info",
                            "publish.version",
                            &crate::obs::Fields {
                                count: Some(("ontology_version", v.n)),
                                ..crate::obs::Fields::default()
                            },
                        );
                        n = Some(v.n);
                    }
                    Ok(PublishOutcome::NoChange { .. }) => {}
                    Ok(PublishOutcome::Busy) => {
                        return Outcome::failed(FailureClass::Busy, "DOCS_ENGINE_BUSY");
                    }
                    Err(_) => {
                        return Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_INTERNAL");
                    }
                }
                match self.lexicon_digest().await {
                    Ok(after) if after != before => {}
                    _ => break,
                }
            }
            // No version came out (a decision with nothing to publish): say the Drives settled, so a client
            // waiting for "publishing" to end does not wait for a version that never comes.
            if n.is_none()
                && let Ok(mut c) = self.pool.acquire().await
            {
                let _ = crate::api::freshness::announce_settled(&mut c, &ctx.job.id).await;
            }
            // A person's decision whose evidence is gone becomes an orphaned
            // overlay item (D22), after every publish.
            if let Ok(mut c) = self.pool.acquire().await {
                let _ = crate::review::overlay::sweep_orphans(&mut c).await;
                // Open types that reached the thresholds become `type_promotion` items (D16).
                let _ = crate::review::promotion::propose(&mut c).await;
                // D93: the structure layer follows the files (a change is a schema version; new properties of a
                // confirmed class are proposals).
                let drives: Vec<String> = sqlx::query_scalar(
                    "SELECT id::text FROM _oo_docs.drive WHERE status = 'active'",
                )
                .fetch_all(&mut *c)
                .await
                .unwrap_or_default();
                for d in drives {
                    if !std::env::var("OO_DOCS_STRUCTURE_ALIGN_MODEL").is_ok_and(|v| v == "0")
                        && llm_wanted(&self.pool, &self.model, &d).await
                        && let Some(port) = self.model.port()
                    {
                        let _ =
                            crate::structure::store::align_unknown(&mut c, &d, port.as_ref()).await;
                    }
                    let _ = crate::structure::store::induce_drive(&mut c, &d).await;
                }
            }
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
    handlers_with_sidecar(pool, factory, cas, progress, None)
}

/// [`handlers_with_progress`] with the Docling sidecar client: without it pdf,
/// docx and pptx are `unsupported`.
pub fn handlers_with_sidecar<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
    progress: Option<Bus>,
    sidecar: Option<Arc<SidecarClient>>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    handlers_with_runtime(
        pool,
        factory,
        cas,
        progress,
        sidecar,
        None,
        ModelRuntime::disabled(),
    )
}

/// [`handlers_with_sidecar`] with the engine's model: a Drive that has the
/// model enabled gets the `extract_llm` stage after the rules.
pub fn handlers_with_model<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
    progress: Option<Bus>,
    sidecar: Option<Arc<SidecarClient>>,
    model: Arc<ModelRuntime>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    handlers_with_runtime(pool, factory, cas, progress, sidecar, None, model)
}

/// [`handlers_with_sidecar`] plus the media-parser: `parse_media` jobs (own pool, D77) are handled by a
/// second parse handler, so a recording that parses for hours never holds a document slot.
pub fn handlers_with_media<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
    progress: Option<Bus>,
    sidecar: Option<Arc<SidecarClient>>,
    media: Option<MediaRuntime>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    handlers_with_runtime(
        pool,
        factory,
        cas,
        progress,
        sidecar,
        media,
        ModelRuntime::disabled(),
    )
}

/// All stage handlers: parse (documents and media), rules, model, publish.
pub fn handlers_with_runtime<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    cas: Arc<Cas>,
    progress: Option<Bus>,
    sidecar: Option<Arc<SidecarClient>>,
    media: Option<MediaRuntime>,
    model: Arc<ModelRuntime>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    let mut map: HashMap<JobKind, Arc<dyn JobHandler>> = HashMap::new();
    for kind in [JobKind::Parse, JobKind::ParseMedia] {
        map.insert(
            kind,
            Arc::new(Contended(ParseHandler {
                pool: pool.clone(),
                factory: factory.clone(),
                cas: cas.clone(),
                progress: progress.clone(),
                sidecar: sidecar.clone(),
                media: media.clone(),
            })),
        );
    }
    map.insert(
        JobKind::ExtractRules,
        Arc::new(Contended(ExtractHandler {
            pool: pool.clone(),
            cas: cas.clone(),
            model: model.clone(),
        })),
    );
    map.insert(
        JobKind::ExtractLlm,
        Arc::new(Contended(ExtractLlmHandler {
            pool: pool.clone(),
            cas: cas.clone(),
            model: model.clone(),
        })),
    );
    map.insert(
        JobKind::Publish,
        Arc::new(Contended(PublishHandler {
            pool: pool.clone(),
            cas,
            linked: std::sync::Mutex::new(HashMap::new()),
            scanned: std::sync::Mutex::new(HashMap::new()),
            anchored: std::sync::Mutex::new(HashMap::new()),
            model,
        })),
    );
    map
}

/// Concurrency of the stage kinds: publish is a single writer (4.9).
/// Parallel model extractions. 8 ran without one wire error against the three measured models (model comparison in
/// testing/super-ontology/evidence/L4/eval/model-choice); `docs.json` `concurrency.extract_llm` sets it (1 to 16).
pub const DEFAULT_EXTRACT_LLM: usize = 8;

pub fn default_concurrency() -> HashMap<JobKind, usize> {
    HashMap::from([
        (JobKind::Parse, 4),
        (
            JobKind::ParseMedia,
            crate::jobs::media_pool::DEFAULT_MEDIA_SLOTS,
        ),
        (JobKind::ExtractRules, 2),
        (JobKind::ExtractLlm, DEFAULT_EXTRACT_LLM),
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

/// `(file, version, drive, b3, failed, inherited quarantine, state, blob format)` of a file waiting for parse.
type PumpRow = (
    String,
    String,
    String,
    String,
    bool,
    bool,
    String,
    Option<String>,
);

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

    // stored (waiting for same content) -> duplicate | queued.
    let _ = settle_waiting(pool).await;

    // parsing -> queued: a failed attempt that will be retried puts the file
    // back in the queue with the reason and `retryable` (4.2).
    requeue_retrying(pool).await;

    // queued / parsing / outdated -> parse. A blob whose parse was
    // quarantined fails every path that carries it, until a parse of that
    // content has succeeded (4.1.6).
    let rows: Vec<PumpRow> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text, f.drive_id::text, fv.b3, \
                EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind IN ('parse', 'parse_media') AND j.file_id = f.id \
                          AND j.version_id = f.current_version_id \
                          AND j.state IN ('failed', 'quarantined') AND j.updated_at >= f.state_since), \
                (EXISTS (SELECT 1 FROM _oo_docs.job j \
                           JOIN _oo_docs.file_version qv ON qv.id = j.version_id \
                          WHERE j.kind IN ('parse', 'parse_media') AND j.state = 'quarantined' AND qv.b3 = fv.b3) \
                 AND NOT EXISTS (SELECT 1 FROM _oo_docs.parse_result p \
                                  WHERE p.b3 = fv.b3 AND p.status = 'complete' \
                                    AND p.ir_b3 IS NOT NULL)), \
                f.state, (SELECT bl.format FROM _oo_docs.blob bl WHERE bl.b3 = fv.b3) \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state IN ('queued', 'parsing', 'outdated') AND f.deleted_at IS NULL \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind IN ('parse', 'parse_media') AND j.file_id = f.id \
                              AND j.version_id = f.current_version_id AND j.state IN ('queued', 'running')) \
          ORDER BY (f.state = 'outdated'), f.state_since, f.id LIMIT $1",
    )
    .bind(PUMP_BATCH)
    .fetch_all(&mut *conn)
    .await?;
    let reprocessing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.job WHERE kind IN ('parse', 'parse_media') AND priority = $1 \
            AND state IN ('queued', 'running')",
    )
    .bind(PRIORITY_OUTDATED)
    .fetch_one(&mut *conn)
    .await?;
    let mut reprocess_slots = (REPROCESS_INFLIGHT - reprocessing).max(0);
    for (file_id, version_id, drive_id, b3, failed, inherited, state, format) in rows {
        if failed {
            fail_file(pool, &file_id, &version_id, FileState::ParseFailed, None).await;
            continue;
        }
        if inherited {
            let code = Some("DOCS_PARSE_QUARANTINED");
            fail_file(pool, &file_id, &version_id, FileState::ParseFailed, code).await;
            continue;
        }
        if state == "outdated" {
            if reprocess_slots == 0 {
                continue;
            }
            reprocess_slots -= 1;
        }
        let key = format!("parse:{b3}:local@{LOCAL_PARSER_VERSION}:v{version_id}");
        let priority = if state == "outdated" {
            PRIORITY_OUTDATED
        } else {
            PRIORITY_NEW
        };
        if enqueue_stage(
            &mut conn,
            parse_kind_of(format.as_deref().unwrap_or("")),
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
            AND NOT (f.state = 'extracting' AND EXISTS (SELECT 1 FROM _oo_docs.job j \
                        WHERE j.kind = 'extract_rules' AND j.file_id = f.id \
                          AND j.version_id = f.current_version_id AND j.state = 'succeeded' \
                          AND j.updated_at >= f.state_since)) \
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

    // extracting, rules done -> extract_llm (a lost wake-up, or a job that
    // failed for good: the file goes on with what the rules found).
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text, f.drive_id::text, fv.b3, \
                EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_llm' AND j.file_id = f.id \
                          AND j.version_id = f.current_version_id \
                          AND j.state IN ('failed', 'quarantined') AND j.updated_at >= f.state_since) \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state = 'extracting' AND f.deleted_at IS NULL \
            AND EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_rules' AND j.file_id = f.id \
                          AND j.version_id = f.current_version_id AND j.state = 'succeeded' \
                          AND j.updated_at >= f.state_since) \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'extract_llm' AND j.file_id = f.id \
                              AND j.version_id = f.current_version_id AND j.state IN ('queued', 'running')) \
          ORDER BY f.state_since, f.id LIMIT $1",
    )
    .bind(PUMP_BATCH)
    .fetch_all(&mut *conn)
    .await?;
    for (file_id, version_id, drive_id, b3, failed) in rows {
        if failed {
            fail_file(pool, &file_id, &version_id, FileState::ExtractPartial, None).await;
            continue;
        }
        let key = llm_key(&version_id, &b3);
        if enqueue_stage(
            &mut conn,
            JobKind::ExtractLlm,
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

/// Paths that wait in `stored` because the same content is being processed
/// under another path (the hash stage leaves them there): when that work has
/// finished they become `duplicate` with their own copy of the evidence
/// (4.7.1); when nobody is working on the content any more the first of them
/// becomes `queued` and leads. Returns how many files moved.
pub async fn settle_waiting(pool: &PgPool) -> Result<u32, crate::store::StoreError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT f.id::text, fv.id::text, fv.b3 \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state = 'stored' AND f.deleted_at IS NULL AND fv.superseded_at IS NULL \
            AND fv.deleted_at IS NULL AND fv.duplicate_of IS NULL \
          ORDER BY f.state_since, f.id LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    let mut moved = 0;
    for (file_id, version_id, b3) in rows {
        let mut tx = begin_stage(pool).await?;
        let r: Result<bool, String> = async {
            // Row lock first, then the content lock: the order the hash
            // stage takes them in.
            let Some((st, deleted)) = files::lock_file(&mut tx, &file_id)
                .await
                .map_err(stage_err)?
            else {
                return Ok(false);
            };
            if st != "stored" || deleted {
                return Ok(false);
            }
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(&b3)
                .execute(&mut *tx)
                .await
                .map_err(stage_err)?;
            let finished: Option<String> = sqlx::query_scalar(
                "SELECT fv.id::text FROM _oo_docs.file_version fv \
                   JOIN _oo_docs.file f ON f.id = fv.file_id \
                  WHERE fv.b3 = $1 AND fv.file_id <> $2::uuid AND fv.superseded_at IS NULL \
                    AND fv.deleted_at IS NULL AND f.deleted_at IS NULL \
                    AND f.current_version_id = fv.id \
                    AND f.state IN ('published', 'indexed', 'extract_partial') \
                  ORDER BY fv.created_at, fv.id LIMIT 1",
            )
            .bind(&b3)
            .bind(&file_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(stage_err)?;
            if let Some(twin) = finished {
                sqlx::query(
                    "UPDATE _oo_docs.file_version SET duplicate_of = $2::uuid WHERE id = $1::uuid",
                )
                .bind(&version_id)
                .bind(&twin)
                .execute(&mut *tx)
                .await
                .map_err(stage_err)?;
                files::copy_evidence(&mut tx, &twin, &version_id)
                    .await
                    .map_err(stage_err)?;
                files::to_state(
                    &mut tx,
                    &file_id,
                    FileState::Stored,
                    FileState::Duplicate,
                    Extra {
                        version_id: Some(&version_id),
                        ..Extra::default()
                    },
                )
                .await
                .map_err(stage_err)?;
                return Ok(true);
            }
            let busy: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM _oo_docs.file_version fv \
                   JOIN _oo_docs.file f ON f.id = fv.file_id \
                  WHERE fv.b3 = $1 AND fv.file_id <> $2::uuid AND fv.superseded_at IS NULL \
                    AND fv.deleted_at IS NULL AND f.deleted_at IS NULL \
                    AND f.current_version_id = fv.id \
                    AND f.state IN ('queued', 'parsing', 'parsed', 'extracting'))",
            )
            .bind(&b3)
            .bind(&file_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(stage_err)?;
            if busy {
                return Ok(false);
            }
            // Nobody leads: this path does.
            files::to_state(
                &mut tx,
                &file_id,
                FileState::Stored,
                FileState::Queued,
                Extra {
                    version_id: Some(&version_id),
                    ..Extra::default()
                },
            )
            .await
            .map_err(stage_err)?;
            Ok(true)
        }
        .await;
        match r {
            Ok(true) => {
                if tx.commit().await.is_ok() {
                    moved += 1;
                }
            }
            Ok(false) | Err(_) => {
                let _ = tx.rollback().await;
            }
        }
    }
    Ok(moved)
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
          WHERE file_id = $1::uuid AND version_id = $2::uuid AND kind IN ('parse', 'parse_media', 'extract_rules', 'extract_llm') \
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
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.file f \
                         WHERE (f.state = 'indexed' OR (f.state = 'extract_partial' AND EXISTS ( \
                                 SELECT 1 FROM _oo_docs.job x WHERE x.kind = 'extract_rules' AND x.file_id = f.id \
                                    AND x.version_id = f.current_version_id AND x.state = 'succeeded'))) \
                           AND f.deleted_at IS NULL \
                         AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'publish' \
                                          AND j.created_at >= f.state_since \
                                          AND j.state IN ('queued', 'running', 'succeeded'))) \
             OR EXISTS (SELECT 1 FROM _oo_docs.file f JOIN _oo_docs.file_version fv \
                          ON fv.id = f.current_version_id \
                         WHERE f.state = 'duplicate' AND f.deleted_at IS NULL \
                           AND f.state_since > COALESCE((SELECT max(published_at) \
                                 FROM _oo_docs.ontology_version), '-infinity'::timestamptz) \
                           AND EXISTS (SELECT 1 FROM _oo_docs.evidence e \
                                        WHERE e.version_id = fv.id AND e.withdrawn_at IS NULL) \
                           AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'publish' \
                                            AND j.created_at >= f.state_since \
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
    // A file whose rules are done and whose model call is still to come: its rules facts publish now. The key is its
    // own, so a later publish in the same window (the file moved on to `indexed`) is never taken for this one.
    let early: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.file f \
                         JOIN _oo_docs.job x ON x.kind = 'extract_rules' AND x.file_id = f.id \
                                            AND x.version_id = f.current_version_id AND x.state = 'succeeded' \
                        WHERE f.state = 'extracting' AND f.deleted_at IS NULL \
                          AND NOT EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.kind = 'publish' \
                                           AND j.created_at >= x.updated_at \
                                           AND j.state IN ('queued', 'running', 'succeeded')))",
    )
    .fetch_one(&mut *conn)
    .await?;
    if early {
        let now_ms: i64 =
            sqlx::query_scalar("SELECT (extract(epoch FROM clock_timestamp()) * 1000)::bigint")
                .fetch_one(&mut *conn)
                .await?;
        let start = now_ms - now_ms.rem_euclid(PUBLISH_WINDOW_MS);
        let key = format!("publish:early:{start}");
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
           JOIN _oo_docs.job j ON j.kind IN ('parse', 'parse_media') AND j.file_id = f.id \
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
                         AND p.status = 'complete' AND p.parser_id IN ('oo-md', 'oo-text', 'oo-csv', 'oo-xlsx')) \
            AND (NOT EXISTS (SELECT 1 FROM _oo_docs.parse_result p WHERE p.b3 = fv.b3 \
                              AND p.status = 'complete' AND p.parser_id IN ('oo-md', 'oo-text', 'oo-csv', 'oo-xlsx') \
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

/// Files made by a parser or prompt older than the current ones, or parked because a sidecar was missing, become
/// `outdated` (D23, B-S10-02). The pump reprocesses them at priority 900 in small batches, the old facts stay valid
/// meanwhile, and DOC-G10 judges the new extraction when it is stored. Returns how many files were marked.
///
/// * parser: the latest complete parse of the blob came from a sidecar parser whose version differs from
///   [`set_current_parser`]'s;
/// * prompt: the file's LLM facts carry another `prompt_version` than `prompt_version`;
/// * `ocr_enabled`: scanned pdf and image files that waited for OCR (`DOCS_PARSE_SCANNED_PDF`,
///   `DOCS_PARSE_IMAGE_OCR_M2`) are tried again.
pub async fn mark_outdated_versions(
    pool: &PgPool,
    prompt_version: &str,
    ocr_enabled: bool,
) -> Result<u32, crate::store::StoreError> {
    let (ids, versions) = current_parsers();
    let mut rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text \
           FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state IN ('published', 'extract_partial', 'parse_failed') AND f.deleted_at IS NULL \
            AND fv.duplicate_of IS NULL \
            AND EXISTS (SELECT 1 FROM _oo_docs.parse_result p, unnest($1::text[], $2::text[]) c(i, v) \
                         WHERE p.b3 = fv.b3 AND p.status = 'complete' AND p.parser_id = c.i) \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.parse_result p, unnest($1::text[], $2::text[]) c(i, v) \
                             WHERE p.b3 = fv.b3 AND p.status = 'complete' AND p.parser_id = c.i \
                               AND p.parser_version = c.v) \
          ORDER BY f.id LIMIT 500",
    )
    .bind(&ids)
    .bind(&versions)
    .fetch_all(pool)
    .await?;
    let by_prompt: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT f.id::text, f.current_version_id::text \
           FROM _oo_docs.file f \
           JOIN _oo_docs.evidence e ON e.version_id = f.current_version_id AND e.target_kind = 'fact' \
                                   AND e.withdrawn_at IS NULL \
           JOIN _oo_docs.fact x ON x.id = e.target_id \
          WHERE f.state IN ('published', 'extract_partial') AND f.deleted_at IS NULL \
            AND x.origin = 'llm' AND x.prompt_version IS NOT NULL AND x.prompt_version <> $1 \
          ORDER BY 1 LIMIT 500",
    )
    .bind(prompt_version)
    .fetch_all(pool)
    .await?;
    rows.extend(by_prompt);
    if ocr_enabled {
        let waiting: Vec<(String, String)> = sqlx::query_as(
            "SELECT f.id::text, f.current_version_id::text FROM _oo_docs.file f \
              WHERE f.state = 'unsupported' AND f.deleted_at IS NULL AND f.current_version_id IS NOT NULL \
                AND f.state_code IN ('DOCS_PARSE_SCANNED_PDF', 'DOCS_PARSE_IMAGE_OCR_M2') \
              ORDER BY f.id LIMIT 500",
        )
        .fetch_all(pool)
        .await?;
        rows.extend(waiting);
    }
    rows.sort();
    rows.dedup();
    let mut n = 0;
    for (file_id, version_id) in rows {
        match state::transition(
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
        .await
        {
            Ok(_) => n += 1,
            Err(TransitionError::Store(e)) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(n)
}

/// The manual full rebuild of one Drive (`POST /drives/{id}:rebuild`, D23): the stored parse results of its files
/// are set aside and every file that has a result becomes `outdated`. The pump then reprocesses them at priority
/// 900, two at a time; each new extraction goes through DOC-G10, and the old facts stay valid until then. Returns
/// how many files were marked.
pub async fn rebuild_drive(pool: &PgPool, drive_id: &str) -> Result<u32, crate::store::StoreError> {
    sqlx::query(
        "UPDATE _oo_docs.parse_result p SET status = 'failed', error_code = 'DOCS_REBUILD_SET_ASIDE' \
          WHERE p.status = 'complete' AND p.b3 IN (SELECT fv.b3 FROM _oo_docs.file f \
                JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
               WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL)",
    )
    .bind(drive_id)
    .execute(pool)
    .await?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text FROM _oo_docs.file f \
          WHERE f.drive_id = $1::uuid AND f.deleted_at IS NULL AND f.current_version_id IS NOT NULL \
            AND f.state IN ('published', 'extract_partial', 'parse_failed', 'unsupported', 'indexed') \
          ORDER BY f.id",
    )
    .bind(drive_id)
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for (file_id, version_id) in rows {
        // `indexed` is not yet `published`: it reaches `outdated` through the table's own edge.
        match state::transition(
            pool,
            &Transition {
                file_id: &file_id,
                expect_from: None,
                to: FileState::Outdated,
                code: None,
                retryable: false,
                version_id: Some(&version_id),
                attempt: None,
                detail_json: Some("{\"reason\":\"rebuild\"}"),
            },
        )
        .await
        {
            Ok(_) => n += 1,
            Err(TransitionError::Store(e)) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(n)
}

type RetryRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
);

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
                f.deleted_at IS NOT NULL, f.state_code \
           FROM _oo_docs.file f WHERE f.id = $1::uuid FOR UPDATE",
    )
    .bind(file_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((state_text, version_id, drive_id, b3, deleted, state_code)) = row else {
        return Ok(RetryResult::NotFound);
    };
    let b3 = b3.unwrap_or_default();
    let (Some(version_id), Some(drive_id)) = (version_id, drive_id) else {
        return Ok(RetryResult::NotRetryable(state_text));
    };
    // A recording parked for want of a decoder (`DOCS_PARSE_CODEC_UNAVAILABLE`)
    // waits for the host to hand over decoded output (10 17.2): the retry goes
    // through `outdated` (the table's own edge) and parses it again.
    let host_decode = state_text == "unsupported"
        && state_code.as_deref() == Some("DOCS_PARSE_CODEC_UNAVAILABLE");
    if deleted
        || !(host_decode || matches!(state_text.as_str(), "parse_failed" | "extract_partial"))
    {
        return Ok(RetryResult::NotRetryable(state_text));
    }
    state::transition_in_tx(
        &mut tx,
        &Transition {
            file_id,
            expect_from: FileState::parse(&state_text),
            to: if host_decode {
                FileState::Outdated
            } else {
                FileState::Queued
            },
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
    let format: Option<String> =
        sqlx::query_scalar("SELECT format FROM _oo_docs.blob WHERE b3 = $1")
            .bind(&b3)
            .fetch_optional(&mut *tx)
            .await?
            .flatten();
    let mut job = NewJob::new(parse_kind_of(format.as_deref().unwrap_or("")), &key);
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
