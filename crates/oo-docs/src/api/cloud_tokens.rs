//! Cloud proxy tokens and the sharing audit (10 §11.3, B-S12-01), local entry only:
//!
//! * `POST /v1/docs/cloud-tokens` `{tenant, drive_ids, label?, expires_in_seconds?}` issues an `oodc_` token (the
//!   only time it is shown); `DELETE /v1/docs/cloud-tokens/{id}` revokes it;
//! * `GET /v1/docs/audit/shares?drive_id&agent&tenant&since&until` answers who shared what to which agent, when;
//! * `GET /v1/docs/audit/verify` checks the hash chain of the audit log.

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};

use super::error::{ApiError, code};
use super::{AppState, Req, http::json_with};

const DEFAULT_TTL: i64 = 7 * 86_400;

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

pub async fn issue(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = super::http::parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = obj
        .keys()
        .find(|k| !["tenant", "drive_ids", "label", "expires_in_seconds"].contains(&k.as_str()))
    {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let tenant = obj
        .get("tenant")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.len() <= 128)
        .ok_or_else(|| ApiError::bad("/tenant", "tenant is 1..=128 characters"))?;
    let drives: Vec<String> = obj
        .get("drive_ids")
        .and_then(Value::as_array)
        .filter(|a| (1..=256).contains(&a.len()))
        .ok_or_else(|| ApiError::bad("/drive_ids", "1 to 256 drive ids"))?
        .iter()
        .map(|d| {
            d.as_str()
                .filter(|d| is_uuid(d))
                .map(str::to_ascii_lowercase)
        })
        .collect::<Option<_>>()
        .ok_or_else(|| ApiError::bad("/drive_ids", "drive ids are uuids"))?;
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .filter(|l| l.len() <= 200);
    let ttl = match obj.get("expires_in_seconds") {
        None => DEFAULT_TTL,
        Some(n) => n
            .as_i64()
            .filter(|n| (60..=2_592_000).contains(n))
            .ok_or_else(|| ApiError::bad("/expires_in_seconds", "60..=2592000"))?,
    };
    let mut conn = app.pool.acquire().await?;
    let known: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.drive WHERE id = ANY($1::text[]::uuid[]) AND status <> 'deleted'",
    )
    .bind(&drives)
    .fetch_one(&mut *conn)
    .await?;
    if known != drives.len() as i64 {
        return Err(ApiError::new(code::NOT_FOUND, "a drive does not exist"));
    }
    let (token, id) = crate::cloud::issue(&mut conn, tenant, &drives, label, ttl).await?;
    crate::audit::record(
        &mut conn,
        &crate::audit::Entry {
            action: crate::audit::TOKEN_ISSUED,
            actor: &req.actor,
            tenant: Some(tenant),
            drive_id: None,
            share_id: None,
            agent: None,
            detail: json!({"token_id": id, "drive_ids": drives, "expires_in_seconds": ttl}),
        },
    )
    .await?;
    Ok(json_with(
        201,
        &json!({"id": id, "token": token, "tenant": tenant, "drive_ids": drives, "expires_in_seconds": ttl}),
        &[],
    ))
}

pub async fn revoke(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(ApiError::new(code::NOT_FOUND, "no such token"));
    }
    let mut conn = app.pool.acquire().await?;
    if !crate::cloud::revoke(&mut conn, id).await? {
        return Err(ApiError::new(code::NOT_FOUND, "no live token with that id"));
    }
    crate::audit::record(
        &mut conn,
        &crate::audit::Entry {
            action: crate::audit::TOKEN_REVOKED,
            actor: &req.actor,
            tenant: None,
            drive_id: None,
            share_id: None,
            agent: None,
            detail: json!({"token_id": id}),
        },
    )
    .await?;
    Ok(json_with(200, &json!({"id": id, "revoked": true}), &[]))
}

pub async fn shares(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let q = &req.query;
    let drive = q.get("drive_id").filter(|d| is_uuid(d));
    if q.get("drive_id").is_some() && drive.is_none() {
        return Err(ApiError::bad("/query/drive_id", "a uuid"));
    }
    let mut conn = app.pool.acquire().await?;
    let rows = crate::audit::who_shared(
        &mut conn,
        &crate::audit::SharedFilter {
            drive_id: drive,
            agent: q.get("agent"),
            tenant: q.get("tenant"),
            since: q.get("since"),
            until: q.get("until"),
        },
    )
    .await
    .map_err(|_| ApiError::bad("/query", "since and until are timestamps"))?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({"seq": r.seq, "at": r.at, "actor": r.actor, "tenant": r.tenant, "drive_id": r.drive_id,
                   "share_id": r.share_id, "agent": r.agent, "grantor": r.grantor, "role": r.role,
                   "scope": r.scope, "revoked_at": r.revoked_at, "refs_reads": r.refs_reads})
        })
        .collect();
    Ok(json_with(200, &json!({"items": items}), &[]))
}

pub async fn verify(app: &AppState) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let v = match crate::audit::verify_chain(&mut conn).await? {
        crate::audit::Chain::Intact { rows } => json!({"intact": true, "rows": rows}),
        crate::audit::Chain::Broken { seq } => json!({"intact": false, "broken_at_seq": seq}),
    };
    Ok(json_with(200, &v, &[]))
}
