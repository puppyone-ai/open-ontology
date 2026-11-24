//! Full-text search over document facts and blocks (D83 WP3b, docs-api 1.9).
//!
//! `GET /v1/docs/search?q=&drive_id=&limit=` answers `{items, as_of_seq, truncated}`; every item is one hit and
//! names where it came from:
//!
//! * `kind: "fact"`: a published, not retracted fact whose text (subject name, predicate words, value text or
//!   object name) matches. It carries the subject entity and up to three evidence objects of the Drive;
//! * `kind: "block"`: a block of a live file version whose text matches, with the file path, version, block id
//!   and anchor.
//!
//! Matching is PostgreSQL full text search with the `simple` configuration, so no language dictionary is needed
//! and the same query works for Chinese and English. A CJK character is one token (the text and the query are
//! spaced around every such character before they reach `to_tsvector`), so a Chinese phrase is the phrase
//! operator `<->` over its characters, which finds the phrase and nothing that merely shares characters. Words
//! are lowercase NFKC tokens matched as prefixes. All terms must match.
//!
//! Ranking is deterministic: `ts_rank_cd(..., 32)` (a number in `[0, 1)`) descending, then fact before block,
//! then the stable id. Facts are scanned in SQL; blocks live in the stored IR, so every live file of the scope is
//! read from the content store and its block texts are ranked in one query per file. The scan is bounded by
//! [`MAX_FILES`] and [`MAX_BLOCKS_PER_FILE`]; `truncated` says when a bound cut it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};
use sqlx::Row;
use unicode_normalization::UnicodeNormalization;

use super::error::ApiError;
use super::http::{self, is_uuid};
use super::{AppState, Req, file_detail, files};

/// Live files read for block hits in one request.
pub const MAX_FILES: i64 = 300;
/// Blocks of one file that are ranked.
pub const MAX_BLOCKS_PER_FILE: usize = 20_000;
/// Longest query, in characters.
pub const MAX_QUERY_CHARS: usize = 200;
/// Query terms (words and CJK runs) that are kept.
pub const MAX_TERMS: usize = 16;
/// Evidence objects per fact hit.
const EVIDENCE_PER_FACT: i64 = 3;
/// Characters of a block text in a hit.
const SNIPPET_CHARS: usize = 240;

/// Is this one of the scripts that have no spaces between words (Han, Hiragana, Katakana, Hangul)?
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}')
}

/// The `to_tsquery('simple', ...)` text of a user query, or `None` when it has no searchable term. Only
/// alphanumeric characters survive, so the text can carry no tsquery operator of the caller.
pub fn tsquery(q: &str) -> Option<String> {
    let text: String = q.nfkc().collect::<String>().to_lowercase();
    let mut terms: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    let flush_word = |word: &mut String, terms: &mut Vec<String>| {
        if !word.is_empty() {
            terms.push(format!("'{word}':*"));
            word.clear();
        }
    };
    let flush_run = |run: &mut Vec<char>, terms: &mut Vec<String>| {
        if !run.is_empty() {
            let parts: Vec<String> = run.iter().map(|c| format!("'{c}'")).collect();
            terms.push(format!("({})", parts.join(" <-> ")));
            run.clear();
        }
    };
    for c in text.chars() {
        if is_cjk(c) {
            flush_word(&mut word, &mut terms);
            run.push(c);
        } else if c.is_alphanumeric() {
            flush_run(&mut run, &mut terms);
            word.push(c);
        } else {
            flush_word(&mut word, &mut terms);
            flush_run(&mut run, &mut terms);
        }
    }
    flush_word(&mut word, &mut terms);
    flush_run(&mut run, &mut terms);
    terms.truncate(MAX_TERMS);
    (!terms.is_empty()).then(|| terms.join(" & "))
}

/// SQL: the search vector of a text. The folding (every CJK character is a token of its own, ASCII and CJK
/// punctuation become spaces, so `HT-2026-017` is `ht 2026 017`; the default parser would keep `2026-017` and
/// `-017` as numbers and a query of three words would never match them) is the IMMUTABLE function of migration
/// 0153, the one that also fills the stored vectors behind the GIN indexes.
fn spaced(t: &str) -> String {
    format!("_oo_docs.fts_tsv({t})")
}

fn snippet(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SNIPPET_CHARS {
        flat
    } else {
        let cut: String = flat.chars().take(SNIPPET_CHARS).collect();
        format!("{cut}...")
    }
}

/// `GET /v1/docs/search`.
pub async fn search(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let q = &req.query;
    let raw = q.get("q").filter(|s| !s.trim().is_empty());
    let Some(raw) = raw else {
        return Err(ApiError::bad("/query/q", "q is required"));
    };
    if raw.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::bad("/query/q", "q is at most 200 characters"));
    }
    let limit = match q.get("limit") {
        None => 20,
        Some(_) => q.limit()?,
    };
    let drive = q.get("drive_id").filter(|d| !d.is_empty());
    if let Some(d) = drive
        && !is_uuid(d)
    {
        return Err(ApiError::bad("/query/drive_id", "expected a uuid"));
    }
    let mut conn = app.pool.acquire().await?;
    let as_of_seq = files::head_seq(&mut conn).await?;
    let Some(query) = tsquery(raw) else {
        return Ok(http::json(
            200,
            &json!({"items": [], "as_of_seq": as_of_seq, "truncated": false}),
        ));
    };

    // ---- facts
    let sql = format!(
        "SELECT ft.id::text AS id, ft.fact_key, ft.predicate, ft.confidence_bp, \
                s.key AS subject_key, s.canonical_name AS subject_name, s.type AS subject_type, \
                o.key AS object_key, o.canonical_name AS object_name, \
                COALESCE(ft.value->>'text', ft.value->>'enum', ft.value->>'date') AS value_text, \
                ts_rank_cd(ft.fts, query.q, 32)::float8 AS rank \
           FROM _oo_docs.fact ft \
           JOIN _oo_docs.entity s ON s.id = ft.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = ft.object_id, \
                (SELECT to_tsquery('simple', $1) AS q) query \
          WHERE ft.status = 'published' AND ft.retracted_in IS NULL AND s.status = 'active' \
            AND ft.fts @@ query.q \
            AND EXISTS (SELECT 1 FROM _oo_docs.evidence ev \
                          JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
                          JOIN _oo_docs.file f ON f.id = fv.file_id \
                         WHERE ev.target_kind = 'fact' AND ev.target_id = ft.id AND ev.withdrawn_at IS NULL \
                           AND ($2::uuid IS NULL OR f.drive_id = $2::uuid)) \
          ORDER BY rank DESC, ft.fact_key, ft.id LIMIT {}",
        limit + 1
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&query)
        .bind(drive)
        .fetch_all(&mut *conn)
        .await?;
    let mut truncated = rows.len() as i64 > limit;
    let mut hits: Vec<(f64, u8, String, Value)> = Vec::new();
    let ids: Vec<String> = rows
        .iter()
        .take(limit as usize)
        .map(|r| r.try_get::<String, _>("id"))
        .collect::<Result<_, _>>()?;
    let ev_rows = sqlx::query(
        "SELECT ev.target_id::text AS target, ev.id::text AS id, fv.file_id::text AS file_id, \
                f.drive_id::text AS drive_id, ev.version_id::text AS version_id, \
                fv.path_at_version AS path, ev.block_id, ev.anchor::text AS anchor \
           FROM _oo_docs.evidence ev JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
           JOIN _oo_docs.file f ON f.id = fv.file_id \
          WHERE ev.target_kind = 'fact' AND ev.target_id = ANY($1::text[]::uuid[]) \
            AND ev.withdrawn_at IS NULL AND ($2::uuid IS NULL OR f.drive_id = $2::uuid) \
          ORDER BY ev.target_id, fv.path_at_version, ev.block_id, ev.id",
    )
    .bind(&ids)
    .bind(drive)
    .fetch_all(&mut *conn)
    .await?;
    let mut evidence: std::collections::HashMap<String, Vec<Value>> = Default::default();
    for r in ev_rows {
        let slot = evidence.entry(r.try_get("target")?).or_default();
        if slot.len() as i64 >= EVIDENCE_PER_FACT {
            continue;
        }
        let anchor: Value =
            serde_json::from_str(&r.try_get::<String, _>("anchor")?).unwrap_or(Value::Null);
        let path: String = r.try_get("path")?;
        let src = serde_json::from_value::<oo_core::docir::Anchor>(anchor.clone())
            .map(|a| crate::compile::source::tag(&path, &a))
            .unwrap_or_default();
        slot.push(json!({
            "id": r.try_get::<String, _>("id")?, "file_id": r.try_get::<String, _>("file_id")?,
            "drive_id": r.try_get::<String, _>("drive_id")?,
            "version_id": r.try_get::<String, _>("version_id")?, "path": path,
            "block_id": r.try_get::<String, _>("block_id")?, "anchor": anchor, "src": src,
        }));
    }
    for r in rows.iter().take(limit as usize) {
        let id: String = r.try_get("id")?;
        let value_text: Option<String> = r.try_get("value_text")?;
        let object_name: Option<String> = r.try_get("object_name")?;
        let rank: f64 = r.try_get("rank")?;
        let item = json!({
            "kind": "fact", "id": id, "fact_key": r.try_get::<String, _>("fact_key")?,
            "predicate": r.try_get::<String, _>("predicate")?,
            "text": value_text.or(object_name).unwrap_or_default(),
            "subject": {"key": r.try_get::<String, _>("subject_key")?,
                        "name": r.try_get::<String, _>("subject_name")?,
                        "type": r.try_get::<String, _>("subject_type")?},
            "score": rank, "evidence": evidence.remove(&id).unwrap_or_default(),
        });
        if item["evidence"].as_array().is_some_and(|e| !e.is_empty()) {
            hits.push((rank, 0, id, item));
        }
    }

    // ---- blocks
    if app.cas.is_some() {
        let files = sqlx::query(
            "SELECT fv.file_id::text AS file_id, fv.id::text AS version_id, fv.path_at_version AS path, \
                    fv.b3, f.drive_id::text AS drive_id \
               FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
              WHERE fv.superseded_at IS NULL AND fv.deleted_at IS NULL AND fv.b3 IS NOT NULL \
                AND ($1::uuid IS NULL OR f.drive_id = $1::uuid) \
              ORDER BY fv.path_at_version, fv.file_id LIMIT $2",
        )
        .bind(drive)
        .bind(MAX_FILES + 1)
        .fetch_all(&mut *conn)
        .await?;
        if files.len() as i64 > MAX_FILES {
            truncated = true;
        }
        let live: Vec<&sqlx::postgres::PgRow> = files.iter().take(MAX_FILES as usize).collect();
        // The IR of every live file (the newest complete parse of its content), and the vectors of its blocks.
        let b3s: Vec<String> = live
            .iter()
            .map(|f| f.try_get::<String, _>("b3"))
            .collect::<Result<_, _>>()?;
        let irs: std::collections::HashMap<String, String> = sqlx::query(
            "SELECT DISTINCT ON (b3) b3, ir_b3 FROM _oo_docs.parse_result \
              WHERE b3 = ANY($1) AND status = 'complete' AND ir_b3 IS NOT NULL \
              ORDER BY b3, finished_at DESC NULLS LAST",
        )
        .bind(&b3s)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| Ok((r.try_get("b3")?, r.try_get("ir_b3")?)))
        .collect::<Result<_, sqlx::Error>>()?;
        let irs_list: Vec<String> = irs
            .values()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut counts: std::collections::HashMap<String, i32> =
            sqlx::query("SELECT ir_b3, blocks FROM _oo_docs.block_fts_done WHERE ir_b3 = ANY($1)")
                .bind(&irs_list)
                .fetch_all(&mut *conn)
                .await?
                .into_iter()
                .map(|r| Ok((r.try_get("ir_b3")?, r.try_get("blocks")?)))
                .collect::<Result<_, sqlx::Error>>()?;
        // A file the index has not seen yet is read once, from the content store, and indexed for good (the IR is
        // content addressed, so its vectors never change).
        let mut parsed: std::collections::HashMap<String, file_detail::Parsed> = Default::default();
        for f in &live {
            let b3: String = f.try_get("b3")?;
            let Some(ir_b3) = irs.get(&b3) else { continue };
            if counts.contains_key(ir_b3) {
                continue;
            }
            let Some(p) = file_detail::load_parsed(&mut conn, app, &b3).await? else {
                continue;
            };
            let total = i32::try_from(p.ir.blocks.len()).unwrap_or(i32::MAX);
            let texts: Vec<String> =
                p.ir.blocks
                    .iter()
                    .take(MAX_BLOCKS_PER_FILE)
                    .map(|b| b.text.clone())
                    .collect();
            let mut tx = sqlx::Connection::begin(&mut *conn).await?;
            let fresh = sqlx::query(
                "INSERT INTO _oo_docs.block_fts_done (ir_b3, blocks) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(ir_b3)
            .bind(total)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                == 1;
            if fresh {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "INSERT INTO _oo_docs.block_fts (ir_b3, ord, tsv) \
                     SELECT $1, (u.i - 1)::int, {} FROM unnest($2::text[]) WITH ORDINALITY AS u(t, i)",
                    spaced("u.t")
                )))
                .bind(ir_b3)
                .bind(&texts)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            counts.insert(ir_b3.clone(), total);
            parsed.insert(b3, p);
        }
        // The match itself is one indexed query over the vectors of the scope.
        let matched = sqlx::query(
            "SELECT bf.ir_b3, bf.ord, ts_rank_cd(bf.tsv, query.q, 32)::float8 AS rank \
               FROM _oo_docs.block_fts bf, (SELECT to_tsquery('simple', $2) AS q) query \
              WHERE bf.tsv @@ query.q AND bf.ir_b3 = ANY($1) AND bf.ord < $3",
        )
        .bind(&irs_list)
        .bind(&query)
        .bind(i32::try_from(MAX_BLOCKS_PER_FILE).unwrap_or(i32::MAX))
        .fetch_all(&mut *conn)
        .await?;
        let mut by_ir: std::collections::HashMap<String, Vec<(usize, f64)>> = Default::default();
        for r in &matched {
            by_ir.entry(r.try_get("ir_b3")?).or_default().push((
                usize::try_from(r.try_get::<i32, _>("ord")?).unwrap_or(0),
                r.try_get::<f64, _>("rank")?,
            ));
        }
        for f in &live {
            let b3: String = f.try_get("b3")?;
            let Some(ir_b3) = irs.get(&b3) else { continue };
            if counts
                .get(ir_b3)
                .is_some_and(|n| usize::try_from(*n).unwrap_or(0) > MAX_BLOCKS_PER_FILE)
            {
                truncated = true;
            }
            let Some(rows) = by_ir.get(ir_b3) else {
                continue;
            };
            let p = match parsed.remove(&b3) {
                Some(p) => p,
                None => match file_detail::load_parsed(&mut conn, app, &b3).await? {
                    Some(p) => p,
                    None => continue,
                },
            };
            let blocks = &p.ir.blocks;
            let path: String = f.try_get("path")?;
            for (i, rank) in rows {
                let Some(b) = blocks.get(*i) else { continue };
                let rank = *rank;
                let anchor = serde_json::to_value(&b.anchor).unwrap_or(Value::Null);
                let id = format!("{}#{}", f.try_get::<String, _>("file_id")?, b.id);
                hits.push((
                    rank,
                    1,
                    id.clone(),
                    json!({
                        "kind": "block", "id": id, "text": snippet(&b.text), "hpath": b.hpath,
                        "block_kind": b.kind, "score": rank,
                        "evidence": [{
                            "file_id": f.try_get::<String, _>("file_id")?,
                            "drive_id": f.try_get::<String, _>("drive_id")?,
                            "version_id": f.try_get::<String, _>("version_id")?,
                            "path": path, "block_id": b.id, "anchor": anchor,
                            "src": crate::compile::source::tag(&path, &b.anchor),
                        }],
                    }),
                ));
            }
        }
    }

    hits.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    if hits.len() as i64 > limit {
        truncated = true;
    }
    hits.truncate(limit as usize);
    let items: Vec<Value> = hits.into_iter().map(|h| h.3).collect();
    Ok(http::json(
        200,
        &json!({"items": items, "as_of_seq": as_of_seq, "truncated": truncated}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_words_become_prefix_terms() {
        assert_eq!(
            tsquery("Contract renews, March!").as_deref(),
            Some("'contract':* & 'renews':* & 'march':*")
        );
    }

    #[test]
    fn chinese_runs_become_phrases() {
        assert_eq!(
            tsquery("合同 每年三月 renew").as_deref(),
            Some("('合' <-> '同') & ('每' <-> '年' <-> '三' <-> '月') & 'renew':*")
        );
    }

    #[test]
    fn mixed_script_splits_and_fullwidth_folds() {
        assert_eq!(
            tsquery("ＡＣＭＥ公司").as_deref(),
            Some("'acme':* & ('公' <-> '司')")
        );
    }

    #[test]
    fn operators_and_quotes_never_reach_tsquery() {
        assert_eq!(tsquery("'; DROP & | ! <-> :*"), Some("'drop':*".to_owned()));
        assert_eq!(tsquery("!!! ..."), None);
    }

    #[test]
    fn terms_are_capped() {
        let q = (0..40)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(tsquery(&q).unwrap().matches(" & ").count(), MAX_TERMS - 1);
    }

    #[test]
    fn snippets_are_flat_and_short() {
        assert_eq!(snippet("a\n  b"), "a b");
        assert!(snippet(&"x".repeat(500)).ends_with("..."));
    }
}
