//! The structure layer of the ontology (D93), local entry:
//!
//! * `GET /v1/docs/drives/{id}/structure`: the classes of record files with their properties, types, coverage and
//!   examples, the schema version and the coverage of the structure layer.
//! * `POST /v1/docs/drives/{id}/structure:induce`: induce the structure now (it also runs after every publish).
//! * `POST /v1/docs/drives/{id}/structure:decide`: a person's decision (accept, reject, rename, merge) on a class or a
//!   property; a new schema version. Only a confirmed class is read by the extraction (template-driven extraction).
//! * `GET /v1/docs/drives/{id}/structure/classes/{class_key}/instances`: the instances a confirmed table class filled
//!   (one per row): the entity, its typed values, its relations and the cell range of the row in the sheet.
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
    let fill = store::fill_totals(&mut conn, id).await?;
    let mut classes = serde_json::to_value(&classes).unwrap_or(Value::Null);
    if let Some(list) = classes.as_array_mut() {
        for c in list {
            // Only a table class that was read row by row has filled counts; the others show none.
            let key = c.get("class_key").and_then(Value::as_str).unwrap_or("");
            if let Some(f) = fill.get(key) {
                c["filled"] = json!(f);
            }
        }
    }
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

/// One page of the instances of a table class.
pub async fn instances(
    app: &AppState,
    id: &str,
    class_key: &str,
    req: &Req,
) -> Result<Response<Body>, ApiError> {
    if req.method != Method::GET {
        return Err(ApiError::not_found("route"));
    }
    ensure(app, id).await?;
    let limit: i64 = match req.query.get("limit") {
        None => 50,
        Some(v) => v
            .parse::<i64>()
            .ok()
            .filter(|n| (1..=200).contains(n))
            .ok_or_else(|| ApiError::bad("/limit", "limit must be 1 to 200"))?,
    };
    let offset: i64 = match req.query.get("cursor") {
        None => 0,
        Some(v) => v
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| ApiError::bad("/cursor", "not a cursor of this list"))?,
    };
    let mut conn = app.pool.acquire().await?;
    let (_, classes): (u32, Vec<ClassStructure>) = store::load(&mut conn, id).await?;
    let class = classes
        .iter()
        .find(|c| c.class_key == class_key)
        .ok_or_else(|| ApiError::not_found("class"))?;
    let totals = store::fill_totals(&mut conn, id).await?;
    let rows = sqlx::query(
        "WITH page AS ( \
           SELECT f.subject_id, ev.block_id, fv.path_at_version AS path, fv.file_id, ev.anchor, \
                  COALESCE(NULLIF(substring(ev.anchor->>'range' from '[0-9]+'), '')::int, 0) AS rownum \
             FROM _oo_docs.fact f \
             JOIN _oo_docs.evidence ev ON ev.target_kind = 'fact' AND ev.target_id = f.id AND ev.withdrawn_at IS NULL \
             JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
             JOIN _oo_docs.structure_fill s ON s.drive_id = $1::uuid AND s.file_id = fv.file_id AND s.version_id = fv.id AND s.class_key = $2 \
            WHERE f.rule_id = 'structure.row' AND f.status IN ('draft', 'published') AND f.retracted_at IS NULL \
            GROUP BY f.subject_id, ev.block_id, fv.path_at_version, fv.file_id, ev.anchor \
            ORDER BY fv.path_at_version, ev.anchor->>'sheet' NULLS FIRST, 6, f.subject_id LIMIT $3 OFFSET $4) \
         SELECT p.block_id, p.path, p.file_id::text AS file_id, p.anchor::text AS anchor, p.rownum, \
                es.key AS skey, es.type AS stype, es.canonical_name AS sname, es.thing_label, \
                f.id::text AS fact_id, f.predicate, f.value::text AS value, eo.key AS okey, eo.type AS otype, eo.canonical_name AS oname \
           FROM page p \
           JOIN _oo_docs.entity es ON es.id = p.subject_id \
           JOIN _oo_docs.fact f ON f.subject_id = p.subject_id AND f.rule_id = 'structure.row' AND f.status IN ('draft', 'published') AND f.retracted_at IS NULL \
           JOIN _oo_docs.evidence ev2 ON ev2.target_kind = 'fact' AND ev2.target_id = f.id AND ev2.withdrawn_at IS NULL AND ev2.block_id = p.block_id \
           LEFT JOIN _oo_docs.entity eo ON eo.id = f.object_id \
          ORDER BY p.path, p.anchor->>'sheet' NULLS FIRST, p.rownum, es.key, f.predicate, f.id",
    )
    .bind(id)
    .bind(class_key)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *conn)
    .await?;
    use sqlx::Row;
    let mut items: Vec<Value> = Vec::new();
    let mut last: Option<(String, String)> = None;
    for r in &rows {
        let key = (r.get::<String, _>("skey"), r.get::<String, _>("block_id"));
        if last.as_ref() != Some(&key) {
            let anchor: Value =
                serde_json::from_str(&r.get::<String, _>("anchor")).unwrap_or(Value::Null);
            items.push(json!({
                "entity": {"key": key.0, "type": r.get::<String, _>("stype"), "name": r.get::<String, _>("sname"),
                           "label": r.get::<Option<String>, _>("thing_label")},
                "file": {"id": r.get::<String, _>("file_id"), "path": r.get::<String, _>("path")},
                "anchor": anchor,
                "row": r.get::<i32, _>("rownum"),
                "cells": [], "relations": [],
            }));
            last = Some(key);
        }
        let item = items.last_mut().expect("an item was pushed");
        let predicate: String = r.get("predicate");
        match r.get::<Option<String>, _>("oname") {
            Some(oname) => item["relations"].as_array_mut().expect("array").push(json!({
                "fact_id": r.get::<String, _>("fact_id"), "predicate": predicate,
                "object": {"key": r.get::<Option<String>, _>("okey"), "type": r.get::<Option<String>, _>("otype"), "name": oname},
            })),
            None => {
                let slug = predicate.strip_prefix("attr.").unwrap_or(&predicate).to_owned();
                let prop = class.property(&slug);
                let value: Value = r
                    .get::<Option<String>, _>("value")
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or(Value::Null);
                item["cells"].as_array_mut().expect("array").push(json!({
                    "fact_id": r.get::<String, _>("fact_id"), "slug": slug,
                    "name": prop.map(|p| p.name.clone()), "type": prop.map(|p| p.value_type.as_str()), "value": value,
                }));
            }
        }
    }
    let total = totals.get(class_key).map_or(0, |t| i64::from(t.instances));
    let next = (offset + limit < total).then(|| (offset + limit).to_string());
    Ok(http::json(
        200,
        &json!({"class_key": class_key, "total": total, "items": items, "next_cursor": next}),
    ))
}
