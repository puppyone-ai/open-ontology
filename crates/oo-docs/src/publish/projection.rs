//! Read-only projection of published document entities into the Agent DB and links to database
//! entities (D12, 10 §11.3, board B-S11-04).
//!
//! The documents' ontology lives in its own database (`docs-db`, §15 Q2); the database
//! ontology lives in the Agent DB. They meet here, one way only:
//!
//! 1. [`project`] reads the *visible graph* of a Drive (the one exit, D26, so the Drive policy of
//!    B-S11-03 applies) and writes `_oo_docproj.doc_entity`, `doc_entity_source` in the Agent DB.
//!    The write is a rebuild in one transaction and is skipped when the digest of what would be
//!    written equals the stored one: idempotent, and the whole schema can be dropped and rebuilt.
//! 2. [`link`] matches document entities with rows of database tables by the rules the caller
//!    supplies (`customers.name` against Organizations, ...): normalized name or alias equality,
//!    the same normalization the resolver uses. `auto` links are recomputed, a person's
//!    `confirmed` and `rejected` decisions stay.
//!
//! Nothing here writes to docs-db and nothing in the Agent DB outside `_oo_docproj`; the database
//! table is only read. The DDL is `projection.sql`; the Agent DB migration (patch
//! `0147_docs_projection.sql`) is a copy of it and a test compares them when the migration exists.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection, Row};

use crate::compile::PackError;
use crate::compile::pg::PgReader;
use crate::compile::visible::VisibleGraph;
use crate::resolve::normalize::norm;
use crate::store::StoreError;

/// The Agent DB schema objects (also `0147_docs_projection.sql`).
pub const DDL: &str = include_str!("projection.sql");
/// Provenance, per entity digests and the reverse link index (also `0152_docproj_provenance_and_link_index.sql`).
pub const DDL_V2: &str = include_str!("projection_v2.sql");

/// One piece of evidence of a projected entity: the file, its version and the anchor inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedEvidence {
    pub path: String,
    pub file_version: String,
    pub block_id: String,
    /// The document IR anchor as JSON text (page and box, sheet and cell, heading and line).
    pub anchor: String,
}

/// One projected entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedEntity {
    pub key: String,
    pub ty: String,
    pub name: String,
    pub norm_name: String,
    pub aliases: Vec<String>,
    pub fact_count: i32,
    pub sources: Vec<String>,
    /// Anchored provenance, ordered; empty in rows built by hand (tests).
    pub evidence: Vec<ProjectedEvidence>,
}

#[derive(Debug)]
pub enum ProjectionError {
    Pack(PackError),
    Store(StoreError),
    Db(sqlx::Error),
    /// A rule names an identifier that is not a plain `schema.table` / column.
    BadRule(&'static str),
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pack(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
            Self::Db(e) => write!(f, "{e}"),
            Self::BadRule(w) => f.write_str(w),
        }
    }
}

impl std::error::Error for ProjectionError {}

impl From<sqlx::Error> for ProjectionError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// Anchored evidence rows kept per entity (the file list in `doc_entity_source` is complete).
pub const MAX_EVIDENCE_PER_ENTITY: usize = 200;

/// The projection of one Drive at one ontology version, from the visible graph.
pub fn entities_of(graph: &VisibleGraph) -> Vec<ProjectedEntity> {
    let mut out: BTreeMap<String, ProjectedEntity> = BTreeMap::new();
    for e in graph.entities() {
        let mut aliases: Vec<String> = graph
            .aliases(&e.key)
            .iter()
            .map(|a| a.alias.clone())
            .collect();
        aliases.sort();
        aliases.dedup();
        let mut sources: Vec<String> = Vec::new();
        let mut evidence: Vec<ProjectedEvidence> = Vec::new();
        let mut facts = 0;
        let mut see = |v: &crate::compile::visible::VisibleEvidence| {
            sources.push(v.path.clone());
            evidence.push(ProjectedEvidence {
                path: v.path.clone(),
                file_version: v.version_id.clone(),
                block_id: v.block_id.clone(),
                anchor: serde_json::to_value(&v.anchor)
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "{}".to_owned()),
            });
        };
        for f in graph.facts_of(&e.key) {
            facts += 1;
            graph.evidence(f).iter().for_each(&mut see);
        }
        for a in graph.aliases(&e.key) {
            a.evidence.iter().for_each(&mut see);
        }
        sources.sort();
        sources.dedup();
        evidence.sort_by(|a, b| {
            (&a.path, &a.block_id, &a.file_version, &a.anchor).cmp(&(
                &b.path,
                &b.block_id,
                &b.file_version,
                &b.anchor,
            ))
        });
        evidence.dedup();
        evidence.truncate(MAX_EVIDENCE_PER_ENTITY);
        out.insert(
            e.key.clone(),
            ProjectedEntity {
                key: e.key.clone(),
                ty: e.ty.as_str().to_owned(),
                name: e.name.clone(),
                norm_name: norm(&e.name),
                aliases,
                fact_count: facts,
                sources,
                evidence,
            },
        );
    }
    out.into_values().collect()
}

/// Digest of one entity row: what is written for it, anchors included.
pub fn row_digest(r: &ProjectedEntity) -> String {
    let ev: Vec<[&str; 4]> = r
        .evidence
        .iter()
        .map(|e| [&*e.path, &*e.file_version, &*e.block_id, &*e.anchor])
        .collect();
    let mut h = Sha256::new();
    h.update(
        serde_json::json!([r.key, r.ty, r.name, r.aliases, r.fact_count, r.sources, ev])
            .to_string()
            .as_bytes(),
    );
    format!("{:x}", h.finalize())
}

/// Digest of what would be written (ordered, so equal graphs give equal digests).
pub fn digest(version: i64, rows: &[ProjectedEntity]) -> String {
    let mut h = Sha256::new();
    h.update(format!("oo.docproj/v2\n{version}\n").as_bytes());
    for r in rows {
        h.update(row_digest(r).as_bytes());
        h.update(b"\n");
    }
    format!("{:x}", h.finalize())
}

/// What [`project`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionReport {
    pub ontology_version: i64,
    pub entities: usize,
    /// False when the stored digest already equalled the new one (nothing was written).
    pub written: bool,
    pub digest: String,
    /// Entities added, rewritten (their digest changed) and removed by this run; the rest were left alone.
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
}

/// Project the published entities of `drive_id` (latest ontology version) into the Agent DB.
/// `docs` is a docs-db connection (read), `agent` the Agent DB connection (the only writes).
pub async fn project(
    docs: &mut PgConnection,
    cas: &crate::cas::Cas,
    agent: &mut PgConnection,
    drive_id: &str,
) -> Result<ProjectionReport, ProjectionError> {
    project_with(docs, cas, agent, drive_id, "").await
}

/// [`project`], recording `trigger_key` (what the caller compares to decide whether a run is due).
///
/// Incremental: an entity whose row digest is unchanged is not touched, a changed one is rewritten with its
/// sources and evidence, a vanished one is removed, all in one transaction. A replay of the same input writes
/// nothing; a rebuild from nothing (the schema dropped) writes everything.
pub async fn project_with(
    docs: &mut PgConnection,
    cas: &crate::cas::Cas,
    agent: &mut PgConnection,
    drive_id: &str,
    trigger_key: &str,
) -> Result<ProjectionReport, ProjectionError> {
    let reader = PgReader::load(docs, cas, drive_id, None, &[])
        .await
        .map_err(ProjectionError::Pack)?;
    let version = crate::compile::reader::OntologyReader::latest_version(&reader)
        .map_err(ProjectionError::Pack)?;
    let (graph, _) = VisibleGraph::load(&reader, &version, &[]).map_err(ProjectionError::Pack)?;
    let rows = entities_of(&graph);
    let dg = digest(version.n, &rows);

    sqlx::raw_sql(DDL).execute(&mut *agent).await?;
    sqlx::raw_sql(DDL_V2).execute(&mut *agent).await?;
    let stored: Option<(String, String)> = sqlx::query_as(
        "SELECT source_digest, trigger_key FROM _oo_docproj.projection_state WHERE drive_id = $1",
    )
    .bind(drive_id)
    .fetch_optional(&mut *agent)
    .await?;
    if stored.as_ref().map(|s| s.0.as_str()) == Some(dg.as_str()) {
        if stored.as_ref().map(|s| s.1.as_str()) != Some(trigger_key) {
            sqlx::query(
                "UPDATE _oo_docproj.projection_state SET trigger_key = $2 WHERE drive_id = $1",
            )
            .bind(drive_id)
            .bind(trigger_key)
            .execute(&mut *agent)
            .await?;
        }
        return Ok(ProjectionReport {
            ontology_version: version.n,
            entities: rows.len(),
            written: false,
            digest: dg,
            inserted: 0,
            updated: 0,
            deleted: 0,
        });
    }
    let have: BTreeMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT entity_key, row_digest FROM _oo_docproj.doc_entity WHERE drive_id = $1",
    )
    .bind(drive_id)
    .fetch_all(&mut *agent)
    .await?
    .into_iter()
    .collect();
    let mut tx = agent.begin().await?;
    let (mut inserted, mut updated) = (0, 0);
    let mut keep: Vec<&str> = Vec::with_capacity(rows.len());
    for r in &rows {
        keep.push(&r.key);
        let rd = row_digest(r);
        match have.get(&r.key) {
            Some(d) if *d == rd => continue,
            Some(_) => updated += 1,
            None => inserted += 1,
        }
        for t in ["doc_entity_evidence", "doc_entity_source", "doc_entity"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM _oo_docproj.{t} WHERE drive_id = $1 AND entity_key = $2"
            )))
            .bind(drive_id)
            .bind(&r.key)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "INSERT INTO _oo_docproj.doc_entity \
               (drive_id, entity_key, type, canonical_name, norm_name, aliases, fact_count, row_digest) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8)",
        )
        .bind(drive_id)
        .bind(&r.key)
        .bind(&r.ty)
        .bind(&r.name)
        .bind(&r.norm_name)
        .bind(serde_json::json!(r.aliases).to_string())
        .bind(r.fact_count)
        .bind(&rd)
        .execute(&mut *tx)
        .await?;
        for path in &r.sources {
            sqlx::query(
                "INSERT INTO _oo_docproj.doc_entity_source (drive_id, entity_key, path) \
                 VALUES ($1, $2, $3)",
            )
            .bind(drive_id)
            .bind(&r.key)
            .bind(path)
            .execute(&mut *tx)
            .await?;
        }
        for (i, e) in r.evidence.iter().enumerate() {
            sqlx::query(
                "INSERT INTO _oo_docproj.doc_entity_evidence \
                   (drive_id, entity_key, ordinal, path, file_version, block_id, anchor) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)",
            )
            .bind(drive_id)
            .bind(&r.key)
            .bind(i32::try_from(i).unwrap_or(i32::MAX))
            .bind(&e.path)
            .bind(&e.file_version)
            .bind(&e.block_id)
            .bind(&e.anchor)
            .execute(&mut *tx)
            .await?;
        }
    }
    // Entities that are no longer published or visible, and the links that pointed at them (their decisions
    // go with them: a person's decision about an entity that does not exist anymore has no subject).
    let gone: Vec<String> = have
        .keys()
        .filter(|k| !keep.contains(&k.as_str()))
        .cloned()
        .collect();
    let deleted = gone.len();
    if !gone.is_empty() {
        for t in ["doc_entity_evidence", "doc_entity_source", "doc_entity"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM _oo_docproj.{t} WHERE drive_id = $1 AND entity_key = ANY($2::text[])"
            )))
            .bind(drive_id)
            .bind(&gone)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "DELETE FROM _oo_docproj.entity_link WHERE drive_id = $1 AND doc_entity_key = ANY($2::text[])",
        )
        .bind(drive_id)
        .bind(&gone)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO _oo_docproj.projection_state \
           (drive_id, ontology_version, source_digest, entities, trigger_key) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (drive_id) DO UPDATE SET ontology_version = EXCLUDED.ontology_version, \
           source_digest = EXCLUDED.source_digest, entities = EXCLUDED.entities, \
           trigger_key = EXCLUDED.trigger_key, projected_at = clock_timestamp()",
    )
    .bind(drive_id)
    .bind(version.n)
    .bind(&dg)
    .bind(i32::try_from(rows.len()).unwrap_or(i32::MAX))
    .bind(trigger_key)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ProjectionReport {
        ontology_version: version.n,
        entities: rows.len(),
        written: true,
        digest: dg,
        inserted,
        updated,
        deleted,
    })
}

/// One matching rule: `match_columns` of `db_table` (`schema.table`) against document entities of
/// `doc_types`; `key_column` identifies the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRule {
    pub db_table: String,
    pub key_column: String,
    pub match_columns: Vec<String>,
    pub doc_types: Vec<String>,
}

fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

impl LinkRule {
    fn quoted_table(&self) -> Result<String, ProjectionError> {
        let (schema, table) = self
            .db_table
            .split_once('.')
            .ok_or(ProjectionError::BadRule("table is schema.table"))?;
        if !ident_ok(schema) || !ident_ok(table) {
            return Err(ProjectionError::BadRule(
                "table name is not a plain identifier",
            ));
        }
        if schema.starts_with("_oo") || schema == "pg_catalog" || schema == "information_schema" {
            return Err(ProjectionError::BadRule("table is not a data table"));
        }
        Ok(format!("\"{schema}\".\"{table}\""))
    }
}

/// A document entity that may be linked: `(entity key, type, name, matched via, confidence)`.
type Candidate = (String, String, String, &'static str, i32);

/// A link found in the rows: `(entity key, table, row key, via, matched text, confidence, column)`.
type Found = (String, String, String, &'static str, String, i32, String);

/// What [`link`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkReport {
    pub rows_read: usize,
    /// Links created by this run.
    pub links: usize,
    /// `auto` links rewritten (their match changed) and removed (no row matches anymore).
    pub updated: usize,
    pub removed: usize,
}

/// Recompute the `auto` links of `drive_id` from the projected entities and the database rows.
pub async fn link(
    agent: &mut PgConnection,
    drive_id: &str,
    rules: &[LinkRule],
) -> Result<LinkReport, ProjectionError> {
    link_with(agent, drive_id, rules, 0).await
}

/// [`link`], stamping every link with the revision of the rule set that produced it. Incremental: a link that is
/// still found with the same match is not touched (its `created_at` stays), a new one is added, a changed one is
/// rewritten, one that no row matches anymore is removed; `confirmed` and `rejected` links are never touched.
pub async fn link_with(
    agent: &mut PgConnection,
    drive_id: &str,
    rules: &[LinkRule],
    rule_revision: i64,
) -> Result<LinkReport, ProjectionError> {
    sqlx::raw_sql(DDL).execute(&mut *agent).await?;
    sqlx::raw_sql(DDL_V2).execute(&mut *agent).await?;
    let mut by_norm: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    let ents = sqlx::query(
        "SELECT entity_key, type, canonical_name, norm_name, aliases::text AS aliases \
           FROM _oo_docproj.doc_entity WHERE drive_id = $1 ORDER BY entity_key",
    )
    .bind(drive_id)
    .fetch_all(&mut *agent)
    .await?;
    for r in &ents {
        let key: String = r.try_get("entity_key")?;
        let ty: String = r.try_get("type")?;
        let name: String = r.try_get("canonical_name")?;
        let n: String = r.try_get("norm_name")?;
        by_norm
            .entry(n)
            .or_default()
            .push((key.clone(), ty.clone(), name, "name", 9000));
        let aliases: Vec<String> =
            serde_json::from_str(&r.try_get::<String, _>("aliases")?).unwrap_or_default();
        for a in aliases {
            by_norm
                .entry(norm(&a))
                .or_default()
                .push((key.clone(), ty.clone(), a, "alias", 8000));
        }
    }
    let ontology_version: i64 = sqlx::query_scalar(
        "SELECT ontology_version FROM _oo_docproj.projection_state WHERE drive_id = $1",
    )
    .bind(drive_id)
    .fetch_optional(&mut *agent)
    .await?
    .unwrap_or(0);
    let mut found: Vec<Found> = Vec::new();
    let mut report = LinkReport::default();
    for rule in rules {
        let table = rule.quoted_table()?;
        if !ident_ok(&rule.key_column) || rule.match_columns.iter().any(|c| !ident_ok(c)) {
            return Err(ProjectionError::BadRule("column is not a plain identifier"));
        }
        for col in &rule.match_columns {
            let sql = format!(
                "SELECT \"{k}\"::text AS k, \"{c}\"::text AS v FROM {table} WHERE \"{c}\" IS NOT NULL",
                k = rule.key_column,
                c = col
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *agent)
                .await?;
            for row in rows {
                report.rows_read += 1;
                let (k, v): (String, String) = (row.try_get("k")?, row.try_get("v")?);
                let Some(cands) = by_norm.get(&norm(&v)) else {
                    continue;
                };
                for (key, ty, _name, via, bp) in cands {
                    if rule.doc_types.is_empty() || rule.doc_types.contains(ty) {
                        found.push((
                            key.clone(),
                            rule.db_table.clone(),
                            k.clone(),
                            via,
                            v.clone(),
                            *bp,
                            col.clone(),
                        ));
                    }
                }
            }
        }
    }
    // One link per (entity, row): the strongest match, then the first column.
    found.sort_by(|a, b| {
        (&a.0, &a.1, &a.2, std::cmp::Reverse(a.5), &a.6).cmp(&(
            &b.0,
            &b.1,
            &b.2,
            std::cmp::Reverse(b.5),
            &b.6,
        ))
    });
    found.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);

    type Existing = (String, String, i32, String, i64, i64);
    let have: BTreeMap<(String, String, String), Existing> = sqlx::query(
        "SELECT doc_entity_key, db_table, db_key, matched_via, matched_text, confidence_bp, matched_column, \
                rule_revision, ontology_version \
           FROM _oo_docproj.entity_link WHERE drive_id = $1 AND status = 'auto'",
    )
    .bind(drive_id)
    .fetch_all(&mut *agent)
    .await?
    .into_iter()
    .map(|r| {
        Ok((
            (r.try_get("doc_entity_key")?, r.try_get("db_table")?, r.try_get("db_key")?),
            (
                r.try_get("matched_via")?,
                r.try_get("matched_text")?,
                r.try_get("confidence_bp")?,
                r.try_get("matched_column")?,
                r.try_get("rule_revision")?,
                r.try_get("ontology_version")?,
            ),
        ))
    })
    .collect::<Result<_, sqlx::Error>>()?;

    let mut tx = agent.begin().await?;
    let mut seen: std::collections::BTreeSet<(String, String, String)> = Default::default();
    for (key, table, dbkey, via, text, bp, col) in &found {
        let id = (key.clone(), table.clone(), dbkey.clone());
        seen.insert(id.clone());
        let new: Existing = (
            (*via).to_owned(),
            text.clone(),
            *bp,
            col.clone(),
            rule_revision,
            ontology_version,
        );
        match have.get(&id) {
            Some(old) if *old == new => continue,
            Some(_) => {
                sqlx::query(
                    "UPDATE _oo_docproj.entity_link SET matched_via = $5, matched_text = $6, confidence_bp = $7, \
                            matched_column = $8, rule_revision = $9, ontology_version = $10 \
                      WHERE drive_id = $1 AND doc_entity_key = $2 AND db_table = $3 AND db_key = $4 \
                        AND status = 'auto'",
                )
                .bind(drive_id)
                .bind(key)
                .bind(table)
                .bind(dbkey)
                .bind(via)
                .bind(text)
                .bind(bp)
                .bind(col)
                .bind(rule_revision)
                .bind(ontology_version)
                .execute(&mut *tx)
                .await?;
                report.updated += 1;
            }
            None => {
                let done = sqlx::query(
                    "INSERT INTO _oo_docproj.entity_link \
                       (drive_id, doc_entity_key, db_table, db_key, matched_via, matched_text, confidence_bp, \
                        matched_column, rule_revision, ontology_version) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT DO NOTHING",
                )
                .bind(drive_id)
                .bind(key)
                .bind(table)
                .bind(dbkey)
                .bind(via)
                .bind(text)
                .bind(bp)
                .bind(col)
                .bind(rule_revision)
                .bind(ontology_version)
                .execute(&mut *tx)
                .await?;
                report.links += usize::try_from(done.rows_affected()).unwrap_or(0);
            }
        }
    }
    for id in have.keys().filter(|k| !seen.contains(*k)) {
        sqlx::query(
            "DELETE FROM _oo_docproj.entity_link WHERE drive_id = $1 AND doc_entity_key = $2 AND db_table = $3 \
                AND db_key = $4 AND status = 'auto'",
        )
        .bind(drive_id)
        .bind(&id.0)
        .bind(&id.1)
        .bind(&id.2)
        .execute(&mut *tx)
        .await?;
        report.removed += 1;
    }
    tx.commit().await?;
    Ok(report)
}

/// A person's decision on one link: `confirmed` or `rejected`; survives rebuilds.
pub async fn decide(
    agent: &mut PgConnection,
    drive_id: &str,
    doc_entity_key: &str,
    db_table: &str,
    db_key: &str,
    status: &str,
) -> Result<bool, ProjectionError> {
    if !matches!(status, "confirmed" | "rejected") {
        return Err(ProjectionError::BadRule("status is confirmed or rejected"));
    }
    let done = sqlx::query(
        "UPDATE _oo_docproj.entity_link SET status = $5 \
          WHERE drive_id = $1 AND doc_entity_key = $2 AND db_table = $3 AND db_key = $4",
    )
    .bind(drive_id)
    .bind(doc_entity_key)
    .bind(db_table)
    .bind(db_key)
    .bind(status)
    .execute(&mut *agent)
    .await?;
    Ok(done.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_only_accept_plain_identifiers_of_data_tables() {
        let ok = LinkRule {
            db_table: "public.customers".into(),
            key_column: "id".into(),
            match_columns: vec!["name".into()],
            doc_types: vec![],
        };
        assert_eq!(ok.quoted_table().unwrap(), "\"public\".\"customers\"");
        for bad in [
            "customers",
            "public.cust;omers",
            "_oo.apply_receipts",
            "pg_catalog.pg_user",
            "a.b.c",
            "public.\"x\"",
        ] {
            let r = LinkRule {
                db_table: bad.into(),
                ..ok.clone()
            };
            assert!(r.quoted_table().is_err(), "{bad}");
        }
    }

    #[test]
    fn digest_depends_on_content_and_version_only() {
        let e = |k: &str, s: &str| ProjectedEntity {
            key: k.into(),
            ty: "Organization".into(),
            name: "x".into(),
            norm_name: "x".into(),
            aliases: vec![],
            fact_count: 1,
            sources: vec![s.into()],
            evidence: vec![],
        };
        let a = digest(3, &[e("k1", "a.pdf")]);
        assert_eq!(a, digest(3, &[e("k1", "a.pdf")]));
        assert_ne!(a, digest(4, &[e("k1", "a.pdf")]));
        assert_ne!(a, digest(3, &[e("k1", "b.pdf")]));
    }

    #[test]
    fn the_digest_sees_the_anchor() {
        let mut e = ProjectedEntity {
            key: "k".into(),
            ty: "Organization".into(),
            name: "x".into(),
            norm_name: "x".into(),
            aliases: vec![],
            fact_count: 1,
            sources: vec!["a.pdf".into()],
            evidence: vec![ProjectedEvidence {
                path: "a.pdf".into(),
                file_version: "v1".into(),
                block_id: "b1".into(),
                anchor: r#"{"kind":"pdf","page":1}"#.into(),
            }],
        };
        let a = row_digest(&e);
        assert_eq!(a, row_digest(&e.clone()));
        e.evidence[0].anchor = r#"{"kind":"pdf","page":2}"#.into();
        assert_ne!(a, row_digest(&e), "a moved anchor is a changed entity");
    }

    #[test]
    fn the_provenance_migration_is_a_copy_of_ddl_v2_when_it_exists() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../oo-pipeline/migrations/0152_docproj_provenance_and_link_index.sql");
        if let Ok(text) = std::fs::read_to_string(p) {
            let body = DDL_V2.split_once("\n\n").map_or(DDL_V2, |(_, b)| b).trim();
            assert!(
                text.contains(body),
                "the Agent DB migration 0152 drifted from projection_v2.sql"
            );
        }
    }

    #[test]
    fn the_migration_is_a_copy_of_the_ddl_when_it_exists() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../oo-pipeline/migrations/0147_docs_projection.sql");
        if let Ok(text) = std::fs::read_to_string(p) {
            assert!(
                text.contains(DDL.trim()),
                "the Agent DB migration drifted from projection.sql"
            );
        }
    }
}
