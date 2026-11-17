//! Shares, minimal (10 §7.6, L2): `POST /v1/docs/shares` compiles (or reuses)
//! a pack, creates the share row and its refs credential and returns the
//! `Share`; `GET /v1/docs/shares/{sid}` reads it back.
//!
//! Two additions to the contract `Share` object, both only on the creation
//! response: `files` (path, sha256, bytes, tokens of the pack files to pull
//! from `context-packs/{pid}/files/{path}`) and `share_json` (the text of
//! `SHARE.json`, which holds the one-time refs credential, so it is not
//! stored and not replayed). The engine writes nothing outside its state
//! directory; the physical mount belongs to the caller (the `oo` CLI, the
//! `oo serve` daemon later).

use std::io::Read;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Map, Value, json};
use sqlx::PgConnection;

use super::error::{ApiError, code};
use super::http::{FMT, is_uuid, json_with, parse_json};
use super::idem;
use super::packs::{compile_and_store, parse_compile};
use super::{AppState, Req};
use crate::compile::pack::ShareFields;
use crate::store::events;

const DEFAULT_EXPIRES: i64 = 7 * 24 * 3600;
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding.
pub fn b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// `prefix` + base64url of 32 bytes from the operating system.
pub fn random_token(prefix: &str) -> Result<String, ApiError> {
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|_| ApiError::new(code::INTERNAL, "no randomness source"))?;
    Ok(format!("{prefix}{}", b64url(&buf)))
}

fn share_not_found(id: &str) -> ApiError {
    ApiError::new(code::SHARE_NOT_FOUND, "share not found").details(json!({ "share_id": id }))
}

type ShareRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    i32,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

/// The `Share` object of `sid`.
pub async fn share_value(conn: &mut PgConnection, sid: &str) -> Result<Option<Value>, ApiError> {
    let row: Option<ShareRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT s.id::text, s.pack_id::text, p.pack_digest, s.drive_id::text, s.scope::text, \
                    s.task, s.budget, s.role, s.status, \
                    to_char(s.created_at AT TIME ZONE 'UTC', {FMT}), \
                    to_char(s.revoked_at AT TIME ZONE 'UTC', {FMT}), \
                    s.refreshed_from::text, s.revision, p.version_n \
               FROM _oo_docs.share s JOIN _oo_docs.context_pack p ON p.id = s.pack_id \
              WHERE s.id = $1::uuid"
    )))
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((
        id,
        pack_id,
        digest,
        drive,
        scope,
        task,
        budget,
        role,
        status,
        created,
        revoked,
        from,
        revision,
        version,
    )) = row
    else {
        return Ok(None);
    };
    let expires: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(expires_at AT TIME ZONE 'UTC', {FMT}) FROM _oo_docs.share WHERE id = $1::uuid"
    )))
    .bind(sid)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Some(json!({
        "id": id,
        "pack_id": pack_id,
        "pack_digest": digest,
        "drive_id": drive,
        "scope": serde_json::from_str::<Value>(&scope).unwrap_or(Value::Null),
        "role": role,
        "task": task,
        "budget": budget,
        "version": version,
        "state": status,
        "mount": {"present": false},
        "refreshed_from": from,
        "created_at": created,
        "expires_at": expires,
        "revoked_at": revoked,
        "revision": revision,
    })))
}

fn parse_create(
    v: &Value,
) -> Result<(String, crate::compile::CompileRequest, String, i64, String), ApiError> {
    let obj: &Map<String, Value> = v
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    const KEYS: [&str; 6] = [
        "compile",
        "mount",
        "expires_in_seconds",
        "grantor",
        "group",
        "refresh_policy",
    ];
    if let Some(k) = obj.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let (drive, creq) = parse_compile(
        obj.get("compile")
            .ok_or_else(|| ApiError::bad("/compile", "compile is required"))?,
        "/compile",
    )?;
    let target = obj
        .get("mount")
        .and_then(|m| m.get("target_dir"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("/mount/target_dir", "mount.target_dir is required"))?;
    if !target.starts_with('/') || target.len() > 4096 || target.contains('\0') {
        return Err(ApiError::new(
            code::SHARE_TARGET_INVALID,
            "target_dir must be an absolute path",
        ));
    }
    let expires = match obj.get("expires_in_seconds") {
        None => DEFAULT_EXPIRES,
        Some(n) => n
            .as_i64()
            .filter(|n| (3600..=2_592_000).contains(n))
            .ok_or_else(|| {
                ApiError::new(
                    code::SHARE_EXPIRY_INVALID,
                    "expires_in_seconds must be 3600..=2592000",
                )
            })?,
    };
    let grantor = match obj.get("grantor") {
        None => "owner".to_owned(),
        Some(g) => g
            .as_str()
            .filter(|g| !g.is_empty() && g.len() <= 128)
            .ok_or_else(|| ApiError::bad("/grantor", "grantor must be 1..=128 characters"))?
            .to_owned(),
    };
    if creq.role == "editor" {
        return Err(ApiError::new(
            code::SHARE_ROLE_UNAVAILABLE,
            "the editor role is not available yet",
        ));
    }
    Ok((drive, creq, target.to_owned(), expires, grantor))
}

/// `POST /v1/docs/shares`.
pub async fn create(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let key = idem::key_from(&req.headers)?;
    let route = "POST /v1/docs/shares";
    let sha = idem::body_sha256(&req.body);
    let body = parse_json(&req.body)?;
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, route, &key, &sha).await? {
        return Ok(json_with(stored.status, &stored.response, &[]));
    }
    let (drive, creq, _target, expires_in, grantor) = parse_create(&body)?;
    let cas = app
        .cas
        .as_deref()
        .ok_or_else(|| ApiError::new(code::INTERNAL, "this engine has no content store"))?;
    let (compiled, record, _) = compile_and_store(&mut conn, cas, &drive, &creq).await?;

    let public = req
        .headers
        .get("x-oo-public-host")
        .or_else(|| req.headers.get("host"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1")
        .to_owned();
    let credential = random_token("oodr_")?;
    let cred_b3 = blake3::hash(credential.as_bytes()).to_hex().to_string();
    let mount_ref = format!(
        "m_{}",
        random_token("")?.chars().take(24).collect::<String>()
    );

    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    let (sid, created, expires): (String, String, String) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO _oo_docs.share \
           (id, pack_id, drive_id, scope, role, task, budget, status, grantor, mount_ref, expires_at) \
         VALUES (gen_random_uuid(), $1::uuid, $2::uuid, $3::jsonb, $4, $5, $6, 'active', $7, $8, \
                 clock_timestamp() + $9::bigint * interval '1 second') \
         RETURNING id::text, to_char(created_at AT TIME ZONE 'UTC', {FMT}), \
                   to_char(expires_at AT TIME ZONE 'UTC', {FMT})"
    )))
    .bind(&record.id)
    .bind(&drive)
    .bind(serde_json::to_string(&compiled.scope).unwrap_or_else(|_| "[]".into()))
    .bind(&compiled.role)
    .bind(if compiled.task.is_empty() { None } else { Some(compiled.task.as_str()) })
    .bind(i32::try_from(compiled.budget).unwrap_or(i32::MAX))
    .bind(&grantor)
    .bind(&mount_ref)
    .bind(expires_in)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO _oo_docs.refs_credential (id, share_id, token_b3, scopes, expires_at) \
         SELECT gen_random_uuid(), $1::uuid, $2, ARRAY['refs:read'], expires_at \
           FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(&sid)
    .bind(&cred_b3)
    .execute(&mut *tx)
    .await?;
    let keys: Vec<String> = compiled.uses.iter().map(|u| u.fact_key.clone()).collect();
    let sections: Vec<String> = compiled.uses.iter().map(|u| u.section.to_owned()).collect();
    let precisions: Vec<String> = compiled
        .uses
        .iter()
        .map(|u| u.precision.to_owned())
        .collect();
    sqlx::query(
        "INSERT INTO _oo_docs.share_fact_use (share_id, fact_id, section, precision) \
         SELECT DISTINCT ON (f.id) $1::uuid, f.id, u.section, u.precision \
           FROM unnest($2::text[], $3::text[], $4::text[]) AS u(fact_key, section, precision) \
           JOIN _oo_docs.fact f ON f.fact_key = u.fact_key \
          WHERE f.status IN ('published', 'retracted') AND f.published_in <= $5 \
            AND (f.retracted_in IS NULL OR f.retracted_in > $5) \
          ORDER BY f.id \
         ON CONFLICT DO NOTHING",
    )
    .bind(&sid)
    .bind(&keys)
    .bind(&sections)
    .bind(&precisions)
    .bind(compiled.version.n)
    .execute(&mut *tx)
    .await?;
    events::append_event(
        &mut tx,
        "share.created",
        Some(&drive),
        Some(&sid),
        &json!({"share_id": sid, "pack_id": record.id, "expires_at": expires}).to_string(),
    )
    .await?;
    tx.commit().await?;

    let share_file = compiled.share_json(&ShareFields {
        share_id: sid.clone(),
        grantor,
        created_at: created,
        expires_at: expires.clone(),
        refs_endpoint: format!("http://{public}/v1/docs/refs"),
        refs_credential: credential,
        refs_expires_at: expires,
    });
    let mut share = share_value(&mut conn, &sid)
        .await?
        .ok_or_else(|| share_not_found(&sid))?;
    // Stored for replay without the credential.
    idem::store(&mut conn, route, &key, &sha, 201, &share).await?;
    let files: Vec<Value> = compiled
        .files
        .iter()
        .map(|f| {
            json!({
                "path": f.path,
                "sha256": crate::compile::pack::sha256_hex(&f.bytes),
                "bytes": f.bytes.len(),
                "tokens": f.tokens,
            })
        })
        .collect();
    share["files"] = Value::Array(files);
    share["share_json"] = Value::String(share_file.as_str().to_owned());
    share["tokens"] = json!({"brief": record.tokens_brief, "budget": compiled.budget});
    share["trimmed"] = record.trimmed.clone();
    share["blocked"] = json!({"facts": record.blocked_facts, "entities": record.blocked_entities});
    Ok(json_with(
        201,
        &share,
        &[("location", format!("/v1/docs/shares/{sid}"))],
    ))
}

/// `GET /v1/docs/shares/{sid}`.
pub async fn get(app: &AppState, sid: &str) -> Result<Response<Body>, ApiError> {
    if !is_uuid(sid) {
        return Err(share_not_found(sid));
    }
    let mut conn = app.pool.acquire().await?;
    let v = share_value(&mut conn, sid)
        .await?
        .ok_or_else(|| share_not_found(sid))?;
    Ok(json_with(200, &v, &[]))
}
