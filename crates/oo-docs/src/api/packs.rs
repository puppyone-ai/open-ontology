//! Context packs: `POST /v1/docs/context-packs:compile`,
//! `GET /v1/docs/context-packs/{pid}` and `.../files/{path}` (10 §8.3).

use axum::body::Body;
use axum::http::{Response, StatusCode, header};
use serde_json::{Map, Value, json};
use sqlx::PgConnection;

use super::error::{ApiError, code};
use super::http::{is_uuid, json_with, parse_json};
use super::idem;
use super::{AppState, Req};
use crate::cas::Cas;
use crate::compile::persist::{self, PackRecord};
use crate::compile::{CompileRequest, DEFAULT_BUDGET, MAX_BUDGET, MIN_BUDGET, compile_from_store};
use crate::store::events;

const COMPILE_KEYS: [&str; 10] = [
    "drive_id",
    "scope",
    "task",
    "seeds",
    "budget",
    "role",
    "version",
    "min_drive_rev",
    "wait_ms",
    "require_complete",
];

/// `CompileRequest` (contract `$defs/CompileRequest`) at `pointer`.
pub fn parse_compile(v: &Value, pointer: &str) -> Result<(String, CompileRequest), ApiError> {
    let at = |k: &str| format!("{pointer}/{k}");
    let obj: &Map<String, Value> = v
        .as_object()
        .ok_or_else(|| ApiError::bad(pointer, "expected an object"))?;
    if let Some(k) = obj.keys().find(|k| !COMPILE_KEYS.contains(&k.as_str())) {
        return Err(ApiError::bad(&at(k), "unknown field"));
    }
    let drive = obj
        .get("drive_id")
        .and_then(Value::as_str)
        .filter(|s| is_uuid(s))
        .ok_or_else(|| ApiError::bad(&at("drive_id"), "drive_id must be a UUID"))?
        .to_owned();
    let strings = |k: &str, max: usize| -> Result<Vec<String>, ApiError> {
        match obj.get(k) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) if a.len() <= max => a
                .iter()
                .map(|x| x.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| ApiError::bad(&at(k), "expected strings")),
            Some(_) => Err(ApiError::bad(&at(k), "expected an array of strings")),
        }
    };
    let mut req = CompileRequest {
        scope: strings("scope", 64)?,
        seeds: strings("seeds", 20)?,
        ..CompileRequest::default()
    };
    if let Some(t) = obj.get("task") {
        req.task = t
            .as_str()
            .ok_or_else(|| ApiError::bad(&at("task"), "expected a string"))?
            .to_owned();
    }
    if let Some(b) = obj.get("budget") {
        let n = b
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (MIN_BUDGET..=MAX_BUDGET).contains(n))
            .ok_or_else(|| ApiError::bad(&at("budget"), "budget must be 200..=20000"))?;
        req.budget = n;
    } else {
        req.budget = DEFAULT_BUDGET;
    }
    if let Some(r) = obj.get("role") {
        req.role = r
            .as_str()
            .filter(|r| matches!(*r, "viewer" | "suggester" | "editor"))
            .ok_or_else(|| ApiError::bad(&at("role"), "unknown role"))?
            .to_owned();
    }
    if let Some(n) = obj.get("version") {
        req.version = Some(
            n.as_i64()
                .filter(|n| *n >= 1)
                .ok_or_else(|| ApiError::bad(&at("version"), "version must be >= 1"))?,
        );
    }
    Ok((drive, req))
}

/// Compile and persist; the second value is whether the pack was new.
pub async fn compile_and_store(
    conn: &mut PgConnection,
    cas: &Cas,
    drive: &str,
    req: &CompileRequest,
) -> Result<(crate::compile::CompiledPack, PackRecord, bool), ApiError> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT status FROM _oo_docs.drive WHERE id = $1::uuid AND status <> 'deleted'",
    )
    .bind(drive)
    .fetch_optional(&mut *conn)
    .await?;
    if exists.is_none() {
        return Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found")
            .details(json!({ "drive_id": drive })));
    }
    let compiled = compile_from_store(&mut *conn, cas, drive, req).await?;
    let (record, created) = persist::persist(&mut *conn, cas, &compiled).await?;
    if created {
        events::append_event(
            conn,
            "pack.compiled",
            Some(drive),
            Some(&record.id),
            &json!({
                "pack_id": record.id,
                "version": record.version,
                "pack_digest": record.pack_digest,
            })
            .to_string(),
        )
        .await?;
    }
    Ok((compiled, record, created))
}

fn cas(app: &AppState) -> Result<&Cas, ApiError> {
    app.cas
        .as_deref()
        .ok_or_else(|| ApiError::new(code::INTERNAL, "this engine has no content store"))
}

async fn with_preview(
    conn: &mut PgConnection,
    cas: &Cas,
    r: &PackRecord,
) -> Result<Value, ApiError> {
    let mut v = r.to_json();
    if let Some((bytes, _)) = persist::read_file(conn, cas, &r.id, "BRIEF.md").await? {
        v["brief_preview"] = Value::String(String::from_utf8_lossy(&bytes).into_owned());
    }
    Ok(v)
}

/// `POST /v1/docs/context-packs:compile`.
pub async fn compile(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let key = idem::key_from(&req.headers)?;
    let route = "POST /v1/docs/context-packs:compile";
    let sha = idem::body_sha256(&req.body);
    let body = parse_json(&req.body)?;
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, route, &key, &sha).await? {
        return Ok(json_with(stored.status, &stored.response, &[]));
    }
    let (drive, creq) = parse_compile(&body, "")?;
    let cas = cas(app)?;
    let (compiled, record, created) = compile_and_store(&mut conn, cas, &drive, &creq).await?;
    let mut v = with_preview(&mut conn, cas, &record).await?;
    v["tokens"]["budget"] = json!(compiled.budget);
    let status = if created { 201 } else { 200 };
    idem::store(&mut conn, route, &key, &sha, status, &v).await?;
    Ok(json_with(status, &v, &[]))
}

/// `GET /v1/docs/context-packs/{pid}`.
pub async fn get(app: &AppState, pid: &str) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let cas = cas(app)?;
    if !is_uuid(pid) {
        return Err(ApiError::new(code::PACK_NOT_FOUND, "pack not found"));
    }
    let record = persist::load(&mut conn, pid)
        .await?
        .ok_or_else(|| ApiError::new(code::PACK_NOT_FOUND, "pack not found"))?;
    let v = with_preview(&mut conn, cas, &record).await?;
    Ok(json_with(200, &v, &[]))
}

/// `GET /v1/docs/context-packs/{pid}/files/{path}`: raw bytes, `ETag` is the
/// sha256 of the file.
pub async fn file(app: &AppState, pid: &str, path: &str) -> Result<Response<Body>, ApiError> {
    let not_found = || ApiError::new(code::PACK_NOT_FOUND, "pack file not found");
    if !is_uuid(pid) {
        return Err(not_found());
    }
    let mut conn = app.pool.acquire().await?;
    let (bytes, sha) = persist::read_file(&mut conn, cas(app)?, pid, path)
        .await?
        .ok_or_else(not_found)?;
    let ctype = if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else {
        "text/markdown; charset=utf-8"
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::ETAG, format!("\"{sha}\""))
        .body(Body::from(bytes))
        .unwrap_or_default())
}
