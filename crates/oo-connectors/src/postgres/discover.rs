//! Bounded catalog discovery: the tables a role can read and the databases
//! it can connect to. Catalog relations only (see `access::ESSENTIAL` and
//! `access::DISCOVERY`); no source row is read and no statistic is refreshed.
//!
//! Sessions pass the same admission as every other operation (identity
//! alignment, clean startup options, version range) inside a read-only
//! transaction. Like `probe`, discovery is not gated on `pg_read_all_stats`:
//! it is one bounded catalog statement, not a table read.

use super::*;

/// One row per readable ordinary or partitioned table outside the system
/// schemas, with statistics and key facts. `$1` is the row limit.
pub(super) const TABLES: &str = "\
SELECT n.nspname::text, c.relname::text, c.reltuples::float8, c.relispartition, \
 EXISTS(SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid=c.oid AND i.indisprimary), \
 EXISTS(SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid=c.oid AND i.indisunique \
   AND NOT i.indisprimary AND i.indisvalid AND i.indpred IS NULL AND i.indexprs IS NULL), \
 COALESCE(r.schemas, ARRAY[]::text[]), COALESCE(r.names, ARRAY[]::text[]), \
 ARRAY(SELECT a.attname::text FROM pg_catalog.pg_attribute a \
   WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum), \
 ARRAY(SELECT t.typname::text FROM pg_catalog.pg_attribute a \
   JOIN pg_catalog.pg_type t ON t.oid=a.atttypid \
   WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum) \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
LEFT JOIN LATERAL ( \
 SELECT array_agg(t.s ORDER BY t.s, t.r) AS schemas, array_agg(t.r ORDER BY t.s, t.r) AS names \
 FROM (SELECT DISTINCT rn.nspname::text AS s, rc.relname::text AS r \
   FROM pg_catalog.pg_constraint f \
   JOIN pg_catalog.pg_class rc ON rc.oid=f.confrelid \
   JOIN pg_catalog.pg_namespace rn ON rn.oid=rc.relnamespace \
   WHERE f.conrelid=c.oid AND f.contype='f' ORDER BY 1, 2 LIMIT 64) t \
) r ON true \
WHERE c.relkind IN ('r','p') \
 AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_' \
 AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') \
 AND pg_catalog.has_table_privilege(c.oid, 'SELECT') \
ORDER BY 1, 2 LIMIT $1";

/// Databases the signed-in role may connect to. `$1` is the row limit.
pub(super) const DATABASES: &str = "\
SELECT d.datname::text FROM pg_catalog.pg_database d \
WHERE d.datallowconn AND NOT d.datistemplate \
 AND pg_catalog.has_database_privilege(d.oid, 'CONNECT') \
ORDER BY 1 LIMIT $1";

/// Upper bound on listed databases.
pub(super) const DATABASE_LIMIT: u32 = 1_024;

/// Rows one statement may return under `budget` and the session's own
/// row bound, leaving room for the truncation sentinel.
pub(super) fn row_limit(
    budget: &ReadBudget,
    session_rows: u32,
    cap: u64,
) -> Result<u32, SourceError> {
    let limit = budget
        .max_chunk_rows
        .min(cap)
        .min(u64::from(session_rows.saturating_sub(1)))
        .min(u64::from(session::MAX_ROWS_PER_SEC - 1));
    if limit == 0 {
        return Err(SourceError::budget("catalog_table_limit"));
    }
    Ok(limit as u32)
}

fn table(row: &sqlx::postgres::PgRow) -> Result<DiscoveredTable, SourceError> {
    let estimate: f64 = row
        .try_get(2)
        .map_err(|_| SourceError::catalog("reltuples"))?;
    let schemas: Vec<String> = row
        .try_get(6)
        .map_err(|_| SourceError::catalog("foreign_key_targets"))?;
    let names: Vec<String> = row
        .try_get(7)
        .map_err(|_| SourceError::catalog("foreign_key_targets"))?;
    if schemas.len() != names.len() {
        return Err(SourceError::catalog("foreign_key_targets"));
    }
    let column_names: Vec<String> = row
        .try_get(8)
        .map_err(|_| SourceError::catalog("column_names"))?;
    let column_types: Vec<String> = row
        .try_get(9)
        .map_err(|_| SourceError::catalog("column_types"))?;
    if column_names.len() != column_types.len() {
        return Err(SourceError::catalog("column_types"));
    }
    Ok(DiscoveredTable {
        table: TableRef {
            schema: cell_string(row, 0)?,
            name: cell_string(row, 1)?,
        },
        estimated_rows: estimate_rows(estimate),
        partition_child: cell_bool(row, 3)?,
        has_primary_key: cell_bool(row, 4)?,
        has_unique_key: cell_bool(row, 5)?,
        columns: column_names.into_iter().zip(column_types).collect(),
        references: schemas
            .into_iter()
            .zip(names)
            .map(|(schema, name)| TableRef { schema, name })
            .collect(),
    })
}

/// `reltuples` is -1 for a never-analyzed table (PostgreSQL 14+).
pub(super) fn estimate_rows(reltuples: f64) -> Option<u64> {
    (reltuples.is_finite() && reltuples >= 0.0).then(|| reltuples.round() as u64)
}

impl PostgresEngine {
    /// Readable user tables with row estimates, key flags and outgoing
    /// foreign-key targets. More than the bound is an error, never a silent cut.
    pub async fn discover_catalog(
        &mut self,
        budget: &ReadBudget,
    ) -> Result<Vec<DiscoveredTable>, SourceError> {
        validate_budget(budget)?;
        let mut s = self.connect().await?;
        guards(&mut s, budget).await?;
        self.transaction(&mut s).await?;
        let started = Instant::now();
        let result = async {
            let limit = row_limit(budget, s.budget.max_rows(), DISCOVERY_TABLE_LIMIT)?;
            s.arm_rows(limit + 1);
            let rows =
                session::fetch_all(&mut s, sqlx::query(TABLES).bind(i64::from(limit) + 1)).await?;
            if rows.len() > limit as usize {
                return Err(SourceError::budget("catalog_table_limit"));
            }
            rows.iter().map(table).collect()
        }
        .await;
        let result = s.finish(result).await;
        if result.is_ok() {
            self.session = Some(s);
        }
        duty(started, budget.duty_cycle_percent).await;
        result
    }

    /// Databases the signed-in role can connect to, sorted. The list is
    /// cluster-wide; the session stays on its configured database.
    pub async fn list_databases(
        &mut self,
        budget: &ReadBudget,
    ) -> Result<Vec<String>, SourceError> {
        validate_budget(budget)?;
        let mut s = self.connect().await?;
        guards(&mut s, budget).await?;
        self.transaction(&mut s).await?;
        let result = async {
            let limit = row_limit(budget, s.budget.max_rows(), u64::from(DATABASE_LIMIT))?;
            s.arm_rows(limit + 1);
            let rows =
                session::fetch_all(&mut s, sqlx::query(DATABASES).bind(i64::from(limit) + 1))
                    .await?;
            if rows.len() > limit as usize {
                return Err(SourceError::budget("catalog_database_limit"));
            }
            rows.iter().map(|r| cell_string(r, 0)).collect()
        }
        .await;
        let result = s.finish(result).await;
        if result.is_ok() {
            self.session = Some(s);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relations(sql: &str) -> Vec<&str> {
        sql.split("pg_catalog.")
            .skip(1)
            .map(|rest| {
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .filter(|name| !name.starts_with("has_"))
            .collect()
    }

    #[test]
    fn discovery_reads_only_allowlisted_catalog_relations() {
        for sql in [TABLES, DATABASES] {
            let found = relations(sql);
            assert!(!found.is_empty());
            for name in found {
                assert!(
                    crate::access::ESSENTIAL.contains(&name)
                        || crate::access::DISCOVERY.contains(&name),
                    "{name} is not an allowlisted catalog relation"
                );
            }
        }
        // Every FROM / JOIN target is schema-qualified.
        for sql in [TABLES, DATABASES] {
            for word in ["FROM ", "JOIN "] {
                for (i, _) in sql.match_indices(word) {
                    let rest = &sql[i + word.len()..];
                    assert!(
                        rest.starts_with("pg_catalog.")
                            || rest.starts_with("(")
                            || rest.starts_with("LATERAL"),
                        "unqualified relation after {word:?}: {}",
                        &rest[..rest.len().min(40)]
                    );
                }
            }
        }
    }

    #[test]
    fn discovery_sql_is_read_only_bounded_and_filters_system_schemas() {
        for sql in [TABLES, DATABASES] {
            assert!(sql.trim_start().starts_with("SELECT "));
            assert!(sql.ends_with("LIMIT $1"));
            let upper = sql.to_ascii_uppercase();
            for verb in [
                "INSERT", "UPDATE", "DELETE", "ANALYZE", "VACUUM", "LOCK", "SET ", "CREATE",
            ] {
                assert!(!upper.contains(verb), "{verb} in discovery SQL");
            }
        }
        assert!(TABLES.contains("c.relkind IN ('r','p')"));
        assert!(TABLES.contains("n.nspname !~ '^pg_'"));
        assert!(TABLES.contains("n.nspname <> 'information_schema'"));
        assert!(TABLES.contains("has_table_privilege(c.oid, 'SELECT')"));
        assert!(TABLES.contains("f.contype='f'"));
        assert!(DATABASES.contains("has_database_privilege(d.oid, 'CONNECT')"));
        assert!(DATABASES.contains("NOT d.datistemplate"));
    }

    #[test]
    fn row_limits_respect_budget_session_and_cap() {
        let mut b = ReadBudget::default();
        assert_eq!(
            row_limit(&b, 100_000, DISCOVERY_TABLE_LIMIT).unwrap(),
            10_000
        );
        assert_eq!(row_limit(&b, 1_000, DISCOVERY_TABLE_LIMIT).unwrap(), 999);
        assert_eq!(
            row_limit(&b, 100_000, u64::from(DATABASE_LIMIT)).unwrap(),
            1_024
        );
        b.max_chunk_rows = 50;
        assert_eq!(row_limit(&b, 100_000, DISCOVERY_TABLE_LIMIT).unwrap(), 50);
        assert!(row_limit(&b, 1, DISCOVERY_TABLE_LIMIT).is_err());
    }

    #[test]
    fn never_analyzed_tables_have_no_estimate() {
        assert_eq!(estimate_rows(-1.0), None);
        assert_eq!(estimate_rows(f64::NAN), None);
        assert_eq!(estimate_rows(0.0), Some(0));
        assert_eq!(estimate_rows(1234.6), Some(1235));
    }
}
