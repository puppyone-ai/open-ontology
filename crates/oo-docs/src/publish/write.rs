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

use crate::extract::model::{EntityType, EvidenceRef};
use crate::extract::rules::extract;
use crate::resolve::exact::{Candidate, MapIndex, Resolved, Via, resolve_extraction};
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
}

/// Run the rules on `doc.ir` and store the result.
pub async fn index_document(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
) -> Result<IndexSummary, StoreError> {
    let extraction = extract(doc.path, doc.ir);
    let mut tx = conn.begin().await?;

    // Lock every name that may create an entity, in byte order.
    let mut norms: BTreeSet<String> = BTreeSet::new();
    for m in &extraction.entities {
        norms.insert(m.entity.norm());
    }
    for a in &extraction.aliases {
        norms.insert(a.entity.norm());
    }
    for f in &extraction.facts {
        norms.insert(f.subject.norm());
        if let Some(o) = &f.object {
            norms.insert(o.norm());
        }
    }
    for n in &norms {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(n)
            .execute(&mut *tx)
            .await?;
    }

    let index = load_candidates(&mut tx, &norms).await?;
    let resolved = resolve_extraction(&index, &extraction);
    let summary = store_resolved(&mut tx, doc, &resolved).await?;
    tx.commit().await?;
    Ok(summary)
}

async fn load_candidates(
    conn: &mut PgConnection,
    norms: &BTreeSet<String>,
) -> Result<MapIndex, StoreError> {
    let wanted: Vec<String> = norms.iter().cloned().collect();
    let rows: Vec<(String, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT e.type, e.thing_label, e.norm_name, e.key, 'canonical' \
           FROM _oo_docs.entity e \
          WHERE e.status = 'active' AND e.norm_name = ANY($1::text[]) \
         UNION ALL \
         SELECT e.type, e.thing_label, a.norm_alias, e.key, a.status \
           FROM _oo_docs.alias a JOIN _oo_docs.entity e ON e.id = a.entity_id \
          WHERE e.status = 'active' AND a.status IN ('draft', 'published') \
            AND a.norm_alias = ANY($1::text[])",
    )
    .bind(&wanted)
    .fetch_all(&mut *conn)
    .await?;
    let mut index = MapIndex::default();
    for (ty, label, name_norm, key, via) in rows {
        let Some(ty) = EntityType::parse(&ty) else {
            return Err(StoreError::Protocol(format!("unknown entity type {ty}")));
        };
        let via = match via.as_str() {
            "canonical" => Via::CanonicalName,
            "published" => Via::AliasPublished,
            _ => Via::AliasDraft,
        };
        index.insert(ty, label.as_deref(), &name_norm, Candidate { key, via });
    }
    Ok(index)
}

async fn add_evidence(
    conn: &mut PgConnection,
    kind: &str,
    target_id: &str,
    doc: &DocumentInput<'_>,
    ev: &EvidenceRef,
) -> Result<bool, StoreError> {
    let anchor = serde_json::to_string(&ev.anchor)
        .map_err(|e| StoreError::Protocol(format!("anchor json: {e}")))?;
    let done = sqlx::query(
        "INSERT INTO _oo_docs.evidence \
           (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
         SELECT gen_random_uuid(), $1, $2::uuid, $3::uuid, $4, $5, $6::jsonb, $7, 'rule' \
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
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected() > 0)
}

async fn store_resolved(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
    r: &Resolved,
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
            if add_evidence(conn, "entity", &id, doc, ev).await? {
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
               (id, entity_id, alias, norm_alias, origin, confidence_bp, status) \
             VALUES (gen_random_uuid(), $1::uuid, $2, $3, 'rule', $4, 'draft') \
             ON CONFLICT (entity_id, norm_alias) DO NOTHING",
        )
        .bind(entity_id)
        .bind(&a.alias)
        .bind(&a.norm)
        .bind(i32::from(a.confidence_bp))
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
            if add_evidence(conn, "alias", &alias_id, doc, ev).await? {
                s.evidence_added += 1;
            }
        }
    }

    for f in &r.facts {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.fact \
              WHERE fact_key = $1 AND status IN ('draft', 'published') \
              ORDER BY recorded_at, id LIMIT 1",
        )
        .bind(&f.fact_key)
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
                        confidence_bp, origin, rule_id, status) \
                     VALUES (gen_random_uuid(), $1, $2::uuid, $2::uuid, $3, $4::uuid, $4::uuid, \
                             $5::jsonb, $6::date, $7::date, $8, $9, 'rule', $10, 'draft') \
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
                .bind(f.rule_id)
                .fetch_one(&mut *conn)
                .await?;
                s.facts_created += 1;
                id
            }
        };
        for ev in &f.evidence {
            if add_evidence(conn, "fact", &fact_id, doc, ev).await? {
                s.evidence_added += 1;
            }
        }
    }

    for c in &r.conflicts {
        let dedupe = format!("alias_conflict:{}:{}", c.ty.slug(), c.norm);
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'alias_conflict', $1, f.drive_id, f.id, fv.id, \
                    jsonb_build_object('alias', $2::text, \
                      'entity_ids', (SELECT COALESCE(jsonb_agg(e.id::text ORDER BY e.key), '[]'::jsonb) \
                                       FROM _oo_docs.entity e WHERE e.key = ANY($3::text[]))), \
                    9000, 'alias_tie' \
               FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
              WHERE fv.id = $4::uuid \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(&dedupe)
        .bind(&c.norm)
        .bind(&c.entity_keys)
        .bind(doc.version_id)
        .execute(&mut *conn)
        .await?;
        s.alias_conflicts += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(s)
}
