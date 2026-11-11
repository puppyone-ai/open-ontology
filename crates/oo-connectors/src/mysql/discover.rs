//! Bounded catalog discovery over `information_schema`: base tables of the
//! configured database with statistics, key flags and foreign-key targets,
//! and the databases the account can see. Dictionary views only; no table
//! row is read and no statistic is refreshed.

use super::*;
use crate::engine::{DISCOVERY_TABLE_LIMIT, DiscoveredTable};

/// Base tables of one schema (`?`): name, TABLE_ROWS, has PK, has unique key.
pub(super) fn tables_sql(limit: u64) -> String {
    format!(
        "SELECT t.TABLE_NAME, CAST(t.TABLE_ROWS AS CHAR), \
         CASE WHEN EXISTS (SELECT 1 FROM information_schema.TABLE_CONSTRAINTS c \
           WHERE c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME AND c.CONSTRAINT_TYPE = 'PRIMARY KEY') THEN 'Y' ELSE 'N' END, \
         CASE WHEN EXISTS (SELECT 1 FROM information_schema.TABLE_CONSTRAINTS c \
           WHERE c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME AND c.CONSTRAINT_TYPE = 'UNIQUE') THEN 'Y' ELSE 'N' END \
         FROM information_schema.TABLES t \
         WHERE t.TABLE_SCHEMA = ? AND t.TABLE_TYPE = 'BASE TABLE' \
         ORDER BY t.TABLE_NAME LIMIT {}",
        limit + 1
    )
}

/// Outgoing foreign keys of one schema (`?`): table, target schema, target table.
pub(super) fn references_sql(limit: u64) -> String {
    format!(
        "SELECT DISTINCT k.TABLE_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME \
         FROM information_schema.KEY_COLUMN_USAGE k \
         WHERE k.TABLE_SCHEMA = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
         ORDER BY 1, 2, 3 LIMIT {}",
        limit + 1
    )
}

/// Databases visible to the account, without the server's own.
pub(super) fn databases_sql(limit: u64) -> String {
    format!(
        "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA \
         WHERE SCHEMA_NAME NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') \
         ORDER BY SCHEMA_NAME LIMIT {}",
        limit + 1
    )
}

pub(super) const DATABASE_LIMIT: u64 = 1_024;

/// Rows of `(table, [target schema, target table])` grouped per table.
pub(super) fn assemble(
    schema: &str,
    tables: Vec<[Option<String>; 4]>,
    references: Vec<[Option<String>; 3]>,
) -> Result<Vec<DiscoveredTable>> {
    let mut targets: BTreeMap<String, Vec<TableRef>> = BTreeMap::new();
    for [table, ref_schema, ref_table] in references {
        let (Some(table), Some(schema), Some(name)) = (table, ref_schema, ref_table) else {
            return Err(SourceError::catalog("foreign_key_targets"));
        };
        targets
            .entry(table)
            .or_default()
            .push(TableRef { schema, name });
    }
    tables
        .into_iter()
        .map(|[name, rows, pk, uk]| {
            let name = name
                .filter(|n| mysql_ident(n))
                .ok_or_else(|| SourceError::identifier("table_ident"))?;
            let mut references = targets.remove(&name).unwrap_or_default();
            references.sort();
            references.dedup();
            Ok(DiscoveredTable {
                table: TableRef {
                    schema: schema.to_owned(),
                    name,
                },
                estimated_rows: rows.and_then(|r| r.parse().ok()),
                has_primary_key: pk.as_deref() == Some("Y"),
                has_unique_key: uk.as_deref() == Some("Y"),
                partition_child: false,
                references,
            })
        })
        .collect()
}

impl MysqlEngine {
    /// Base tables of the configured database. More than the bound is an
    /// error, never a silent cut; a name outside the identifier rules the
    /// reader accepts fails the call, as `discover_tables` always did.
    pub async fn discover_catalog(&mut self, budget: &ReadBudget) -> Result<Vec<DiscoveredTable>> {
        self.apply_budget(budget).await?;
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        let result = self.discover_inner(budget).await;
        self.finish_op(started);
        result
    }

    async fn discover_inner(&mut self, budget: &ReadBudget) -> Result<Vec<DiscoveredTable>> {
        let schema = self.config.mysql_database.clone();
        let limit = budget.max_chunk_rows.min(DISCOVERY_TABLE_LIMIT);
        let rows = self
            .exec(&tables_sql(limit), vec![Value::from(schema.as_str())])
            .await?;
        if rows.len() as u64 > limit {
            return Err(SourceError::budget("catalog_table_limit"));
        }
        let tables = rows
            .iter()
            .map(|r| {
                [
                    row_text(r, 0),
                    row_text(r, 1),
                    row_text(r, 2),
                    row_text(r, 3),
                ]
            })
            .collect();
        let fk_limit = (limit * 4).min(100_000);
        let rows = self
            .exec(
                &references_sql(fk_limit),
                vec![Value::from(schema.as_str())],
            )
            .await?;
        if rows.len() as u64 > fk_limit {
            return Err(SourceError::budget("catalog_reference_limit"));
        }
        let references = rows
            .iter()
            .map(|r| [row_text(r, 0), row_text(r, 1), row_text(r, 2)])
            .collect();
        assemble(&schema, tables, references)
    }

    /// Databases the account can see (`SCHEMATA` lists only those it holds
    /// a privilege on, unless it has `SHOW DATABASES`), sorted.
    pub async fn list_databases(&mut self, budget: &ReadBudget) -> Result<Vec<String>> {
        self.apply_budget(budget).await?;
        self.ensure_basics().await?;
        let started = Instant::now();
        let rows = self.exec(&databases_sql(DATABASE_LIMIT), vec![]).await;
        self.finish_op(started);
        let rows = rows?;
        if rows.len() as u64 > DATABASE_LIMIT {
            return Err(SourceError::budget("catalog_database_limit"));
        }
        rows.iter()
            .map(|r| row_text(r, 0).ok_or_else(|| SourceError::catalog("database_name")))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_sql_is_guarded_bounded_and_dictionary_only() {
        for sql in [
            tables_sql(10),
            references_sql(40),
            databases_sql(DATABASE_LIMIT),
        ] {
            assert!(keyset::guard(&sql).is_ok(), "{sql}");
            assert!(sql.starts_with("SELECT "));
            let upper = sql.to_ascii_uppercase();
            for verb in ["INSERT", "UPDATE", "DELETE", "ANALYZE", "LOCK", "FLUSH"] {
                assert!(!upper.contains(verb), "{verb} in {sql}");
            }
            for (i, _) in upper.match_indices("FROM ") {
                assert!(upper[i + 5..].starts_with("INFORMATION_SCHEMA."), "{sql}");
            }
        }
        assert!(tables_sql(10).ends_with("LIMIT 11"));
        assert!(tables_sql(1).contains("TABLE_TYPE = 'BASE TABLE'"));
        assert!(databases_sql(1).contains("'performance_schema'"));
    }

    fn s(v: &str) -> Option<String> {
        (!v.is_empty()).then(|| v.to_owned())
    }

    #[test]
    fn assemble_groups_targets_and_rejects_unreadable_names() {
        let out = assemble(
            "shop",
            vec![
                [s("customers"), s("4800"), s("Y"), s("N")],
                [s("orders"), None, s("Y"), s("Y")],
            ],
            vec![
                [s("orders"), s("shop"), s("customers")],
                [s("orders"), s("ref"), s("regions")],
            ],
        )
        .unwrap();
        assert_eq!(out[0].estimated_rows, Some(4800));
        assert_eq!(out[1].estimated_rows, None);
        assert!(out[1].has_unique_key);
        assert_eq!(out[1].references[0].schema, "ref");
        assert!(assemble("shop", vec![[s("bad-name"), None, None, None]], vec![]).is_err());
    }
}
