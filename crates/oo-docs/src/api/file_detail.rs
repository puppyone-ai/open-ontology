//! One file: detail, blocks, original bytes, batch retry (10 §8.3, §8.8 G2).
//!
//! * `GET /v1/docs/files/{fid}?include=history,entities,facts,shares`
//! * `GET /v1/docs/files/{fid}/blocks?limit&cursor&page`
//! * `GET /v1/docs/files/{fid}/content` (G2): the original bytes, `Range`,
//!   `ETag` = content hash, `If-None-Match`
//! * `POST /v1/docs/files:retry`: up to 500 files, one result per file

use std::collections::{BTreeSet, HashMap};

use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode, header};
use oo_core::docir::{Anchor, DocIr};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::error::{ApiError, code};
use super::http::{FMT, Query, is_uuid, json as json_response, json_with, parse_json};
use super::{AppState, Req, files, idem};
use crate::cas::{CasError, CasKind};
use crate::compile::source::tag;
use crate::jobs::stages::{self, RetryResult};

/// Largest batch of `files:retry`.
pub const RETRY_BATCH_MAX: usize = 500;
/// Recent history entries returned.
const HISTORY_MAX: i64 = 200;
const ENTITIES_MAX: i64 = 500;
const FACTS_MAX: i64 = 500;
const EVIDENCE_PER_FACT: usize = 50;
const QUOTE_MAX_CHARS: usize = 600;

fn not_found() -> ApiError {
    ApiError::not_found("file")
}

/// What the routes need to know about a file.
struct Head {
    drive_id: String,
    raw_key: Vec<u8>,
    path: String,
    state: String,
    deleted: bool,
    version_id: Option<String>,
    b3: Option<String>,
    size: Option<i64>,
    format: Option<String>,
}

async fn head(conn: &mut PgConnection, fid: &str) -> Result<Head, ApiError> {
    if !is_uuid(fid) {
        return Err(not_found());
    }
    let row = sqlx::query(
        "SELECT f.drive_id::text AS drive_id, f.raw_key, f.path, f.state, \
                f.deleted_at IS NOT NULL AS deleted, f.current_version_id::text AS version_id, \
                fv.b3, b.size, b.format \
           FROM _oo_docs.file f \
           LEFT JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
           LEFT JOIN _oo_docs.blob b ON b.b3 = fv.b3 WHERE f.id = $1::uuid",
    )
    .bind(fid)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(not_found)?;
    Ok(Head {
        drive_id: row.try_get("drive_id")?,
        raw_key: row.try_get("raw_key")?,
        path: row.try_get("path")?,
        state: row.try_get("state")?,
        deleted: row.try_get("deleted")?,
        version_id: row.try_get("version_id")?,
        b3: row.try_get("b3")?,
        size: row.try_get("size")?,
        format: row.try_get("format")?,
    })
}

fn cas(app: &AppState) -> Result<&crate::cas::Cas, ApiError> {
    app.cas
        .as_deref()
        .ok_or_else(|| ApiError::new(code::INTERNAL, "content store not configured"))
}

type ParseRow = (String, String, String, Option<i32>, Option<i32>);

/// The finished parse of a blob: the IR and what is recorded about it.
struct Parsed {
    ir: DocIr,
    parser_id: String,
    parser_version: String,
    block_count: i32,
    page_count: Option<i32>,
}

async fn load_parsed(
    conn: &mut PgConnection,
    app: &AppState,
    b3: &str,
) -> Result<Option<Parsed>, ApiError> {
    let row: Option<ParseRow> = sqlx::query_as(
        "SELECT ir_b3, parser_id, parser_version, block_count, page_count \
           FROM _oo_docs.parse_result \
          WHERE b3 = $1 AND status = 'complete' AND ir_b3 IS NOT NULL \
          ORDER BY finished_at DESC NULLS LAST LIMIT 1",
    )
    .bind(b3)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((ir_b3, parser_id, parser_version, blocks, pages)) = row else {
        return Ok(None);
    };
    let bytes = match cas(app)?.get(CasKind::Ir, &ir_b3) {
        Ok(b) => b,
        // The object is gone (GC or corruption): as if not parsed yet; the
        // extract stage reparses.
        Err(CasError::NotFound) => return Ok(None),
        Err(_) => return Err(ApiError::new(code::INTERNAL, "content store read failed")),
    };
    let ir: DocIr = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::new(code::INTERNAL, "stored IR is not valid"))?;
    Ok(Some(Parsed {
        block_count: blocks.unwrap_or(i32::try_from(ir.blocks.len()).unwrap_or(i32::MAX)),
        ir,
        parser_id,
        parser_version,
        page_count: pages,
    }))
}

fn block_json(path: &str, block: &oo_core::docir::Block) -> Value {
    let mut v = serde_json::to_value(block).unwrap_or(Value::Null);
    v["src"] = json!(tag(path, &block.anchor));
    v
}

// ------------------------------------------------------------------ detail

const ENTITY_REF: &str = "jsonb_build_object('id', {a}.id::text, 'key', {a}.key, 'type', {a}.type, \
    'label', {a}.thing_label, 'subkind', {a}.subkind, 'name', {a}.canonical_name)";

fn entity_ref(alias: &str) -> String {
    ENTITY_REF.replace("{a}", alias)
}

/// Includes of the detail request.
#[derive(Default)]
struct Include {
    history: bool,
    entities: bool,
    facts: bool,
    shares: bool,
}

fn parse_include(q: &Query) -> Result<Include, ApiError> {
    let mut inc = Include::default();
    let Some(text) = q.get("include").filter(|t| !t.is_empty()) else {
        return Ok(inc);
    };
    for part in text.split(',') {
        match part {
            "history" => inc.history = true,
            "entities" => inc.entities = true,
            "facts" => inc.facts = true,
            "shares" => inc.shares = true,
            _ => {
                return Err(ApiError::bad(
                    "/query/include",
                    "include takes history, entities, facts and shares",
                ));
            }
        }
    }
    Ok(inc)
}

/// `GET /v1/docs/files/{fid}`.
pub async fn detail(app: &AppState, fid: &str, q: &Query) -> Result<Response<Body>, ApiError> {
    let inc = parse_include(q)?;
    let mut conn = app.pool.acquire().await?;
    let head = head(&mut conn, fid).await?;
    let mut summary = files::summaries(&mut conn, &[fid.to_owned()])
        .await?
        .remove(fid)
        .ok_or_else(not_found)?;

    let versions: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_build_object('id', id::text, 'b3', b3, \
                'created_at', to_char(created_at AT TIME ZONE 'UTC', {FMT}), \
                'superseded_at', CASE WHEN superseded_at IS NULL THEN NULL ELSE \
                    to_char(superseded_at AT TIME ZONE 'UTC', {FMT}) END)::text \
           FROM _oo_docs.file_version WHERE file_id = $1::uuid ORDER BY created_at DESC, id LIMIT 50"
    )))
    .bind(fid)
    .fetch_all(&mut *conn)
    .await?;
    summary["versions"] = Value::Array(
        versions
            .iter()
            .filter_map(|t| serde_json::from_str(t).ok())
            .collect(),
    );

    let parsed = match &head.b3 {
        Some(b3) if app.cas.is_some() => load_parsed(&mut conn, app, b3).await?,
        _ => None,
    };
    summary["parse"] = match &parsed {
        Some(p) => {
            let warnings: Vec<&str> =
                p.ir.warnings
                    .iter()
                    .map(|w| w.code.as_str())
                    .take(50)
                    .collect();
            json!({
                "parser_id": p.parser_id,
                "parser_version": p.parser_version,
                "block_count": p.block_count,
                "page_count": p.page_count,
                "warnings": warnings,
            })
        }
        None => Value::Null,
    };
    // No model is configured in this build: extraction is rules only.
    summary["llm"] = json!({ "status": "skipped_not_configured", "calls": 0, "cache_hits": 0 });

    if inc.history {
        summary["history"] = Value::Array(history(&mut conn, fid).await?);
    }
    if let Some(version_id) = head.version_id.as_deref() {
        if inc.entities {
            summary["entities"] = Value::Array(entities(&mut conn, version_id).await?);
        }
        if inc.facts {
            summary["facts"] = Value::Array(
                facts(
                    &mut conn,
                    fid,
                    &head,
                    version_id,
                    parsed.as_ref().map(|p| &p.ir),
                )
                .await?,
            );
        }
        if inc.shares {
            summary["shares"] = Value::Array(shares(&mut conn, version_id).await?);
        }
    } else {
        if inc.entities {
            summary["entities"] = json!([]);
        }
        if inc.facts {
            summary["facts"] = json!([]);
        }
        if inc.shares {
            summary["shares"] = json!([]);
        }
    }
    Ok(json_response(200, &summary))
}

/// The most recent 200 entries, oldest first. `duration_ms` is the time the
/// file spent in `from` before this entry (the same timestamps `latency_ms`
/// is made of).
async fn history(conn: &mut PgConnection, fid: &str) -> Result<Vec<Value>, ApiError> {
    let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT j::text FROM ( \
           SELECT seq, jsonb_strip_nulls(jsonb_build_object('seq', seq, 'from', from_state, \
                  'to', to_state, 'code', code, 'attempt', attempt, \
                  'at', to_char(at AT TIME ZONE 'UTC', {FMT}), \
                  'version_id', version_id::text, \
                  'duration_ms', (extract(epoch FROM at - lag(at) OVER (ORDER BY seq)) * 1000)::bigint)) AS j \
             FROM _oo_docs.file_state_history WHERE file_id = $1::uuid \
            ORDER BY seq DESC LIMIT {HISTORY_MAX}) t ORDER BY seq"
    )))
    .bind(fid)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|t| serde_json::from_str(t).ok())
        .collect())
}

/// Entities this version of the file gives evidence for: subjects and
/// objects of its facts, owners of its aliases, entities it names.
async fn entities(conn: &mut PgConnection, version_id: &str) -> Result<Vec<Value>, ApiError> {
    let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_strip_nulls({})::text FROM _oo_docs.entity e \
          WHERE e.status = 'active' AND e.id IN ( \
            SELECT ft.subject_id FROM _oo_docs.evidence ev \
              JOIN _oo_docs.fact ft ON ev.target_kind = 'fact' AND ev.target_id = ft.id \
             WHERE ev.version_id = $1::uuid AND ev.withdrawn_at IS NULL \
            UNION SELECT ft.object_id FROM _oo_docs.evidence ev \
              JOIN _oo_docs.fact ft ON ev.target_kind = 'fact' AND ev.target_id = ft.id \
             WHERE ev.version_id = $1::uuid AND ev.withdrawn_at IS NULL AND ft.object_id IS NOT NULL \
            UNION SELECT a.entity_id FROM _oo_docs.evidence ev \
              JOIN _oo_docs.alias a ON ev.target_kind = 'alias' AND ev.target_id = a.id \
             WHERE ev.version_id = $1::uuid AND ev.withdrawn_at IS NULL \
            UNION SELECT ev.target_id FROM _oo_docs.evidence ev \
             WHERE ev.target_kind = 'entity' AND ev.version_id = $1::uuid AND ev.withdrawn_at IS NULL) \
          ORDER BY e.canonical_name COLLATE \"C\", e.key LIMIT {ENTITIES_MAX}",
        entity_ref("e")
    )))
    .bind(version_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|t| serde_json::from_str(t).ok())
        .collect())
}

/// Facts with live evidence in this version, each with this file's evidence.
async fn facts(
    conn: &mut PgConnection,
    fid: &str,
    head: &Head,
    version_id: &str,
    ir: Option<&DocIr>,
) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT ft.id::text AS id, jsonb_strip_nulls(jsonb_build_object( \
             'id', ft.id::text, 'fact_key', ft.fact_key, 'subject', {subject}, \
             'predicate', ft.predicate, 'object', CASE WHEN o.id IS NULL THEN NULL ELSE {object} END, \
             'value', ft.value, 'valid_from', to_char(ft.valid_from, 'YYYY-MM-DD'), \
             'valid_to', to_char(ft.valid_to, 'YYYY-MM-DD'), 'valid_precision', ft.valid_precision, \
             'recorded_at', to_char(ft.recorded_at AT TIME ZONE 'UTC', {FMT}), \
             'retracted_at', CASE WHEN ft.retracted_at IS NULL THEN NULL ELSE \
                 to_char(ft.retracted_at AT TIME ZONE 'UTC', {FMT}) END, \
             'confidence_bp', ft.confidence_bp, 'origin', ft.origin, 'status', ft.status, \
             'published_in', ft.published_in, 'superseded_by', ft.superseded_by::text))::text AS j \
           FROM _oo_docs.fact ft JOIN _oo_docs.entity s ON s.id = ft.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = ft.object_id \
          WHERE EXISTS (SELECT 1 FROM _oo_docs.evidence ev WHERE ev.target_kind = 'fact' \
                         AND ev.target_id = ft.id AND ev.version_id = $1::uuid \
                         AND ev.withdrawn_at IS NULL) \
          ORDER BY ft.fact_key, ft.id LIMIT {FACTS_MAX}",
        subject = entity_ref("s"),
        object = entity_ref("o"),
    )))
    .bind(version_id)
    .fetch_all(&mut *conn)
    .await?;
    let ids: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String, _>("id"))
        .collect::<Result<_, _>>()?;
    let ev_rows = sqlx::query(
        "SELECT ev.target_id::text AS target, ev.id::text AS id, ev.block_id, ev.anchor::text AS anchor, \
                ev.origin \
           FROM _oo_docs.evidence ev \
          WHERE ev.target_kind = 'fact' AND ev.target_id = ANY($1::text[]::uuid[]) \
            AND ev.version_id = $2::uuid AND ev.withdrawn_at IS NULL \
          ORDER BY ev.target_id, ev.block_id, ev.id",
    )
    .bind(&ids)
    .bind(version_id)
    .fetch_all(&mut *conn)
    .await?;
    let quotes: HashMap<&str, &str> = ir
        .map(|ir| {
            ir.blocks
                .iter()
                .map(|b| (b.id.as_str(), b.text.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let mut by_fact: HashMap<String, (Vec<Value>, usize)> = HashMap::new();
    for r in &ev_rows {
        let target: String = r.try_get("target")?;
        let block_id: String = r.try_get("block_id")?;
        let anchor_text: String = r.try_get("anchor")?;
        let anchor: Value = serde_json::from_str(&anchor_text).unwrap_or(Value::Null);
        let src = serde_json::from_value::<Anchor>(anchor.clone())
            .map(|a| tag(&head.path, &a))
            .unwrap_or_default();
        let entry = by_fact.entry(target).or_default();
        entry.1 += 1;
        if entry.0.len() < EVIDENCE_PER_FACT {
            let mut ev = json!({
                "id": r.try_get::<String, _>("id")?,
                "file_id": fid,
                "version_id": version_id,
                "path": head.path,
                "block_id": block_id,
                "anchor": anchor,
                "src": src,
                "origin": r.try_get::<String, _>("origin")?,
                "withdrawn_at": Value::Null,
            });
            if let Some(text) = quotes.get(block_id.as_str()) {
                ev["quote"] = json!(text.chars().take(QUOTE_MAX_CHARS).collect::<String>());
            }
            entry.0.push(ev);
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: String = r.try_get("id")?;
        let j: String = r.try_get("j")?;
        let mut fact: Value = serde_json::from_str(&j).unwrap_or(Value::Null);
        let (evidence, total) = by_fact.remove(&id).unwrap_or_default();
        fact["evidence"] = Value::Array(evidence);
        fact["evidence_total"] = json!(total);
        out.push(fact);
    }
    Ok(out)
}

/// Shares whose packs used facts this version is evidence for.
async fn shares(conn: &mut PgConnection, version_id: &str) -> Result<Vec<Value>, ApiError> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT s.id::text, s.status, count(DISTINCT u.fact_id)::bigint \
           FROM _oo_docs.share_fact_use u JOIN _oo_docs.share s ON s.id = u.share_id \
          WHERE u.fact_id IN (SELECT ev.target_id FROM _oo_docs.evidence ev \
                               WHERE ev.target_kind = 'fact' AND ev.version_id = $1::uuid \
                                 AND ev.withdrawn_at IS NULL) \
          GROUP BY s.id, s.status, s.created_at ORDER BY s.created_at DESC, s.id LIMIT 100",
    )
    .bind(version_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, state, used)| json!({ "id": id, "state": state, "facts_used": used }))
        .collect())
}

// ------------------------------------------------------------------ blocks

fn page_of(anchor: &Anchor) -> Option<u32> {
    match anchor {
        Anchor::Pdf { page, .. } => Some(*page),
        Anchor::Slide { slide, .. } => Some(*slide),
        _ => None,
    }
}

/// `GET /v1/docs/files/{fid}/blocks`.
pub async fn blocks(app: &AppState, fid: &str, q: &Query) -> Result<Response<Body>, ApiError> {
    let limit = q.limit()?;
    let page: Option<u32> = match q.get("page") {
        None => None,
        Some(p) => Some(
            p.parse::<u32>()
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| ApiError::bad("/query/page", "page is a number from 1"))?,
        ),
    };
    let filters = format!(
        "blocks|{fid}|{}",
        page.map_or(String::new(), |p| p.to_string())
    );
    let after: Option<usize> = match q.get("cursor") {
        None => None,
        Some(token) => {
            let key = super::cursor::decode(token, &filters)?;
            Some(
                key.first()
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| ApiError::new(code::CURSOR_INVALID, "cursor shape"))?,
            )
        }
    };
    let mut conn = app.pool.acquire().await?;
    let head = head(&mut conn, fid).await?;
    let not_ready = |state: &str| {
        ApiError::new(code::PARSE_NOT_READY, "the file has not been parsed yet")
            .details(json!({ "state": state }))
    };
    let Some(b3) = head.b3.as_deref().filter(|_| !head.deleted) else {
        return Err(not_ready(&head.state));
    };
    let parsed = load_parsed(&mut conn, app, b3)
        .await?
        .ok_or_else(|| not_ready(&head.state))?;
    let as_of = files::head_seq(&mut conn).await?;
    let start = after.map_or(0, |i| i + 1);
    let limit = limit as usize;
    let mut items = Vec::new();
    let mut last = None;
    let mut more = false;
    for (i, block) in parsed.ir.blocks.iter().enumerate().skip(start) {
        if page.is_some_and(|p| page_of(&block.anchor) != Some(p)) {
            continue;
        }
        if items.len() == limit {
            more = true;
            break;
        }
        items.push(block_json(&head.path, block));
        last = Some(i);
    }
    let next = if more {
        last.map(|i| super::cursor::encode(&[json!(i)], &filters))
    } else {
        None
    };
    Ok(json_response(
        200,
        &json!({ "items": items, "next_cursor": next, "as_of_seq": as_of }),
    ))
}

// ----------------------------------------------------------------- content

/// A `Range` header reduced to `[start, end]` inclusive.
#[derive(Debug, PartialEq, Eq)]
pub enum RangeSpec {
    /// No usable range: the whole object.
    Whole,
    /// A satisfiable range.
    Bytes(u64, u64),
    /// Syntactically valid but outside the object.
    Unsatisfiable,
}

/// Parse a single `bytes=` range against an object of `len` bytes. Several
/// ranges and unknown units are ignored (the whole object, as RFC 9110 allows).
pub fn parse_range(header: Option<&str>, len: u64) -> RangeSpec {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeSpec::Whole;
    };
    if spec.contains(',') {
        return RangeSpec::Whole;
    }
    let Some((a, b)) = spec.split_once('-') else {
        return RangeSpec::Whole;
    };
    let (a, b) = (a.trim(), b.trim());
    match (a.is_empty(), b.is_empty()) {
        (true, true) => RangeSpec::Whole,
        // suffix: the last n bytes
        (true, false) => match b.parse::<u64>() {
            Ok(0) => RangeSpec::Unsatisfiable,
            Ok(n) if len == 0 => {
                let _ = n;
                RangeSpec::Unsatisfiable
            }
            Ok(n) => RangeSpec::Bytes(len.saturating_sub(n), len - 1),
            Err(_) => RangeSpec::Whole,
        },
        (false, _) => {
            let Ok(start) = a.parse::<u64>() else {
                return RangeSpec::Whole;
            };
            let end = if b.is_empty() {
                u64::MAX
            } else {
                match b.parse::<u64>() {
                    Ok(e) => e,
                    Err(_) => return RangeSpec::Whole,
                }
            };
            if end < start {
                return RangeSpec::Whole; // invalid: ignore (RFC 9110 14.1.2)
            }
            if start >= len {
                return RangeSpec::Unsatisfiable;
            }
            RangeSpec::Bytes(start, end.min(len - 1))
        }
    }
}

fn content_type(format: Option<&str>) -> &'static str {
    match format {
        Some("md") => "text/markdown; charset=utf-8",
        Some("txt") | Some("csv") => "text/plain; charset=utf-8",
        Some("pdf") | Some("pdf_scanned") => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',').any(|t| {
                let t = t.trim().trim_start_matches("W/");
                t == "*" || t == etag
            })
        })
}

/// `GET /v1/docs/files/{fid}/content`.
pub async fn content(app: &AppState, fid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let head = head(&mut conn, fid).await?;
    let (Some(b3), Some(size)) = (head.b3.clone(), head.size) else {
        return Err(ApiError::not_found("content"));
    };
    if head.deleted {
        return Err(ApiError::not_found("content"));
    }
    drop(conn);
    let size = u64::try_from(size).unwrap_or(0);
    let etag = format!("\"{b3}\"");
    let ctype = content_type(head.format.as_deref());
    let base = |status: StatusCode| {
        Response::builder()
            .status(status)
            .header(header::ETAG, etag.as_str())
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_TYPE, ctype)
            .header(header::CACHE_CONTROL, "private, no-cache")
    };
    if etag_matches(&req.headers, &etag) {
        return Ok(base(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .unwrap_or_default());
    }
    let range = parse_range(
        req.headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
        size,
    );
    let (start, end, partial) = match range {
        RangeSpec::Unsatisfiable => {
            let mut e = ApiError::bad("/headers/range", "range is outside the file")
                .header("content-range", format!("bytes */{size}"));
            e.status = 416;
            return Err(e);
        }
        RangeSpec::Bytes(a, b) => (a, b, true),
        RangeSpec::Whole => (0, size.saturating_sub(1), false),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    let unavailable = |e: oo_connectors::objectstore::DocSourceError| {
        use oo_connectors::objectstore::DocSourceError as E;
        match e {
            E::NotFound => ApiError::not_found("content"),
            other => {
                let mut err = ApiError::new(code::INTERNAL, other.code());
                err.status = if other.retryable() { 503 } else { 502 };
                err.retryable = other.retryable();
                err.details = json!({ "source_code": other.code() });
                err
            }
        }
    };
    // The cached original first (remote backends), else the source itself.
    let cached = if app
        .cas
        .as_ref()
        .is_some_and(|c| c.exists(CasKind::Orig, &b3).unwrap_or(false))
    {
        cas(app)?.get(CasKind::Orig, &b3).ok()
    } else {
        None
    };
    let bytes = match cached {
        Some(all) => {
            let (s, e) = (start as usize, (start + len) as usize);
            all.get(s..e.min(all.len()))
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        }
        None => app
            .sync
            .read_original(&head.drive_id, head.raw_key.clone(), start, len)
            .await
            .map_err(unavailable)?,
    };
    // A whole read can be checked against the hash it is served under; if the
    // file changed since it was hashed, say so instead of mislabelling bytes.
    if !partial && blake3::hash(&bytes).to_hex().as_str() != b3 {
        let mut e = ApiError::new(
            code::INTERNAL,
            "the file changed on disk since it was hashed",
        );
        e.status = 409;
        e.retryable = true;
        e.code = super::error::Code::SOURCE_CHANGED_DURING_READ;
        return Err(e);
    }
    let mut builder = base(if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    })
    .header(header::CONTENT_LENGTH, bytes.len());
    if partial {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    Ok(builder.body(Body::from(bytes)).unwrap_or_default())
}

// ------------------------------------------------------------------- retry

/// `POST /v1/docs/files:retry`.
pub async fn retry(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let key = idem::key_from(&req.headers)?;
    let route = "POST /v1/docs/files:retry";
    let sha = idem::body_sha256(&req.body);
    let body = parse_json(&req.body)?;
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, route, &key, &sha).await? {
        return Ok(json_with(stored.status, &stored.response, &[]));
    }
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(extra) = obj
        .keys()
        .find(|k| !matches!(k.as_str(), "file_ids" | "mode"))
    {
        return Err(ApiError::bad(&format!("/{extra}"), "unknown field"));
    }
    let safe = match obj.get("mode").and_then(Value::as_str) {
        None | Some("normal") => false,
        Some("safe") => true,
        Some(_) if obj.get("mode").is_some_and(|m| !m.is_string()) => {
            return Err(ApiError::bad("/mode", "mode is normal or safe"));
        }
        Some(_) => return Err(ApiError::bad("/mode", "mode is normal or safe")),
    };
    let ids = obj
        .get("file_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad("/file_ids", "file_ids is an array"))?;
    if ids.is_empty() {
        return Err(ApiError::bad("/file_ids", "at least one file id"));
    }
    if ids.len() > RETRY_BATCH_MAX {
        return Err(
            ApiError::new(code::PAYLOAD_TOO_LARGE, "at most 500 files per call")
                .details(json!({ "max": RETRY_BATCH_MAX })),
        );
    }
    let mut ids_text = Vec::with_capacity(ids.len());
    for (i, v) in ids.iter().enumerate() {
        match v.as_str().filter(|s| is_uuid(s)) {
            Some(s) => ids_text.push(s.to_owned()),
            None => {
                return Err(ApiError::bad(
                    &format!("/file_ids/{i}"),
                    "a file id is a lowercase UUID",
                ));
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut results = Vec::with_capacity(ids_text.len());
    let mut queued = 0;
    for id in &ids_text {
        if !seen.insert(id.clone()) {
            continue; // a repeated id is answered once
        }
        let item = match stages::retry_file(&app.pool, id, safe).await? {
            RetryResult::Queued => {
                queued += 1;
                json!({ "id": id, "status": "ok" })
            }
            RetryResult::NotFound => {
                json!({ "id": id, "status": "not_found", "code": code::NOT_FOUND.as_str() })
            }
            RetryResult::NotRetryable(state) => {
                json!({ "id": id, "status": "not_retryable", "code": format!("state:{state}") })
            }
        };
        results.push(item);
    }
    if queued > 0 {
        app.wake.notify_waiters();
    }
    let response = json!({ "results": results });
    idem::store(&mut conn, route, &key, &sha, 200, &response).await?;
    Ok(json_with(200, &response, &[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        use RangeSpec::*;
        assert_eq!(parse_range(None, 100), Whole);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), Bytes(0, 9));
        assert_eq!(parse_range(Some("bytes=90-"), 100), Bytes(90, 99));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Bytes(90, 99));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Bytes(90, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Bytes(0, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-0"), 0), Unsatisfiable);
        assert_eq!(
            parse_range(Some("bytes=5-2"), 100),
            Whole,
            "reversed is ignored"
        );
        assert_eq!(
            parse_range(Some("bytes=0-1,4-5"), 100),
            Whole,
            "one range only"
        );
        assert_eq!(parse_range(Some("items=0-1"), 100), Whole);
        assert_eq!(parse_range(Some("bytes=a-b"), 100), Whole);
    }

    #[test]
    fn if_none_match_lists_and_weak_tags() {
        let mut h = HeaderMap::new();
        h.insert(header::IF_NONE_MATCH, "\"x\", W/\"abc\"".parse().unwrap());
        assert!(etag_matches(&h, "\"abc\""));
        assert!(!etag_matches(&h, "\"abd\""));
        h.insert(header::IF_NONE_MATCH, "*".parse().unwrap());
        assert!(etag_matches(&h, "\"abd\""));
        assert!(!etag_matches(&HeaderMap::new(), "\"abd\""));
    }

    #[test]
    fn include_list_is_checked() {
        let q = Query::parse(Some("include=history,facts")).unwrap();
        let i = parse_include(&q).unwrap();
        assert!(i.history && i.facts && !i.entities && !i.shares);
        assert!(parse_include(&Query::parse(Some("include=nope")).unwrap()).is_err());
    }
}
