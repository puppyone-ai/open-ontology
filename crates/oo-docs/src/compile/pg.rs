//! The store reader: [`PgReader`] loads one `(version, scope)` from `_oo_docs`
//! and answers [`OntologyReader`] from memory (the trait is synchronous, the
//! database is not). The scope is applied in SQL (10 §7.2 point 2): evidence
//! outside the scope is never read; for what it holds back the queries only
//! test existence and return the keys of entities and the texts of aliases
//! needed for the counts and the leak guard.
//!
//! This is one of the two files of the compiler that may mention the store
//! (`compile.rs` is the other); the renderers never do.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::{Anchor, DocIr};
use sqlx::PgConnection;

use super::PackError;
use super::reader::{
    BlockText, BlockedInfo, DriveInfo, OntologyReader, RawAlias, RawConflict, RawEntity,
    RawEvidence, RawFact, Scoped, SnapshotFile, VersionInfo, in_prefix,
};
use crate::cas::{Cas, CasError, CasKind};
use crate::extract::model::{EntityType, Predicate};

/// `(fact_key, subject key, predicate, object key, value, valid_from, valid_to,
/// origin, file version id, block id, anchor)`.
type FactSqlRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    String,
);
/// `(key, type, subkind, thing_label, canonical_name)`.
type EntitySqlRow = (String, String, Option<String>, Option<String>, String);

fn store<E: std::fmt::Display>(e: E) -> PackError {
    PackError::Store(e.to_string())
}

/// Everything loaded for one `(version, scope)`.
pub struct PgReader {
    drive: DriveInfo,
    version: VersionInfo,
    prefixes: Vec<String>,
    scoped: Scoped,
}

fn evidence(version_id: String, block_id: String, anchor: &str) -> Result<RawEvidence, PackError> {
    let anchor: Anchor = serde_json::from_str(anchor).map_err(store)?;
    Ok(RawEvidence {
        version_id,
        block_id,
        anchor,
    })
}

/// Block texts of immutable IRs by `ir_b3`: content addressed, so a cached
/// entry never goes stale. Bounded; compiling is the hot path of every share.
fn ir_cache() -> &'static std::sync::Mutex<BTreeMap<String, std::sync::Arc<Vec<BlockText>>>> {
    static C: std::sync::OnceLock<
        std::sync::Mutex<BTreeMap<String, std::sync::Arc<Vec<BlockText>>>>,
    > = std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(BTreeMap::new()))
}

const IR_CACHE_MAX: usize = 20_000;

/// Block texts of the complete parse of every `b3` in one query and, for a
/// miss, one CAS read: `b3 -> blocks`.
async fn load_blocks(
    conn: &mut PgConnection,
    cas: &Cas,
    b3s: &[String],
) -> Result<BTreeMap<String, std::sync::Arc<Vec<BlockText>>>, PackError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT ON (b3) b3, ir_b3 FROM _oo_docs.parse_result \
          WHERE b3 = ANY($1::text[]) AND status = 'complete' AND ir_b3 IS NOT NULL \
          ORDER BY b3, finished_at DESC NULLS LAST, created_at DESC",
    )
    .bind(b3s)
    .fetch_all(&mut *conn)
    .await
    .map_err(store)?;
    let mut out = BTreeMap::new();
    for (b3, ir_b3) in rows {
        let hit = ir_cache()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&ir_b3)
            .cloned();
        let blocks = match hit {
            Some(h) => h,
            None => {
                let bytes = match cas.get(CasKind::Ir, &ir_b3) {
                    Ok(b) => b,
                    Err(CasError::NotFound) => continue,
                    Err(e) => return Err(store(e)),
                };
                let ir: DocIr = serde_json::from_slice(&bytes).map_err(store)?;
                let list: std::sync::Arc<Vec<BlockText>> = std::sync::Arc::new(
                    ir.blocks
                        .iter()
                        .map(|b| BlockText {
                            block_id: b.id.clone(),
                            ordinal: b.ordinal,
                            text: b.text.clone(),
                            anchor: Some(b.anchor.clone()),
                        })
                        .collect(),
                );
                let mut c = ir_cache().lock().unwrap_or_else(|p| p.into_inner());
                if c.len() >= IR_CACHE_MAX {
                    c.clear();
                }
                c.insert(ir_b3, list.clone());
                list
            }
        };
        out.insert(b3, blocks);
    }
    Ok(out)
}

impl PgReader {
    /// Load for `drive_id` at ontology version `version` (latest when
    /// `None`) and the normalized `prefixes` (empty = whole drive).
    pub async fn load(
        conn: &mut PgConnection,
        cas: &Cas,
        drive_id: &str,
        version: Option<i64>,
        prefixes: &[String],
    ) -> Result<Self, PackError> {
        let backend: Option<String> =
            sqlx::query_scalar("SELECT backend FROM _oo_docs.drive WHERE id = $1::uuid")
                .bind(drive_id)
                .fetch_optional(&mut *conn)
                .await
                .map_err(store)?;
        let drive = DriveInfo {
            id: drive_id.to_owned(),
            backend: backend.ok_or(PackError::Store("drive not found".into()))?,
        };
        // Drive access policy (10 11.3, B-S11-03): the second of its two places. A denied path is
        // not in the snapshot, so no fact whose evidence lives only there is visible to any scope.
        let config: Option<String> =
            sqlx::query_scalar("SELECT config::text FROM _oo_docs.drive WHERE id = $1::uuid")
                .bind(drive_id)
                .fetch_optional(&mut *conn)
                .await
                .map_err(store)?;
        let policy = crate::policy::DrivePolicy::from_config(
            &config
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|_| PackError::Internal("drive policy is not valid"))?;
        let row: Option<(i64, String)> = match version {
            Some(n) => {
                sqlx::query_as("SELECT n, as_of::text FROM _oo_docs.ontology_version WHERE n = $1")
                    .bind(n)
                    .fetch_optional(&mut *conn)
                    .await
            }
            None => {
                sqlx::query_as(
                    "SELECT n, as_of::text FROM _oo_docs.ontology_version ORDER BY n DESC LIMIT 1",
                )
                .fetch_optional(&mut *conn)
                .await
            }
        }
        .map_err(store)?;
        let (n, as_of) = row.ok_or(PackError::VersionNotFound)?;
        let version = VersionInfo { n, as_of };

        // The snapshot: file versions live when `n` was published.
        let all: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT fv.path_at_version, fv.b3, fv.id::text \
               FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
              WHERE f.drive_id = $1::uuid \
                AND fv.created_at <= (SELECT published_at FROM _oo_docs.ontology_version WHERE n = $2) \
                AND (fv.superseded_at IS NULL OR fv.superseded_at > (SELECT published_at FROM _oo_docs.ontology_version WHERE n = $2)) \
                AND (fv.deleted_at IS NULL OR fv.deleted_at > (SELECT published_at FROM _oo_docs.ontology_version WHERE n = $2)) \
              ORDER BY fv.path_at_version, fv.b3, fv.id",
        )
        .bind(drive_id)
        .bind(n)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let all: Vec<(String, String, String)> = all
            .into_iter()
            .filter(|(path, _, _)| policy.permits(path))
            .collect();
        let missing: Vec<String> = prefixes
            .iter()
            .filter(|p| !all.iter().any(|(path, _, _)| in_prefix(path, p)))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(PackError::ScopeInvalid { paths: missing });
        }
        let snapshot: Vec<SnapshotFile> = all
            .into_iter()
            .filter(|(path, _, _)| {
                prefixes.is_empty() || prefixes.iter().any(|p| in_prefix(path, p))
            })
            .map(|(path, b3, version_id)| SnapshotFile {
                path,
                b3,
                version_id,
            })
            .collect();
        let scope_ids: Vec<String> = snapshot.iter().map(|f| f.version_id.clone()).collect();

        // Visible facts, with only their in-scope evidence.
        let rows: Vec<FactSqlRow> = sqlx::query_as(
            "SELECT f.fact_key, s.key, f.predicate, o.key, f.value::text, f.valid_from::text, \
                        f.valid_to::text, f.origin, e.version_id::text, e.block_id, e.anchor::text \
                   FROM _oo_docs.fact f \
                   JOIN _oo_docs.entity s ON s.id = f.subject_id \
                   LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
                   JOIN _oo_docs.evidence e ON e.target_kind = 'fact' AND e.target_id = f.id \
                                           AND e.withdrawn_at IS NULL \
                  WHERE f.published_in <= $1 AND (f.retracted_in IS NULL OR f.retracted_in > $1) \
                    AND (f.valid_to IS NULL OR f.valid_to > $2::date) \
                    AND e.version_id = ANY($3::text[]::uuid[]) \
                  ORDER BY f.fact_key, e.version_id, e.block_id",
        )
        .bind(n)
        .bind(&version.as_of)
        .bind(&scope_ids)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let mut facts: BTreeMap<String, RawFact> = BTreeMap::new();
        for (fact_key, subject, predicate, object, value, from, to, origin, vid, block, anchor) in
            rows
        {
            let ev = evidence(vid, block, &anchor)?;
            match facts.get_mut(&fact_key) {
                Some(f) => f.evidence.push(ev),
                None => {
                    let predicate = Predicate::parse(&predicate)
                        .ok_or(PackError::Store("unknown predicate in the store".into()))?;
                    let value = match value {
                        Some(v) => Some(serde_json::from_str(&v).map_err(store)?),
                        None => None,
                    };
                    facts.insert(
                        fact_key.clone(),
                        RawFact {
                            fact_key,
                            subject_key: subject,
                            predicate,
                            object_key: object,
                            value,
                            valid_from: from,
                            valid_to: to,
                            origin,
                            evidence: vec![ev],
                        },
                    );
                }
            }
        }

        let alias_rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT en.key, a.alias, e.version_id::text, e.block_id, e.anchor::text \
               FROM _oo_docs.alias a JOIN _oo_docs.entity en ON en.id = a.entity_id \
               JOIN _oo_docs.evidence e ON e.target_kind = 'alias' AND e.target_id = a.id \
                                       AND e.withdrawn_at IS NULL \
              WHERE a.published_in <= $1 AND (a.retracted_in IS NULL OR a.retracted_in > $1) \
                AND e.version_id = ANY($2::text[]::uuid[]) \
              ORDER BY en.key, a.alias, e.version_id, e.block_id",
        )
        .bind(n)
        .bind(&scope_ids)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let mut aliases: BTreeMap<(String, String), RawAlias> = BTreeMap::new();
        for (entity_key, alias, vid, block, anchor) in alias_rows {
            let ev = evidence(vid, block, &anchor)?;
            aliases
                .entry((entity_key.clone(), alias.clone()))
                .or_insert_with(|| RawAlias {
                    entity_key,
                    alias,
                    evidence: Vec::new(),
                })
                .evidence
                .push(ev);
        }

        let mut visible: BTreeSet<String> = BTreeSet::new();
        for f in facts.values() {
            visible.insert(f.subject_key.clone());
            if let Some(o) = &f.object_key {
                visible.insert(o.clone());
            }
        }
        for (k, _) in aliases.keys() {
            visible.insert(k.clone());
        }
        let keys: Vec<String> = visible.iter().cloned().collect();
        let ent_rows: Vec<EntitySqlRow> = sqlx::query_as(
            "SELECT key, type, subkind, thing_label, canonical_name \
               FROM _oo_docs.entity WHERE key = ANY($1::text[]) ORDER BY key",
        )
        .bind(&keys)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let mut entities = Vec::with_capacity(ent_rows.len());
        for (key, ty, subkind, thing_label, name) in ent_rows {
            entities.push(RawEntity {
                key,
                ty: EntityType::parse(&ty).ok_or(PackError::Store("unknown entity type".into()))?,
                subkind,
                thing_label,
                name,
            });
        }

        // Block text of the in-scope file versions.
        let mut blocks: BTreeMap<String, Vec<BlockText>> = BTreeMap::new();
        let mut by_b3: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for f in &snapshot {
            by_b3.entry(&f.b3).or_default().push(&f.version_id);
        }
        let b3_list: Vec<String> = by_b3.keys().map(|k| (*k).to_owned()).collect();
        let loaded = load_blocks(conn, cas, &b3_list).await?;
        for (b3, vids) in by_b3 {
            if let Some(list) = loaded.get(b3) {
                for v in vids {
                    blocks.insert(v.to_owned(), list.as_ref().clone());
                }
            }
        }

        // What was held back: existence tests only.
        let blocked_facts: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT f.fact_key, s.key, o.key \
               FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
               LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
              WHERE f.published_in <= $1 AND (f.retracted_in IS NULL OR f.retracted_in > $1) \
                AND (f.valid_to IS NULL OR f.valid_to > $2::date) \
                AND EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' \
                              AND e.target_id = f.id AND e.withdrawn_at IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' \
                              AND e.target_id = f.id AND e.withdrawn_at IS NULL \
                              AND e.version_id = ANY($3::text[]::uuid[]))",
        )
        .bind(n)
        .bind(&version.as_of)
        .bind(&scope_ids)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let blocked_aliases: Vec<(String, String)> = sqlx::query_as(
            "SELECT en.key, a.alias \
               FROM _oo_docs.alias a JOIN _oo_docs.entity en ON en.id = a.entity_id \
              WHERE a.published_in <= $1 AND (a.retracted_in IS NULL OR a.retracted_in > $1) \
                AND EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'alias' \
                              AND e.target_id = a.id AND e.withdrawn_at IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'alias' \
                              AND e.target_id = a.id AND e.withdrawn_at IS NULL \
                              AND e.version_id = ANY($2::text[]::uuid[]))",
        )
        .bind(n)
        .bind(&scope_ids)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let mut blocked_keys: BTreeSet<String> = BTreeSet::new();
        let mut terms: BTreeSet<String> = BTreeSet::new();
        for (_, s, o) in &blocked_facts {
            blocked_keys.insert(s.clone());
            if let Some(o) = o {
                blocked_keys.insert(o.clone());
            }
        }
        for (k, alias) in &blocked_aliases {
            blocked_keys.insert(k.clone());
            terms.insert(alias.clone());
        }
        blocked_keys.retain(|k| !visible.contains(k));
        let only: Vec<String> = blocked_keys.iter().cloned().collect();
        let names: Vec<(String,)> = sqlx::query_as(
            "SELECT canonical_name FROM _oo_docs.entity WHERE key = ANY($1::text[]) \
             UNION SELECT a.alias FROM _oo_docs.alias a JOIN _oo_docs.entity en ON en.id = a.entity_id \
              WHERE en.key = ANY($1::text[]) AND a.status IN ('draft', 'published')",
        )
        .bind(&only)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        terms.extend(names.into_iter().map(|(t,)| t));

        // Open constraint violations (16.7) on the entities of this scope. The
        // sides are held-back facts: only their in-scope evidence is read.
        let viol: Vec<(String, String, Vec<String>)> = sqlx::query_as(
            "SELECT entity_key, constraint_id, fact_ids::text[] FROM _oo_docs.constraint_violation \
              WHERE status = 'open' AND entity_key = ANY($1::text[]) ORDER BY entity_key, constraint_id",
        )
        .bind(&keys)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let side_ids: Vec<String> = viol.iter().flat_map(|v| v.2.iter().cloned()).collect();
        type SideRow = (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let side_rows: Vec<SideRow> = sqlx::query_as(
            "SELECT f.id::text, f.fact_key, s.key, f.predicate, o.key, f.value::text, \
                    f.valid_from::text, f.valid_to::text, f.origin, \
                    e.version_id::text, e.block_id, e.anchor::text \
               FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
               LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
               LEFT JOIN _oo_docs.evidence e ON e.target_kind = 'fact' AND e.target_id = f.id \
                    AND e.withdrawn_at IS NULL AND e.version_id = ANY($2::text[]::uuid[]) \
              WHERE f.id = ANY($1::text[]::uuid[]) \
              ORDER BY f.fact_key, e.version_id, e.block_id",
        )
        .bind(&side_ids)
        .bind(&scope_ids)
        .fetch_all(&mut *conn)
        .await
        .map_err(store)?;
        let mut sides: BTreeMap<String, RawFact> = BTreeMap::new();
        for (
            id,
            fact_key,
            subject,
            predicate,
            object,
            value,
            from,
            to,
            origin,
            vid,
            block,
            anchor,
        ) in side_rows
        {
            let slot = match sides.get_mut(&id) {
                Some(f) => f,
                None => {
                    let predicate = Predicate::parse(&predicate)
                        .ok_or(PackError::Store("unknown predicate in the store".into()))?;
                    let value = match value {
                        Some(v) => Some(serde_json::from_str(&v).map_err(store)?),
                        None => None,
                    };
                    sides.entry(id.clone()).or_insert(RawFact {
                        fact_key,
                        subject_key: subject,
                        predicate,
                        object_key: object,
                        value,
                        valid_from: from,
                        valid_to: to,
                        origin,
                        evidence: Vec::new(),
                    })
                }
            };
            if let (Some(vid), Some(block), Some(anchor)) = (vid, block, anchor) {
                slot.evidence.push(evidence(vid, block, &anchor)?);
            }
        }
        let conflicts: Vec<RawConflict> = viol
            .into_iter()
            .map(|(entity_key, constraint_id, ids)| {
                let mut hidden = 0u32;
                let mut list = Vec::new();
                for id in ids {
                    match sides.get(&id) {
                        Some(f) if !f.evidence.is_empty() => list.push(f.clone()),
                        _ => hidden += 1,
                    }
                }
                RawConflict {
                    entity_key,
                    constraint_id,
                    sides: list,
                    hidden_sides: hidden,
                }
            })
            .collect();

        let scoped = Scoped {
            conflicts,
            snapshot,
            entities,
            aliases: aliases.into_values().collect(),
            facts: facts.into_values().collect(),
            blocks,
            blocked: BlockedInfo {
                facts: blocked_facts
                    .iter()
                    .map(|(k, _, _)| k.as_str())
                    .collect::<BTreeSet<_>>()
                    .len() as u64,
                entities: only.len() as u64,
                terms: terms.into_iter().collect(),
            },
        };
        Ok(Self {
            drive,
            version,
            prefixes: prefixes.to_vec(),
            scoped,
        })
    }
}

impl OntologyReader for PgReader {
    fn drive(&self) -> DriveInfo {
        self.drive.clone()
    }

    fn latest_version(&self) -> Result<VersionInfo, PackError> {
        Ok(self.version.clone())
    }

    fn version(&self, n: i64) -> Result<VersionInfo, PackError> {
        if n == self.version.n {
            Ok(self.version.clone())
        } else {
            Err(PackError::VersionNotFound)
        }
    }

    fn scoped(&self, version: &VersionInfo, prefixes: &[String]) -> Result<Scoped, PackError> {
        if *version != self.version || prefixes != self.prefixes.as_slice() {
            return Err(PackError::Internal(
                "reader was loaded for another version or scope",
            ));
        }
        Ok(self.scoped.clone())
    }
}
