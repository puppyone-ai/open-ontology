//! Entities and resolve (10 §8.3, §16.9). Board WS-BE3.
//!
//! * `GET /v1/docs/entities?q=&type=&drive_id=&limit&cursor`: active entities
//!   with fact and alias counts, ordered by key;
//! * `GET /v1/docs/entities/{eid}?include=history&as_of=`: `eid` is an id or a
//!   key; a merged entity answers as its redirect target with `redirect_from`
//!   non-empty. `as_of` is `latest` (default) or `v<N>`; the response header
//!   `OO-Docs-As-Of` names the version read. `share` needs a refs credential
//!   and is not served on the control token (400);
//! * `POST /v1/docs/resolve`: read-only version of resolve steps 1 to 5 over
//!   the text: spans that name a stored entity, with scored candidates.
//!
//! Visible facts are the published ones that are not retracted and not ended
//! (`valid_to` empty or after the version's `as_of`); a historic version is
//! read by `published_in <= n < coalesce(retracted_in, infinity)`.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::error::ApiError;
use super::http::{self, FMT, Query, is_uuid, json_with, parse_json};
use super::{AppState, Req, cursor};
use crate::extract::model::{EntityRef, EntityType};
use crate::resolve::exact::{Context, Via, rank};
use crate::resolve::honorific::{stripped_forms, surname_form};
use crate::resolve::normalize::norm;

/// `EntityRef` of the entity row aliased `a`.
pub fn entity_ref_sql(a: &str) -> String {
    format!(
        "jsonb_build_object('id', {a}.id::text, 'key', {a}.key, 'type', {a}.type, \
         'label', {a}.thing_label, 'subkind', {a}.subkind, 'name', {a}.canonical_name)"
    )
}

/// `Fact` (without evidence) of the fact row `ft` with subject `s` and
/// optional object `o`.
pub fn fact_sql() -> String {
    let object_sql = format!(
        "CASE WHEN o.id IS NULL THEN NULL ELSE {} END",
        entity_ref_sql("o")
    );
    format!(
        "jsonb_strip_nulls(jsonb_build_object('id', ft.id::text, 'fact_key', ft.fact_key, \
         'subject', {s}, 'predicate', ft.predicate, 'object', {o}, 'value', ft.value, \
         'valid_from', to_char(ft.valid_from, 'YYYY-MM-DD'), 'valid_to', to_char(ft.valid_to, 'YYYY-MM-DD'), \
         'valid_precision', ft.valid_precision, \
         'recorded_at', to_char(ft.recorded_at AT TIME ZONE 'UTC', {FMT}), \
         'retracted_at', CASE WHEN ft.retracted_at IS NULL THEN NULL ELSE \
             to_char(ft.retracted_at AT TIME ZONE 'UTC', {FMT}) END, \
         'confidence_bp', ft.confidence_bp, 'origin', ft.origin, 'status', ft.status, \
         'published_in', ft.published_in, 'superseded_by', ft.superseded_by::text))",
        s = entity_ref_sql("s"),
        o = object_sql,
    )
}

/// Evidence objects of `target_ids` (at most `per` each), by target id.
pub async fn evidence_for(
    conn: &mut PgConnection,
    kind: &str,
    ids: &[String],
    per: usize,
) -> Result<std::collections::HashMap<String, Vec<Value>>, ApiError> {
    use std::collections::HashMap;
    let rows = sqlx::query(
        "SELECT ev.target_id::text AS target, ev.id::text AS id, fv.file_id::text AS file_id, \
                ev.version_id::text AS version_id, fv.path_at_version AS path, ev.block_id, \
                ev.anchor::text AS anchor, ev.origin \
           FROM _oo_docs.evidence ev JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
          WHERE ev.target_kind = $1 AND ev.target_id = ANY($2::text[]::uuid[]) \
            AND ev.withdrawn_at IS NULL \
          ORDER BY ev.target_id, fv.path_at_version, ev.block_id, ev.id",
    )
    .bind(kind)
    .bind(ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut out: HashMap<String, Vec<Value>> = HashMap::new();
    for r in rows {
        let target: String = r.try_get("target")?;
        let slot = out.entry(target).or_default();
        if slot.len() >= per {
            continue;
        }
        let path: String = r.try_get("path")?;
        let anchor: Value =
            serde_json::from_str(&r.try_get::<String, _>("anchor")?).unwrap_or(Value::Null);
        let src = serde_json::from_value::<oo_core::docir::Anchor>(anchor.clone())
            .map(|a| crate::compile::source::tag(&path, &a))
            .unwrap_or_default();
        slot.push(json!({
            "id": r.try_get::<String, _>("id")?, "file_id": r.try_get::<String, _>("file_id")?,
            "version_id": r.try_get::<String, _>("version_id")?, "path": path,
            "block_id": r.try_get::<String, _>("block_id")?, "anchor": anchor, "src": src,
            "origin": r.try_get::<String, _>("origin")?,
        }));
    }
    Ok(out)
}

/// Version read by a request: its number and `as_of` date, or `None` when no
/// version exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsOf {
    pub version: Option<(i64, String)>,
    /// `v<N>` was asked for (historic) rather than `latest`.
    pub historic: bool,
}

/// Parse `as_of` (`latest` or `v<N>`).
pub async fn read_as_of(conn: &mut PgConnection, q: &Query) -> Result<AsOf, ApiError> {
    let (sql, n) = match q.get("as_of").unwrap_or("latest") {
        "latest" => (
            "SELECT n, as_of::text FROM _oo_docs.ontology_version ORDER BY n DESC LIMIT 1",
            None,
        ),
        "share" => {
            return Err(ApiError::bad(
                "/query/as_of",
                "as_of=share needs a refs credential",
            ));
        }
        v => match v.strip_prefix('v').and_then(|n| n.parse::<i64>().ok()) {
            Some(n) if n >= 1 => (
                "SELECT n, as_of::text FROM _oo_docs.ontology_version WHERE n = $1",
                Some(n),
            ),
            _ => return Err(ApiError::bad("/query/as_of", "expected latest or v<N>")),
        },
    };
    let mut query = sqlx::query_as::<_, (i64, String)>(sql);
    if let Some(n) = n {
        query = query.bind(n);
    }
    let version = query.fetch_optional(&mut *conn).await?;
    if version.is_none() && n.is_some() {
        return Err(ApiError::not_found("ontology version"));
    }
    Ok(AsOf {
        version,
        historic: n.is_some(),
    })
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// `GET /v1/docs/entities`.
pub async fn list(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let q = &req.query;
    let limit = q.limit()?;
    let ty = q.get("type").filter(|t| !t.is_empty());
    if let Some(t) = ty
        && EntityType::parse(t).is_none()
    {
        return Err(ApiError::bad("/query/type", "unknown entity type"));
    }
    let drive = q.get("drive_id").filter(|d| !d.is_empty());
    if let Some(d) = drive
        && !is_uuid(d)
    {
        return Err(ApiError::bad("/query/drive_id", "expected a uuid"));
    }
    let needle = q.get("q").filter(|s| !s.is_empty()).map(norm);
    let filters = format!(
        "entities|{}|{}|{}",
        ty.unwrap_or(""),
        drive.unwrap_or(""),
        needle.as_deref().unwrap_or("")
    );
    let after = match q.get("cursor") {
        Some(t) => cursor::decode(t, &filters)?
            .first()
            .and_then(Value::as_str)
            .map(str::to_owned),
        None => None,
    };
    let mut conn = app.pool.acquire().await?;
    let as_of_seq = super::files::head_seq(&mut conn).await?;
    let sql = format!(
        "SELECT e.key AS k, jsonb_build_object('id', e.id::text, 'key', e.key, 'type', e.type, \
                'label', e.thing_label, 'subkind', e.subkind, 'name', e.canonical_name, \
                'counts', jsonb_build_object( \
                   'facts', (SELECT count(*) FROM _oo_docs.fact ft WHERE (ft.subject_id = e.id OR ft.object_id = e.id) \
                              AND ft.status = 'published' AND ft.retracted_in IS NULL), \
                   'aliases', (SELECT count(*) FROM _oo_docs.alias a WHERE a.entity_id = e.id \
                                AND a.status IN ('draft', 'published'))))::text AS j \
           FROM _oo_docs.entity e \
          WHERE e.status = 'active' AND ($1::text IS NULL OR e.type = $1) \
            AND ($2::text IS NULL OR strpos(e.norm_name, $2) > 0 OR EXISTS ( \
                   SELECT 1 FROM _oo_docs.alias a WHERE a.entity_id = e.id AND strpos(a.norm_alias, $2) > 0)) \
            AND ($3::uuid IS NULL OR EXISTS ( \
                   SELECT 1 FROM _oo_docs.evidence ev JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
                     JOIN _oo_docs.file f ON f.id = fv.file_id \
                    WHERE ev.target_kind = 'entity' AND ev.target_id = e.id AND ev.withdrawn_at IS NULL \
                      AND f.drive_id = $3::uuid)) \
            AND ($4::text IS NULL OR e.key > $4) \
          ORDER BY e.key LIMIT {}",
        limit + 1
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(ty)
        .bind(needle.as_deref())
        .bind(drive)
        .bind(after.as_deref())
        .fetch_all(&mut *conn)
        .await?;
    let mut items = Vec::new();
    let mut last = None;
    for r in rows.iter().take(limit as usize) {
        items.push(
            serde_json::from_str::<Value>(&r.try_get::<String, _>("j")?).unwrap_or(Value::Null),
        );
        last = Some(r.try_get::<String, _>("k")?);
    }
    let next = if rows.len() as i64 > limit {
        last.map(|k| cursor::encode(&[json!(k)], &filters))
    } else {
        None
    };
    let _ = like_escape;
    Ok(http::json(
        200,
        &json!({"items": items, "next_cursor": next, "as_of_seq": as_of_seq}),
    ))
}

/// `GET /v1/docs/entities/{eid}`.
pub async fn detail(app: &Arc<AppState>, eid: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    let history = req
        .query
        .get("include")
        .is_some_and(|v| v.split(',').any(|x| x == "history"));
    let mut conn = app.pool.acquire().await?;
    let as_of = read_as_of(&mut conn, &req.query).await?;
    let not_found = || ApiError::not_found("entity");
    // id or key; a key that was merged away goes through the redirect.
    let by_key = !is_uuid(eid);
    let row = if by_key {
        sqlx::query(
            "SELECT e.id::text AS id, e.key FROM _oo_docs.entity e WHERE e.key = \
               COALESCE((SELECT r.to_key FROM _oo_docs.entity_redirect r WHERE r.from_key = $1), $1)",
        )
        .bind(eid)
        .fetch_optional(&mut *conn)
        .await?
    } else {
        sqlx::query(
            "SELECT e.id::text AS id, e.key FROM _oo_docs.entity e WHERE e.key = \
               COALESCE((SELECT r.to_key FROM _oo_docs.entity_redirect r \
                          JOIN _oo_docs.entity x ON x.key = r.from_key WHERE x.id = $1::uuid), \
                        (SELECT y.key FROM _oo_docs.entity y WHERE y.id = $1::uuid))",
        )
        .bind(eid)
        .fetch_optional(&mut *conn)
        .await?
    };
    let Some(row) = row else {
        return Err(not_found());
    };
    let (id, key): (String, String) = (row.try_get("id")?, row.try_get("key")?);
    let head = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {ref_e}::text AS j, e.status, e.identity::text AS identity, e.revision, \
                (SELECT m.key FROM _oo_docs.entity m WHERE m.id = e.merged_into) AS merged_into \
           FROM _oo_docs.entity e WHERE e.id = $1::uuid",
        ref_e = entity_ref_sql("e")
    )))
    .bind(&id)
    .fetch_one(&mut *conn)
    .await?;
    let mut entity: Value =
        serde_json::from_str(&head.try_get::<String, _>("j")?).unwrap_or(Value::Null);
    entity["status"] = json!(head.try_get::<String, _>("status")?);
    entity["merged_into"] = json!(head.try_get::<Option<String>, _>("merged_into")?);
    entity["identity"] =
        serde_json::from_str(&head.try_get::<String, _>("identity")?).unwrap_or(json!({}));
    entity["revision"] = json!(head.try_get::<i64, _>("revision")?);
    let redirect_from: Vec<String> = sqlx::query_scalar(
        "SELECT from_key FROM _oo_docs.entity_redirect WHERE to_key = $1 ORDER BY from_key",
    )
    .bind(&key)
    .fetch_all(&mut *conn)
    .await?;
    entity["redirect_from"] = json!(redirect_from);

    // Aliases.
    let alias_rows = sqlx::query(
        "SELECT a.id::text AS id, a.alias, a.origin, a.status, a.confidence_bp FROM _oo_docs.alias a \
          WHERE a.entity_id = $1::uuid AND a.status IN ('draft', 'published') ORDER BY a.norm_alias",
    )
    .bind(&id)
    .fetch_all(&mut *conn)
    .await?;
    let alias_ids: Vec<String> = alias_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id"))
        .collect::<Result<_, _>>()?;
    let mut alias_ev = evidence_for(&mut conn, "alias", &alias_ids, 5).await?;
    let mut aliases = Vec::new();
    for r in &alias_rows {
        let aid: String = r.try_get("id")?;
        aliases.push(json!({
            "alias": r.try_get::<String, _>("alias")?, "origin": r.try_get::<String, _>("origin")?,
            "status": r.try_get::<String, _>("status")?, "confidence_bp": r.try_get::<i32, _>("confidence_bp")?,
            "evidence": alias_ev.remove(&aid).unwrap_or_default(),
        }));
    }
    entity["aliases"] = Value::Array(aliases);

    // Facts: visible ones by default, everything with include=history.
    let visibility = match (&as_of.version, history) {
        (_, true) => "TRUE".to_owned(),
        (None, false) => "FALSE".to_owned(),
        (Some((n, date)), false) => {
            if as_of.historic {
                format!(
                    "(ft.published_in <= {n} AND (ft.retracted_in IS NULL OR ft.retracted_in > {n}) \
                      AND (ft.valid_to IS NULL OR ft.valid_to > '{date}'::date))"
                )
            } else {
                format!(
                    "(ft.status = 'published' AND ft.retracted_at IS NULL \
                      AND (ft.valid_to IS NULL OR ft.valid_to > '{date}'::date))"
                )
            }
        }
    };
    let sql = format!(
        "SELECT ft.id::text AS id, {fact}::text AS j FROM _oo_docs.fact ft \
           JOIN _oo_docs.entity s ON s.id = ft.subject_id LEFT JOIN _oo_docs.entity o ON o.id = ft.object_id \
          WHERE (ft.subject_id = $1::uuid OR ft.object_id = $1::uuid) AND {visibility} \
          ORDER BY ft.valid_from NULLS FIRST, ft.fact_key, ft.id LIMIT 500",
        fact = fact_sql()
    );
    let fact_rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&id)
        .fetch_all(&mut *conn)
        .await?;
    let fact_ids: Vec<String> = fact_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id"))
        .collect::<Result<_, _>>()?;
    let mut ev = evidence_for(&mut conn, "fact", &fact_ids, 5).await?;
    let mut facts = Vec::new();
    for r in &fact_rows {
        let mut f: Value =
            serde_json::from_str(&r.try_get::<String, _>("j")?).unwrap_or(Value::Null);
        let fid: String = r.try_get("id")?;
        f["evidence"] = Value::Array(ev.remove(&fid).unwrap_or_default());
        facts.push(f);
    }
    if history {
        entity["history"] = Value::Array(facts);
        entity["facts"] = Value::Array(Vec::new());
    } else {
        entity["facts"] = Value::Array(facts);
    }
    let files: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT fv.path_at_version FROM _oo_docs.evidence ev \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
          WHERE ev.target_kind = 'entity' AND ev.target_id = $1::uuid AND ev.withdrawn_at IS NULL \
          ORDER BY 1 LIMIT 200",
    )
    .bind(&id)
    .fetch_all(&mut *conn)
    .await?;
    entity["files"] = json!(files);
    let version_header = as_of
        .version
        .as_ref()
        .map_or_else(|| "none".to_owned(), |(n, _)| format!("v{n}"));
    Ok(json_with(
        200,
        &entity,
        &[("OO-Docs-As-Of", version_header)],
    ))
}

/// A text span with its scored candidates.
type Span = (
    usize,
    usize,
    String,
    Vec<(u16, crate::resolve::exact::Candidate)>,
);

/// N-grams of `text` (2 to 20 characters, starting at a non-blank), with their
/// character spans.
fn ngrams(text: &str) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for i in 0..chars.len() {
        if chars[i].is_whitespace() || chars[i].is_ascii_punctuation() {
            continue;
        }
        for len in 2..=20usize {
            let end = i + len;
            if end > chars.len() {
                break;
            }
            let s: String = chars[i..end].iter().collect();
            if s.ends_with(char::is_whitespace) {
                continue;
            }
            out.push((i, end, s));
        }
    }
    out
}

fn via_name(v: Via) -> &'static str {
    match v {
        Via::CanonicalName => "name",
        Via::AliasPublished => "alias",
        Via::AliasDraft => "alias_draft",
        Via::Honorific => "honorific",
        Via::Identity => "identity",
    }
}

/// `POST /v1/docs/resolve`.
pub async fn resolve(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = parse_json(&req.body)?;
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.chars().count() <= 2000)
        .ok_or_else(|| ApiError::bad("/text", "text is required, at most 2000 characters"))?;
    let limit = match body.get("limit") {
        None => 5,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=50).contains(n))
            .ok_or_else(|| ApiError::bad("/limit", "limit must be 1 to 50"))?
            as usize,
    };
    let types: Option<BTreeSet<EntityType>> = match body.get("types") {
        None => None,
        Some(v) => Some(
            v.as_array()
                .ok_or_else(|| ApiError::bad("/types", "expected an array"))?
                .iter()
                .map(|t| {
                    t.as_str()
                        .and_then(EntityType::parse)
                        .ok_or_else(|| ApiError::bad("/types", "unknown entity type"))
                })
                .collect::<Result<_, _>>()?,
        ),
    };
    let drive = body.get("drive_id").and_then(Value::as_str);
    if let Some(d) = drive
        && !is_uuid(d)
    {
        return Err(ApiError::bad("/drive_id", "expected a uuid"));
    }
    let grams = ngrams(text);
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let mut surnames: BTreeSet<String> = BTreeSet::new();
    for (_, _, g) in &grams {
        let n = norm(g);
        for f in stripped_forms(g) {
            if let Some(s) = surname_form(&f) {
                surnames.insert(s);
            }
            wanted.insert(f);
        }
        wanted.insert(n);
    }
    let mut conn = app.pool.acquire().await?;
    let index = crate::resolve::pg::load_index_for(&mut conn, &wanted, &surnames).await?;
    // Entity refs of every candidate key.
    let mut spans: Vec<Span> = Vec::new();
    for (a, b, g) in &grams {
        for ty in EntityType::ALL {
            if types.as_ref().is_some_and(|t| !t.contains(&ty)) || ty == EntityType::Thing {
                continue;
            }
            let m = EntityRef::new(ty, g.clone());
            let r = rank(&index, &m, &Default::default(), &Context::default());
            let hits: Vec<_> = r
                .ranked
                .into_iter()
                .filter(|(s, c)| *s >= 5000 && !(c.via == Via::Honorific && *s < 5000))
                .collect();
            if !hits.is_empty() {
                spans.push((*a, *b, g.clone(), hits));
            }
        }
    }
    // Longest span first, then best score; no overlaps.
    spans.sort_by(|x, y| {
        (y.1 - y.0)
            .cmp(&(x.1 - x.0))
            .then(y.3[0].0.cmp(&x.3[0].0))
            .then(x.0.cmp(&y.0))
    });
    let mut taken: Vec<(usize, usize)> = Vec::new();
    let mut chosen: Vec<Span> = Vec::new();
    for s in spans {
        if taken.iter().any(|(a, b)| s.0 < *b && *a < s.1) {
            continue;
        }
        taken.push((s.0, s.1));
        chosen.push(s);
    }
    chosen.sort_by_key(|s| s.0);
    let keys: Vec<String> = chosen
        .iter()
        .flat_map(|s| s.3.iter().map(|(_, c)| c.key.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT e.key AS k, {}::text AS j FROM _oo_docs.entity e WHERE e.key = ANY($1::text[])",
        entity_ref_sql("e")
    )))
    .bind(&keys)
    .fetch_all(&mut *conn)
    .await?;
    let refs: std::collections::HashMap<String, Value> = rows
        .iter()
        .filter_map(|r| {
            Some((
                r.try_get::<String, _>("k").ok()?,
                serde_json::from_str(&r.try_get::<String, _>("j").ok()?).ok()?,
            ))
        })
        .collect();
    let mut mentions = Vec::new();
    for (a, b, g, hits) in chosen {
        let mut cands: Vec<Value> = Vec::new();
        for (score, c) in hits.iter().take(limit) {
            if let Some(e) = refs.get(&c.key) {
                cands.push(json!({"entity": e, "score_bp": score, "via": via_name(c.via)}));
            }
        }
        if cands.is_empty() {
            continue;
        }
        let ambiguous =
            hits.len() > 1 && hits[0].0 - hits[1].0 < crate::resolve::exact::LEAD_MIN_BP;
        mentions.push(
            json!({"span": [a, b], "surface": g, "candidates": cands, "ambiguous": ambiguous}),
        );
    }
    Ok(http::json(200, &json!({ "mentions": mentions })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ngrams_cover_the_spans() {
        let g = ngrams("陈总来了");
        assert!(g.contains(&(0, 2, "陈总".to_owned())));
        assert!(g.contains(&(0, 4, "陈总来了".to_owned())));
        assert!(g.iter().all(|(a, b, s)| s.chars().count() == b - a));
        assert!(ngrams("a").is_empty());
    }

    #[test]
    fn candidate_sources_map_to_the_contract_enum() {
        for v in [
            Via::CanonicalName,
            Via::AliasPublished,
            Via::AliasDraft,
            Via::Honorific,
            Via::Identity,
        ] {
            assert!(
                ["name", "alias", "alias_draft", "honorific", "identity"].contains(&via_name(v))
            );
        }
    }
}
