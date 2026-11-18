//! Agent tokens (D83), local entry:
//!
//! * `POST /v1/docs/agent-tokens` `{name, scopes, drive_ids | all_drives, gw_profile?, label?, expires_in_seconds?}`
//!   issues an `ooa_` token (the only time it is shown); `GET /v1/docs/agent-tokens` lists them (never a token);
//!   `DELETE /v1/docs/agent-tokens/{id}` revokes; `POST /v1/docs/agent-tokens:introspect` `{token}` is what the CLI
//!   proxy asks (control token only); `GET /v1/docs/agent-tokens/self` is what an Agent asks about itself.
//! * `GET /v1/docs/audit/agents?agent&since&until&limit`: every call, refusal, issue and revoke of Agent tokens.
//! * [`guard`]: what an `ooa_` token may call, checked on every route before the handler runs.

use axum::body::Body;
use axum::http::{Method, Response};
use serde_json::{Value, json};
use sqlx::Row;

use super::error::{ApiError, code};
use super::{AppState, Req, http::json_with};
use crate::agent::{self, AgentPrincipal};

fn is_uuid(s: &str) -> bool {
    super::http::is_uuid(s)
}

pub async fn issue(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = super::http::parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = obj.keys().find(|k| {
        ![
            "name",
            "scopes",
            "drive_ids",
            "all_drives",
            "gw_profile",
            "label",
            "expires_in_seconds",
        ]
        .contains(&k.as_str())
    }) {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| agent::valid_name(n))
        .ok_or_else(|| {
            ApiError::bad("/name", "1..=64 of letters, digits, dot, dash, underscore")
        })?;
    let scopes = agent::parse_scopes(obj.get("scopes").unwrap_or(&Value::Null))?;
    let all_drives = match obj.get("all_drives") {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ApiError::bad("/all_drives", "a boolean"))?,
    };
    let drives: Vec<String> = match obj.get("drive_ids") {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .filter(|a| a.len() <= 256)
            .ok_or_else(|| ApiError::bad("/drive_ids", "at most 256 drive ids"))?
            .iter()
            .map(|d| {
                d.as_str()
                    .filter(|d| is_uuid(d))
                    .map(str::to_ascii_lowercase)
            })
            .collect::<Option<_>>()
            .ok_or_else(|| ApiError::bad("/drive_ids", "drive ids are uuids"))?,
    };
    if all_drives && !drives.is_empty() {
        return Err(ApiError::bad(
            "/all_drives",
            "all_drives and drive_ids are exclusive",
        ));
    }
    let docs_scope = scopes.iter().any(|s| s.starts_with("docs:"));
    if docs_scope && !all_drives && drives.is_empty() {
        return Err(ApiError::bad(
            "/drive_ids",
            "a docs scope needs drive_ids or all_drives",
        ));
    }
    if !docs_scope && (all_drives || !drives.is_empty()) {
        return Err(ApiError::bad("/scopes", "drives are only for docs scopes"));
    }
    let db_scope = scopes.iter().any(|s| s == agent::DB_READ);
    let gw_profile = match obj.get("gw_profile") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .filter(|p| agent::valid_name(p))
                .ok_or_else(|| ApiError::bad("/gw_profile", "a profile name"))?,
        ),
    };
    if db_scope != gw_profile.is_some() {
        return Err(ApiError::bad(
            "/gw_profile",
            "db:read needs gw_profile and gw_profile needs db:read",
        ));
    }
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .filter(|l| l.len() <= 200);
    let ttl = match obj.get("expires_in_seconds") {
        None => agent::DEFAULT_TTL,
        Some(n) => n
            .as_i64()
            .filter(|n| (agent::MIN_TTL..=agent::MAX_TTL).contains(n))
            .ok_or_else(|| ApiError::bad("/expires_in_seconds", "60..=2592000"))?,
    };
    let mut conn = app.pool.acquire().await?;
    if !drives.is_empty() {
        let known: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.drive WHERE id = ANY($1::text[]::uuid[]) AND status <> 'deleted'",
        )
        .bind(&drives)
        .fetch_one(&mut *conn)
        .await?;
        if known != drives.len() as i64 {
            return Err(ApiError::new(code::NOT_FOUND, "a drive does not exist"));
        }
    }
    let (token, id) = agent::issue(
        &mut conn,
        &agent::NewAgent {
            name,
            scopes: &scopes,
            drive_ids: &drives,
            all_drives,
            gw_profile,
            label,
            ttl_seconds: ttl,
        },
    )
    .await?;
    crate::audit::record(
        &mut conn,
        &crate::audit::Entry {
            action: crate::audit::AGENT_TOKEN_ISSUED,
            actor: &req.actor,
            tenant: None,
            drive_id: None,
            share_id: None,
            agent: Some(name),
            detail: json!({"token_id": id, "scopes": scopes, "drive_ids": drives, "all_drives": all_drives,
                           "gw_profile": gw_profile, "expires_in_seconds": ttl}),
        },
    )
    .await?;
    let expires_at: String = sqlx::query_scalar(
        "SELECT to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
           FROM _oo_docs.cloud_token WHERE id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(json_with(
        201,
        &json!({"id": id, "token": token, "name": name, "scopes": scopes, "drive_ids": drives,
                "all_drives": all_drives, "gw_profile": gw_profile, "expires_in_seconds": ttl,
                "expires_at": expires_at}),
        &[],
    ))
}

pub async fn list(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let all = req.query.get("state") == Some("all");
    let mut conn = app.pool.acquire().await?;
    let items = agent::list(&mut conn, all).await?;
    Ok(json_with(200, &json!({"items": items}), &[]))
}

pub async fn revoke(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(ApiError::new(code::NOT_FOUND, "no such token"));
    }
    let mut conn = app.pool.acquire().await?;
    let name = agent::name_of(&mut conn, id).await?;
    if !agent::revoke(&mut conn, id).await? {
        return Err(ApiError::new(
            code::NOT_FOUND,
            "no live agent token with that id",
        ));
    }
    crate::audit::record(
        &mut conn,
        &crate::audit::Entry {
            action: crate::audit::AGENT_TOKEN_REVOKED,
            actor: &req.actor,
            tenant: None,
            drive_id: None,
            share_id: None,
            agent: name.as_deref(),
            detail: json!({"token_id": id}),
        },
    )
    .await?;
    Ok(json_with(200, &json!({"id": id, "revoked": true}), &[]))
}

/// `POST /v1/docs/agent-tokens:introspect` `{token}`: 200 with `active` either way (the proxy decides), the
/// principal when active. Does not count as a use of the token.
pub async fn introspect(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = super::http::parse_json(&req.body)?;
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| t.len() <= 200)
        .ok_or_else(|| ApiError::bad("/token", "a token"))?;
    let mut conn = app.pool.acquire().await?;
    match agent::authenticate(&mut conn, token, false).await {
        Ok(p) => {
            let mut v = p.to_json();
            v["active"] = json!(true);
            Ok(json_with(200, &v, &[]))
        }
        Err(e) if e.status == 401 => {
            let reason = e.details["reason"].as_str().unwrap_or("unknown").to_owned();
            Ok(json_with(
                200,
                &json!({"active": false, "reason": reason}),
                &[],
            ))
        }
        Err(e) => Err(e),
    }
}

/// `GET /v1/docs/agent-tokens/self`: the token as the engine sees it, and the Drives it reaches.
pub async fn myself(app: &AppState, p: &AgentPrincipal) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let rows = sqlx::query(
        "SELECT id::text AS id, name, backend, status FROM _oo_docs.drive \
          WHERE status <> 'deleted' AND ($1 OR id = ANY($2::text[]::uuid[])) ORDER BY name, id",
    )
    .bind(p.all_drives)
    .bind(&p.drive_ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut drives = Vec::new();
    for r in rows {
        drives.push(json!({"id": r.try_get::<String, _>("id")?, "name": r.try_get::<String, _>("name")?,
                           "backend": r.try_get::<String, _>("backend")?, "status": r.try_get::<String, _>("status")?}));
    }
    let mut v = p.to_json();
    v["drives"] = Value::Array(drives);
    Ok(json_with(200, &v, &[]))
}

/// `GET /v1/docs/audit/agents`.
pub async fn audit(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let q = &req.query;
    let limit = q
        .get("limit")
        .map(|l| l.parse::<i64>().ok().filter(|n| (1..=1000).contains(n)))
        .unwrap_or(Some(200))
        .ok_or_else(|| ApiError::bad("/query/limit", "1..=1000"))?;
    let mut conn = app.pool.acquire().await?;
    let rows = sqlx::query(
        "SELECT seq, at::text AS at, action, actor, agent, drive_id::text AS drive, detail::text AS detail \
           FROM _oo_docs.audit_log \
          WHERE action IN ('agent_token.issued', 'agent_token.revoked', 'agent.read', 'agent.denied') \
            AND ($1::text IS NULL OR agent = $1) \
            AND ($2::timestamptz IS NULL OR at >= $2::timestamptz) \
            AND ($3::timestamptz IS NULL OR at < $3::timestamptz) \
          ORDER BY seq DESC LIMIT $4",
    )
    .bind(q.get("agent"))
    .bind(q.get("since"))
    .bind(q.get("until"))
    .bind(limit)
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| ApiError::bad("/query", "since and until are timestamps"))?;
    let mut items = Vec::new();
    for r in rows {
        let detail: Value =
            serde_json::from_str(&r.try_get::<String, _>("detail")?).unwrap_or(Value::Null);
        items.push(json!({"seq": r.try_get::<i64, _>("seq")?, "at": r.try_get::<String, _>("at")?,
            "action": r.try_get::<String, _>("action")?, "actor": r.try_get::<String, _>("actor")?,
            "agent": r.try_get::<Option<String>, _>("agent")?, "drive_id": r.try_get::<Option<String>, _>("drive")?,
            "detail": detail}));
    }
    Ok(json_with(200, &json!({"items": items}), &[]))
}

/// The path of a call as the audit keeps it: the words of the API, every value (key, id, name) as `{id}`.
pub fn audit_route(path: &str) -> String {
    const WORDS: [&str; 22] = [
        "v1",
        "docs",
        "entities",
        "resolve",
        "files",
        "blocks",
        "content",
        "drives",
        "tree",
        "links",
        "links:decide",
        "suggestions",
        "ontology",
        "versions",
        "agent-tokens",
        "self",
        "audit",
        "agents",
        "shares",
        "review",
        "config",
        "model",
    ];
    path.trim_start_matches('/')
        .split('/')
        .map(|s| if WORDS.contains(&s) { s } else { "{id}" })
        .collect::<Vec<_>>()
        .join("/")
        .replace("v1/docs", "/v1/docs")
}

/// The refusal of the guard: 403, with the reason in the details.
fn denied(why: &'static str) -> ApiError {
    ApiError::new(
        code::SHARE_REFS_OUT_OF_SCOPE,
        "an agent token cannot do this",
    )
    .details(json!({"reason": why}))
}

/// What the call needs of the Agent token, or why it is refused. Returns the Drive the call names (for the audit
/// row), if any. Everything that is not listed here is refused: an Agent token starts with nothing.
pub async fn guard(
    app: &AppState,
    p: &AgentPrincipal,
    req: &Req,
) -> Result<Option<String>, ApiError> {
    let segs: Vec<&str> = req.path.trim_start_matches('/').split('/').collect();
    let need = |scope: &str| {
        if p.has_scope(scope) {
            Ok(())
        } else {
            Err(denied("scope"))
        }
    };
    let drive_ok = |d: &str| -> Result<(), ApiError> {
        if p.allows_drive(d) {
            Ok(())
        } else {
            Err(denied("drive"))
        }
    };
    let body = || -> Value { serde_json::from_slice(&req.body).unwrap_or(Value::Null) };
    let mut conn = app.pool.acquire().await?;
    match (&req.method, segs.as_slice()) {
        (&Method::GET, ["v1", "docs", "agent-tokens", "self"]) => Ok(None),
        (&Method::GET, ["v1", "docs", "ontology", "versions"]) => {
            need(agent::DOCS_READ)?;
            Ok(None)
        }
        (&Method::GET, ["v1", "docs", "entities"]) => {
            need(agent::DOCS_READ)?;
            match req.query.get("drive_id").filter(|d| !d.is_empty()) {
                Some(d) => {
                    drive_ok(d)?;
                    Ok(Some(d.to_ascii_lowercase()))
                }
                None if p.all_drives => Ok(None),
                None => Err(denied("drive_id_required")),
            }
        }
        (&Method::GET, ["v1", "docs", "entities", eid]) => {
            need(agent::DOCS_READ)?;
            if !p.all_drives && !entity_in_drives(&mut conn, eid, &p.drive_ids).await? {
                return Err(ApiError::not_found("entity"));
            }
            Ok(None)
        }
        (&Method::POST, ["v1", "docs", "resolve"]) => {
            need(agent::DOCS_READ)?;
            match body()["drive_id"].as_str() {
                Some(d) => {
                    drive_ok(d)?;
                    Ok(Some(d.to_ascii_lowercase()))
                }
                None => Ok(None),
            }
        }
        (&Method::GET, ["v1", "docs", "files", fid, ..]) if is_uuid(fid) => {
            need(agent::DOCS_READ)?;
            let drive: Option<String> =
                sqlx::query_scalar("SELECT drive_id::text FROM _oo_docs.file WHERE id = $1::uuid")
                    .bind(*fid)
                    .fetch_optional(&mut *conn)
                    .await?;
            match drive {
                Some(d) if p.allows_drive(&d) => Ok(Some(d)),
                _ => Err(ApiError::not_found("file")),
            }
        }
        (&Method::GET, ["v1", "docs", "drives", id])
        | (&Method::GET, ["v1", "docs", "drives", id, "files" | "tree"])
            if is_uuid(id) =>
        {
            need(agent::DOCS_READ)?;
            if p.allows_drive(id) {
                Ok(Some(id.to_ascii_lowercase()))
            } else {
                Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found")
                    .details(json!({ "drive_id": id })))
            }
        }
        (&Method::GET, ["v1", "docs", "links"]) => {
            need(agent::DOCS_READ)?;
            match req.query.get("drive_id").filter(|d| !d.is_empty()) {
                Some(d) => {
                    drive_ok(d)?;
                    Ok(Some(d.to_ascii_lowercase()))
                }
                None => Ok(None),
            }
        }
        (&Method::POST, ["v1", "docs", "links:decide"]) => {
            need(agent::DOCS_WRITE)?;
            let d = body()["drive_id"].as_str().map(str::to_owned);
            let d = d.ok_or_else(|| ApiError::bad("/drive_id", "a drive id"))?;
            drive_ok(&d)?;
            Ok(Some(d.to_ascii_lowercase()))
        }
        (&Method::POST, ["v1", "docs", "suggestions"]) => {
            need(agent::DOCS_SUGGEST)?;
            let share = body()["share_id"].as_str().map(str::to_owned);
            let drive: Option<String> = match share {
                Some(s) if is_uuid(&s) => {
                    sqlx::query_scalar(
                        "SELECT drive_id::text FROM _oo_docs.share WHERE id = $1::uuid",
                    )
                    .bind(s)
                    .fetch_optional(&mut *conn)
                    .await?
                }
                _ => None,
            };
            match drive {
                Some(d) if p.allows_drive(&d) => Ok(Some(d)),
                _ => Err(denied("drive")),
            }
        }
        _ => Err(denied("route")),
    }
}

/// Does the entity (id or key, redirects followed) have evidence in one of `drives`? A key that is not visible
/// there is as good as unknown.
pub async fn entity_in_drives(
    conn: &mut sqlx::PgConnection,
    eid: &str,
    drives: &[String],
) -> Result<bool, ApiError> {
    let by_key = !is_uuid(eid);
    let sql = "SELECT EXISTS (SELECT 1 FROM _oo_docs.entity e \
                 JOIN _oo_docs.evidence ev ON ev.target_kind = 'entity' AND ev.target_id = e.id AND ev.withdrawn_at IS NULL \
                 JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
                 JOIN _oo_docs.file f ON f.id = fv.file_id \
                WHERE f.drive_id = ANY($2::text[]::uuid[]) AND ";
    let cond = if by_key {
        "e.key = COALESCE((SELECT r.to_key FROM _oo_docs.entity_redirect r WHERE r.from_key = $1), $1))"
    } else {
        "e.id = $1::uuid)"
    };
    let found: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("{sql}{cond}")))
        .bind(eid)
        .bind(drives)
        .fetch_one(&mut *conn)
        .await?;
    Ok(found)
}

/// Keys among `keys` that have evidence in one of `drives` (the resolve route filters its candidates with it).
pub async fn keys_in_drives(
    conn: &mut sqlx::PgConnection,
    keys: &[String],
    drives: &[String],
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT e.key FROM _oo_docs.entity e \
           JOIN _oo_docs.evidence ev ON ev.target_kind = 'entity' AND ev.target_id = e.id AND ev.withdrawn_at IS NULL \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
           JOIN _oo_docs.file f ON f.id = fv.file_id \
          WHERE e.key = ANY($1::text[]) AND f.drive_id = ANY($2::text[]::uuid[])",
    )
    .bind(keys)
    .bind(drives)
    .fetch_all(&mut *conn)
    .await
}
