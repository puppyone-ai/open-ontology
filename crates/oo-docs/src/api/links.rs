//! The link surface of the one ontology layer (D83), local entry:
//!
//! * `GET /v1/docs/links?doc=<entity key>|db_table=&db_key=&drive_id=&status=&limit=`: links between document
//!   entities and rows of database tables, each with its provenance (the file anchors of the entity, the table
//!   row and the rule that matched). An Agent token only sees links of its Drives.
//! * `POST /v1/docs/links:decide` `{drive_id, doc_entity_key, db_table, db_key, status}`: a person's (or a
//!   `docs:write` Agent's) `confirmed` / `rejected`, which every rebuild keeps.
//! * `POST /v1/docs/links:project` `{drive_id?, force?}`: run the projection now (the job runs by itself on
//!   every published version; this is for the CLI and tests, and answers what it did).
//! * `GET|PUT|DELETE /v1/docs/link-rules`: which table columns match document entities of which type.
//! * `GET|PUT|DELETE /v1/docs/agent-db-binding`: the secret reference of the Agent DB the projection writes to.
//!
//! The Agent DB is read and written only under `_oo_docproj`. Without a binding the read routes answer an empty
//! list with `agent_db: "unbound"` (documents only is a normal state), the write routes 400.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};
use sqlx::Row;

use super::error::{ApiError, code};
use super::http::{is_uuid, json_with, parse_json};
use super::{AppState, Req};
use crate::publish::agentdb::{self, AgentDbConfig};
use crate::publish::projection;
use crate::publish::projector::{self, Projector};

fn cfg(app: &AppState) -> AgentDbConfig {
    app.agent_db.clone().unwrap_or_default()
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

fn is_table(s: &str) -> bool {
    s.split_once('.').is_some_and(|(a, b)| {
        is_ident(a)
            && is_ident(b)
            && !a.starts_with("_oo")
            && a != "pg_catalog"
            && a != "information_schema"
    })
}

fn bad_drive(path: &str, v: Option<&str>) -> Result<String, ApiError> {
    v.filter(|d| is_uuid(d))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| ApiError::bad(path, "a drive id"))
}

/// `GET /v1/docs/links`.
pub async fn list(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let q = &req.query;
    let doc = q.get("doc").filter(|d| !d.is_empty());
    let table = q.get("db_table").filter(|d| !d.is_empty());
    let key = q.get("db_key").filter(|d| !d.is_empty());
    if doc.is_none() && table.is_none() {
        return Err(ApiError::bad(
            "/query",
            "doc=<entity key> or db_table=<schema.table> (with db_key)",
        ));
    }
    if key.is_some() && table.is_none() {
        return Err(ApiError::bad("/query/db_table", "db_key needs db_table"));
    }
    if table.is_some_and(|t| !is_table(t)) {
        return Err(ApiError::bad("/query/db_table", "schema.table"));
    }
    let status = q.get("status").unwrap_or("live");
    if !["live", "auto", "confirmed", "rejected", "all"].contains(&status) {
        return Err(ApiError::bad(
            "/query/status",
            "live, auto, confirmed, rejected or all",
        ));
    }
    let drive = match q.get("drive_id").filter(|d| !d.is_empty()) {
        Some(d) => Some(bad_drive("/query/drive_id", Some(d))?),
        None => None,
    };
    let limit = q.limit()?;
    // An entity key that was merged away is followed to the key it lives under.
    let mut docs = app.pool.acquire().await?;
    let doc_key: Option<String> = match doc {
        Some(d) => Some(
            sqlx::query_scalar(
                "SELECT COALESCE((SELECT r.to_key FROM _oo_docs.entity_redirect r WHERE r.from_key = $1), $1)",
            )
            .bind(d)
            .fetch_one(&mut *docs)
            .await?,
        ),
        None => None,
    };
    let reference = cfg(app).binding(&mut docs).await?;
    drop(docs);
    let Some(reference) = reference else {
        return Ok(json_with(
            200,
            &json!({"items": [], "agent_db": "unbound"}),
            &[],
        ));
    };
    let mut agent = match cfg(app).connect(&reference).await {
        Ok(c) => c,
        Err(e) => {
            return Err(
                ApiError::new(code::DB_UNAVAILABLE, "the Agent DB is not reachable")
                    .details(json!({"reason": e.code()})),
            );
        }
    };
    let scope: Option<Vec<String>> = match &req.agent_token {
        Some(a) if !a.all_drives => Some(a.drive_ids.clone()),
        _ => None,
    };
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('_oo_docproj.entity_link') IS NOT NULL")
            .fetch_one(&mut agent)
            .await?;
    if !exists {
        return Ok(json_with(
            200,
            &json!({"items": [], "agent_db": "bound"}),
            &[],
        ));
    }
    let rows = sqlx::query(
        "SELECT l.drive_id, l.doc_entity_key, e.type AS doc_type, e.canonical_name, l.db_table, l.db_key, \
                l.matched_via, l.matched_column, l.matched_text, l.confidence_bp, l.status, \
                l.rule_revision, l.ontology_version, \
                COALESCE((SELECT jsonb_agg(jsonb_build_object('path', v.path, 'file_version', v.file_version, \
                              'block_id', v.block_id, 'anchor', v.anchor) ORDER BY v.ordinal) \
                            FROM (SELECT * FROM _oo_docproj.doc_entity_evidence x \
                                   WHERE x.drive_id = l.drive_id AND x.entity_key = l.doc_entity_key \
                                   ORDER BY x.ordinal LIMIT 5) v), '[]'::jsonb)::text AS evidence \
           FROM _oo_docproj.entity_link l \
           JOIN _oo_docproj.doc_entity e ON e.drive_id = l.drive_id AND e.entity_key = l.doc_entity_key \
          WHERE ($1::text IS NULL OR l.doc_entity_key = $1) \
            AND ($2::text IS NULL OR l.db_table = $2) AND ($3::text IS NULL OR l.db_key = $3) \
            AND ($4::text IS NULL OR l.drive_id = $4) \
            AND ($5::text[] IS NULL OR l.drive_id = ANY($5)) \
            AND (CASE $6 WHEN 'live' THEN l.status <> 'rejected' WHEN 'all' THEN TRUE ELSE l.status = $6 END) \
          ORDER BY l.drive_id, l.doc_entity_key, l.db_table, l.db_key LIMIT $7",
    )
    .bind(doc_key.as_deref())
    .bind(table)
    .bind(key)
    .bind(drive.as_deref())
    .bind(scope.as_deref())
    .bind(status)
    .bind(limit)
    .fetch_all(&mut agent)
    .await?;
    let mut items = Vec::new();
    for r in &rows {
        let evidence: Value =
            serde_json::from_str(&r.try_get::<String, _>("evidence")?).unwrap_or(json!([]));
        let table: String = r.try_get("db_table")?;
        let dbkey: String = r.try_get("db_key")?;
        items.push(json!({
            "drive_id": r.try_get::<String, _>("drive_id")?,
            "doc_entity_key": r.try_get::<String, _>("doc_entity_key")?,
            "doc_entity_type": r.try_get::<String, _>("doc_type")?,
            "canonical_name": r.try_get::<String, _>("canonical_name")?,
            "db_table": table, "db_key": dbkey,
            "matched_via": r.try_get::<String, _>("matched_via")?,
            "matched_column": r.try_get::<String, _>("matched_column")?,
            "matched_text": r.try_get::<String, _>("matched_text")?,
            "confidence_bp": r.try_get::<i32, _>("confidence_bp")?,
            "status": r.try_get::<String, _>("status")?,
            "provenance": {
                "file": evidence,
                "table_row": {"table": table, "key": dbkey,
                              "matched_column": r.try_get::<String, _>("matched_column")?,
                              "rule_revision": r.try_get::<i64, _>("rule_revision")?,
                              "ontology_version": r.try_get::<i64, _>("ontology_version")?},
            },
        }));
    }
    Ok(json_with(
        200,
        &json!({"items": items, "agent_db": "bound"}),
        &[],
    ))
}

/// `POST /v1/docs/links:decide`.
pub async fn decide(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = obj.keys().find(|k| {
        !["drive_id", "doc_entity_key", "db_table", "db_key", "status"].contains(&k.as_str())
    }) {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let drive = bad_drive("/drive_id", obj.get("drive_id").and_then(Value::as_str))?;
    let text = |k: &str, max: usize| -> Result<&str, ApiError> {
        obj.get(k)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty() && t.len() <= max)
            .ok_or_else(|| ApiError::bad(&format!("/{k}"), "required text"))
    };
    let (ekey, table, dbkey) = (
        text("doc_entity_key", 200)?,
        text("db_table", 130)?,
        text("db_key", 400)?,
    );
    let status = obj
        .get("status")
        .and_then(Value::as_str)
        .filter(|s| ["confirmed", "rejected"].contains(s))
        .ok_or_else(|| ApiError::bad("/status", "confirmed or rejected"))?;
    if !is_table(table) {
        return Err(ApiError::bad("/db_table", "schema.table"));
    }
    let mut docs = app.pool.acquire().await?;
    let reference = cfg(app).binding(&mut docs).await?;
    drop(docs);
    let reference = reference
        .ok_or_else(|| ApiError::bad("/", "no Agent DB is bound: there are no links to decide"))?;
    let mut agent = cfg(app).connect(&reference).await.map_err(|e| {
        ApiError::new(code::DB_UNAVAILABLE, "the Agent DB is not reachable")
            .details(json!({"reason": e.code()}))
    })?;
    let done = projection::decide(&mut agent, &drive, ekey, table, dbkey, status)
        .await
        .map_err(|e| ApiError::bad("/", e.to_string()))?;
    if !done {
        return Err(ApiError::not_found("link"));
    }
    Ok(json_with(
        200,
        &json!({"drive_id": drive, "doc_entity_key": ekey, "db_table": table, "db_key": dbkey, "status": status}),
        &[],
    ))
}

/// `POST /v1/docs/links:project`.
pub async fn project(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = if req.body.is_empty() {
        json!({})
    } else {
        parse_json(&req.body)?
    };
    let drive = match body.get("drive_id") {
        None | Some(Value::Null) => None,
        Some(v) => Some(bad_drive("/drive_id", v.as_str())?),
    };
    let force = body.get("force").and_then(Value::as_bool).unwrap_or(true);
    let cas = app
        .cas
        .clone()
        .ok_or_else(|| ApiError::new(code::INTERNAL, "the engine has no content store"))?;
    let p = Projector::new(app.pool.clone(), cas, cfg(app));
    let ids: Vec<String> = match drive {
        Some(d) => vec![d],
        None => {
            let mut c = app.pool.acquire().await?;
            sqlx::query_scalar(
                "SELECT id::text FROM _oo_docs.drive WHERE status IN ('active', 'paused') ORDER BY id",
            )
            .fetch_all(&mut *c)
            .await?
        }
    };
    let mut runs = Vec::new();
    for id in ids {
        match p.run_drive(&id, force).await {
            Ok(r) => runs.push(r.to_json()),
            Err(e) => {
                runs.push(json!({"drive_id": id, "error": e.code()}));
            }
        }
    }
    Ok(json_with(200, &json!({"runs": runs}), &[]))
}

/// `GET /v1/docs/link-rules?drive_id=`.
pub async fn rules_list(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let drive = bad_drive("/query/drive_id", req.query.get("drive_id"))?;
    let mut c = app.pool.acquire().await?;
    let rules = projector::load_rules(&mut c, &drive).await?;
    let items: Vec<Value> = rules
        .iter()
        .map(|r| {
            json!({"drive_id": drive, "db_table": r.db_table, "key_col": r.key_col, "name_col": r.name_col,
                   "alias_cols": r.alias_cols, "entity_type": r.entity_type, "revision": r.revision})
        })
        .collect();
    Ok(json_with(200, &json!({"items": items}), &[]))
}

/// `PUT /v1/docs/link-rules`: one rule per (Drive, table); a changed rule moves its revision.
pub async fn rules_put(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = parse_json(&req.body)?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = obj.keys().find(|k| {
        ![
            "drive_id",
            "db_table",
            "key_col",
            "name_col",
            "alias_cols",
            "entity_type",
        ]
        .contains(&k.as_str())
    }) {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let drive = bad_drive("/drive_id", obj.get("drive_id").and_then(Value::as_str))?;
    let table = obj
        .get("db_table")
        .and_then(Value::as_str)
        .filter(|t| is_table(t))
        .ok_or_else(|| ApiError::bad("/db_table", "schema.table of a data table"))?;
    let ident = |k: &str| -> Result<&str, ApiError> {
        obj.get(k)
            .and_then(Value::as_str)
            .filter(|t| is_ident(t))
            .ok_or_else(|| ApiError::bad(&format!("/{k}"), "a column name"))
    };
    let (key_col, name_col) = (ident("key_col")?, ident("name_col")?);
    let alias_cols: Vec<String> = match obj.get("alias_cols") {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .filter(|a| a.len() <= 8)
            .ok_or_else(|| ApiError::bad("/alias_cols", "at most 8 column names"))?
            .iter()
            .map(|c| c.as_str().filter(|c| is_ident(c)).map(str::to_owned))
            .collect::<Option<_>>()
            .ok_or_else(|| ApiError::bad("/alias_cols", "column names"))?,
    };
    let entity_type = match obj.get("entity_type") {
        None => "",
        Some(v) => v
            .as_str()
            .filter(|t| {
                t.len() <= 64 && crate::extract::model::EntityType::parse(t).is_some()
                    || t.is_empty()
            })
            .ok_or_else(|| ApiError::bad("/entity_type", "an entity type, or empty for any"))?,
    };
    let mut c = app.pool.acquire().await?;
    let known: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.drive WHERE id = $1::uuid AND status <> 'deleted')",
    )
    .bind(&drive)
    .fetch_one(&mut *c)
    .await?;
    if !known {
        return Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found"));
    }
    let revision: i64 = sqlx::query_scalar(
        "INSERT INTO _oo_docs.db_link_rule (drive_id, db_table, key_col, name_col, alias_cols, entity_type) \
         VALUES ($1::uuid, $2, $3, $4, $5, $6) \
         ON CONFLICT (drive_id, db_table) DO UPDATE SET \
           revision = CASE WHEN (db_link_rule.key_col, db_link_rule.name_col, db_link_rule.alias_cols, db_link_rule.entity_type) \
                              IS DISTINCT FROM (EXCLUDED.key_col, EXCLUDED.name_col, EXCLUDED.alias_cols, EXCLUDED.entity_type) \
                           THEN db_link_rule.revision + 1 ELSE db_link_rule.revision END, \
           key_col = EXCLUDED.key_col, name_col = EXCLUDED.name_col, alias_cols = EXCLUDED.alias_cols, \
           entity_type = EXCLUDED.entity_type, updated_at = clock_timestamp() \
         RETURNING revision",
    )
    .bind(&drive)
    .bind(table)
    .bind(key_col)
    .bind(name_col)
    .bind(&alias_cols)
    .bind(entity_type)
    .fetch_one(&mut *c)
    .await?;
    Ok(json_with(
        200,
        &json!({"drive_id": drive, "db_table": table, "key_col": key_col, "name_col": name_col,
                "alias_cols": alias_cols, "entity_type": entity_type, "revision": revision}),
        &[],
    ))
}

/// `DELETE /v1/docs/link-rules?drive_id=&db_table=`.
pub async fn rules_delete(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let drive = bad_drive("/query/drive_id", req.query.get("drive_id"))?;
    let table = req
        .query
        .get("db_table")
        .filter(|t| is_table(t))
        .ok_or_else(|| ApiError::bad("/query/db_table", "schema.table"))?;
    let mut c = app.pool.acquire().await?;
    let done = sqlx::query(
        "DELETE FROM _oo_docs.db_link_rule WHERE drive_id = $1::uuid AND db_table = $2",
    )
    .bind(&drive)
    .bind(table)
    .execute(&mut *c)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("link rule"));
    }
    Ok(json_with(
        200,
        &json!({"drive_id": drive, "db_table": table, "deleted": true}),
        &[],
    ))
}

/// `GET /v1/docs/agent-db-binding`.
pub async fn binding_get(app: &Arc<AppState>) -> Result<Response<Body>, ApiError> {
    let mut c = app.pool.acquire().await?;
    let row = sqlx::query(
        "SELECT secret_ref, status, last_error_code, updated_at::text AS at FROM _oo_docs.agent_db_binding",
    )
    .fetch_optional(&mut *c)
    .await?;
    let env_ref = cfg(app).env_ref;
    let v = match (&row, &env_ref) {
        (_, Some(r)) => json!({"bound": true, "source": "environment", "secret_ref": r}),
        (Some(r), None) => {
            json!({"bound": true, "source": "binding", "secret_ref": r.try_get::<String, _>("secret_ref")?,
                                   "status": r.try_get::<String, _>("status")?,
                                   "last_error_code": r.try_get::<Option<String>, _>("last_error_code")?,
                                   "updated_at": r.try_get::<String, _>("at")?})
        }
        (None, None) => json!({"bound": false}),
    };
    Ok(json_with(200, &v, &[]))
}

/// `PUT /v1/docs/agent-db-binding` `{secret_ref}`: the reference must resolve and the login must connect.
pub async fn binding_put(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = parse_json(&req.body)?;
    if let Some(k) = body
        .as_object()
        .and_then(|o| o.keys().find(|k| *k != "secret_ref"))
    {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let reference = body
        .get("secret_ref")
        .and_then(Value::as_str)
        .filter(|r| agentdb::valid_ref(r))
        .ok_or_else(|| {
            ApiError::bad(
                "/secret_ref",
                "env:NAME, file:/absolute/path or a secret name; a URL is never accepted",
            )
        })?;
    if let Err(e) = cfg(app).connect(reference).await {
        return Err(ApiError::new(
            code::INVALID_CONFIG,
            "the Agent DB reference does not resolve to a reachable database",
        )
        .details(json!({"reason": e.code()})));
    }
    let mut c = app.pool.acquire().await?;
    sqlx::query(
        "INSERT INTO _oo_docs.agent_db_binding (singleton, secret_ref, status) VALUES (true, $1, 'ok') \
         ON CONFLICT (singleton) DO UPDATE SET secret_ref = EXCLUDED.secret_ref, status = 'ok', \
           last_error_code = NULL, updated_at = clock_timestamp()",
    )
    .bind(reference)
    .execute(&mut *c)
    .await?;
    Ok(json_with(
        200,
        &json!({"bound": true, "source": "binding", "secret_ref": reference, "status": "ok"}),
        &[],
    ))
}

/// `DELETE /v1/docs/agent-db-binding`: the projection stops; what it wrote stays in the Agent DB.
pub async fn binding_delete(app: &Arc<AppState>) -> Result<Response<Body>, ApiError> {
    let mut c = app.pool.acquire().await?;
    let done = sqlx::query("DELETE FROM _oo_docs.agent_db_binding")
        .execute(&mut *c)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("binding"));
    }
    Ok(json_with(200, &json!({"bound": false}), &[]))
}
