//! The structure layer of the ontology (D93), local entry:
//!
//! * `GET /v1/docs/drives/{id}/structure`: the classes of record files with their properties, types, coverage and
//!   examples, the schema version and the coverage of the structure layer.
//! * `POST /v1/docs/drives/{id}/structure:induce`: induce the structure now (it also runs after every publish).
//! * `POST /v1/docs/drives/{id}/structure:decide`: a person's decision (accept, reject, rename, merge) on a class or a
//!   property; a new schema version. Only a confirmed class is read by the extraction (template-driven extraction).
//! * `GET /v1/docs/drives/{id}/structure/yaml[?database=0]`: the ontology YAML, with the database side and the
//!   cross-links when an Agent DB is bound.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Response};
use serde_json::{Value, json};

use super::error::{ApiError, code};
use super::{AppState, Req, http};
use crate::state::{self, FileState, Transition};
use crate::structure::store::{self, DecideError, Decision};
use crate::structure::{self, ClassStructure};

async fn ensure(app: &AppState, id: &str) -> Result<(), ApiError> {
    if !http::is_uuid(id) {
        return Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found"));
    }
    let st: Option<String> =
        sqlx::query_scalar("SELECT status FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(id)
            .fetch_optional(&app.pool)
            .await?;
    if st.as_deref().is_none_or(|s| s == "deleted") {
        return Err(ApiError::new(code::DRIVE_NOT_FOUND, "drive not found"));
    }
    Ok(())
}

async fn schema(app: &AppState, id: &str) -> Result<Value, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let (version, classes) = store::load(&mut conn, id).await?;
    let cov = store::coverage(&mut conn, id).await?;
    Ok(json!({
        "protocol": structure::PROTOCOL,
        "version": version,
        "classes": classes,
        "coverage": {
            "record_files": cov.record_files, "in_class": cov.in_class, "confirmed": cov.confirmed,
            "share_in_class": cov.share_in_class, "share_confirmed": cov.share_confirmed,
        },
    }))
}

pub async fn get(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    if req.method != Method::GET {
        return Err(ApiError::not_found("route"));
    }
    ensure(app, id).await?;
    Ok(http::json(200, &schema(app, id).await?))
}

pub async fn induce(app: &AppState, id: &str) -> Result<Response<Body>, ApiError> {
    ensure(app, id).await?;
    let mut conn = app.pool.acquire().await?;
    let r = store::induce_drive(&mut conn, id).await?;
    drop(conn);
    let mut body = schema(app, id).await?;
    body["induced"] = json!({
        "version": r.version, "changed": r.changed, "classes": r.classes, "record_files": r.record_files,
        "observed_files": r.observed_files, "unclustered": r.unclustered, "proposals": r.proposals,
    });
    Ok(http::json(200, &body))
}

fn parse_decision(body: &Value) -> Result<Decision, ApiError> {
    let op = body.get("op").and_then(Value::as_str).unwrap_or("");
    let class_key = body
        .get("class_key")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("/class_key", "required string"))?
        .to_owned();
    let property = match body.get("property") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(ApiError::bad("/property", "a property name or null")),
    };
    Ok(match op {
        "accept" => Decision::Accept {
            class_key,
            property,
        },
        "reject" => Decision::Reject {
            class_key,
            property,
        },
        "rename" => Decision::Rename {
            class_key,
            property,
            name: body
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad("/name", "required string"))?
                .to_owned(),
        },
        "merge" => Decision::Merge {
            class_key,
            property: property
                .ok_or_else(|| ApiError::bad("/property", "the property to merge"))?,
            into: body
                .get("into")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad("/into", "required string"))?
                .to_owned(),
        },
        _ => return Err(ApiError::bad("/op", "accept, reject, rename or merge")),
    })
}

pub async fn decide(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    ensure(app, id).await?;
    let body = http::parse_json(&req.body)?;
    let d = parse_decision(&body)?;
    let mut conn = app.pool.acquire().await?;
    match store::decide(&mut conn, id, &d).await {
        Ok((_, _)) => {}
        Err(DecideError::NotFound) => return Err(ApiError::not_found("class or property")),
        Err(DecideError::Invalid(what)) => return Err(ApiError::bad("/", what)),
        Err(DecideError::Store(m)) => return Err(ApiError::new(code::DB_UNAVAILABLE, m)),
    }
    // What a confirmed class reads changes: its files are extracted again (`outdated`, low priority, the old facts
    // stay valid meanwhile).
    if matches!(
        d,
        Decision::Accept { .. } | Decision::Merge { .. } | Decision::Reject { .. }
    ) {
        let key = match &d {
            Decision::Accept { class_key, .. }
            | Decision::Reject { class_key, .. }
            | Decision::Merge { class_key, .. }
            | Decision::Rename { class_key, .. } => class_key,
        };
        reextract(app, &mut conn, id, key).await?;
    }
    drop(conn);
    Ok(http::json(200, &schema(app, id).await?))
}

async fn reextract(
    app: &AppState,
    conn: &mut sqlx::PgConnection,
    drive_id: &str,
    class_key: &str,
) -> Result<u32, sqlx::Error> {
    let files: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.id::text, f.current_version_id::text FROM _oo_docs.structure_member m JOIN _oo_docs.file f ON f.id = m.file_id \
          WHERE m.drive_id = $1::uuid AND m.class_key = $2 AND f.deleted_at IS NULL AND f.current_version_id IS NOT NULL \
            AND f.state IN ('published', 'extract_partial', 'indexed') ORDER BY f.id LIMIT 20000",
    )
    .bind(drive_id)
    .bind(class_key)
    .fetch_all(&mut *conn)
    .await?;
    let mut n = 0;
    for (file_id, version_id) in files {
        let _ = state::transition(
            &app.pool,
            &Transition {
                file_id: &file_id,
                expect_from: None,
                to: FileState::Outdated,
                code: None,
                retryable: false,
                version_id: Some(&version_id),
                attempt: None,
                detail_json: Some("{\"structure_changed\":true}"),
            },
        )
        .await;
        n += 1;
    }
    Ok(n)
}

pub async fn yaml(app: &Arc<AppState>, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    ensure(app, id).await?;
    let mut conn = app.pool.acquire().await?;
    let (version, classes): (u32, Vec<ClassStructure>) = store::load(&mut conn, id).await?;
    let want_db = req.query.get("database") != Some("0");
    let mut db = None;
    let mut agent_db = "unbound";
    if want_db {
        let cfg = app.agent_db.clone().unwrap_or_default();
        if let Some(reference) = cfg.binding(&mut conn).await? {
            let rules = crate::publish::projector::load_rules(&mut conn, id).await?;
            match cfg.connect(&reference).await {
                Ok(mut agent) => {
                    db = Some(structure::dbside::load(&mut agent, &rules).await?);
                    agent_db = "bound";
                }
                Err(e) => {
                    return Err(ApiError::new(
                        code::DB_UNAVAILABLE,
                        "the Agent DB is not reachable",
                    )
                    .details(json!({"reason": e.code()})));
                }
            }
        }
    }
    let text = structure::yaml::render(version, &classes, db.as_ref());
    Ok(http::json(
        200,
        &json!({
            "protocol": structure::PROTOCOL, "version": version, "sources": if db.is_some() { json!(["documents", "database"]) } else { json!(["documents"]) },
            "agent_db": agent_db, "text": text,
        }),
    ))
}
