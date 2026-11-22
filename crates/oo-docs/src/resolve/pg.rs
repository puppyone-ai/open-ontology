//! Database side of resolve (10 §5.4, §4.7): load the candidates of one file
//! into a [`MapIndex`], store identity attributes, merge duplicates by
//! identity and propose merges for shared published aliases.
//!
//! Callers hold the advisory locks of the canonical names (`4.7`) and run
//! inside one transaction.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use sqlx::PgConnection;

use super::exact::{Candidate, MapIndex, Via, dir_of, identities_of};
use super::honorific::{stripped_forms, surname_form};
use super::merge::{MergeError, merge_entities};
use crate::extract::model::{EntityType, Extraction};
use crate::store::StoreError;

fn parse_type(ty: &str) -> Result<EntityType, StoreError> {
    EntityType::parse(ty).ok_or_else(|| StoreError::Protocol(format!("unknown entity type {ty}")))
}

/// The names a file may look up: canonical forms of every mention, the
/// title-free forms of people, and the surname prefixes.
pub fn lookup_names(x: &Extraction) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut surnames: BTreeSet<String> = BTreeSet::new();
    let mut add = |e: &crate::extract::model::EntityRef| {
        names.insert(e.norm());
        if e.ty == EntityType::Person {
            for f in stripped_forms(&e.name) {
                if let Some(s) = surname_form(&f) {
                    surnames.insert(s);
                }
                names.insert(f);
            }
        }
    };
    for m in &x.entities {
        add(&m.entity);
    }
    for a in &x.aliases {
        add(&a.entity);
    }
    for f in &x.facts {
        add(&f.subject);
        if let Some(o) = &f.object {
            add(o);
        }
    }
    for i in &x.identities {
        add(&i.entity);
    }
    for h in &x.hints {
        let t = crate::extract::model::EntityRef::new(h.target_ty, h.target_name.clone());
        add(&t);
    }
    (names, surnames)
}

/// The canonical names of every linkable entity of the Drive, for
/// `extract::links` (`doc.link`).
///
/// Only *established* entities: one a rule found, or one a published fact of
/// another source rests on. An entity only the model proposed, and nothing
/// corroborated, is not published through the back door of a name match in
/// other files (5.3: a single source waits for a person).
pub async fn load_lexicon(
    conn: &mut PgConnection,
) -> Result<Vec<crate::extract::links::LexEntry>, StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT e.type, e.canonical_name FROM _oo_docs.entity e \
          WHERE e.status = 'active' \
            AND e.type IN ('Person', 'Organization', 'Project', 'Topic', 'Term', 'Event') \
            AND EXISTS (SELECT 1 FROM _oo_docs.fact f \
                         WHERE e.id IN (f.subject_id, f.object_id) \
                           AND COALESCE(f.rule_id, '') NOT IN ('doc.link', 'doc.about') \
                           AND ((f.origin = 'rule' AND f.status IN ('draft', 'published')) \
                                OR (f.origin IN ('llm', 'human', 'suggest') AND f.status = 'published'))) \
          ORDER BY e.key",
    )
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|(ty, name)| {
            Ok(crate::extract::links::LexEntry {
                ty: parse_type(&ty)?,
                name,
            })
        })
        .collect()
}

fn like_prefix(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// Candidates for the names of a text (the read-only resolve API): canonical
/// names, aliases, title-free forms and surname prefixes, no identity.
pub async fn load_index_for(
    conn: &mut PgConnection,
    names: &BTreeSet<String>,
    surnames: &BTreeSet<String>,
) -> Result<MapIndex, StoreError> {
    load_index_inner(conn, names, surnames, &BTreeMap::new(), &[]).await
}

/// Candidates for every mention of `x`, with their identity, one-hop
/// neighbours and evidence directories.
pub async fn load_index(conn: &mut PgConnection, x: &Extraction) -> Result<MapIndex, StoreError> {
    let (names, surnames) = lookup_names(x);
    // Own keys that were merged away resolve through the redirect.
    let mut own: BTreeMap<String, (EntityType, Option<String>, String)> = BTreeMap::new();
    let mut every: Vec<&crate::extract::model::EntityRef> = Vec::new();
    for m in &x.entities {
        every.push(&m.entity);
    }
    for a in &x.aliases {
        every.push(&a.entity);
    }
    for f in &x.facts {
        every.push(&f.subject);
        if let Some(o) = &f.object {
            every.push(o);
        }
    }
    for e in every {
        own.insert(e.key(), (e.ty, e.thing_label.clone(), e.norm()));
    }
    let pairs: Vec<(EntityType, String, String)> = identities_of(x)
        .iter()
        .flat_map(|(e, id)| id.iter().map(|(k, v)| (e.ty, k.clone(), v.clone())))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    load_index_inner(conn, &names, &surnames, &own, &pairs).await
}

async fn load_index_inner(
    conn: &mut PgConnection,
    names: &BTreeSet<String>,
    surnames: &BTreeSet<String>,
    own: &BTreeMap<String, (EntityType, Option<String>, String)>,
    pairs: &[(EntityType, String, String)],
) -> Result<MapIndex, StoreError> {
    let names_v: Vec<String> = names.iter().cloned().collect();
    let name_rows: Vec<(String, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT e.type, e.thing_label, e.norm_name, e.key, 'canonical' \
           FROM _oo_docs.entity e \
          WHERE e.status = 'active' AND e.norm_name = ANY($1::text[]) \
         UNION ALL \
         SELECT e.type, e.thing_label, a.norm_alias, e.key, a.status \
           FROM _oo_docs.alias a JOIN _oo_docs.entity e ON e.id = a.entity_id \
          WHERE e.status = 'active' AND a.status IN ('draft', 'published') \
            AND a.norm_alias = ANY($1::text[])",
    )
    .bind(&names_v)
    .fetch_all(&mut *conn)
    .await?;

    let own_keys: Vec<String> = own.keys().cloned().collect();
    let redirects: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT r.from_key, r.to_key, t.type, t.thing_label \
           FROM _oo_docs.entity_redirect r \
           JOIN _oo_docs.entity t ON t.key = r.to_key AND t.status = 'active' \
          WHERE r.from_key = ANY($1::text[])",
    )
    .bind(&own_keys)
    .fetch_all(&mut *conn)
    .await?;

    // Identity hits.
    let id_rows: Vec<(String, String, String, String)> = if pairs.is_empty() {
        Vec::new()
    } else {
        let tys: Vec<String> = pairs.iter().map(|p| p.0.as_str().to_owned()).collect();
        let kinds: Vec<String> = pairs.iter().map(|p| p.1.clone()).collect();
        let values: Vec<String> = pairs.iter().map(|p| p.2.clone()).collect();
        sqlx::query_as(
            "SELECT e.type, e.key, j.key, j.value \
               FROM _oo_docs.entity e, jsonb_each_text(e.identity) j \
              WHERE e.status = 'active' \
                AND (e.type, j.key, j.value) IN \
                    (SELECT * FROM unnest($1::text[], $2::text[], $3::text[]))",
        )
        .bind(&tys)
        .bind(&kinds)
        .bind(&values)
        .fetch_all(&mut *conn)
        .await?
    };

    // Surname candidates.
    let patterns: Vec<String> = surnames.iter().map(|s| like_prefix(s)).collect();
    let sn_rows: Vec<(String, String)> = if patterns.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            "SELECT e.key, e.norm_name FROM _oo_docs.entity e \
              WHERE e.type = 'Person' AND e.status = 'active' \
                AND e.norm_name LIKE ANY($1::text[])",
        )
        .bind(&patterns)
        .fetch_all(&mut *conn)
        .await?
    };

    // Everything that may become a candidate gets its context loaded once.
    let mut keys: BTreeSet<String> = BTreeSet::new();
    keys.extend(name_rows.iter().map(|r| r.3.clone()));
    keys.extend(redirects.iter().map(|r| r.1.clone()));
    keys.extend(id_rows.iter().map(|r| r.1.clone()));
    keys.extend(sn_rows.iter().map(|r| r.0.clone()));
    let key_list: Vec<String> = keys.iter().cloned().collect();

    let mut identity: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, identity::text FROM _oo_docs.entity WHERE key = ANY($1::text[])",
    )
    .bind(&key_list)
    .fetch_all(&mut *conn)
    .await?;
    for (key, text) in rows {
        let map: BTreeMap<String, String> = serde_json::from_str(&text)
            .map_err(|e| StoreError::Protocol(format!("entity identity: {e}")))?;
        identity.insert(key, map);
    }
    let mut neighbors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT s.key, o.key FROM _oo_docs.fact f \
           JOIN _oo_docs.entity s ON s.id = f.subject_id \
           JOIN _oo_docs.entity o ON o.id = f.object_id \
          WHERE f.status IN ('draft', 'published') \
            AND (s.key = ANY($1::text[]) OR o.key = ANY($1::text[]))",
    )
    .bind(&key_list)
    .fetch_all(&mut *conn)
    .await?;
    for (s, o) in rows {
        neighbors.entry(s.clone()).or_default().insert(o.clone());
        neighbors.entry(o).or_default().insert(s);
    }
    let mut dirs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT e.key, fv.path_at_version \
           FROM _oo_docs.evidence ev \
           JOIN _oo_docs.entity e ON ev.target_kind = 'entity' AND ev.target_id = e.id \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
          WHERE ev.withdrawn_at IS NULL AND e.key = ANY($1::text[])",
    )
    .bind(&key_list)
    .fetch_all(&mut *conn)
    .await?;
    for (key, path) in rows {
        dirs.entry(key).or_default().insert(dir_of(&path));
    }
    let make = |key: &str, via: Via| Candidate {
        key: key.to_owned(),
        via,
        neighbors: neighbors.get(key).cloned().unwrap_or_default(),
        dirs: dirs.get(key).cloned().unwrap_or_default(),
        identity: identity.get(key).cloned().unwrap_or_default(),
    };

    let mut index = MapIndex::default();
    for (ty, label, name_norm, key, via) in name_rows {
        let via = match via.as_str() {
            "canonical" => Via::CanonicalName,
            "published" => Via::AliasPublished,
            _ => Via::AliasDraft,
        };
        index.insert(
            parse_type(&ty)?,
            label.as_deref(),
            &name_norm,
            make(&key, via),
        );
    }
    for (from_key, to_key, ty, label) in redirects {
        if let Some((_, _, name_norm)) = own.get(&from_key) {
            index.insert(
                parse_type(&ty)?,
                label.as_deref(),
                name_norm,
                make(&to_key, Via::CanonicalName),
            );
        }
    }
    for (ty, key, kind, value) in id_rows {
        index.insert_identity(parse_type(&ty)?, &kind, &value, make(&key, Via::Identity));
    }
    for (key, name_norm) in sn_rows {
        for s in surnames {
            if name_norm.starts_with(s.as_str()) && name_norm.len() > s.len() {
                index.insert_surname(s, make(&key, Via::CanonicalName));
            }
        }
    }
    Ok(index)
}

/// Add the identity attributes found in this file to their entities; an
/// attribute an entity already has is never overwritten. Returns the keys of
/// entities that gained one.
pub async fn store_identities(
    conn: &mut PgConnection,
    resolved: &super::exact::Resolved,
) -> Result<Vec<String>, StoreError> {
    let mut gained = Vec::new();
    for e in &resolved.entities {
        if e.identity.is_empty() {
            continue;
        }
        let new = serde_json::to_string(&e.identity)
            .map_err(|er| StoreError::Protocol(format!("identity json: {er}")))?;
        let done = sqlx::query(
            "UPDATE _oo_docs.entity SET identity = $2::jsonb || identity, revision = revision + 1 \
              WHERE key = $1 AND status = 'active' AND NOT (identity @> $2::jsonb)",
        )
        .bind(&e.key)
        .bind(&new)
        .execute(&mut *conn)
        .await?;
        if done.rows_affected() > 0 {
            gained.push(e.key.clone());
        }
    }
    Ok(gained)
}

/// Active entities of the same type that share an identity attribute with
/// one of `keys` (5.4 step 2: they are one entity).
pub async fn same_identity_pairs(
    conn: &mut PgConnection,
    keys: &[String],
) -> Result<Vec<(String, String)>, StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.key, b.key \
           FROM _oo_docs.entity a \
           JOIN _oo_docs.entity b ON b.type = a.type AND b.id <> a.id AND b.status = 'active' \
           JOIN LATERAL jsonb_each_text(a.identity) ja ON true \
           JOIN LATERAL jsonb_each_text(b.identity) jb ON jb.key = ja.key AND jb.value = ja.value \
          WHERE a.status = 'active' AND a.key = ANY($1::text[]) \
          ORDER BY 1, 2",
    )
    .bind(keys)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

/// `(entity key, type, name, content hash, version:block, path)` of one live evidence row.
type EvidenceRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// What [`propose_merges`] did with the pairs of entities that share a published alias.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeOutcome {
    /// `entity_merge` items queued: pairs only a person can judge.
    pub queued: u32,
    /// Pairs merged without a person (D91): the same name once punctuation and width are ignored, or the same
    /// sentence of the same file, or the same file content.
    pub merged: u32,
    /// Pairs that need no question at all: two different documents are never one entity.
    pub dismissed: u32,
}

/// The name with everything but letters and digits removed: `A.B Corp` and `a b corp` are one name.
fn loose_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfkc()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Entities of `keys` that share at least one published alias with another active entity of the same type
/// (5.4 step 6). D91: what a program can settle is settled here and never queued.
/// * documents: the same content (`b3`) is one document; different content is never one entity;
/// * the same name once punctuation, spacing and width are ignored, or evidence in the same block: merged;
/// * everything else is queued as `entity_merge` with the files of both entities.
pub async fn propose_merges(
    conn: &mut PgConnection,
    keys: &[String],
    version_id: &str,
) -> Result<MergeOutcome, StoreError> {
    let pairs: Vec<(String, String, String)> = sqlx::query_as(
        // Two branches, one anchored on each end of the pair: an `OR` over the two keys is applied after the alias join, so every pair of
        // entities sharing an alias (thousands of documents of one stem in a code folder) was built and thrown away on each publish.
        "SELECT DISTINCT lo, hi, norm_alias FROM ( \
           SELECT LEAST(a.key, b.key) AS lo, GREATEST(a.key, b.key) AS hi, pa.norm_alias \
             FROM _oo_docs.entity a \
             JOIN _oo_docs.alias pa ON pa.entity_id = a.id AND pa.status = 'published' \
             JOIN _oo_docs.alias pb ON pb.norm_alias = pa.norm_alias AND pb.status = 'published' \
             JOIN _oo_docs.entity b ON b.id = pb.entity_id AND b.id <> a.id \
                  AND b.type = a.type AND b.status = 'active' \
            WHERE a.status = 'active' AND a.key = ANY($1::text[]) \
              AND EXISTS (SELECT 1 FROM _oo_docs.evidence ea WHERE ea.target_kind = 'entity' \
                           AND ea.target_id = a.id AND ea.withdrawn_at IS NULL) \
              AND EXISTS (SELECT 1 FROM _oo_docs.evidence eb WHERE eb.target_kind = 'entity' \
                           AND eb.target_id = b.id AND eb.withdrawn_at IS NULL) \
           UNION ALL \
           SELECT LEAST(a.key, b.key), GREATEST(a.key, b.key), pa.norm_alias \
             FROM _oo_docs.entity b \
             JOIN _oo_docs.alias pb ON pb.entity_id = b.id AND pb.status = 'published' \
             JOIN _oo_docs.alias pa ON pa.norm_alias = pb.norm_alias AND pa.status = 'published' \
             JOIN _oo_docs.entity a ON a.id = pa.entity_id AND a.id <> b.id \
                  AND a.type = b.type AND a.status = 'active' \
            WHERE b.status = 'active' AND b.key = ANY($1::text[]) \
              AND EXISTS (SELECT 1 FROM _oo_docs.evidence ea WHERE ea.target_kind = 'entity' \
                           AND ea.target_id = a.id AND ea.withdrawn_at IS NULL) \
              AND EXISTS (SELECT 1 FROM _oo_docs.evidence eb WHERE eb.target_kind = 'entity' \
                           AND eb.target_id = b.id AND eb.withdrawn_at IS NULL) \
         ) pairs ORDER BY 1, 2, 3",
    )
    .bind(keys)
    .fetch_all(&mut *conn)
    .await?;
    let mut grouped: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (a, b, alias) in pairs {
        grouped.entry((a, b)).or_default().push(alias);
    }
    let mut out = MergeOutcome::default();
    if grouped.is_empty() {
        return Ok(out);
    }
    // What the decision reads about every entity of every pair: type, name, content hashes, evidence blocks, files.
    let involved: Vec<String> = grouped
        .keys()
        .flat_map(|(a, b)| [a.clone(), b.clone()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let rows: Vec<EvidenceRow> = sqlx::query_as(
        "SELECT e.key, e.type, e.canonical_name, ev.b3, ev.version_id::text || ':' || ev.block_id, fv.path_at_version \
           FROM _oo_docs.entity e \
           JOIN _oo_docs.evidence ev ON ev.target_kind = 'entity' AND ev.target_id = e.id AND ev.withdrawn_at IS NULL \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
          WHERE e.key = ANY($1::text[])",
    )
    .bind(&involved)
    .fetch_all(&mut *conn)
    .await?;
    #[derive(Default)]
    struct Info {
        ty: String,
        name: String,
        b3: BTreeSet<String>,
        blocks: BTreeSet<String>,
        files: BTreeSet<String>,
    }
    let mut info: BTreeMap<String, Info> = BTreeMap::new();
    for (key, ty, name, b3, block, path) in rows {
        let i = info.entry(key).or_default();
        i.ty = ty;
        i.name = name;
        i.b3.extend(b3);
        i.blocks.extend(block);
        i.files.extend(path);
    }
    for ((a, b), aliases) in grouped {
        let (Some(ia), Some(ib)) = (info.get(&a), info.get(&b)) else {
            continue;
        };
        let same_content = !ia.b3.is_empty() && ia.b3 == ib.b3;
        let (settled, why): (bool, &str) = if ia.ty == "Document" {
            if same_content {
                (true, "same_content")
            } else {
                out.dismissed += 1;
                continue;
            }
        } else if loose_name(&ia.name) == loose_name(&ib.name) {
            (true, "same_name")
        } else if !ia.blocks.is_disjoint(&ib.blocks) {
            (true, "same_evidence")
        } else {
            (false, "shared_alias")
        };
        let dedupe = format!("entity_merge:{a}:{b}");
        if settled {
            match merge_entities(conn, &b, &a, "merge", None).await {
                Ok(_) => {
                    out.merged += 1;
                    // The decision stays visible: a closed item, decided by the engine.
                    sqlx::query(
                        "INSERT INTO _oo_docs.review_item \
                           (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                            confidence_bp, reason_code, status, decided_at, decided_by, decision_note) \
                         SELECT gen_random_uuid(), 'entity_merge', $1, f.drive_id, f.id, fv.id, \
                                jsonb_build_object('entity_keys', jsonb_build_array($2::text, $3::text), 'shared_aliases', $4::jsonb), \
                                NULL, $6, 'accepted', clock_timestamp(), 'engine', $6 \
                           FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
                          WHERE fv.id = $5::uuid \
                         ON CONFLICT (dedupe_key) DO NOTHING",
                    )
                    .bind(&dedupe)
                    .bind(&a)
                    .bind(&b)
                    .bind(serde_json::to_string(&aliases).unwrap_or_else(|_| "[]".into()))
                    .bind(version_id)
                    .bind(why)
                    .execute(&mut *conn)
                    .await?;
                    continue;
                }
                // An identity conflict, an entity merged meanwhile: a person decides, or nothing is left to decide.
                Err(MergeError::IdentityConflict(..)) => {}
                Err(
                    MergeError::NotActive(_) | MergeError::NotFound(_) | MergeError::SameEntity,
                ) => continue,
                Err(MergeError::Store(e)) => return Err(e),
                Err(e) => return Err(StoreError::Protocol(e.to_string())),
            }
        }
        let evidence = json!([
            {"key": a, "files": ia.files.iter().take(3).collect::<Vec<_>>()},
            {"key": b, "files": ib.files.iter().take(3).collect::<Vec<_>>()},
        ]);
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'entity_merge', $1, f.drive_id, f.id, fv.id, \
                    jsonb_build_object('entity_keys', jsonb_build_array($2::text, $3::text), \
                                       'shared_aliases', $4::jsonb, 'evidence', $6::jsonb), \
                    NULL, 'shared_alias' \
               FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id \
              WHERE fv.id = $5::uuid \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(&dedupe)
        .bind(&a)
        .bind(&b)
        .bind(serde_json::to_string(&aliases).unwrap_or_else(|_| "[]".into()))
        .bind(version_id)
        .bind(evidence.to_string())
        .execute(&mut *conn)
        .await?;
        out.queued += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(out)
}
