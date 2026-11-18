//! Drive resources and `:sync` (10 §8.3, G11).

use std::collections::{BTreeMap, HashMap};

use axum::body::Body;
use axum::http::Response;
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Row};

use super::cursor;
use super::error::{ApiError, code};
use super::freshness;
use super::http::{FMT, etag, if_match, is_uuid, json_with};
use super::idem;
use super::{AppState, Req};
use crate::ingest::reconcile::{EnqueuedSync, SYNC_JSON, enqueue_sync};
use crate::ingest::{IngestError, SyncFlavor, Trigger};
use crate::jobs::JobKind;
use crate::store::{events, jobs as store};

const BACKENDS: [&str; 5] = ["fs", "s3", "oss", "cos", "r2"];

/// The Drive's freshness token now (D54); sync responses carry it so a
/// caller can ask for a pack that covers at least this.
async fn current_rev(conn: &mut PgConnection, id: &str) -> Result<i64, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT rev FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .unwrap_or(0),
    )
}

fn drive_not_found(id: &str) -> ApiError {
    ApiError::new(code::DRIVE_NOT_FOUND, "drive not found").details(json!({ "drive_id": id }))
}

/// `Drive` objects for ids, in the order given; missing ids are skipped.
pub async fn drive_values(conn: &mut PgConnection, ids: &[String]) -> Result<Vec<Value>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let base = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT id::text AS id, name, backend, config::text AS config, status, status_code, \
         model_enabled, llm_daily_calls, revision, rev, \
         to_char(created_at AT TIME ZONE 'UTC', {FMT}) AS created_at \
         FROM _oo_docs.drive WHERE id = ANY($1::text[]::uuid[])"
    )))
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let counts_rows = sqlx::query(
        "SELECT drive_id::text AS id, state, count(*)::bigint AS n, \
         COALESCE(sum(pending_review), 0)::bigint AS pending, COALESCE(sum(fp_size), 0)::bigint AS bytes \
         FROM _oo_docs.file WHERE drive_id = ANY($1::text[]::uuid[]) AND deleted_at IS NULL \
         GROUP BY drive_id, state",
    )
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut counts: HashMap<String, (BTreeMap<String, i64>, i64, i64)> = HashMap::new();
    for r in counts_rows {
        let id: String = r.try_get("id")?;
        let e = counts.entry(id).or_default();
        let n: i64 = r.try_get("n")?;
        e.0.insert(r.try_get("state")?, n);
        e.1 += r.try_get::<i64, _>("pending")?;
        e.2 += r.try_get::<i64, _>("bytes")?;
    }
    let sync_rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT DISTINCT ON (drive_id) drive_id::text AS id, ({SYNC_JSON})::text AS j \
         FROM _oo_docs.drive_sync WHERE drive_id = ANY($1::text[]::uuid[]) \
         ORDER BY drive_id, generation DESC"
    )))
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut last_sync: HashMap<String, Value> = HashMap::new();
    for r in sync_rows {
        let j: String = r.try_get("j")?;
        last_sync.insert(
            r.try_get("id")?,
            serde_json::from_str(&j).unwrap_or(Value::Null),
        );
    }
    let mut fresh = freshness::compute(conn, ids).await?;

    let mut by_id: HashMap<String, Value> = HashMap::new();
    for r in base {
        let id: String = r.try_get("id")?;
        let config: String = r.try_get("config")?;
        let (by_state, pending, bytes) = counts.remove(&id).unwrap_or_default();
        let total: i64 = by_state.values().sum();
        let mut v = json!({
            "id": id,
            "name": r.try_get::<String, _>("name")?,
            "backend": r.try_get::<String, _>("backend")?,
            "config": serde_json::from_str::<Value>(&config).unwrap_or(Value::Null),
            "status": r.try_get::<String, _>("status")?,
            "status_code": r.try_get::<Option<String>, _>("status_code")?,
            "model_enabled": r.try_get::<bool, _>("model_enabled")?,
            "llm_daily_calls": r.try_get::<i32, _>("llm_daily_calls")?,
            "revision": r.try_get::<i64, _>("revision")?,
            "drive_rev": r.try_get::<i64, _>("rev")?,
            "counts": by_state,
            "pending_review": pending,
            "files_total": total,
            "bytes_total": bytes,
            "created_at": r.try_get::<String, _>("created_at")?,
        });
        if let Some(s) = last_sync.remove(&id) {
            v["last_sync"] = s;
        }
        if let Some(f) = fresh.remove(&id) {
            v["freshness"] = f;
        }
        by_id.insert(id, v);
    }
    Ok(ids.iter().filter_map(|i| by_id.remove(i)).collect())
}

async fn one(conn: &mut PgConnection, id: &str) -> Result<Value, ApiError> {
    drive_values(conn, &[id.to_owned()])
        .await?
        .pop()
        .ok_or_else(|| drive_not_found(id))
}

fn revision_of(v: &Value) -> i64 {
    v["revision"].as_i64().unwrap_or(1)
}

// ---------------------------------------------------------------------------
// list / get
// ---------------------------------------------------------------------------

/// `GET /drives`.
pub async fn list(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let limit = req.query.limit()?;
    let filters = "drives";
    let after = match req.query.get("cursor") {
        Some(t) => {
            let k = cursor::decode(t, filters)?;
            match (
                k.first().and_then(Value::as_str),
                k.get(1).and_then(Value::as_str),
            ) {
                (Some(a), Some(b)) => Some((a.to_owned(), b.to_owned())),
                _ => return Err(ApiError::new(code::CURSOR_INVALID, "cursor shape")),
            }
        }
        None => None,
    };
    let mut conn = app.pool.acquire().await?;
    let as_of = super::files::head_seq(&mut conn).await?;
    let mut sql = format!(
        "SELECT id::text AS id, to_char(created_at AT TIME ZONE 'UTC', {FMT}) AS k \
         FROM _oo_docs.drive WHERE status <> 'deleted'"
    );
    if after.is_some() {
        sql.push_str(" AND (created_at, id) > ($1::timestamptz, $2::uuid)");
    }
    sql.push_str(&format!(" ORDER BY created_at, id LIMIT {}", limit + 1));
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    if let Some((k, i)) = &after {
        q = q.bind(k).bind(i);
    }
    let rows = q.fetch_all(&mut *conn).await?;
    let mut ids = Vec::new();
    let mut last: Option<(String, String)> = None;
    for r in rows.iter().take(limit as usize) {
        let id: String = r.try_get("id")?;
        last = Some((r.try_get("k")?, id.clone()));
        ids.push(id);
    }
    let next = if rows.len() as i64 > limit {
        last.map(|(k, i)| cursor::encode(&[json!(k), json!(i)], filters))
    } else {
        None
    };
    let items = drive_values(&mut conn, &ids).await?;
    Ok(super::http::json(
        200,
        &json!({ "items": items, "next_cursor": next, "as_of_seq": as_of }),
    ))
}

/// `GET /drives/{id}`.
pub async fn get(app: &AppState, id: &str) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(drive_not_found(id));
    }
    let mut conn = app.pool.acquire().await?;
    let v = one(&mut conn, id).await?;
    if v["status"] == "deleted" {
        return Err(drive_not_found(id));
    }
    Ok(json_with(200, &v, &[("etag", etag(revision_of(&v)))]))
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

struct Create {
    name: String,
    backend: String,
    config: Value,
    model_enabled: bool,
    llm_daily_calls: i64,
}

fn only_keys(obj: &Map<String, Value>, allowed: &[&str], pointer: &str) -> Result<(), ApiError> {
    for k in obj.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(ApiError::bad(&format!("{pointer}/{k}"), "unknown property"));
        }
    }
    Ok(())
}

fn str_field<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
    min: usize,
    max: usize,
) -> Result<Option<&'a str>, ApiError> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => {
            let n = s.chars().count();
            if n < min || n > max {
                return Err(ApiError::bad(
                    &format!("{pointer}/{key}"),
                    format!("length must be {min} to {max}"),
                ));
            }
            Ok(Some(s))
        }
        Some(_) => Err(ApiError::bad(
            &format!("{pointer}/{key}"),
            "expected a string",
        )),
    }
}

fn int_field(
    obj: &Map<String, Value>,
    key: &str,
    pointer: &str,
    min: i64,
    max: i64,
) -> Result<Option<i64>, ApiError> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => match v.as_i64() {
            Some(n) if (min..=max).contains(&n) => Ok(Some(n)),
            _ => Err(ApiError::bad(
                &format!("{pointer}/{key}"),
                format!("expected an integer {min} to {max}"),
            )),
        },
    }
}

fn bool_field(
    obj: &Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<Option<bool>, ApiError> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ApiError::bad(
            &format!("{pointer}/{key}"),
            "expected a boolean",
        )),
    }
}

fn exclude_field(v: &Value, pointer: &str) -> Result<Vec<String>, ApiError> {
    let arr = v
        .as_array()
        .ok_or_else(|| ApiError::bad(pointer, "expected an array"))?;
    if arr.len() > 64 {
        return Err(ApiError::bad(pointer, "at most 64 patterns"));
    }
    arr.iter()
        .enumerate()
        .map(|(i, e)| match e.as_str() {
            Some(s) if s.chars().count() <= 256 => Ok(s.to_owned()),
            _ => Err(ApiError::bad(
                &format!("{pointer}/{i}"),
                "expected a string of at most 256 characters",
            )),
        })
        .collect()
}

fn review_field(obj: &Map<String, Value>) -> Result<(), ApiError> {
    if let Some(r) = obj.get("review") {
        let o = r
            .as_object()
            .ok_or_else(|| ApiError::bad("/review", "expected an object"))?;
        only_keys(o, &["auto_publish_bp"], "/review")?;
        int_field(o, "auto_publish_bp", "/review", 5000, 10000)?;
    }
    Ok(())
}

fn fs_config(cfg: &Map<String, Value>) -> Result<Value, ApiError> {
    only_keys(cfg, &["root", "exclude"], "/config")?;
    let root = str_field(cfg, "root", "/config", 1, 4096)?
        .ok_or_else(|| ApiError::bad("/config/root", "required"))?;
    if !root.starts_with('/') {
        return Err(ApiError::bad("/config/root", "must be an absolute path"));
    }
    let mut config = json!({ "root": root });
    if let Some(e) = cfg.get("exclude") {
        config["exclude"] = json!(exclude_field(e, "/config/exclude")?);
    }
    Ok(config)
}

/// `s3` Drives: bucket, prefix, endpoint, region, path_style, exclude and a
/// `secret_ref`. Keys and secrets are never accepted inline (10 section 9).
#[cfg(feature = "objectstore-s3")]
fn s3_config(cfg: &Map<String, Value>) -> Result<Value, ApiError> {
    only_keys(
        cfg,
        &[
            "bucket",
            "prefix",
            "endpoint",
            "region",
            "path_style",
            "secret_ref",
            "exclude",
        ],
        "/config",
    )?;
    let bucket = str_field(cfg, "bucket", "/config", 1, 255)?
        .ok_or_else(|| ApiError::bad("/config/bucket", "required"))?;
    let reference = str_field(cfg, "secret_ref", "/config", 1, 2048)?
        .ok_or_else(|| ApiError::bad("/config/secret_ref", "required"))?;
    if !oo_connectors::objectstore_s3::opendal::is_secret_ref(reference) {
        return Err(ApiError::bad(
            "/config/secret_ref",
            "not a valid secret reference",
        ));
    }
    let mut config = json!({ "bucket": bucket, "secret_ref": reference });
    for key in ["prefix", "endpoint", "region"] {
        if let Some(v) = str_field(cfg, key, "/config", 0, 1024)? {
            config[key] = json!(v);
        }
    }
    if let Some(v) = bool_field(cfg, "path_style", "/config")? {
        config["path_style"] = json!(v);
    }
    if let Some(e) = cfg.get("exclude") {
        config["exclude"] = json!(exclude_field(e, "/config/exclude")?);
    }
    Ok(config)
}

fn parse_create(body: &Value) -> Result<Create, ApiError> {
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    only_keys(
        obj,
        &[
            "name",
            "backend",
            "config",
            "model_enabled",
            "llm_daily_calls",
            "review",
        ],
        "",
    )?;
    let name = str_field(obj, "name", "", 1, 128)?
        .ok_or_else(|| ApiError::bad("/name", "required"))?
        .to_owned();
    let backend = str_field(obj, "backend", "", 1, 16)?
        .ok_or_else(|| ApiError::bad("/backend", "required"))?;
    if !BACKENDS.contains(&backend) {
        return Err(ApiError::bad("/backend", "unknown backend"));
    }
    let cfg = obj
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::bad("/config", "required object"))?;
    let config = match backend {
        "fs" => fs_config(cfg)?,
        #[cfg(feature = "objectstore-s3")]
        "s3" => s3_config(cfg)?,
        _ => {
            return Err(ApiError::new(
                code::INVALID_CONFIG,
                "this engine build does not serve that backend",
            )
            .details(json!({ "pointer": "/backend" })));
        }
    };
    review_field(obj)?;
    Ok(Create {
        name,
        backend: backend.to_owned(),
        config,
        model_enabled: bool_field(obj, "model_enabled", "")?.unwrap_or(false),
        llm_daily_calls: int_field(obj, "llm_daily_calls", "", 0, 1_000_000)?.unwrap_or(2000),
    })
}

/// `POST /drives`.
pub async fn create(app: &AppState, req: &Req) -> Result<Response<Body>, ApiError> {
    let key = idem::key_from(&req.headers)?;
    let route = "POST /v1/docs/drives";
    let sha = idem::body_sha256(&req.body);
    let body = super::http::parse_json(&req.body)?;
    let mut tx = app.pool.begin().await?;
    if let Some(stored) = idem::lookup(&mut tx, route, &key, &sha).await? {
        return Ok(replayed(stored, "/v1/docs/drives"));
    }
    let c = parse_create(&body)?;
    app.sync.probe(&c.backend, &c.config).map_err(|e| {
        let (c, msg) = match e.code() {
            "DOCS_SOURCE_AUTH_FAILED" => (code::ROOT_MISSING, "source refused access"),
            _ => (
                code::ROOT_MISSING,
                "source root is missing or not a directory",
            ),
        };
        ApiError::new(c, msg).details(json!({ "pointer": "/config/root" }))
    })?;
    let id: Result<String, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO _oo_docs.drive (id, name, backend, config, model_enabled, llm_daily_calls) \
         VALUES (gen_random_uuid(), $1, $5, $2::jsonb, $3, $4::integer) RETURNING id::text",
    )
    .bind(&c.name)
    .bind(c.config.to_string())
    .bind(c.model_enabled)
    .bind(c.llm_daily_calls)
    .bind(&c.backend)
    .fetch_one(&mut *tx)
    .await;
    let id = match id {
        Ok(id) => id,
        Err(e) if is_unique(&e) => {
            drop(tx);
            let mut conn = app.pool.acquire().await?;
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT id::text FROM _oo_docs.drive WHERE lower(name) = lower($1) AND status <> 'deleted'",
            )
            .bind(&c.name)
            .fetch_optional(&mut *conn)
            .await?;
            return Err(
                ApiError::new(code::DRIVE_EXISTS, "a drive with this name exists")
                    .details(json!({ "drive_id": existing })),
            );
        }
        Err(e) => return Err(e.into()),
    };
    let drive = one(&mut tx, &id).await?;
    let payload = json!({ "drive": drive }).to_string();
    events::append_event(&mut tx, "drive.created", Some(&id), Some(&id), &payload).await?;
    idem::store(&mut tx, route, &key, &sha, 201, &drive).await?;
    tx.commit().await?;
    // First reconcile right away; the poller would pick it up within a tick.
    let flavor = SyncFlavor {
        trigger: Trigger::Startup,
        ..SyncFlavor::POLL
    };
    if let Ok(EnqueuedSync::Enqueued { .. }) = enqueue_sync(&app.pool, &id, flavor).await {
        app.wake.notify_waiters();
    }
    Ok(json_with(
        201,
        &drive,
        &[
            ("location", format!("/v1/docs/drives/{id}")),
            ("etag", etag(revision_of(&drive))),
        ],
    ))
}

fn is_unique(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code().map(|c| c == "23505"))
        .unwrap_or(false)
}

fn replayed(stored: idem::Stored, collection: &str) -> Response<Body> {
    let mut headers = vec![("oo-idempotent-replay", "true".to_owned())];
    if stored.status == 201
        && let Some(id) = stored.response["id"].as_str()
    {
        headers.push(("location", format!("{collection}/{id}")));
    }
    if let Some(rev) = stored.response["revision"].as_i64() {
        headers.push(("etag", etag(rev)));
    }
    json_with(stored.status, &stored.response, &headers)
}

// ---------------------------------------------------------------------------
// patch / delete
// ---------------------------------------------------------------------------

async fn lock_drive(conn: &mut PgConnection, id: &str) -> Result<(i64, String, Value), ApiError> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT revision, status, config::text FROM _oo_docs.drive WHERE id = $1::uuid FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let (rev, status, config) = row.ok_or_else(|| drive_not_found(id))?;
    if status == "deleted" {
        return Err(drive_not_found(id));
    }
    Ok((
        rev,
        status,
        serde_json::from_str(&config).unwrap_or(Value::Null),
    ))
}

fn precondition_failed(current: i64) -> ApiError {
    ApiError::new(code::PRECONDITION_FAILED, "revision does not match")
        .details(json!({ "current_revision": current }))
}

/// `PATCH /drives/{id}`.
pub async fn patch(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(drive_not_found(id));
    }
    let want = if_match(&req.headers)?;
    let body = super::http::parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    only_keys(
        obj,
        &[
            "name",
            "model_enabled",
            "llm_daily_calls",
            "paused",
            "exclude",
            "review",
        ],
        "",
    )?;
    if obj.is_empty() {
        return Err(ApiError::bad("", "at least one property is required"));
    }
    let name = str_field(obj, "name", "", 1, 128)?;
    let model_enabled = bool_field(obj, "model_enabled", "")?;
    let calls = int_field(obj, "llm_daily_calls", "", 0, 1_000_000)?;
    let paused = bool_field(obj, "paused", "")?;
    let exclude = match obj.get("exclude") {
        Some(e) => Some(exclude_field(e, "/exclude")?),
        None => None,
    };
    review_field(obj)?;

    let mut tx = app.pool.begin().await?;
    let (rev, status, mut config) = lock_drive(&mut tx, id).await?;
    if rev != want {
        return Err(precondition_failed(rev));
    }
    if status == "deleting" {
        return Err(drive_not_found(id));
    }
    let new_status = match paused {
        Some(true) => "paused",
        Some(false) => "active",
        None => status.as_str(),
    };
    if let Some(e) = exclude {
        config["exclude"] = json!(e);
    }
    let r = sqlx::query(
        "UPDATE _oo_docs.drive SET name = COALESCE($2, name), \
         model_enabled = COALESCE($3, model_enabled), \
         llm_daily_calls = COALESCE($4::integer, llm_daily_calls), status = $5, config = $6::jsonb, \
         revision = revision + 1, updated_at = clock_timestamp() WHERE id = $1::uuid",
    )
    .bind(id)
    .bind(name)
    .bind(model_enabled)
    .bind(calls)
    .bind(new_status)
    .bind(config.to_string())
    .execute(&mut *tx)
    .await;
    if let Err(e) = r {
        return Err(if is_unique(&e) {
            ApiError::new(code::DRIVE_EXISTS, "a drive with this name exists")
        } else {
            e.into()
        });
    }
    let drive = one(&mut tx, id).await?;
    let payload = json!({ "drive": drive }).to_string();
    events::append_event(&mut tx, "drive.updated", Some(id), Some(id), &payload).await?;
    tx.commit().await?;
    Ok(json_with(
        200,
        &drive,
        &[("etag", etag(revision_of(&drive)))],
    ))
}

/// `DELETE /drives/{id}?purge=`: `202`, status `deleting`; the reaper finishes it.
pub async fn delete(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(drive_not_found(id));
    }
    let want = if_match(&req.headers)?;
    let _purge = req.query.flag("purge", false)?;
    let mut tx = app.pool.begin().await?;
    let (rev, status, _) = lock_drive(&mut tx, id).await?;
    if rev != want {
        return Err(precondition_failed(rev));
    }
    if status == "deleting" {
        let v = one(&mut tx, id).await?;
        return Ok(json_with(202, &v, &[("etag", etag(revision_of(&v)))]));
    }
    let shares: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.share WHERE drive_id = $1::uuid \
         AND status IN ('active', 'drifted') ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    if !shares.is_empty() {
        return Err(ApiError::new(
            code::DRIVE_HAS_ACTIVE_SHARES,
            "revoke the active shares first",
        )
        .details(json!({ "share_ids": shares })));
    }
    // Pending work for this drive is pointless now (4.1.5).
    let jobs: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.job WHERE drive_id = $1::uuid \
         AND state IN ('queued', 'running') ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for job in &jobs {
        store::cancel(&mut tx, job).await?;
    }
    sqlx::query(
        "UPDATE _oo_docs.drive SET status = 'deleting', status_code = NULL, revision = revision + 1, \
         updated_at = clock_timestamp() WHERE id = $1::uuid",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let drive = one(&mut tx, id).await?;
    let payload = json!({ "drive": drive }).to_string();
    events::append_event(&mut tx, "drive.updated", Some(id), Some(id), &payload).await?;
    tx.commit().await?;
    app.wake.notify_waiters();
    Ok(json_with(
        202,
        &drive,
        &[("etag", etag(revision_of(&drive)))],
    ))
}

// ---------------------------------------------------------------------------
// :sync
// ---------------------------------------------------------------------------

struct SyncReq {
    confirm: Option<i64>,
    deep: bool,
    paths: Vec<String>,
}

fn valid_drive_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 1024
        && !p.starts_with('/')
        && !p.contains("//")
        && !p.split('/').any(|s| s == "." || s == "..")
}

fn parse_sync(body: &Value) -> Result<SyncReq, ApiError> {
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    only_keys(obj, &["confirm_mass_delete", "deep", "paths"], "")?;
    let confirm = match obj.get("confirm_mass_delete") {
        None => None,
        Some(v) => Some(
            v.as_i64()
                .filter(|n| *n >= 1)
                .ok_or_else(|| ApiError::bad("/confirm_mass_delete", "expected a generation"))?,
        ),
    };
    let deep = bool_field(obj, "deep", "")?.unwrap_or(false);
    let mut paths = Vec::new();
    if let Some(p) = obj.get("paths") {
        let arr = p
            .as_array()
            .ok_or_else(|| ApiError::bad("/paths", "expected an array"))?;
        if arr.len() > 500 {
            return Err(ApiError::bad("/paths", "at most 500 paths"));
        }
        for (i, e) in arr.iter().enumerate() {
            match e.as_str() {
                Some(s) if valid_drive_path(s) => paths.push(s.to_owned()),
                _ => return Err(ApiError::bad(&format!("/paths/{i}"), "invalid drive path")),
            }
        }
    }
    if !paths.is_empty() && (confirm.is_some() || deep) {
        return Err(ApiError::bad(
            "/paths",
            "a targeted sync cannot be combined with deep or confirm_mass_delete",
        ));
    }
    Ok(SyncReq {
        confirm,
        deep,
        paths,
    })
}

async fn now_text(conn: &mut PgConnection) -> Result<String, ApiError> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', {FMT})"
    )))
    .fetch_one(&mut *conn)
    .await?)
}

/// `POST /drives/{id}:sync`.
pub async fn sync(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(drive_not_found(id));
    }
    let key = idem::key_from(&req.headers)?;
    let route = format!("POST /v1/docs/drives/{id}:sync");
    let sha = idem::body_sha256(&req.body);
    let body = super::http::parse_json(&req.body)?;
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, &route, &key, &sha).await? {
        return Ok(replayed(stored, "/v1/docs/drives"));
    }
    let sr = parse_sync(&body)?;
    let drive = crate::ingest::reconcile::load_drive(&app.pool, id)
        .await?
        .filter(|d| d.status != "deleted")
        .ok_or_else(|| drive_not_found(id))?;
    if drive.status != "active" {
        return Err(
            ApiError::new(code::INVALID_CONFIG, "the drive is not active")
                .details(json!({ "status": drive.status })),
        );
    }

    let info = if !sr.paths.is_empty() {
        let started = now_text(&mut conn).await?;
        let report = app
            .sync
            .targeted(id, sr.paths.clone())
            .await
            .map_err(|e| match e {
                IngestError::DriveNotFound => drive_not_found(id),
                other => ApiError::from(other),
            })?;
        let finished = now_text(&mut conn).await?;
        json!({
            "generation": drive.sync_generation,
            "status": "complete",
            "trigger": "manual",
            "added": report.added,
            "changed": report.changed,
            "removed": 0,
            "renamed": 0,
            "error_code": null,
            "started_at": started,
            "finished_at": finished,
            "targeted": true,
            "maybe_deleted": report.maybe_deleted,
            "drive_rev": current_rev(&mut conn, id).await?,
        })
    } else {
        let flavor = SyncFlavor {
            trigger: Trigger::Manual,
            deep: sr.deep,
            confirm: sr.confirm,
        };
        match enqueue_sync(&app.pool, id, flavor).await {
            Ok(EnqueuedSync::Enqueued { generation, .. }) => {
                app.wake.notify_waiters();
                json!({
                    "generation": generation,
                    "status": "running",
                    "trigger": "manual",
                    "added": 0,
                    "changed": 0,
                    "removed": 0,
                    "renamed": 0,
                    "error_code": null,
                    "started_at": now_text(&mut conn).await?,
                    "finished_at": null,
                    "drive_rev": current_rev(&mut conn, id).await?,
                })
            }
            Ok(EnqueuedSync::Running { generation }) => {
                return Err(
                    ApiError::new(code::SYNC_RUNNING, "a sync is already running")
                        .details(json!({ "generation": generation })),
                );
            }
            Ok(EnqueuedSync::Held {
                generation,
                would_delete,
            }) => {
                return Err(ApiError::new(
                    code::MASS_DELETE_HELD,
                    "a mass delete is held; confirm its generation",
                )
                .details(json!({ "generation": generation, "would_delete": would_delete })));
            }
            Err(IngestError::ConfirmMismatch { generation }) => {
                let would = crate::ingest::reconcile::would_delete(&app.pool, id, generation)
                    .await
                    .unwrap_or(0);
                return Err(ApiError::new(
                    code::MASS_DELETE_HELD,
                    "confirm_mass_delete does not name the held generation",
                )
                .details(json!({ "generation": generation, "would_delete": would })));
            }
            Err(IngestError::DriveNotFound) => return Err(drive_not_found(id)),
            Err(e) => return Err(e.into()),
        }
    };
    idem::store(&mut conn, &route, &key, &sha, 202, &info).await?;
    Ok(json_with(202, &info, &[]))
}

/// `POST /drives/{id}:rebuild` (10 4.2, D23, B-S10-03): the one manual full rebuild. Every file of the Drive is
/// parsed and extracted again at the reprocess priority, one by one, with DOC-G10 judging each new extraction; the
/// old facts stay valid until a file's diff is applied. Nothing else triggers a full rebuild. The body must say
/// `{"confirm": "<drive id>"}`: a rebuild is expensive and cannot be undone by the caller.
pub async fn rebuild(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(drive_not_found(id));
    }
    let key = idem::key_from(&req.headers)?;
    let route = format!("POST /v1/docs/drives/{id}:rebuild");
    let sha = idem::body_sha256(&req.body);
    let body = super::http::parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    only_keys(obj, &["confirm"], "")?;
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, &route, &key, &sha).await? {
        return Ok(replayed(stored, "/v1/docs/drives"));
    }
    match obj.get("confirm").and_then(Value::as_str) {
        Some(c) if c == id => {}
        Some(_) => {
            return Err(ApiError::bad(
                "/confirm",
                "confirm must be the id of the drive to rebuild",
            ));
        }
        None => {
            return Err(ApiError::bad(
                "/confirm",
                "a full rebuild needs \"confirm\": the drive id",
            ));
        }
    }
    let drive = crate::ingest::reconcile::load_drive(&app.pool, id)
        .await?
        .filter(|d| d.status != "deleted")
        .ok_or_else(|| drive_not_found(id))?;
    if drive.status != "active" {
        return Err(
            ApiError::new(code::INVALID_CONFIG, "the drive is not active")
                .details(json!({ "status": drive.status })),
        );
    }
    let outdated = crate::jobs::stages::rebuild_drive(&app.pool, id).await?;
    app.wake.notify_waiters();
    let info = json!({
        "drive_id": id,
        "status": "queued",
        "files": outdated,
        "priority": 900,
    });
    idem::store(&mut conn, &route, &key, &sha, 202, &info).await?;
    Ok(json_with(202, &info, &[]))
}

#[allow(dead_code)]
fn _kinds() -> JobKind {
    JobKind::Sync
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_validation() {
        let ok = parse_create(&json!({"name":"团队资料","backend":"fs","config":{"root":"/data/d","exclude":["*.bak"]}})).unwrap();
        assert_eq!(ok.name, "团队资料");
        assert_eq!(ok.config["root"], "/data/d");
        assert!(!ok.model_enabled);
        assert_eq!(ok.llm_daily_calls, 2000);
        for (body, pointer) in [
            (json!({"backend":"fs","config":{"root":"/x"}}), "/name"),
            (
                json!({"name":"x","backend":"fs","config":{"root":"x"}}),
                "/config/root",
            ),
            (
                json!({"name":"x","backend":"fs","config":{"root":"/x","bad":1}}),
                "/config/bad",
            ),
            (
                json!({"name":"x","backend":"tape","config":{"root":"/x"}}),
                "/backend",
            ),
            (
                json!({"name":"","backend":"fs","config":{"root":"/x"}}),
                "/name",
            ),
            (
                json!({"name":"x","backend":"fs","config":{"root":"/x"},"extra":1}),
                "/extra",
            ),
            (
                json!({"name":"x","backend":"fs","config":{"root":"/x"},"llm_daily_calls":-1}),
                "/llm_daily_calls",
            ),
            (
                json!({"name":"x","backend":"fs","config":{"root":"/x"},"review":{"auto_publish_bp":10}}),
                "/review/auto_publish_bp",
            ),
        ] {
            let e = parse_create(&body)
                .err()
                .unwrap_or_else(|| panic!("{body}"));
            assert_eq!(e.code, code::BAD_REQUEST, "{body}");
            assert_eq!(e.details["pointer"], pointer, "{body}");
        }
        let e = parse_create(&json!({"name":"x","backend":"s3","config":{}}))
            .err()
            .unwrap();
        #[cfg(not(feature = "objectstore-s3"))]
        assert_eq!(e.code, code::INVALID_CONFIG);
        #[cfg(feature = "objectstore-s3")]
        assert_eq!(e.details["pointer"], "/config/bucket");
    }

    #[test]
    fn sync_request_validation() {
        let r = parse_sync(&json!({})).unwrap();
        assert!(r.paths.is_empty() && !r.deep && r.confirm.is_none());
        let r = parse_sync(&json!({"paths":["a/b.md"]})).unwrap();
        assert_eq!(r.paths, ["a/b.md"]);
        assert!(parse_sync(&json!({"paths":["/abs"]})).is_err());
        assert!(parse_sync(&json!({"paths":["a/../b"]})).is_err());
        assert!(parse_sync(&json!({"paths":["a//b"]})).is_err());
        let many: Vec<String> = (0..501).map(|i| format!("f{i}")).collect();
        assert!(parse_sync(&json!({"paths": many})).is_err());
        assert!(parse_sync(&json!({"paths":["a"],"deep":true})).is_err());
        assert!(parse_sync(&json!({"confirm_mass_delete":0})).is_err());
        assert_eq!(
            parse_sync(&json!({"confirm_mass_delete":3}))
                .unwrap()
                .confirm,
            Some(3)
        );
        assert!(parse_sync(&json!({"nope":1})).is_err());
    }
}
