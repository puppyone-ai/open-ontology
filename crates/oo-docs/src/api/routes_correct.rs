//! Person corrections (10 §16.8, D59). Board WS-BE3.
//!
//! * `POST /v1/docs/facts/{id}:correct` `{action: mark_wrong | set_value, value?, reason?}`
//!   (`If-Match: "r<fact revision>"`, `Idempotency-Key`) answers the `Fact`:
//!   `mark_wrong` rejects the content for good (overlay `fact_reject`, a
//!   published fact is retracted); `set_value` replaces the value by a person's
//!   (overlay `fact_edit`, a new fact with `origin=human`, the evidence copied);
//! * `POST /v1/docs/entities:merge` `{into, from[]}` (`If-Match` on `into`)
//!   merges entities (overlay `entity_merge`, `entity_redirect`) and answers the
//!   merged `Entity`;
//! * `POST /v1/docs/entities/{eid}:split` `{groups:[{aliases[], facts[]}]}`
//!   (`If-Match` on `eid`): the first group stays with the entity, every other
//!   group becomes a new entity with its own key; answers a `BatchResult` with
//!   the new keys.
//!
//! Each correction advances `drive_rev` (the overlay carries it) and asks for
//! a publish in the 2 s window, so it takes the same pipeline as everything
//! else: gates, constraints, drift.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};

use super::error::{ApiError, code};
use super::http::{self, etag, if_match, is_uuid, parse_json};
use super::routes_entities::{entity_ref_sql, evidence_for, fact_sql};
use super::{AppState, Req, idem};
use crate::resolve::merge::{MergeError, merge_entities};
use crate::resolve::normalize::{entity_key, norm};
use crate::review::decide::{edit_fact, load_fact};
use crate::review::overlay;
use crate::store::rev::bump_drive_rev;

const BY: &str = "api";

/// Drive of the evidence of a fact (or of the first live Drive).
async fn drive_of_fact(conn: &mut PgConnection, fact_id: &str) -> Result<Option<String>, ApiError> {
    let d: Option<String> = sqlx::query_scalar(
        "SELECT fl.drive_id::text FROM _oo_docs.evidence e \
           JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
           JOIN _oo_docs.file fl ON fl.id = fv.file_id \
          WHERE e.target_kind = 'fact' AND e.target_id = $1::uuid ORDER BY e.created_at LIMIT 1",
    )
    .bind(fact_id)
    .fetch_optional(&mut *conn)
    .await?;
    if d.is_some() {
        return Ok(d);
    }
    Ok(sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.drive WHERE status <> 'deleted' ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await?)
}

async fn fact_json(conn: &mut PgConnection, id: &str) -> Result<Value, ApiError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {}::text AS j FROM _oo_docs.fact ft JOIN _oo_docs.entity s ON s.id = ft.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = ft.object_id WHERE ft.id = $1::uuid",
        fact_sql()
    )))
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| ApiError::not_found("fact"))?;
    let mut f: Value = serde_json::from_str(&row.try_get::<String, _>("j")?).unwrap_or(Value::Null);
    let mut ev = evidence_for(conn, "fact", &[id.to_owned()], 50).await?;
    let list = ev.remove(id).unwrap_or_default();
    f["evidence_total"] = json!(list.len());
    f["evidence"] = Value::Array(list);
    Ok(f)
}

/// An `attr.*` value: exactly one typed key (`text`, `date`, `enum`, `amount`, `number`, `ratio`), a non-empty string.
fn valid_value(v: &Value) -> bool {
    v.as_object()
        .is_some_and(crate::publish::gates::valid_attr_value)
}

/// Replay or run `work`, storing the response (the common envelope of the
/// three corrections).
async fn idempotent<F, Fut>(
    app: &Arc<AppState>,
    req: &Req,
    route: &str,
    work: F,
) -> Result<Response<Body>, ApiError>
where
    F: FnOnce(PgConnectionHandle, Value) -> Fut,
    Fut: std::future::Future<Output = Result<(u16, Value, Option<i64>), ApiError>>,
{
    let key = idem::key_from(&req.headers)?;
    let sha = idem::body_sha256(&req.body);
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, route, &key, &sha).await? {
        return Ok(http::json(stored.status, &stored.response));
    }
    let body = parse_json(&req.body)?;
    let handle = PgConnectionHandle(app.pool.clone());
    let (status, response, revision) = work(handle, body).await?;
    idem::store(&mut conn, route, &key, &sha, status, &response).await?;
    app.wake.notify_waiters();
    let headers: Vec<(&str, String)> = revision.map(|r| ("etag", etag(r))).into_iter().collect();
    Ok(http::json_with(status, &response, &headers))
}

/// Owns the pool so the work closure can acquire its own connection.
pub struct PgConnectionHandle(sqlx::PgPool);

impl PgConnectionHandle {
    async fn conn(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>, ApiError> {
        Ok(self.0.acquire().await?)
    }
}

/// `POST /v1/docs/facts/{id}:correct`.
pub async fn correct_fact(
    app: &Arc<AppState>,
    id: &str,
    req: &Req,
) -> Result<Response<Body>, ApiError> {
    if !is_uuid(id) {
        return Err(ApiError::not_found("fact"));
    }
    let revision = if_match(&req.headers)?;
    let id = id.to_owned();
    idempotent(app, req, "POST /v1/docs/facts/:correct", move |h, body| async move {
        let o = body
            .as_object()
            .ok_or_else(|| ApiError::bad("", "expected an object"))?;
        if let Some(k) = o.keys().find(|k| !["action", "value", "reason"].contains(&k.as_str())) {
            return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
        }
        let action = o.get("action").and_then(Value::as_str).unwrap_or_default();
        if !["mark_wrong", "set_value"].contains(&action) {
            return Err(ApiError::bad("/action", "expected mark_wrong or set_value"));
        }
        if let Some(r) = o.get("reason")
            && !r.as_str().is_some_and(|s| s.chars().count() <= 500)
        {
            return Err(ApiError::bad("/reason", "at most 500 characters"));
        }
        if action == "set_value" && !o.get("value").is_some_and(valid_value) {
            return Err(ApiError::bad(
                "/value",
                "expected {\"text\"|\"date\"|\"enum\": non-empty string}",
            ));
        }
        let mut conn = h.conn().await?;
        let mut tx = conn.begin().await?;
        let fact = load_fact(&mut tx, &id).await?.ok_or_else(|| ApiError::not_found("fact"))?;
        let current: i64 = sqlx::query_scalar("SELECT revision FROM _oo_docs.fact WHERE id = $1::uuid FOR UPDATE")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
        if current != revision {
            return Err(ApiError::new(code::PRECONDITION_FAILED, "the fact changed")
                .details(json!({ "current_revision": current })));
        }
        let status: String = sqlx::query_scalar("SELECT status FROM _oo_docs.fact WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
        if !["draft", "published"].contains(&status.as_str()) {
            return Err(ApiError::new(code::PRECONDITION_FAILED, "the fact is no longer live")
                .details(json!({ "status": status })));
        }
        let drive = drive_of_fact(&mut tx, &id).await?;
        let drive_rev = match &drive {
            Some(d) => Some(bump_drive_rev(&mut tx, d).await?),
            None => None,
        };
        let reason = o.get("reason").and_then(Value::as_str).unwrap_or_default();
        let shown = if action == "mark_wrong" {
            overlay::insert_rev(
                &mut tx,
                "fact_reject",
                &fact.fact_key,
                json!({"fact_id": id, "reason": reason, "human": true}),
                BY,
                None,
                "active",
                None,
                drive_rev,
            )
            .await?;
            sqlx::query(
                "UPDATE _oo_docs.fact SET status = CASE WHEN status = 'published' THEN 'retracted' ELSE 'rejected' END, \
                        retracted_at = CASE WHEN status = 'published' THEN clock_timestamp() ELSE retracted_at END, \
                        revision = revision + 1 WHERE id = $1::uuid",
            )
            .bind(&id)
            .execute(&mut *tx)
            .await?;
            id.clone()
        } else {
            let value = o.get("value").cloned().unwrap_or(Value::Null);
            if fact.object_key.is_some() {
                return Err(ApiError::bad("/action", "set_value applies to attributes only"));
            }
            match edit_fact(&mut tx, &fact, &json!({"value": value}), BY, None, drive_rev).await? {
                Some(new_id) => new_id,
                None => return Err(ApiError::bad("/value", "the value is unchanged")),
            }
        };
        crate::publish::debounce::schedule_publish(&mut tx).await?;
        tx.commit().await?;
        let mut view = h.conn().await?;
        let f = fact_json(&mut view, &shown).await?;
        let rev: i64 = sqlx::query_scalar("SELECT revision FROM _oo_docs.fact WHERE id = $1::uuid")
            .bind(&shown)
            .fetch_one(&mut *view)
            .await?;
        Ok((200, f, Some(rev)))
    })
    .await
}

async fn entity_detail(conn: &mut PgConnection, key: &str) -> Result<Value, ApiError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {}::text AS j, e.status, e.identity::text AS identity, e.revision \
           FROM _oo_docs.entity e WHERE e.key = $1",
        entity_ref_sql("e")
    )))
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| ApiError::not_found("entity"))?;
    let mut e: Value = serde_json::from_str(&row.try_get::<String, _>("j")?).unwrap_or(Value::Null);
    e["status"] = json!(row.try_get::<String, _>("status")?);
    e["identity"] =
        serde_json::from_str(&row.try_get::<String, _>("identity")?).unwrap_or(json!({}));
    e["revision"] = json!(row.try_get::<i64, _>("revision")?);
    e["merged_into"] = Value::Null;
    e["aliases"] = json!([]);
    e["facts"] = json!([]);
    e["redirect_from"] = json!(
        sqlx::query_scalar::<_, String>(
            "SELECT from_key FROM _oo_docs.entity_redirect WHERE to_key = $1 ORDER BY 1"
        )
        .bind(key)
        .fetch_all(&mut *conn)
        .await?
    );
    Ok(e)
}

async fn entity_key_of(conn: &mut PgConnection, x: &str) -> Result<String, ApiError> {
    let k: Option<String> =
        sqlx::query_scalar("SELECT key FROM _oo_docs.entity WHERE key = $1 OR id::text = $1")
            .bind(x)
            .fetch_optional(&mut *conn)
            .await?;
    k.ok_or_else(|| ApiError::not_found("entity"))
}

/// `POST /v1/docs/entities:merge`.
pub async fn merge(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let revision = if_match(&req.headers)?;
    idempotent(app, req, "POST /v1/docs/entities:merge", move |h, body| async move {
        let o = body
            .as_object()
            .ok_or_else(|| ApiError::bad("", "expected an object"))?;
        if let Some(k) = o.keys().find(|k| !["into", "from"].contains(&k.as_str())) {
            return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
        }
        let into = o
            .get("into")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad("/into", "expected an entity key or id"))?;
        let from: Vec<&str> = o
            .get("from")
            .and_then(Value::as_array)
            .filter(|a| (1..=50).contains(&a.len()))
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .ok_or_else(|| ApiError::bad("/from", "expected 1 to 50 entities"))?;
        let mut conn = h.conn().await?;
        let mut tx = conn.begin().await?;
        let into_key = entity_key_of(&mut tx, into).await?;
        let current: i64 = sqlx::query_scalar("SELECT revision FROM _oo_docs.entity WHERE key = $1 FOR UPDATE")
            .bind(&into_key)
            .fetch_one(&mut *tx)
            .await?;
        if current != revision {
            return Err(ApiError::new(code::PRECONDITION_FAILED, "the entity changed")
                .details(json!({ "current_revision": current })));
        }
        let drive: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.drive WHERE status <> 'deleted' ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let drive_rev = match &drive {
            Some(d) => Some(bump_drive_rev(&mut tx, d).await?),
            None => None,
        };
        let mut merged = BTreeSet::new();
        for f in from {
            let fk = entity_key_of(&mut tx, f).await?;
            if !merged.insert(fk.clone()) {
                continue;
            }
            match merge_entities(&mut tx, &fk, &into_key, "human", None).await {
                Ok(_) => {
                    overlay::insert_rev(
                        &mut tx,
                        "entity_merge",
                        &fk,
                        json!({"into": into_key, "human": true}),
                        BY,
                        None,
                        "active",
                        None,
                        drive_rev,
                    )
                    .await?;
                }
                Err(MergeError::Store(e)) => return Err(e.into()),
                Err(e) => {
                    // errors.v1.json gives this code no HTTP status (it is an
                    // item level code of review:decide); a refused merge is a 409.
                    let mut err = ApiError::new(
                        code::REVIEW_MERGE_CONFLICT,
                        format!("merge refused: {e}"),
                    )
                    .details(json!({ "from": fk }));
                    err.status = 409;
                    return Err(err);
                }
            }
        }
        crate::publish::debounce::schedule_publish(&mut tx).await?;
        tx.commit().await?;
        let mut view = h.conn().await?;
        let e = entity_detail(&mut view, &into_key).await?;
        let rev = e["revision"].as_i64();
        Ok((200, e, rev))
    })
    .await
}

/// `POST /v1/docs/entities/{eid}:split`.
pub async fn split(app: &Arc<AppState>, eid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    let revision = if_match(&req.headers)?;
    let eid = eid.to_owned();
    idempotent(app, req, "POST /v1/docs/entities/:split", move |h, body| async move {
        let groups = body
            .get("groups")
            .and_then(Value::as_array)
            .filter(|g| (2..=20).contains(&g.len()))
            .ok_or_else(|| ApiError::bad("/groups", "expected 2 to 20 groups"))?
            .clone();
        let mut conn = h.conn().await?;
        let mut tx = conn.begin().await?;
        let row = sqlx::query(
            "SELECT id::text AS id, key, type, thing_label, canonical_name, status, revision \
               FROM _oo_docs.entity WHERE key = $1 OR id::text = $1 FOR UPDATE",
        )
        .bind(&eid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("entity"))?;
        let (id, key, ty, label, name): (String, String, String, Option<String>, String) = (
            row.try_get("id")?,
            row.try_get("key")?,
            row.try_get("type")?,
            row.try_get("thing_label")?,
            row.try_get("canonical_name")?,
        );
        let current: i64 = row.try_get("revision")?;
        if current != revision {
            return Err(ApiError::new(code::PRECONDITION_FAILED, "the entity changed")
                .details(json!({ "current_revision": current })));
        }
        if row.try_get::<String, _>("status")? != "active" {
            return Err(ApiError::bad("/eid", "only an active entity can be split"));
        }
        let entity_type = crate::extract::model::EntityType::parse(&ty)
            .ok_or_else(|| ApiError::new(code::INTERNAL, "unknown entity type"))?;
        let drive: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.drive WHERE status <> 'deleted' ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let drive_rev = match &drive {
            Some(d) => Some(bump_drive_rev(&mut tx, d).await?),
            None => None,
        };
        let mut seen_facts: BTreeSet<String> = BTreeSet::new();
        let mut seen_aliases: BTreeSet<String> = BTreeSet::new();
        let mut results = Vec::new();
        let mut new_keys = Vec::new();
        for (i, g) in groups.iter().enumerate().skip(1) {
            let aliases: Vec<String> = g
                .get("aliases")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let facts: Vec<String> = g
                .get("facts")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            if facts.iter().any(|f| !is_uuid(f)) {
                return Err(ApiError::bad(&format!("/groups/{i}/facts"), "expected fact ids"));
            }
            for a in &aliases {
                if !seen_aliases.insert(norm(a)) {
                    return Err(ApiError::bad(&format!("/groups/{i}/aliases"), "an alias is in two groups"));
                }
            }
            for f in &facts {
                if !seen_facts.insert(f.clone()) {
                    return Err(ApiError::bad(&format!("/groups/{i}/facts"), "a fact is in two groups"));
                }
            }
            let new_name = aliases.first().cloned().unwrap_or_else(|| name.clone());
            let identity = oo_builder::canonical_json(&json!({"split_of": key, "group": i}));
            let new_key = entity_key(entity_type, label.as_deref(), &norm(&new_name), Some(&identity));
            sqlx::query(
                "INSERT INTO _oo_docs.entity (id, key, type, thing_label, canonical_name, norm_name) \
                 VALUES (gen_random_uuid(), $1, $2, $3, $4, $5) ON CONFLICT (key) DO NOTHING",
            )
            .bind(&new_key)
            .bind(&ty)
            .bind(&label)
            .bind(&new_name)
            .bind(norm(&new_name))
            .execute(&mut *tx)
            .await?;
            let new_id: String = sqlx::query_scalar("SELECT id::text FROM _oo_docs.entity WHERE key = $1")
                .bind(&new_key)
                .fetch_one(&mut *tx)
                .await?;
            let norms: Vec<String> = aliases.iter().map(|a| norm(a)).collect();
            sqlx::query(
                "UPDATE _oo_docs.alias SET entity_id = $2::uuid, original_entity_id = $2::uuid \
                  WHERE entity_id = $1::uuid AND norm_alias = ANY($3::text[])",
            )
            .bind(&id)
            .bind(&new_id)
            .bind(&norms)
            .execute(&mut *tx)
            .await?;
            for (col, orig) in [("subject_id", "original_subject_id"), ("object_id", "original_object_id")] {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "UPDATE _oo_docs.fact SET {col} = $2::uuid, {orig} = $2::uuid, revision = revision + 1 \
                      WHERE id = ANY($3::text[]::uuid[]) AND {col} = $1::uuid"
                )))
                .bind(&id)
                .bind(&new_id)
                .bind(&facts)
                .execute(&mut *tx)
                .await?;
            }
            // Evidence of the entity itself follows the first fact it cites.
            new_keys.push(new_key.clone());
            results.push(json!({"id": new_key, "status": "ok"}));
        }
        overlay::insert_rev(
            &mut tx,
            "entity_split",
            &key,
            json!({"into_groups": new_keys, "human": true}),
            BY,
            None,
            "active",
            None,
            drive_rev,
        )
        .await?;
        sqlx::query("UPDATE _oo_docs.entity SET revision = revision + 1 WHERE id = $1::uuid")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        crate::publish::debounce::schedule_publish(&mut tx).await?;
        tx.commit().await?;
        Ok((200, json!({ "results": results }), None))
    })
    .await
}
