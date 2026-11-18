//! Submitting, applying and deciding suggestions (10 §11.1, B-S7-03).
//!
//! `submit` gathers the facts about the world ([`Findings`]), asks `rules` and
//! either applies the suggestion in the same transaction (auto accept, silent
//! supersession) or opens one `suggestion` review item. A fact made by a
//! suggestion has `origin = suggest`, confidence 8000 and copies of the
//! evidence blocks the record cited inside the share's scope (DOC-G2); an
//! `overlay` row `fact_assert` makes it eligible like any person's assertion.
//! OO never writes a Drive file: the file-level change is a draft (`diff`).

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::impact;
use super::rules::{self, Findings, Outcome, Rule};
use super::schema::{Evidence, Suggestion};
use crate::api::error::{ApiError, code};
use crate::api::http::FMT;
use crate::compile::source::{split_tag, tags_in};
use crate::resolve::normalize::{fact_key, norm};
use crate::review::overlay;

pub const BY_AUTO: &str = "suggest:auto";
pub const BY_HUMAN: &str = "api";
const CONFIDENCE_BP: i32 = 8000;

/// The share a suggestion comes from.
#[derive(Clone, Debug)]
pub struct ShareCtx {
    pub id: String,
    pub drive_id: String,
    pub role: String,
    pub scope: Vec<String>,
}

/// `path` lies in `scope` (empty means the whole drive).
pub fn path_in_scope(path: &str, scope: &[String]) -> bool {
    scope.is_empty()
        || scope
            .iter()
            .any(|p| path == p || path.starts_with(&format!("{}/", p.trim_end_matches('/'))))
}

/// Load the share; it must be live and not a viewer.
pub async fn load_share(conn: &mut PgConnection, sid: &str) -> Result<ShareCtx, ApiError> {
    if !crate::api::http::is_uuid(sid) {
        return Err(crate::share::lifecycle::share_not_found(sid));
    }
    let row: Option<(String, String, String, String, bool, String)> = sqlx::query_as(
        "SELECT id::text, drive_id::text, role, status, expires_at <= clock_timestamp(), scope::text \
           FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, drive_id, role, status, late, scope)) = row else {
        return Err(crate::share::lifecycle::share_not_found(sid));
    };
    if !matches!(status.as_str(), "active" | "drifted") || late {
        return Err(
            ApiError::new(code::SHARE_REFS_CREDENTIAL_INVALID, "the share has ended")
                .details(json!({"state": status})),
        );
    }
    if role == "viewer" {
        return Err(ApiError::new(
            code::SUGGEST_ROLE_FORBIDDEN,
            "a viewer share cannot suggest",
        )
        .details(json!({"role": role})));
    }
    Ok(ShareCtx {
        id,
        drive_id,
        role,
        scope: serde_json::from_str(&scope).unwrap_or_default(),
    })
}

/// One resolved evidence block.
#[derive(Clone, Debug)]
pub struct EvRow {
    pub version_id: String,
    pub b3: String,
    pub block_id: String,
    pub anchor: Value,
    pub quote_b3: String,
    pub path: String,
    pub file_id: String,
    pub frag: String,
}

fn frag_of(anchor: &Value) -> String {
    serde_json::from_value::<oo_core::docir::Anchor>(anchor.clone())
        .map(|a| crate::compile::source::frag(&a))
        .unwrap_or_default()
}

/// Path and optional fragment of one evidence reference.
fn parse_ref(e: &Evidence) -> Option<(String, Option<String>)> {
    let body = match e.kind.as_str() {
        "brief_line" => tags_in(&e.reference)
            .first()
            .map(|t| (*t).to_owned())
            .or_else(|| Some(e.reference.clone()))?,
        "ref" => e
            .reference
            .trim_start_matches("[src:")
            .trim_end_matches(']')
            .to_owned(),
        _ => return None,
    };
    match split_tag(&body) {
        Some((p, f)) => Some((p, Some(f))),
        None => Some((
            crate::compile::source::decode_path(&body).unwrap_or(body),
            None,
        )),
    }
}

/// Evidence blocks the references name, inside the scope.
pub async fn resolve_evidence(
    conn: &mut PgConnection,
    share: &ShareCtx,
    list: &[Evidence],
) -> Result<Vec<EvRow>, ApiError> {
    let mut out: Vec<EvRow> = Vec::new();
    for e in list {
        let Some((path, frag)) = parse_ref(e) else {
            continue;
        };
        if !path_in_scope(&path, &share.scope) {
            continue;
        }
        let rows = sqlx::query(
            "SELECT e.version_id::text AS version_id, e.b3, e.block_id, e.anchor::text AS anchor, e.quote_b3, \
                    f.path, f.id::text AS file_id \
               FROM _oo_docs.file f \
               JOIN _oo_docs.file_version v ON v.file_id = f.id AND v.superseded_at IS NULL AND v.deleted_at IS NULL \
               JOIN _oo_docs.evidence e ON e.version_id = v.id AND e.withdrawn_at IS NULL \
              WHERE f.drive_id = $1::uuid AND f.path = $2 AND f.state NOT IN ('deleted') \
              ORDER BY e.block_id, e.id LIMIT 400",
        )
        .bind(&share.drive_id)
        .bind(&path)
        .fetch_all(&mut *conn)
        .await?;
        for r in rows {
            let anchor: Value =
                serde_json::from_str(&r.try_get::<String, _>("anchor")?).unwrap_or(Value::Null);
            let f = frag_of(&anchor);
            if frag.as_ref().is_some_and(|want| *want != f) {
                continue;
            }
            let row = EvRow {
                version_id: r.try_get("version_id")?,
                b3: r.try_get("b3")?,
                block_id: r.try_get("block_id")?,
                anchor,
                quote_b3: r.try_get("quote_b3")?,
                path: r.try_get("path")?,
                file_id: r.try_get("file_id")?,
                frag: f,
            };
            if !out
                .iter()
                .any(|x| x.block_id == row.block_id && x.version_id == row.version_id)
            {
                out.push(row);
            }
            if out.len() >= 8 {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

struct Ent {
    id: String,
    key: String,
}

async fn resolve_entity(conn: &mut PgConnection, name: &str) -> Result<Option<Ent>, ApiError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id::text, key FROM _oo_docs.entity WHERE status = 'active' AND (key = $1 OR norm_name = $2) \
          ORDER BY key LIMIT 2",
    )
    .bind(name)
    .bind(norm(name))
    .fetch_all(&mut *conn)
    .await?;
    Ok(match rows.as_slice() {
        [(id, key)] => Some(Ent {
            id: id.clone(),
            key: key.clone(),
        }),
        _ => None,
    })
}

fn value_json(s: &Suggestion) -> Option<Value> {
    s.value.as_ref().map(|v| {
        let date = v.len() == 10 && v.as_bytes()[4] == b'-' && v.as_bytes()[7] == b'-';
        if s.predicate.starts_with("attr.") && date {
            json!({"date": v})
        } else {
            json!({"text": v})
        }
    })
}

/// Everything `submit` and `accept` need about one record.
struct Resolved {
    subject: Option<Ent>,
    object: Option<Ent>,
    fact_key: Option<String>,
    value: Option<Value>,
    evidence: Vec<EvRow>,
    old_fact: Option<(String, String)>,
    findings: Findings,
}

async fn old_fact_of(
    conn: &mut PgConnection,
    share: &ShareCtx,
    s: &Suggestion,
    subject: Option<&Ent>,
) -> Result<Option<(String, String)>, ApiError> {
    let Some(sup) = s.supersedes.as_deref() else {
        return Ok(None);
    };
    if sup.len() == 64 && sup.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(sqlx::query_as(
            "SELECT id::text, fact_key FROM _oo_docs.fact WHERE fact_key = $1 AND status = 'published' \
              ORDER BY recorded_at DESC LIMIT 1",
        )
        .bind(sup.to_ascii_lowercase())
        .fetch_optional(&mut *conn)
        .await?);
    }
    let ev = resolve_evidence(
        conn,
        share,
        &[Evidence {
            kind: "brief_line".into(),
            reference: sup.to_owned(),
        }],
    )
    .await?;
    // Best match first: same subject and predicate, then the same predicate, then the only fact on the block.
    let mut found: Vec<(String, String, u8)> = Vec::new();
    for e in &ev {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT f.id::text, f.fact_key, f.subject_id::text, f.predicate FROM _oo_docs.evidence x \
               JOIN _oo_docs.fact f ON x.target_kind = 'fact' AND x.target_id = f.id AND f.status = 'published' \
              WHERE x.version_id = $1::uuid AND x.block_id = $2 AND x.withdrawn_at IS NULL \
              ORDER BY f.fact_key",
        )
        .bind(&e.version_id)
        .bind(&e.block_id)
        .fetch_all(&mut *conn)
        .await?;
        for (id, key, subj, pred) in rows {
            let tier = match (pred == s.predicate, subject.is_some_and(|x| x.id == subj)) {
                (true, true) => 2,
                (true, false) => 1,
                _ => 0,
            };
            found.push((id, key, tier));
        }
    }
    let best = found.iter().map(|f| f.2).max().unwrap_or(0);
    let top: Vec<&(String, String, u8)> = found.iter().filter(|f| f.2 == best).collect();
    let pick = if best > 0 {
        top.first().copied()
    } else {
        (found.len() == 1).then(|| &found[0])
    };
    Ok(pick.map(|f| (f.0.clone(), f.1.clone())))
}

async fn resolve(
    conn: &mut PgConnection,
    share: &ShareCtx,
    s: &Suggestion,
) -> Result<Resolved, ApiError> {
    let fact_kind = matches!(s.kind.as_str(), "new_fact" | "supersede" | "correction");
    let mut r = Resolved {
        subject: None,
        object: None,
        fact_key: None,
        value: None,
        evidence: Vec::new(),
        old_fact: None,
        findings: Findings {
            fact_kind,
            ..Findings::default()
        },
    };
    if !fact_kind {
        return Ok(r);
    }
    r.subject = resolve_entity(conn, &s.subject).await?;
    if let Some(o) = &s.object {
        r.object = resolve_entity(conn, o).await?;
    }
    r.value = value_json(s);
    r.findings.entities_resolved =
        r.subject.is_some() && (s.object.is_none() || r.object.is_some());
    r.evidence = resolve_evidence(conn, share, &s.evidence).await?;
    r.findings.evidence_in_scope = !r.evidence.is_empty();
    let Some(subj) = r.subject.as_ref().filter(|_| r.findings.entities_resolved) else {
        return Ok(r);
    };
    let key = fact_key(
        &subj.key,
        &s.predicate,
        r.object.as_ref().map(|o| o.key.as_str()),
        r.value.as_ref(),
        s.valid_from.as_deref(),
    );
    r.findings.already_known = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.fact WHERE fact_key = $1 AND status IN ('draft','published'))",
    )
    .bind(&key)
    .fetch_one(&mut *conn)
    .await?;
    if s.kind == "new_fact" && rules::functional_shape(&s.predicate, s.object.is_some()) {
        r.findings.functional_conflict = if let Some(o) = &r.object {
            sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM _oo_docs.fact WHERE predicate = $1 AND object_id = $2::uuid \
                   AND subject_id <> $3::uuid AND status = 'published' AND valid_to IS NULL AND fact_key <> $4)",
            )
            .bind(&s.predicate)
            .bind(&o.id)
            .bind(&subj.id)
            .bind(&key)
            .fetch_one(&mut *conn)
            .await?
        } else {
            sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM _oo_docs.fact WHERE predicate = $1 AND subject_id = $2::uuid \
                   AND status = 'published' AND valid_to IS NULL AND fact_key <> $3)",
            )
            .bind(&s.predicate)
            .bind(&subj.id)
            .bind(&key)
            .fetch_one(&mut *conn)
            .await?
        };
    }
    r.fact_key = Some(key);
    if matches!(s.kind.as_str(), "supersede" | "correction") {
        r.old_fact = old_fact_of(conn, share, s, Some(subj)).await?;
        r.findings.old_found = r.old_fact.is_some();
        if let Some((old, _)) = &r.old_fact {
            r.findings.old_in_use = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM _oo_docs.share_fact_use WHERE fact_id = $1::uuid)",
            )
            .bind(old)
            .fetch_one(&mut *conn)
            .await?;
        }
    }
    Ok(r)
}

async fn insert_fact(
    conn: &mut PgConnection,
    r: &Resolved,
    s: &Suggestion,
    extra_evidence: &[EvRow],
) -> Result<String, ApiError> {
    let (Some(subj), Some(key)) = (&r.subject, &r.fact_key) else {
        return Err(ApiError::new(
            code::SUGGEST_UNRESOLVED_ENTITY,
            "subject or object is not a known entity",
        ));
    };
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.fact WHERE fact_key = $1 AND status IN ('draft','published') \
          ORDER BY recorded_at LIMIT 1",
    )
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    let id = match existing {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, object_id, \
                    original_object_id, value, valid_from, valid_precision, confidence_bp, origin, status) \
                 VALUES (gen_random_uuid(), $1, $2::uuid, $2::uuid, $3, $4::uuid, $4::uuid, $5::jsonb, $6::date, \
                         CASE WHEN $6::date IS NULL THEN NULL ELSE 'day' END, $7, 'suggest', 'draft') \
                 RETURNING id::text",
            )
            .bind(key)
            .bind(&subj.id)
            .bind(&s.predicate)
            .bind(r.object.as_ref().map(|o| o.id.as_str()))
            .bind(r.value.as_ref().map(Value::to_string))
            .bind(s.valid_from.as_deref())
            .bind(CONFIDENCE_BP)
            .fetch_one(&mut *conn)
            .await?
        }
    };
    for e in r.evidence.iter().chain(extra_evidence) {
        sqlx::query(
            "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
             SELECT gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, $4, $5::jsonb, $6, 'suggest' \
              WHERE NOT EXISTS (SELECT 1 FROM _oo_docs.evidence x WHERE x.target_kind = 'fact' AND x.target_id = $1::uuid \
                                 AND x.version_id = $2::uuid AND x.block_id = $4 AND x.withdrawn_at IS NULL)",
        )
        .bind(&id)
        .bind(&e.version_id)
        .bind(&e.b3)
        .bind(&e.block_id)
        .bind(e.anchor.to_string())
        .bind(&e.quote_b3)
        .execute(&mut *conn)
        .await?;
    }
    Ok(id)
}

/// Evidence of the old fact, for a person's accept of a record that cited none inside the scope.
async fn old_evidence(conn: &mut PgConnection, old: &str) -> Result<Vec<EvRow>, ApiError> {
    let rows = sqlx::query(
        "SELECT e.version_id::text AS version_id, e.b3, e.block_id, e.anchor::text AS anchor, e.quote_b3, \
                f.path, f.id::text AS file_id \
           FROM _oo_docs.evidence e JOIN _oo_docs.file_version v ON v.id = e.version_id \
           JOIN _oo_docs.file f ON f.id = v.file_id \
          WHERE e.target_kind = 'fact' AND e.target_id = $1::uuid AND e.withdrawn_at IS NULL LIMIT 4",
    )
    .bind(old)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            let anchor: Value =
                serde_json::from_str(&r.try_get::<String, _>("anchor")?).unwrap_or(Value::Null);
            Ok(EvRow {
                version_id: r.try_get("version_id")?,
                b3: r.try_get("b3")?,
                block_id: r.try_get("block_id")?,
                frag: frag_of(&anchor),
                anchor,
                quote_b3: r.try_get("quote_b3")?,
                path: r.try_get("path")?,
                file_id: r.try_get("file_id")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

/// Make the suggestion's fact eligible and end or retract the old one.
async fn apply_fact(
    conn: &mut PgConnection,
    r: &Resolved,
    s: &Suggestion,
    sid: &str,
    by: &str,
    review_id: Option<&str>,
) -> Result<String, ApiError> {
    let extra = if r.evidence.is_empty() {
        match &r.old_fact {
            Some((old, _)) => old_evidence(conn, old).await?,
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    if r.evidence.is_empty() && extra.is_empty() {
        return Err(ApiError::new(
            code::SUGGEST_NO_EVIDENCE,
            "no evidence inside the share's scope",
        ));
    }
    let fid = insert_fact(conn, r, s, &extra).await?;
    let key = r.fact_key.clone().unwrap_or_default();
    overlay::insert(
        conn,
        "fact_assert",
        &key,
        json!({"fact_id": fid, "suggestion_id": sid}),
        by,
        review_id,
        "active",
        None,
    )
    .await?;
    if let Some((old, old_key)) = &r.old_fact {
        if s.kind == "correction" {
            overlay::insert(
                conn,
                "fact_reject",
                old_key,
                json!({"fact_id": old, "because": fid, "suggestion_id": sid}),
                by,
                review_id,
                "active",
                None,
            )
            .await?;
            sqlx::query(
                "UPDATE _oo_docs.fact SET status = 'retracted', retracted_at = clock_timestamp(), \
                        revision = revision + 1 WHERE id = $1::uuid AND status = 'published'",
            )
            .bind(old)
            .execute(&mut *conn)
            .await?;
        } else {
            sqlx::query(
                "UPDATE _oo_docs.fact o SET valid_to = n.valid_from, superseded_by = n.id, revision = o.revision + 1 \
                   FROM _oo_docs.fact n WHERE n.id = $1::uuid AND o.id = $2::uuid AND n.valid_from IS NOT NULL \
                    AND (o.valid_to IS NULL OR o.valid_to > n.valid_from) \
                    AND (o.valid_from IS NULL OR o.valid_from < n.valid_from)",
            )
            .bind(&fid)
            .bind(old)
            .execute(&mut *conn)
            .await?;
        }
    }
    crate::publish::debounce::schedule_publish(&mut *conn).await?;
    Ok(fid)
}

const SELECT_VIEW: &str = "SELECT id::text AS id, share_id::text AS share_id, drive_id::text AS drive_id, kind, \
    payload::text AS payload, status, rule, reason, review_id::text AS review_id, fact_id::text AS fact_id, \
    old_fact_id::text AS old_fact_id, credential_id::text AS credential_id, submitted_by, revision, \
    to_char(created_at AT TIME ZONE 'UTC', {FMT}) AS created_at, \
    to_char(decided_at AT TIME ZONE 'UTC', {FMT}) AS decided_at, decided_by FROM _oo_docs.suggestion";

fn view_sql(tail: &str) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(format!("{} {tail}", SELECT_VIEW.replace("{FMT}", FMT)))
}

fn row_view(r: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    let payload: Value =
        serde_json::from_str(&r.try_get::<String, _>("payload")?).unwrap_or(Value::Null);
    Ok(json!({
        "id": r.try_get::<String, _>("id")?, "share_id": r.try_get::<String, _>("share_id")?,
        "drive_id": r.try_get::<String, _>("drive_id")?, "kind": r.try_get::<String, _>("kind")?,
        "status": r.try_get::<String, _>("status")?, "rule": r.try_get::<String, _>("rule")?,
        "reason": r.try_get::<Option<String>, _>("reason")?,
        "review_id": r.try_get::<Option<String>, _>("review_id")?,
        "fact_id": r.try_get::<Option<String>, _>("fact_id")?,
        "old_fact_id": r.try_get::<Option<String>, _>("old_fact_id")?,
        "credential_id": r.try_get::<Option<String>, _>("credential_id")?,
        "submitted_by": r.try_get::<String, _>("submitted_by")?,
        "revision": r.try_get::<i64, _>("revision")?, "payload": payload,
        "created_at": r.try_get::<String, _>("created_at")?,
        "decided_at": r.try_get::<Option<String>, _>("decided_at")?,
        "decided_by": r.try_get::<Option<String>, _>("decided_by")?,
    }))
}

/// One suggestion as the API shows it, with the live impact for a pending one.
pub async fn get(conn: &mut PgConnection, id: &str) -> Result<Value, ApiError> {
    if !crate::api::http::is_uuid(id) {
        return Err(ApiError::new(
            code::SUGGEST_NOT_FOUND,
            "suggestion not found",
        ));
    }
    let row = sqlx::query(view_sql("WHERE id = $1::uuid"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| ApiError::new(code::SUGGEST_NOT_FOUND, "suggestion not found"))?;
    let mut v = row_view(&row)?;
    let facts: Vec<String> = [v["old_fact_id"].as_str(), v["fact_id"].as_str()]
        .into_iter()
        .flatten()
        .map(str::to_owned)
        .collect();
    v["impact"] = json!({"shares": impact::affected(conn, &facts, None).await?});
    Ok(v)
}

/// List with filters: `status`, `share_id`, `day` (UTC date), `limit`.
pub async fn list(
    conn: &mut PgConnection,
    status: Option<&str>,
    share_id: Option<&str>,
    day: Option<&str>,
    limit: i64,
) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(view_sql(
        "WHERE ($1::text IS NULL OR status = $1) AND ($2::text IS NULL OR share_id = $2::uuid) \
           AND ($3::text IS NULL OR (created_at AT TIME ZONE 'UTC')::date = $3::date) \
         ORDER BY created_at, id LIMIT $4",
    ))
    .bind(status)
    .bind(share_id)
    .bind(day)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(|r| Ok(row_view(r)?)).collect()
}

/// Insert, apply or queue one record. Replaying the same record of the same share is a no-op that
/// returns the stored suggestion (`"replayed": true`).
pub async fn submit(
    conn: &mut PgConnection,
    share: &ShareCtx,
    s: &Suggestion,
    cred_id: Option<&str>,
    submitted_by: &str,
) -> Result<Value, ApiError> {
    let dedupe = s.dedupe_key();
    let prior: Option<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.suggestion WHERE share_id = $1::uuid AND idempotency_key = $2",
    )
    .bind(&share.id)
    .bind(&dedupe)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = prior {
        let mut v = get(conn, &id).await?;
        v["replayed"] = json!(true);
        return Ok(v);
    }
    let r = resolve(conn, share, s).await?;
    let outcome = rules::decide(&s.kind, &r.findings);
    let (status, rule, reason): (&str, Rule, Option<&str>) = match &outcome {
        Outcome::Apply { rule, status } => (status, *rule, None),
        Outcome::Duplicate => ("duplicate", Rule::NewFact, None),
        Outcome::Review { rule, reason } => ("pending", *rule, Some(*reason)),
    };
    let id: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.suggestion (id, share_id, drive_id, kind, payload, status, rule, reason, idempotency_key, \
            old_fact_id, credential_id, submitted_by) \
         VALUES (gen_random_uuid(), $1::uuid, $2::uuid, $3, $4::jsonb, $5, $6, $7, $8, $9::uuid, $10::uuid, $11) \
         RETURNING id::text",
    )
    .bind(&share.id)
    .bind(&share.drive_id)
    .bind(&s.kind)
    .bind(s.payload().to_string())
    .bind(status)
    .bind(rule.as_str())
    .bind(reason)
    .bind(&dedupe)
    .bind(r.old_fact.as_ref().map(|o| o.0.as_str()))
    .bind(cred_id)
    .bind(submitted_by)
    .fetch_one(&mut *conn)
    .await?;
    match outcome {
        Outcome::Apply { .. } => {
            let fid = apply_fact(conn, &r, s, &id, BY_AUTO, None).await?;
            sqlx::query(
                "UPDATE _oo_docs.suggestion SET fact_id = $2::uuid, decided_at = clock_timestamp(), decided_by = $3 \
                  WHERE id = $1::uuid",
            )
            .bind(&id)
            .bind(&fid)
            .bind(BY_AUTO)
            .execute(&mut *conn)
            .await?;
        }
        Outcome::Review { .. } => {
            let review: String = sqlx::query_scalar(
                "INSERT INTO _oo_docs.review_item (id, kind, dedupe_key, drive_id, subject, confidence_bp, reason_code) \
                 VALUES (gen_random_uuid(), 'suggestion', $1, $2::uuid, $3::jsonb, $4, $5) RETURNING id::text",
            )
            .bind(format!("suggestion:{id}"))
            .bind(&share.drive_id)
            .bind(json!({"suggestion_id": id, "share_id": share.id, "kind": s.kind}).to_string())
            .bind(CONFIDENCE_BP)
            .bind(reason.unwrap_or("suggestion_manual_kind"))
            .fetch_one(&mut *conn)
            .await?;
            sqlx::query("UPDATE _oo_docs.suggestion SET review_id = $2::uuid WHERE id = $1::uuid")
                .bind(&id)
                .bind(&review)
                .execute(&mut *conn)
                .await?;
        }
        Outcome::Duplicate => {}
    }
    get(conn, &id).await
}

/// A person's decision on a pending suggestion (`accept`, `reject`) or the undo of an applied one.
/// `by_review` is the review item id when the call comes through `review:decide`.
pub async fn decide(
    conn: &mut PgConnection,
    id: &str,
    decision: &str,
    revision: Option<i64>,
    by: &str,
    note: Option<&str>,
) -> Result<Value, ApiError> {
    if !crate::api::http::is_uuid(id) {
        return Err(ApiError::new(
            code::SUGGEST_NOT_FOUND,
            "suggestion not found",
        ));
    }
    let row = sqlx::query(view_sql("WHERE id = $1::uuid FOR UPDATE"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| ApiError::new(code::SUGGEST_NOT_FOUND, "suggestion not found"))?;
    let cur = row_view(&row)?;
    if let Some(rev) = revision
        && cur["revision"].as_i64() != Some(rev)
    {
        return Err(
            ApiError::new(code::PRECONDITION_FAILED, "the suggestion changed")
                .details(json!({"current_revision": cur["revision"]})),
        );
    }
    let status = cur["status"].as_str().unwrap_or_default();
    let review_id = cur["review_id"].as_str().map(str::to_owned);
    let share_id = cur["share_id"].as_str().unwrap_or_default().to_owned();
    let new_status = match (decision, status) {
        ("accept", "pending") => {
            let share = load_share_any(conn, &share_id).await?;
            let s = super::schema::validate(&cur["payload"])
                .map_err(|e| ApiError::bad("/payload", e))?;
            if matches!(s.kind.as_str(), "new_fact" | "supersede" | "correction") {
                let r = resolve(conn, &share, &s).await?;
                if !r.findings.entities_resolved {
                    return Err(ApiError::new(
                        code::SUGGEST_UNRESOLVED_ENTITY,
                        "subject or object is not a known entity",
                    ));
                }
                let used = impact::affected_shares_of_fact(
                    conn,
                    r.old_fact.as_ref().map(|o| o.0.as_str()),
                )
                .await?;
                let fid = apply_fact(conn, &r, &s, id, by, review_id.as_deref()).await?;
                sqlx::query("UPDATE _oo_docs.suggestion SET fact_id = $2::uuid, old_fact_id = $3::uuid WHERE id = $1::uuid")
                    .bind(id)
                    .bind(&fid)
                    .bind(r.old_fact.as_ref().map(|o| o.0.as_str()))
                    .execute(&mut *conn)
                    .await?;
                impact::broadcast(
                    conn,
                    id,
                    &used,
                    r.old_fact.as_ref().map(|o| o.1.as_str()),
                    &fid,
                )
                .await?;
            } else {
                overlay::insert(
                    conn,
                    "suggestion_accept",
                    &format!("suggestion:{id}"),
                    json!({"suggestion_id": id, "kind": s.kind, "subject": s.subject, "predicate": s.predicate,
                           "value": s.value}),
                    by,
                    review_id.as_deref(),
                    "active",
                    None,
                )
                .await?;
            }
            "accepted"
        }
        ("reject", "pending") => "rejected",
        ("undo", "auto_accepted" | "superseded_silently" | "accepted") => {
            undo(conn, &cur, by).await?;
            "undone"
        }
        _ => {
            return Err(ApiError::new(
                code::SUGGEST_NOT_PENDING,
                "the suggestion cannot take this decision",
            )
            .details(json!({"status": status, "decision": decision})));
        }
    };
    sqlx::query(
        "UPDATE _oo_docs.suggestion SET status = $2, decided_at = clock_timestamp(), decided_by = $3, \
                decision_note = $4, revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(id)
    .bind(new_status)
    .bind(by)
    .bind(note)
    .execute(&mut *conn)
    .await?;
    if let Some(rid) = review_id {
        let item = match new_status {
            "accepted" => Some("accepted"),
            "rejected" => Some("rejected"),
            _ => None,
        };
        if let Some(item) = item {
            sqlx::query(
                "UPDATE _oo_docs.review_item SET status = $2, decided_at = clock_timestamp(), decided_by = $3, \
                        decision_note = $4, revision = revision + 1 WHERE id = $1::uuid AND status = 'pending'",
            )
            .bind(&rid)
            .bind(item)
            .bind(by)
            .bind(note)
            .execute(&mut *conn)
            .await?;
        }
    }
    get(conn, id).await
}

/// Same as `load_share` but the share may have ended since the suggestion came in.
async fn load_share_any(conn: &mut PgConnection, sid: &str) -> Result<ShareCtx, ApiError> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT id::text, drive_id::text, role, scope::text FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let (id, drive_id, role, scope) =
        row.ok_or_else(|| crate::share::lifecycle::share_not_found(sid))?;
    Ok(ShareCtx {
        id,
        drive_id,
        role,
        scope: serde_json::from_str(&scope).unwrap_or_default(),
    })
}

/// Take back an applied suggestion: its fact is rejected (or retracted when already published) and
/// the old fact it ended is open again.
async fn undo(conn: &mut PgConnection, cur: &Value, by: &str) -> Result<(), ApiError> {
    let Some(fid) = cur["fact_id"].as_str() else {
        return Ok(());
    };
    let key: Option<String> =
        sqlx::query_scalar("SELECT fact_key FROM _oo_docs.fact WHERE id = $1::uuid")
            .bind(fid)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(key) = key else { return Ok(()) };
    overlay::insert(
        conn,
        "fact_reject",
        &key,
        json!({"fact_id": fid, "undo_of_suggestion": cur["id"]}),
        by,
        None,
        "active",
        None,
    )
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.fact SET status = CASE WHEN status = 'published' THEN 'retracted' ELSE 'rejected' END, \
                retracted_at = CASE WHEN status = 'published' THEN clock_timestamp() ELSE retracted_at END, \
                revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(fid)
    .execute(&mut *conn)
    .await?;
    if let Some(old) = cur["old_fact_id"].as_str() {
        sqlx::query(
            "UPDATE _oo_docs.fact SET valid_to = NULL, superseded_by = NULL, revision = revision + 1 \
              WHERE id = $1::uuid AND superseded_by = $2::uuid",
        )
        .bind(old)
        .bind(fid)
        .execute(&mut *conn)
        .await?;
    }
    crate::publish::debounce::schedule_publish(&mut *conn).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_matching() {
        let s = vec!["Projects/a".to_owned()];
        assert!(path_in_scope("Projects/a/x.md", &s));
        assert!(path_in_scope("Projects/a", &s));
        assert!(!path_in_scope("Projects/ab/x.md", &s));
        assert!(!path_in_scope("People/x.md", &s));
        assert!(path_in_scope("anything.md", &[]));
    }

    #[test]
    fn refs_parse_from_both_forms() {
        let e = Evidence {
            kind: "brief_line".into(),
            reference: "- 状态进行中 [src:Projects/a/周会.md#L3-L3]".into(),
        };
        let (p, f) = parse_ref(&e).unwrap();
        assert_eq!(p, "Projects/a/周会.md");
        assert_eq!(f.as_deref(), Some("#L3-L3"));
        let e = Evidence {
            kind: "ref".into(),
            reference: "Projects/a/周会.md".into(),
        };
        assert_eq!(
            parse_ref(&e).unwrap(),
            ("Projects/a/周会.md".to_owned(), None)
        );
        assert!(
            parse_ref(&Evidence {
                kind: "external".into(),
                reference: "https://x".into()
            })
            .is_none()
        );
    }
}
