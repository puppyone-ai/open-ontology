//! Store one parsed file's rule output as drafts in `_oo_docs` (10 §4.6, §4.7).
//!
//! One transaction per file version: take the advisory lock of every
//! canonical name that might create an entity (sorted, so two files cannot
//! deadlock), look up exact-name candidates, resolve, then insert entities,
//! aliases, facts and evidence. Everything is idempotent: running it twice
//! for the same version adds nothing, and a fact that already exists in
//! `draft` or `published` (same `fact_key`) only gains evidence. Evidence is
//! kept per file version, so a duplicate blob at another path gets its own
//! rows (10 §4.7.1).

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::DocIr;
use sqlx::{Connection, PgConnection};

use crate::extract::model::EvidenceRef;
use crate::extract::rules::{Siblings, extract_in};
use crate::resolve::exact::{Resolved, resolve_extraction};
use crate::resolve::merge::{MergeError, merge_entities};
use crate::resolve::pg::{
    load_index, lookup_names, propose_merges, same_identity_pairs, store_identities,
};
use crate::store::StoreError;

/// What to index: one parsed file version.
#[derive(Clone, Copy, Debug)]
pub struct DocumentInput<'a> {
    /// `file_version.id` (UUID text).
    pub version_id: &'a str,
    /// Blob `b3` of that version (the IR is keyed by it).
    pub b3: &'a str,
    /// Drive-relative path at that version (NFC).
    pub path: &'a str,
    pub ir: &'a DocIr,
}

/// Rows written (existing rows are not counted).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexSummary {
    pub entities_created: u32,
    pub aliases_created: u32,
    pub facts_created: u32,
    pub evidence_added: u32,
    pub alias_conflicts: u32,
    /// Identity attributes added to entities.
    pub identities_added: u32,
    /// Entities merged because they share an identity (5.4 step 2).
    pub identity_merges: u32,
    /// `entity_merge` review items queued (5.4 step 6, refused identity merges).
    pub merge_proposals: u32,
    /// The file version repeats the assertions of an earlier version of the
    /// same file (early cutoff, 16.1).
    pub early_cutoff: bool,
}

/// Run the rules on `doc.ir` and store the result.
pub async fn index_document(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
) -> Result<IndexSummary, StoreError> {
    index_document_with(conn, doc, &[]).await
}

/// [`index_document`] with the other live paths of the Drive, which
/// `name.version` needs to find an older version of the same document.
pub async fn index_document_with(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
    siblings: &[String],
) -> Result<IndexSummary, StoreError> {
    let extraction = extract_in(doc.path, doc.ir, &Siblings { paths: siblings });
    index_extraction(conn, doc, &extraction).await
}

/// `doc.link`: mentions and `about` links of this file version to the entities the
/// Drive knows by name (`extract::links`), stored as drafts. Run after the rules
/// or the model added entities, and again for the other files when new entities
/// appeared ([`index_links`] is idempotent).
pub async fn index_links(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
) -> Result<IndexSummary, StoreError> {
    let lex = crate::resolve::pg::load_lexicon(conn).await?;
    let stem = doc.path.rsplit('/').next().unwrap_or(doc.path);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    let title = doc
        .ir
        .blocks
        .iter()
        .find(|b| b.kind == oo_core::docir::BlockKind::Heading && b.level == Some(1))
        .map(|b| b.text.trim().to_owned())
        .or_else(|| doc.ir.meta.title.clone());
    let links = crate::extract::links::doc_links(doc.path, doc.ir, title.as_deref(), stem, &lex);
    if links.hints.is_empty() {
        return Ok(IndexSummary::default());
    }
    index_extraction(conn, doc, &links).await
}

/// Store an extraction (rules or LLM supplement) for one file version.
pub async fn index_extraction(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
    extraction: &crate::extract::model::Extraction,
) -> Result<IndexSummary, StoreError> {
    let extraction = extraction.clone();
    let mut tx = conn.begin().await?;

    // Lock every name that may create an entity, in byte order.
    let (norms, _) = lookup_names(&extraction);
    for n in &norms {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(n)
            .execute(&mut *tx)
            .await?;
    }

    let index = load_index(&mut tx, &extraction).await?;
    let resolved = resolve_extraction(&index, &extraction);
    let src = match &extraction.llm {
        Some(m) => WriteSource {
            origin: "llm",
            prompt_version: Some(m.prompt_version.clone()),
            extraction_key: Some(m.extraction_key.clone()),
        },
        None => WriteSource {
            origin: "rule",
            prompt_version: None,
            extraction_key: None,
        },
    };
    let mut summary = store_resolved(&mut tx, doc, &resolved, &src).await?;
    post_resolve(&mut tx, doc, &resolved, &mut summary).await?;
    if let Some((_, same)) = super::hashes::store_assertion_hash(&mut tx, doc.version_id).await? {
        summary.early_cutoff = same;
    }
    share_evidence_with_duplicates(&mut tx, doc.version_id).await?;
    tx.commit().await?;
    Ok(summary)
}

/// Every path keeps its own evidence rows (10 4.7.1), but a duplicate copies them
/// when its twin is only indexed, before the link pass and the model supplement add
/// theirs. Copy again whatever this version gained to the live duplicates of it;
/// [`crate::ingest::files::copy_evidence`] skips rows the duplicate already has.
async fn share_evidence_with_duplicates(
    conn: &mut PgConnection,
    version_id: &str,
) -> Result<(), StoreError> {
    let duplicates: Vec<String> = sqlx::query_scalar(
        "SELECT fv.id::text FROM _oo_docs.file_version fv \
           JOIN _oo_docs.file f ON f.id = fv.file_id \
          WHERE fv.duplicate_of = $1::uuid AND fv.superseded_at IS NULL \
            AND fv.deleted_at IS NULL AND f.deleted_at IS NULL \
            AND f.current_version_id = fv.id \
          ORDER BY fv.id",
    )
    .bind(version_id)
    .fetch_all(&mut *conn)
    .await?;
    for duplicate in duplicates {
        crate::ingest::files::copy_evidence(&mut *conn, version_id, &duplicate)
            .await
            .map_err(|e| StoreError::Protocol(e.to_string()))?;
    }
    Ok(())
}

/// Identity attributes, identity merges and merge proposals of one file.
async fn post_resolve(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
    resolved: &Resolved,
    summary: &mut IndexSummary,
) -> Result<(), StoreError> {
    let gained = store_identities(conn, resolved).await?;
    summary.identities_added += u32::try_from(gained.len()).unwrap_or(u32::MAX);
    let mut pairs: BTreeSet<(String, String)> = BTreeSet::new();
    for (a, b) in same_identity_pairs(conn, &gained).await? {
        pairs.insert(if a <= b { (a, b) } else { (b, a) });
    }
    for (into, from) in pairs {
        match merge_entities(conn, &from, &into, "identity", None).await {
            Ok(_) => summary.identity_merges += 1,
            Err(MergeError::IdentityConflict(..) | MergeError::NotActive(_)) => {
                // Already merged by an earlier pair, or contradicting: a person decides.
                let kinds = sqlx::query(
                    "INSERT INTO _oo_docs.review_item \
                       (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                        confidence_bp, reason_code) \
                     SELECT gen_random_uuid(), 'entity_merge', $1, f.drive_id, f.id, fv.id, \
                            jsonb_build_object('entity_keys', jsonb_build_array($2::text, $3::text), \
                                               'shared_aliases', '[]'::jsonb), \
                            NULL, 'identity_conflict' \
                       FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
                      WHERE fv.id = $4::uuid \
                        AND EXISTS (SELECT 1 FROM _oo_docs.entity e WHERE e.key = $2 AND e.status = 'active') \
                        AND EXISTS (SELECT 1 FROM _oo_docs.entity e WHERE e.key = $3 AND e.status = 'active') \
                     ON CONFLICT (dedupe_key) DO NOTHING",
                )
                .bind(format!("entity_merge:{into}:{from}"))
                .bind(&into)
                .bind(&from)
                .bind(doc.version_id)
                .execute(&mut *conn)
                .await?;
                summary.merge_proposals += u32::try_from(kinds.rows_affected()).unwrap_or(0);
            }
            Err(MergeError::Store(e)) => return Err(e),
            Err(e) => return Err(StoreError::Protocol(e.to_string())),
        }
    }
    let keys: Vec<String> = resolved.entities.iter().map(|e| e.key.clone()).collect();
    summary.merge_proposals += propose_merges(conn, &keys, doc.version_id).await?;
    Ok(())
}

/// Who produced what is stored: `rule` or the LLM supplement.
struct WriteSource {
    origin: &'static str,
    prompt_version: Option<String>,
    extraction_key: Option<String>,
}

async fn add_evidence(
    conn: &mut PgConnection,
    kind: &str,
    target_id: &str,
    doc: &DocumentInput<'_>,
    ev: &EvidenceRef,
    src: &WriteSource,
) -> Result<bool, StoreError> {
    let anchor = serde_json::to_string(&ev.anchor)
        .map_err(|e| StoreError::Protocol(format!("anchor json: {e}")))?;
    let done = sqlx::query(
        "INSERT INTO _oo_docs.evidence \
           (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
         SELECT gen_random_uuid(), $1, $2::uuid, $3::uuid, $4, $5, $6::jsonb, $7, $8 \
          WHERE NOT EXISTS (SELECT 1 FROM _oo_docs.evidence \
                             WHERE target_kind = $1 AND target_id = $2::uuid \
                               AND version_id = $3::uuid AND block_id = $5 \
                               AND withdrawn_at IS NULL)",
    )
    .bind(kind)
    .bind(target_id)
    .bind(doc.version_id)
    .bind(doc.b3)
    .bind(&ev.block_id)
    .bind(&anchor)
    .bind(&ev.quote_b3)
    .bind(src.origin)
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected() > 0)
}

async fn store_resolved(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
    r: &Resolved,
    src: &WriteSource,
) -> Result<IndexSummary, StoreError> {
    let mut s = IndexSummary::default();
    let mut entity_ids: BTreeMap<&str, String> = BTreeMap::new();

    for e in &r.entities {
        if !e.bound {
            let created = sqlx::query(
                "INSERT INTO _oo_docs.entity \
                   (id, key, type, thing_label, subkind, canonical_name, norm_name) \
                 VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6) ON CONFLICT (key) DO NOTHING",
            )
            .bind(&e.key)
            .bind(e.ty.as_str())
            .bind(e.thing_label.as_deref())
            .bind(e.subkind)
            .bind(&e.name)
            .bind(&e.norm)
            .execute(&mut *conn)
            .await?;
            s.entities_created += u32::try_from(created.rows_affected()).unwrap_or(0);
        }
        let id: Option<String> =
            sqlx::query_scalar("SELECT id::text FROM _oo_docs.entity WHERE key = $1")
                .bind(&e.key)
                .fetch_optional(&mut *conn)
                .await?;
        let Some(id) = id else {
            return Err(StoreError::Protocol(format!("entity {} vanished", e.key)));
        };
        for ev in &e.evidence {
            if add_evidence(conn, "entity", &id, doc, ev, src).await? {
                s.evidence_added += 1;
            }
        }
        entity_ids.insert(&e.key, id);
    }

    for a in &r.aliases {
        let Some(entity_id) = entity_ids.get(a.entity_key.as_str()) else {
            return Err(StoreError::Protocol("alias of unknown entity".into()));
        };
        let created = sqlx::query(
            "INSERT INTO _oo_docs.alias \
               (id, entity_id, alias, norm_alias, origin, confidence_bp, status, original_entity_id) \
             VALUES (gen_random_uuid(), $1::uuid, $2, $3, $5, $4, 'draft', $1::uuid) \
             ON CONFLICT (entity_id, norm_alias) DO NOTHING",
        )
        .bind(entity_id)
        .bind(&a.alias)
        .bind(&a.norm)
        .bind(i32::from(a.confidence_bp))
        .bind(src.origin)
        .execute(&mut *conn)
        .await?;
        s.aliases_created += u32::try_from(created.rows_affected()).unwrap_or(0);
        let alias_id: String = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.alias WHERE entity_id = $1::uuid AND norm_alias = $2",
        )
        .bind(entity_id)
        .bind(&a.norm)
        .fetch_one(&mut *conn)
        .await?;
        for ev in &a.evidence {
            if add_evidence(conn, "alias", &alias_id, doc, ev, src).await? {
                s.evidence_added += 1;
            }
        }
    }

    // Content a person rejected or replaced is not a draft again (DOC-G4).
    let all_keys: Vec<String> = r.facts.iter().map(|f| f.fact_key.clone()).collect();
    let rejected = crate::review::overlay::rejected_keys(conn, &all_keys).await?;
    for f in &r.facts {
        if rejected.contains(&f.fact_key) {
            continue;
        }
        // A statement without a date (a model said "X owns Y", the rules found the
        // dated row) is the same fact as a dated one of the same triple: it only
        // adds evidence.
        let undated_relation =
            f.valid_from.is_none() && f.value.is_none() && f.object_key.is_some();
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.fact \
              WHERE status IN ('draft', 'published') \
                AND (fact_key = $1 \
                     OR ($2 AND subject_id = $3::uuid AND predicate = $4 AND object_id = $5::uuid)) \
              ORDER BY (fact_key = $1) DESC, recorded_at, id LIMIT 1",
        )
        .bind(&f.fact_key)
        .bind(undated_relation)
        .bind(entity_ids.get(f.subject_key.as_str()))
        .bind(f.predicate.name())
        .bind(f.object_key.as_ref().and_then(|k| entity_ids.get(k.as_str())))
        .fetch_optional(&mut *conn)
        .await?;
        let fact_id = match existing {
            Some(id) => id,
            None => {
                let subject = entity_ids
                    .get(f.subject_key.as_str())
                    .ok_or_else(|| StoreError::Protocol("fact subject unknown".into()))?;
                let object = match &f.object_key {
                    Some(k) => Some(
                        entity_ids
                            .get(k.as_str())
                            .ok_or_else(|| StoreError::Protocol("fact object unknown".into()))?,
                    ),
                    None => None,
                };
                let value = f.value.as_ref().map(|v| v.to_string());
                let id: String = sqlx::query_scalar(
                    "INSERT INTO _oo_docs.fact \
                       (id, fact_key, subject_id, original_subject_id, predicate, object_id, \
                        original_object_id, value, valid_from, valid_to, valid_precision, \
                        confidence_bp, origin, rule_id, prompt_version, extraction_key, status) \
                     VALUES (gen_random_uuid(), $1, $2::uuid, $2::uuid, $3, $4::uuid, $4::uuid, \
                             $5::jsonb, $6::date, $7::date, $8, $9, $11, $10, $12, $13, 'draft') \
                     RETURNING id::text",
                )
                .bind(&f.fact_key)
                .bind(subject)
                .bind(f.predicate.name())
                .bind(object)
                .bind(value)
                .bind(f.valid_from.as_deref())
                .bind(f.valid_to.as_deref())
                .bind(f.valid_precision)
                .bind(i32::from(f.confidence_bp))
                .bind(if src.origin == "rule" {
                    Some(f.rule_id)
                } else {
                    None
                })
                .bind(src.origin)
                .bind(src.prompt_version.as_deref())
                .bind(src.extraction_key.as_deref())
                .fetch_one(&mut *conn)
                .await?;
                s.facts_created += 1;
                id
            }
        };
        for ev in &f.evidence {
            if add_evidence(conn, "fact", &fact_id, doc, ev, src).await? {
                s.evidence_added += 1;
            }
        }
    }

    for c in &r.conflicts {
        let dedupe = format!("alias_conflict:{}:{}", c.ty.slug(), c.norm);
        let reason = if c.reason == "identity" {
            "identity_conflict"
        } else {
            "alias_tie"
        };
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'alias_conflict', $1, f.drive_id, f.id, fv.id, \
                    jsonb_build_object('alias', $2::text, \
                      'entity_ids', (SELECT COALESCE(jsonb_agg(e.id::text ORDER BY e.key), '[]'::jsonb) \
                                       FROM _oo_docs.entity e WHERE e.key = ANY($3::text[]))), \
                    9000, $5 \
               FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
              WHERE fv.id = $4::uuid \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(&dedupe)
        .bind(&c.norm)
        .bind(&c.entity_keys)
        .bind(doc.version_id)
        .bind(reason)
        .execute(&mut *conn)
        .await?;
        s.alias_conflicts += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(s)
}
