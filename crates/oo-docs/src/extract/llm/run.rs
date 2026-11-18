//! The extraction loop over the batches of one file version (10 §5.2.4, §4.6).
//!
//! Per batch: cache (`llm_cache` key, response in the CAS) -> per file cap ->
//! reserve a call against the Drive's daily limit -> ask the model -> validate
//! (one retry on a reply that breaks the schema; a call each) -> store in the
//! cache -> map. A reached cap stops the model for the rest of the file (or
//! of the day) and the batches already done stand; the rules' result is
//! untouched, so a limit never interrupts the pipeline.

use std::collections::BTreeSet;

use oo_core::docir::DocIr;
use sqlx::{PgConnection, Row};

use super::batch::batches;
use super::egress::{ChatReply, LlmError, LlmPort};
use super::post::{Dropped, MapCtx, map_into};
use super::prompt::{Known, cache_key, request};
use super::schema::{LlmOutput, parse};
use super::usage::{Outcome, Reserve, record, reserve};
use super::{ConstrainedType, FILE_CALL_LIMIT, prompt_version};
use crate::cas::{Cas, CasError, CasKind};
use crate::extract::model::{Collector, Extraction, LlmMeta};
use crate::store::StoreError;

#[derive(Debug)]
pub enum RunError {
    Store(StoreError),
    Cas(CasError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Cas(e) => write!(f, "llm cas failed: {e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<StoreError> for RunError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<sqlx::Error> for RunError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.into())
    }
}

impl From<CasError> for RunError {
    fn from(e: CasError) -> Self {
        Self::Cas(e)
    }
}

/// Wait before the next try of a call that failed on the wire.
const INFRA_RETRY_WAIT: std::time::Duration = std::time::Duration::from_secs(3);
/// Calls of one batch that may fail on the wire before the batch is given up.
const INFRA_TRIES: u32 = 4;

/// Limits of one run.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The Drive's daily call limit (`drive.llm_daily_calls`).
    pub daily: i32,
    /// Calls for this file version (D19).
    pub per_file: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            daily: 2000,
            per_file: FILE_CALL_LIMIT,
        }
    }
}

/// What one file version needs.
pub struct LlmInput<'a> {
    pub drive_id: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub ir: &'a DocIr,
    /// Entities the Drive already knows, `Type:name` (at most 50 are sent).
    pub known: Vec<Known>,
    /// Open types a person promoted (`review::promotion::constrained_types`); they constrain the prompt and are
    /// part of the prompt version.
    pub types: Vec<ConstrainedType>,
    /// Calls this file version already used (from the checkpoint).
    pub calls_used: u32,
}

/// What the run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunStats {
    pub batches: u32,
    pub calls: u32,
    pub cache_hits: u32,
    pub schema_invalid: u32,
    pub failed_batches: u32,
    pub dropped: Dropped,
    /// The file's own cap stopped the model (`DOCS_EXTRACT_LLM_LIMIT_FILE`).
    pub file_limited: bool,
    /// The Drive's daily cap stopped the model (`DOCS_EXTRACT_LLM_LIMIT_DRIVE`).
    pub drive_limited: bool,
    /// The model asked to wait.
    pub rate_limited_ms: Option<u64>,
}

async fn cache_get(
    conn: &mut PgConnection,
    key: &str,
) -> Result<Option<(String, String)>, StoreError> {
    let row = sqlx::query("SELECT response_b3, outcome FROM _oo_docs.llm_cache WHERE key = $1")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(match row {
        Some(r) => Some((r.try_get("response_b3")?, r.try_get("outcome")?)),
        None => None,
    })
}

async fn cache_put(
    conn: &mut PgConnection,
    key: &str,
    version: &str,
    model: &str,
    b3: &str,
    outcome: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO _oo_docs.llm_cache (key, prompt_version, model, response_b3, outcome) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (key) DO NOTHING",
    )
    .bind(key)
    .bind(version)
    .bind(model)
    .bind(b3)
    .bind(outcome)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Run the supplement for one file version. Returns the LLM extraction (to
/// store with `index_extraction`) and what happened.
pub async fn extract_llm(
    conn: &mut PgConnection,
    cas: &Cas,
    port: &dyn LlmPort,
    limits: Limits,
    input: &LlmInput<'_>,
) -> Result<(Extraction, RunStats), RunError> {
    let model = port.model().to_owned();
    let version = prompt_version(&input.types);
    let mut stats = RunStats::default();
    let mut col = Collector::default();
    let mut used = input.calls_used;
    let mut keys: Vec<String> = Vec::new();
    let mut infra_errors = 0u32;
    let format = format!("{:?}", input.ir.format).to_lowercase();
    let ctx = MapCtx {
        doc_path: input.path,
        known: &input.known,
    };
    'batches: for batch in batches(input.ir) {
        stats.batches += 1;
        let key = cache_key(&model, &input.known, &input.types, &batch);
        let ids: BTreeSet<&str> = batch.blocks.iter().map(|b| b.id.as_str()).collect();
        // 1. cache
        if let Some((b3, outcome)) = cache_get(conn, &key).await? {
            record(conn, input.drive_id, Outcome::CacheHit).await?;
            stats.cache_hits += 1;
            if outcome == "ok" {
                let text = String::from_utf8_lossy(&cas.get(CasKind::Llm, &b3)?).into_owned();
                if let Ok(out) = parse(&text) {
                    keys.push(key);
                    stats
                        .dropped
                        .add(&map_into(&mut col, input.ir, &ids, &out, &ctx));
                    continue;
                }
            }
            stats.failed_batches += 1;
            continue;
        }
        // 2. the request, up to two attempts
        let req = request(
            &model,
            input.title,
            &format,
            &input.known,
            &input.types,
            &batch,
        );
        let mut parsed: Option<LlmOutput> = None;
        let mut last_reply: Option<ChatReply> = None;
        // Up to two schema attempts; an unreachable or erroring model is tried
        // again after a short wait, up to INFRA_TRIES calls (10 §4.1.4: 5xx and
        // timeouts retry), every call counted against the caps.
        let mut infra_tries = 0u32;
        let mut attempt = 0u32;
        while attempt < 2 {
            attempt += 1;
            if used >= limits.per_file {
                if !stats.file_limited {
                    record(conn, input.drive_id, Outcome::FileLimited).await?;
                }
                stats.file_limited = true;
                break 'batches;
            }
            match reserve(conn, input.drive_id, limits.daily).await? {
                Reserve::Granted { .. } => {}
                Reserve::DriveLimit { .. } => {
                    stats.drive_limited = true;
                    break 'batches;
                }
            }
            used += 1;
            stats.calls += 1;
            match port.complete(&req).await {
                Ok(reply) => {
                    infra_errors = 0;
                    record(
                        conn,
                        input.drive_id,
                        Outcome::Completed {
                            tokens_in: reply.tokens_in,
                            tokens_out: reply.tokens_out,
                        },
                    )
                    .await?;
                    match parse(&reply.content) {
                        Ok(out) => {
                            parsed = Some(out);
                            last_reply = Some(reply);
                            break;
                        }
                        Err(_) => {
                            stats.schema_invalid += 1;
                            last_reply = Some(reply);
                        }
                    }
                }
                Err(LlmError::RateLimited(ms)) => {
                    record(conn, input.drive_id, Outcome::Failed).await?;
                    stats.rate_limited_ms = Some(ms);
                    stats.failed_batches += 1;
                    break 'batches;
                }
                Err(_) => {
                    record(conn, input.drive_id, Outcome::Failed).await?;
                    infra_tries += 1;
                    last_reply = None;
                    if infra_tries < INFRA_TRIES {
                        tokio::time::sleep(INFRA_RETRY_WAIT).await;
                        attempt -= 1; // an infrastructure failure is not a schema attempt
                        continue;
                    }
                    infra_errors += 1;
                    break;
                }
            }
        }
        match (parsed, last_reply) {
            (Some(out), Some(reply)) => {
                let b3 = cas.put(CasKind::Llm, reply.content.as_bytes())?.b3;
                cache_put(conn, &key, &version, &model, &b3, "ok").await?;
                keys.push(key);
                stats
                    .dropped
                    .add(&map_into(&mut col, input.ir, &ids, &out, &ctx));
            }
            (None, Some(reply)) => {
                // Both attempts broke the schema: remembered, so it is not paid again.
                let b3 = cas.put(CasKind::Llm, reply.content.as_bytes())?.b3;
                cache_put(conn, &key, &version, &model, &b3, "schema_invalid").await?;
                stats.failed_batches += 1;
            }
            _ => {
                stats.failed_batches += 1;
                if infra_errors >= 3 {
                    break;
                }
            }
        }
    }
    super::post::cap_doc_about(&mut col, input.ir);
    keys.sort();
    let mut h = blake3::Hasher::new();
    h.update(b"oo.docs.llm-extraction/v1\0");
    for k in &keys {
        h.update(k.as_bytes());
    }
    let mut x = col.finish(input.path);
    x.llm = Some(LlmMeta {
        prompt_version: version,
        extraction_key: h.finalize().to_hex().to_string(),
    });
    Ok((x, stats))
}
