//! Review queue (10 §8.3). Board WS-BE3.
//!
//! * `GET /v1/docs/review?kind=&source=&drive_id=&status=pending&group_by=source&limit&cursor`
//!   lists items (`ReviewItem`, with the fact or entities they are about and the
//!   evidence); `group_by=source` lists `ReviewGroup` (source file, kind,
//!   count);
//! * `POST /v1/docs/review:decide` (`Idempotency-Key` required) decides up to 500
//!   items, each with the revision the client saw, or every pending
//!   `low_confidence_fact` item of one source in one operation (`by_source`).
//!   The response lists each item (`ok`, `conflict`, `not_found`, `invalid`,
//!   `already_decided`) with its `DOCS_REVIEW_*` code.
//!
//! A decision that makes something publishable asks for a publish in the
//! 2 second window (`publish_pending` is true) and emits `review.decided` and
//! `review.changed`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::error::{ApiError, code};
use super::http::{self, FMT, is_uuid, parse_json};
use super::routes_entities::{entity_ref_sql, evidence_for, fact_sql};
use super::{AppState, Req, cursor, idem};
use crate::review::decide::{Decision, ItemDecision, decide_by_source, decide_items};
use crate::review::queue::refresh_pending_counts;
use crate::store::events;

const KINDS: &[&str] = &[
    "low_confidence_fact",
    "entity_merge",
    "alias_conflict",
    "orphaned_overlay",
    "type_promotion",
    "suggestion",
    "reprocess_diff",
    "constraint_conflict",
    "speaker_identity",
];
const MAX_BATCH: usize = 500;
const EVIDENCE_PER_ITEM: usize = 20;

/// `GET /v1/docs/review`.
pub async fn list(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let q = &req.query;
    let limit = q.limit()?;
    let kind = q.get("kind").filter(|k| !k.is_empty());
    if let Some(k) = kind
        && !KINDS.contains(&k)
    {
        return Err(ApiError::bad("/query/kind", "unknown review kind"));
    }
    let source = q.get("source").filter(|s| !s.is_empty());
    let drive = q.get("drive_id").filter(|s| !s.is_empty());
    for (name, v) in [("source", source), ("drive_id", drive)] {
        if v.is_some_and(|v| !is_uuid(v)) {
            return Err(ApiError::bad(&format!("/query/{name}"), "expected a uuid"));
        }
    }
    let status = q.get("status").unwrap_or("pending");
    if !["pending", "accepted", "rejected", "superseded"].contains(&status) {
        return Err(ApiError::bad("/query/status", "unknown status"));
    }
    let grouped = match q.get("group_by") {
        None => false,
        Some("source") => true,
        Some(_) => return Err(ApiError::bad("/query/group_by", "expected source")),
    };
    let mut conn = app.pool.acquire().await?;
    let as_of_seq = super::files::head_seq(&mut conn).await?;
    if grouped {
        let rows = sqlx::query(
            "SELECT r.source_file_id::text AS file_id, f.path, r.kind, count(*)::bigint AS n \
               FROM _oo_docs.review_item r LEFT JOIN _oo_docs.file f ON f.id = r.source_file_id \
              WHERE r.status = $1 AND ($2::text IS NULL OR r.kind = $2) \
                AND ($3::uuid IS NULL OR r.source_file_id = $3::uuid) \
                AND ($4::uuid IS NULL OR r.drive_id = $4::uuid) \
              GROUP BY 1, 2, 3 ORDER BY 2 NULLS LAST, 3 LIMIT 1000",
        )
        .bind(status)
        .bind(kind)
        .bind(source)
        .bind(drive)
        .fetch_all(&mut *conn)
        .await?;
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                Ok::<_, sqlx::Error>(json!({
                    "source": {"file_id": r.try_get::<Option<String>, _>("file_id")?,
                               "path": r.try_get::<Option<String>, _>("path")?},
                    "kind": r.try_get::<String, _>("kind")?,
                    "count": r.try_get::<i64, _>("n")?,
                }))
            })
            .collect::<Result<_, _>>()?;
        return Ok(http::json(
            200,
            &json!({"items": items, "next_cursor": null, "as_of_seq": as_of_seq}),
        ));
    }
    let filters = format!(
        "review|{}|{}|{}|{status}",
        kind.unwrap_or(""),
        source.unwrap_or(""),
        drive.unwrap_or("")
    );
    let after = match q.get("cursor") {
        Some(t) => {
            let k = cursor::decode(t, &filters)?;
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
    let sql = format!(
        "SELECT r.id::text AS id, r.kind, r.status, r.reason_code, r.confidence_bp, \
                r.drive_id::text AS drive_id, r.source_file_id::text AS file_id, f.path, \
                r.subject::text AS subject, \
                to_char(r.created_at AT TIME ZONE 'UTC', {FMT}) AS created_at, \
                to_char(r.decided_at AT TIME ZONE 'UTC', {FMT}) AS decided_at, r.revision, \
                r.created_at::text AS ck \
           FROM _oo_docs.review_item r LEFT JOIN _oo_docs.file f ON f.id = r.source_file_id \
          WHERE r.status = $1 AND ($2::text IS NULL OR r.kind = $2) \
            AND ($3::uuid IS NULL OR r.source_file_id = $3::uuid) \
            AND ($4::uuid IS NULL OR r.drive_id = $4::uuid) \
            AND ($5::text IS NULL OR (r.created_at, r.id) > ($5::timestamptz, $6::uuid)) \
          ORDER BY r.created_at, r.id LIMIT {}",
        limit + 1
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(status)
        .bind(kind)
        .bind(source)
        .bind(drive)
        .bind(after.as_ref().map(|a| a.0.as_str()))
        .bind(after.as_ref().map(|a| a.1.as_str()))
        .fetch_all(&mut *conn)
        .await?;
    let mut items = Vec::new();
    let mut last = None;
    for r in rows.iter().take(limit as usize) {
        let id: String = r.try_get("id")?;
        let kind: String = r.try_get("kind")?;
        let subject: Value =
            serde_json::from_str(&r.try_get::<String, _>("subject")?).unwrap_or(Value::Null);
        let (subject, evidence) = enrich(&mut conn, &kind, subject).await?;
        let file_id: Option<String> = r.try_get("file_id")?;
        let path: Option<String> = r.try_get("path")?;
        let mut item = json!({
            "id": id, "kind": kind, "status": r.try_get::<String, _>("status")?,
            "reason_code": r.try_get::<String, _>("reason_code")?,
            "confidence_bp": r.try_get::<Option<i32>, _>("confidence_bp")?,
            "drive_id": r.try_get::<Option<String>, _>("drive_id")?,
            "subject": subject, "evidence": evidence,
            "created_at": r.try_get::<String, _>("created_at")?,
            "decided_at": r.try_get::<Option<String>, _>("decided_at")?,
            "revision": r.try_get::<i64, _>("revision")?,
        });
        item["source"] = match (file_id, path) {
            (Some(f), Some(p)) => json!({"file_id": f, "path": p}),
            _ => Value::Null,
        };
        last = Some((r.try_get::<String, _>("ck")?, id));
        items.push(item);
    }
    let next = if rows.len() as i64 > limit {
        last.map(|(k, i)| cursor::encode(&[json!(k), json!(i)], &filters))
    } else {
        None
    };
    Ok(http::json(
        200,
        &json!({"items": items, "next_cursor": next, "as_of_seq": as_of_seq}),
    ))
}

/// The subject object by kind (`{fact}`, `{entities, shared_aliases}`,
/// `{alias, entities}`, `{overlay, missing_evidence}`) and the evidence.
async fn enrich(
    conn: &mut PgConnection,
    kind: &str,
    subject: Value,
) -> Result<(Value, Vec<Value>), ApiError> {
    match kind {
        "low_confidence_fact" => {
            let Some(fid) = subject.get("fact_id").and_then(Value::as_str) else {
                return Ok((subject, Vec::new()));
            };
            let row = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {}::text AS j FROM _oo_docs.fact ft JOIN _oo_docs.entity s ON s.id = ft.subject_id \
                   LEFT JOIN _oo_docs.entity o ON o.id = ft.object_id WHERE ft.id = $1::uuid",
                fact_sql()
            )))
            .bind(fid)
            .fetch_optional(&mut *conn)
            .await?;
            let fact: Value = match row {
                Some(r) => {
                    serde_json::from_str(&r.try_get::<String, _>("j")?).unwrap_or(Value::Null)
                }
                None => Value::Null,
            };
            let mut ev = evidence_for(conn, "fact", &[fid.to_owned()], EVIDENCE_PER_ITEM).await?;
            let mut out = json!({ "fact": fact });
            if let Some(o) = subject.get("other_fact_id") {
                out["other_fact_id"] = o.clone();
            }
            Ok((out, ev.remove(fid).unwrap_or_default()))
        }
        "entity_merge" | "alias_conflict" => {
            let keys: Vec<String> = subject
                .get("entity_keys")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let ids: Vec<String> = subject
                .get("entity_ids")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {}::text AS j FROM _oo_docs.entity e \
                  WHERE e.key = ANY($1::text[]) OR e.id = ANY($2::text[]::uuid[]) ORDER BY e.key",
                entity_ref_sql("e")
            )))
            .bind(&keys)
            .bind(&ids)
            .fetch_all(&mut *conn)
            .await?;
            let entities: Vec<Value> = rows
                .iter()
                .filter_map(|r| serde_json::from_str(&r.try_get::<String, _>("j").ok()?).ok())
                .collect();
            let mut out = json!({ "entities": entities });
            for k in ["shared_aliases", "alias"] {
                if let Some(v) = subject.get(k) {
                    out[k] = v.clone();
                }
            }
            Ok((out, Vec::new()))
        }
        "orphaned_overlay" => Ok((
            json!({
                "overlay": subject.get("overlay_id").cloned().unwrap_or(Value::Null),
                "binds": subject.get("binds").cloned().unwrap_or(Value::Null),
                "missing_evidence": subject.get("missing_evidence").cloned().unwrap_or(json!(true)),
            }),
            Vec::new(),
        )),
        _ => Ok((subject, Vec::new())),
    }
}

/// Validate a `ReviewDecideRequest`.
fn parse_request(body: &Value) -> Result<(Vec<ItemDecision>, Option<BySource>), ApiError> {
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = obj
        .keys()
        .find(|k| !["items", "by_source"].contains(&k.as_str()))
    {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    match (obj.get("items"), obj.get("by_source")) {
        (Some(_), Some(_)) | (None, None) => Err(ApiError::new(
            code::REVIEW_INVALID_DECISION,
            "send either items or by_source",
        )),
        (Some(items), None) => {
            let arr = items
                .as_array()
                .ok_or_else(|| ApiError::bad("/items", "expected an array"))?;
            if arr.len() > MAX_BATCH {
                return Err(
                    ApiError::new(code::REVIEW_BATCH_TOO_LARGE, "too many items")
                        .details(json!({ "max": MAX_BATCH })),
                );
            }
            let mut out = Vec::with_capacity(arr.len());
            for (i, it) in arr.iter().enumerate() {
                let at = |f: &str| format!("/items/{i}/{f}");
                let o = it
                    .as_object()
                    .ok_or_else(|| ApiError::bad(&format!("/items/{i}"), "expected an object"))?;
                if let Some(k) = o
                    .keys()
                    .find(|k| !["id", "revision", "decision", "edit", "note"].contains(&k.as_str()))
                {
                    return Err(ApiError::bad(&at(k), "unknown field"));
                }
                let id = o
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| is_uuid(s))
                    .ok_or_else(|| ApiError::bad(&at("id"), "expected a uuid"))?;
                let revision = o
                    .get("revision")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| ApiError::bad(&at("revision"), "expected an integer"))?;
                let decision = o
                    .get("decision")
                    .and_then(Value::as_str)
                    .and_then(Decision::parse)
                    .ok_or_else(|| {
                        ApiError::new(
                            code::REVIEW_INVALID_DECISION,
                            "decision must be accept, reject or edit",
                        )
                    })?;
                let note = match o.get("note") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) if s.chars().count() <= 500 => Some(s.clone()),
                    _ => return Err(ApiError::bad(&at("note"), "at most 500 characters")),
                };
                out.push(ItemDecision {
                    id: id.to_owned(),
                    revision,
                    decision,
                    edit: o.get("edit").filter(|e| !e.is_null()).cloned(),
                    note,
                });
            }
            Ok((out, None))
        }
        (None, Some(b)) => {
            let o = b
                .as_object()
                .ok_or_else(|| ApiError::bad("/by_source", "expected an object"))?;
            let file_id = o
                .get("file_id")
                .and_then(Value::as_str)
                .filter(|s| is_uuid(s))
                .ok_or_else(|| ApiError::bad("/by_source/file_id", "expected a uuid"))?
                .to_owned();
            let accept = match o.get("decision").and_then(Value::as_str) {
                Some("accept") => true,
                Some("reject") => false,
                _ => {
                    return Err(ApiError::new(
                        code::REVIEW_INVALID_DECISION,
                        "by_source decision must be accept or reject",
                    ));
                }
            };
            let kinds: Vec<String> = o
                .get("kinds")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if kinds.iter().any(|k| !KINDS.contains(&k.as_str())) {
                return Err(ApiError::bad("/by_source/kinds", "unknown review kind"));
            }
            Ok((
                Vec::new(),
                Some(BySource {
                    file_id,
                    accept,
                    kinds,
                    as_of_seq: o.get("as_of_seq").and_then(Value::as_i64),
                }),
            ))
        }
    }
}

/// `GET /v1/docs/constraints`: the core constraints with their open violations.
pub async fn constraints(app: &Arc<AppState>) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let as_of_seq = super::files::head_seq(&mut conn).await?;
    let open: Vec<(String, i64)> = sqlx::query_as(
        "SELECT constraint_id, count(*)::bigint FROM _oo_docs.constraint_violation \
          WHERE status = 'open' GROUP BY 1",
    )
    .fetch_all(&mut *conn)
    .await?;
    let items: Vec<Value> = crate::publish::constraints::CORE
        .iter()
        .map(|c| {
            let n = open
                .iter()
                .find(|(id, _)| id == c.id)
                .map_or(0, |(_, n)| *n);
            json!({"id": c.id, "core": c.core, "description": c.description, "violations": n})
        })
        .collect();
    Ok(http::json(
        200,
        &json!({"items": items, "next_cursor": null, "as_of_seq": as_of_seq}),
    ))
}

struct BySource {
    file_id: String,
    accept: bool,
    kinds: Vec<String>,
    as_of_seq: Option<i64>,
}

/// `POST /v1/docs/review:decide`.
pub async fn decide(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let key = idem::key_from(&req.headers)?;
    let sha = idem::body_sha256(&req.body);
    let route = "POST /v1/docs/review:decide";
    let mut conn = app.pool.acquire().await?;
    if let Some(stored) = idem::lookup(&mut conn, route, &key, &sha).await? {
        return Ok(http::json(stored.status, &stored.response));
    }
    let body = parse_json(&req.body)?;
    let (items, by_source) = parse_request(&body)?;
    let by = "api";
    let (results, accepted, rejected, publish_pending, files) = match by_source {
        Some(b) => {
            let out = decide_by_source(&mut conn, by, &b.file_id, &b.kinds, b.accept, b.as_of_seq)
                .await?;
            let n = out.accepted + out.rejected;
            let results: Vec<Value> = Vec::new();
            (
                results,
                out.accepted,
                out.rejected,
                b.accept && n > 0,
                vec![b.file_id],
            )
        }
        None => {
            let out = decide_items(&mut conn, by, &items).await?;
            let results: Vec<Value> = out
                .results
                .iter()
                .map(|r| {
                    let mut v = json!({"id": r.id, "status": r.status});
                    if let Some(c) = r.code {
                        v["code"] = json!(c);
                    }
                    if let Some(rev) = r.current_revision {
                        v["current_revision"] = json!(rev);
                    }
                    v
                })
                .collect();
            (
                results,
                out.accepted,
                out.rejected,
                out.publish_pending,
                out.files.into_iter().collect(),
            )
        }
    };
    let body = json!({
        "results": results, "accepted": accepted, "rejected": rejected,
        "publish_pending": publish_pending,
    });
    if accepted + rejected > 0 {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        refresh_pending_counts(&mut tx).await?;
        if publish_pending {
            crate::publish::debounce::schedule_publish(&mut tx).await?;
        }
        let ids: Vec<&str> = body["results"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|r| r["status"] == "ok")
                    .filter_map(|r| r["id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        events::append_event(
            &mut tx,
            "review.decided",
            None,
            None,
            &json!({"accepted": accepted, "rejected": rejected, "item_ids": ids}).to_string(),
        )
        .await?;
        // One `review.changed` per Drive the decision touched (10 8.5): that Drive's queue by kind and
        // the files whose items changed.
        let touched: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT drive_id::text FROM _oo_docs.review_item WHERE id = ANY($1::text[]::uuid[]) \
             UNION SELECT drive_id::text FROM _oo_docs.file WHERE id = ANY($2::text[]::uuid[])",
        )
        .bind(&ids)
        .bind(&files)
        .fetch_all(&mut *tx)
        .await?;
        for drive in touched {
            let pending: Vec<(String, i64)> = sqlx::query_as(
                "SELECT kind, count(*)::bigint FROM _oo_docs.review_item \
                  WHERE status = 'pending' AND kind <> 'speaker_identity' AND drive_id IS NOT DISTINCT FROM $1::uuid GROUP BY kind",
            )
            .bind(&drive)
            .fetch_all(&mut *tx)
            .await?;
            let pending: serde_json::Map<String, Value> =
                pending.into_iter().map(|(k, n)| (k, json!(n))).collect();
            let changed: Vec<String> = sqlx::query_scalar(
                "SELECT id::text FROM _oo_docs.file WHERE id = ANY($1::text[]::uuid[]) \
                   AND drive_id IS NOT DISTINCT FROM $2::uuid ORDER BY id LIMIT 100",
            )
            .bind(&files)
            .bind(&drive)
            .fetch_all(&mut *tx)
            .await?;
            events::append_event(
                &mut tx,
                "review.changed",
                drive.as_deref(),
                None,
                &json!({"drive_id": drive, "pending": pending, "by_source_changed": changed})
                    .to_string(),
            )
            .await?;
        }
        tx.commit().await?;
        app.events.notify_waiters();
        app.wake.notify_waiters();
    }
    idem::store(&mut conn, route, &key, &sha, 200, &body).await?;
    Ok(http::json(200, &body))
}
