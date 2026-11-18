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
        let mut facts = 0;
        for f in graph.facts_of(&e.key) {
            facts += 1;
            sources.extend(graph.evidence(f).iter().map(|v| v.path.clone()));
        }
        for a in graph.aliases(&e.key) {
            sources.extend(a.evidence.iter().map(|v| v.path.clone()));
        }
        sources.sort();
        sources.dedup();
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
            },
        );
    }
    out.into_values().collect()
}

/// Digest of what would be written (ordered, so equal graphs give equal digests).
pub fn digest(version: i64, rows: &[ProjectedEntity]) -> String {
    let mut h = Sha256::new();
    h.update(format!("oo.docproj/v1\n{version}\n").as_bytes());
    for r in rows {
        h.update(
            serde_json::json!([r.key, r.ty, r.name, r.aliases, r.fact_count, r.sources])
                .to_string()
                .as_bytes(),
        );
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
}

/// Project the published entities of `drive_id` (latest ontology version) into the Agent DB.
/// `docs` is a docs-db connection (read), `agent` the Agent DB connection (the only writes).
pub async fn project(
    docs: &mut PgConnection,
    cas: &crate::cas::Cas,
    agent: &mut PgConnection,
    drive_id: &str,
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
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT source_digest FROM _oo_docproj.projection_state WHERE drive_id = $1",
    )
    .bind(drive_id)
    .fetch_optional(&mut *agent)
    .await?;
    if stored.as_deref() == Some(dg.as_str()) {
        return Ok(ProjectionReport {
            ontology_version: version.n,
            entities: rows.len(),
            written: false,
            digest: dg,
        });
    }
    let mut tx = agent.begin().await?;
    for t in ["doc_entity_source", "doc_entity"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM _oo_docproj.{t} WHERE drive_id = $1"
        )))
        .bind(drive_id)
        .execute(&mut *tx)
        .await?;
    }
    for r in &rows {
        sqlx::query(
            "INSERT INTO _oo_docproj.doc_entity \
               (drive_id, entity_key, type, canonical_name, norm_name, aliases, fact_count) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7)",
        )
        .bind(drive_id)
        .bind(&r.key)
        .bind(&r.ty)
        .bind(&r.name)
        .bind(&r.norm_name)
        .bind(serde_json::json!(r.aliases).to_string())
        .bind(r.fact_count)
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
    }
    sqlx::query(
        "INSERT INTO _oo_docproj.projection_state (drive_id, ontology_version, source_digest, entities) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (drive_id) DO UPDATE SET ontology_version = EXCLUDED.ontology_version, \
           source_digest = EXCLUDED.source_digest, entities = EXCLUDED.entities, \
           projected_at = clock_timestamp()",
    )
    .bind(drive_id)
    .bind(version.n)
    .bind(&dg)
    .bind(i32::try_from(rows.len()).unwrap_or(i32::MAX))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ProjectionReport {
        ontology_version: version.n,
        entities: rows.len(),
        written: true,
        digest: dg,
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

/// What [`link`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkReport {
    pub rows_read: usize,
    pub links: usize,
}

/// Recompute the `auto` links of `drive_id` from the projected entities and the database rows.
pub async fn link(
    agent: &mut PgConnection,
    drive_id: &str,
    rules: &[LinkRule],
) -> Result<LinkReport, ProjectionError> {
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
    let mut found: Vec<(String, String, String, &'static str, String, i32)> = Vec::new();
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
                        ));
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
    let mut tx = agent.begin().await?;
    sqlx::query("DELETE FROM _oo_docproj.entity_link WHERE drive_id = $1 AND status = 'auto'")
        .bind(drive_id)
        .execute(&mut *tx)
        .await?;
    for (key, table, dbkey, via, text, bp) in &found {
        let done = sqlx::query(
            "INSERT INTO _oo_docproj.entity_link \
               (drive_id, doc_entity_key, db_table, db_key, matched_via, matched_text, confidence_bp) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
        )
        .bind(drive_id)
        .bind(key)
        .bind(table)
        .bind(dbkey)
        .bind(via)
        .bind(text)
        .bind(bp)
        .execute(&mut *tx)
        .await?;
        report.links += usize::try_from(done.rows_affected()).unwrap_or(0);
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
        };
        let a = digest(3, &[e("k1", "a.pdf")]);
        assert_eq!(a, digest(3, &[e("k1", "a.pdf")]));
        assert_ne!(a, digest(4, &[e("k1", "a.pdf")]));
        assert_ne!(a, digest(3, &[e("k1", "b.pdf")]));
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
