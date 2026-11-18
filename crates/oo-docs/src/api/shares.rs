//! Shares (10 §7.6, §8.3, §16): create, list, get, `:refresh`, `:revoke`,
//! `:check`, `/updates`, and the task-group refresh. The lifecycle lives in
//! `crate::share`; this file is the HTTP shape.
//!
//! Two additions to the contract `Share` object, both only on the creation
//! and refresh responses: `files` (path, sha256, bytes, tokens of the pack
//! files to pull from `context-packs/{pid}/files/{path}`) and `share_json`
//! (the text of `SHARE.json`, which holds the one-time refs credential, so it
//! is not stored and not replayed). The engine writes nothing outside its
//! state directory; the physical mount belongs to the `oo` CLI and the
//! `oo serve` daemon.

use axum::body::Body;
use axum::http::Response;
use serde_json::{Map, Value, json};

use super::error::{ApiError, code};
use super::http::{is_uuid, json_with, parse_json};
use super::idem;
use super::packs::{parse_compile, parse_freshness};
use super::{AppState, Req};
use crate::share::lifecycle::{self, DEFAULT_EXPIRES, Plan};

pub use crate::share::lifecycle::{b64url, random_token, share_value};

fn share_not_found(id: &str) -> ApiError {
    lifecycle::share_not_found(id)
}

fn public_host(req: &Req) -> String {
    req.headers
        .get("x-oo-public-host")
        .or_else(|| req.headers.get("host"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1")
        .to_owned()
}

struct Create {
    drive: String,
    plan: Plan,
}

fn parse_create(v: &Value, host: String) -> Result<Create, ApiError> {
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
    let compile = obj
        .get("compile")
        .ok_or_else(|| ApiError::bad("/compile", "compile is required"))?;
    let (drive, creq) = parse_compile(compile, "/compile")?;
    let fresh = parse_freshness(compile, "/compile")?;
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
    let group = match obj.get("group") {
        None => None,
        Some(g) => Some(
            g.as_str()
                .filter(|g| {
                    !g.is_empty()
                        && g.len() <= 128
                        && g.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "._#:-".contains(c))
                })
                .ok_or_else(|| ApiError::bad("/group", "group must match [A-Za-z0-9._#:-]{1,128}"))?
                .to_owned(),
        ),
    };
    let refresh_policy = match obj.get("refresh_policy").map(Value::as_str) {
        None => "manual".to_owned(),
        Some(Some(p @ ("manual" | "at-checkpoint" | "pinned"))) => p.to_owned(),
        Some(_) => {
            return Err(ApiError::bad(
                "/refresh_policy",
                "refresh_policy is manual, at-checkpoint or pinned",
            ));
        }
    };
    Ok(Create {
        drive: drive.clone(),
        plan: Plan {
            drive,
            creq,
            fresh,
            expires_in: expires,
            grantor,
            group,
            refresh_policy,
            public_host: host,
        },
    })
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
    let c = parse_create(&body, public_host(req))?;
    let _ = &c.drive;
    let (audit_scope, audit_role, audit_grantor) = (
        c.plan.creq.scope.clone(),
        c.plan.creq.role.clone(),
        c.plan.grantor.clone(),
    );
    let prepared = lifecycle::prepare(app, c.plan).await?;
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    let ins = lifecycle::insert(&mut tx, &prepared, None).await?;
    // Who shared what to which agent: in the same transaction as the share, so none exists without it (B-S12-01).
    crate::audit::record(
        &mut tx,
        &crate::audit::Entry {
            action: crate::audit::SHARE_CREATED,
            actor: &req.actor,
            tenant: req.cloud.as_ref().map(|c| c.tenant.as_str()),
            drive_id: Some(&c.drive),
            share_id: Some(&ins.id),
            agent: req.agent.as_deref(),
            detail: json!({"scope": audit_scope, "role": audit_role, "grantor": audit_grantor,
                           "expires_at": ins.expires_at, "pack_id": prepared.record.id, "pack_digest": prepared.record.pack_digest}),
        },
    )
    .await?;
    tx.commit().await?;
    let share = lifecycle::payload(&mut conn, &prepared, &ins).await?;
    // Stored for replay without the credential.
    let mut replay = share.clone();
    for k in ["files", "share_json"] {
        replay.as_object_mut().map(|o| o.remove(k));
    }
    idem::store(&mut conn, route, &key, &sha, 201, &replay).await?;
    Ok(json_with(
        201,
        &share,
        &[("location", format!("/v1/docs/shares/{}", ins.id))],
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

/// `PATCH /v1/docs/shares/{sid}` (G6): the expiry of an active or drifted share,
/// at most 30 days after its creation; `If-Match: "r<revision>"`. Content,
/// version and refresh policy do not change and nothing is refreshed.
pub async fn patch(app: &AppState, sid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(sid) {
        return Err(share_not_found(sid));
    }
    let want = super::http::if_match(&req.headers)?;
    let body = parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = obj.keys().find(|k| k.as_str() != "expires_at") {
        return Err(ApiError::bad(
            &format!("/{k}"),
            "only expires_at can be changed",
        ));
    }
    let expires = obj
        .get("expires_at")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("/expires_at", "expires_at is required"))?;
    let mut conn = app.pool.acquire().await?;
    let v = lifecycle::extend(&mut conn, sid, want, expires).await?;
    let rev = v["revision"].as_i64().unwrap_or(want + 1);
    Ok(json_with(200, &v, &[("ETag", super::http::etag(rev))]))
}

/// `GET /v1/docs/shares?state&drive_id&group_id&limit&cursor`.
pub async fn list(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let limit = req.query.limit()?;
    let state = req.query.get("state");
    if let Some(s) = state
        && !matches!(
            s,
            "active" | "drifted" | "refreshed" | "expired" | "revoked"
        )
    {
        return Err(ApiError::bad("/query/state", "unknown share state"));
    }
    let filters = format!(
        "shares:{}:{}:{}",
        state.unwrap_or(""),
        req.query.get("drive_id").unwrap_or(""),
        req.query.get("group_id").unwrap_or("")
    );
    let after = match req.query.get("cursor") {
        Some(t) => {
            let v = super::cursor::decode(t, &filters)?;
            match (
                v.first().and_then(Value::as_str),
                v.get(1).and_then(Value::as_str),
            ) {
                (Some(a), Some(b)) => Some((a.to_owned(), b.to_owned())),
                _ => return Err(ApiError::new(code::CURSOR_INVALID, "cursor is not valid")),
            }
        }
        None => None,
    };
    for k in ["drive_id", "group_id"] {
        if let Some(v) = req.query.get(k)
            && !is_uuid(v)
        {
            return Err(ApiError::bad(&format!("/query/{k}"), "expected a UUID"));
        }
    }
    let mut conn = app.pool.acquire().await?;
    let (items, next) = lifecycle::list(
        &mut conn,
        state,
        req.query.get("drive_id"),
        req.query.get("group_id"),
        after,
        limit,
    )
    .await?;
    let next = next.map(|(ts, id)| super::cursor::encode(&[json!(ts), json!(id)], &filters));
    Ok(json_with(
        200,
        &json!({"items": items, "next_cursor": next}),
        &[],
    ))
}

/// `POST /v1/docs/shares/{sid}:revoke`.
pub async fn revoke(app: &AppState, sid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(sid) {
        return Err(share_not_found(sid));
    }
    let key = idem::key_from(&req.headers)?;
    let route = format!("POST /v1/docs/shares/{sid}:revoke");
    let sha = idem::body_sha256(&req.body);
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, &route, &key, &sha).await? {
        return Ok(json_with(stored.status, &stored.response, &[]));
    }
    let share = lifecycle::revoke(&mut conn, sid, "revoked_by_owner").await?;
    crate::audit::record(
        &mut conn,
        &crate::audit::Entry {
            action: crate::audit::SHARE_REVOKED,
            actor: &req.actor,
            tenant: req.cloud.as_ref().map(|c| c.tenant.as_str()),
            drive_id: share["drive_id"].as_str(),
            share_id: Some(sid),
            agent: req.agent.as_deref(),
            detail: json!({"state": share["state"]}),
        },
    )
    .await?;
    let v = json!({"share": share, "local_removed": false,
                   "notice_code": "DOCS_SHARE_READ_CONTENT_IRREVOCABLE"});
    idem::store(&mut conn, &route, &key, &sha, 200, &v).await?;
    Ok(json_with(200, &v, &[]))
}

/// `POST /v1/docs/shares/{sid}:refresh`.
pub async fn refresh(app: &AppState, sid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(sid) {
        return Err(share_not_found(sid));
    }
    let key = idem::key_from(&req.headers)?;
    let route = format!("POST /v1/docs/shares/{sid}:refresh");
    let sha = idem::body_sha256(&req.body);
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, &route, &key, &sha).await? {
        return Ok(json_with(stored.status, &stored.response, &[]));
    }
    let share = lifecycle::refresh(app, sid, &public_host(req)).await?;
    let mut replay = share.clone();
    for k in ["files", "share_json"] {
        replay.as_object_mut().map(|o| o.remove(k));
    }
    idem::store(&mut conn, &route, &key, &sha, 200, &replay).await?;
    Ok(json_with(200, &share, &[]))
}

/// `POST /v1/docs/shares/{sid}:check`; `?since=<seq>` restricts the entries.
pub async fn check(app: &AppState, sid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(sid) {
        return Err(share_not_found(sid));
    }
    let since = since_of(req)?;
    // A caller with the control token gets the at-checkpoint upgrade by
    // default; one with only a refs credential asks for it (`auto_refresh=true`)
    // when it can mount the new pack itself.
    let auto = req.query.flag("auto_refresh", req.cred.is_none())?;
    let v = crate::share::check::run(app, sid, since, &public_host(req), auto).await?;
    Ok(json_with(200, &v, &[]))
}

fn since_of(req: &Req) -> Result<i64, ApiError> {
    match req.query.get("since") {
        None => Ok(0),
        Some(s) => s
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| ApiError::bad("/query/since", "since is a sequence number")),
    }
}

/// `GET /v1/docs/shares/{sid}/updates?since=<seq>&limit=`.
pub async fn updates(app: &AppState, sid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(sid) {
        return Err(share_not_found(sid));
    }
    let since = since_of(req)?;
    let limit = req.query.limit()?;
    let mut conn = app.pool.acquire().await?;
    if share_value(&mut conn, sid).await?.is_none() {
        return Err(share_not_found(sid));
    }
    let mut items = crate::share::updates::list(&mut conn, sid, since, limit + 1).await?;
    let more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    let next = if more {
        items.last().and_then(|i| i["seq"].as_i64())
    } else {
        None
    };
    Ok(json_with(
        200,
        &json!({"items": items, "next_cursor": next.map(|n| n.to_string())}),
        &[],
    ))
}

/// `POST /v1/docs/share-groups/{gid}:refresh`.
pub async fn group_refresh(
    app: &AppState,
    gid: &str,
    req: &Req,
) -> Result<Response<Body>, ApiError> {
    let key = idem::key_from(&req.headers)?;
    let route = format!("POST /v1/docs/share-groups/{gid}:refresh");
    let sha = idem::body_sha256(&req.body);
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, &route, &key, &sha).await? {
        return Ok(json_with(stored.status, &stored.response, &[]));
    }
    let v = crate::share::group::refresh(app, gid, &public_host(req)).await?;
    let mut replay = v.clone();
    replay.as_object_mut().map(|o| o.remove("refreshed"));
    idem::store(&mut conn, &route, &key, &sha, 200, &replay).await?;
    Ok(json_with(200, &v, &[]))
}

/// `GET /v1/docs/share-groups/{gid}`.
pub async fn group_get(app: &AppState, gid: &str) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let v = crate::share::group::get(&mut conn, gid).await?;
    Ok(json_with(200, &v, &[]))
}
