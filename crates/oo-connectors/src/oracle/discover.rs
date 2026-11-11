//! Bounded catalog discovery over `ALL_*` dictionary views: tables the
//! session user can see outside Oracle-maintained schemas, with statistics,
//! key flags and foreign-key targets, plus the PDB services it can see.
//! Dictionary views only; no table row is read and no statistic gathered.

use super::*;
use crate::engine::{DISCOVERY_TABLE_LIMIT, DiscoveredTable};

/// Schemas Oracle itself maintains, for 11g where `ALL_USERS` has no
/// `ORACLE_MAINTAINED` column.
const SYSTEM_SCHEMAS_11G: &[&str] = &[
    "ANONYMOUS",
    "APEX_030200",
    "APEX_040000",
    "APEX_040200",
    "APPQOSSYS",
    "AUDSYS",
    "CTXSYS",
    "DBSNMP",
    "DIP",
    "DVSYS",
    "EXFSYS",
    "FLOWS_FILES",
    "GSMADMIN_INTERNAL",
    "LBACSYS",
    "MDDATA",
    "MDSYS",
    "MGMT_VIEW",
    "OJVMSYS",
    "OLAPSYS",
    "ORACLE_OCM",
    "ORDDATA",
    "ORDPLUGINS",
    "ORDSYS",
    "OUTLN",
    "OWBSYS",
    "SI_INFORMTN_SCHEMA",
    "SPATIAL_CSW_ADMIN_USR",
    "SPATIAL_WFS_ADMIN_USR",
    "SYS",
    "SYSMAN",
    "SYSTEM",
    "WMSYS",
    "XDB",
    "XS$NULL",
];

/// Predicate excluding Oracle-maintained owners of `column`.
pub(super) fn user_schema(column: &str, maintained_flag: bool) -> String {
    if maintained_flag {
        format!(
            "NOT EXISTS (SELECT 1 FROM all_users u WHERE u.username = {column} AND u.oracle_maintained = 'Y')"
        )
    } else {
        let list = SYSTEM_SCHEMAS_11G
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{column} NOT IN ({list})")
    }
}

fn bounded(sql: &str, limit: usize) -> String {
    format!("SELECT * FROM ({sql}) WHERE ROWNUM <= {}", limit + 1)
}

/// Tables: owner, name, num_rows, has PK, has other unique key.
pub(super) fn tables_sql(maintained_flag: bool, limit: usize) -> String {
    bounded(
        &format!(
            "SELECT t.owner AS o, t.table_name AS n, TO_CHAR(t.num_rows) AS r, \
             CASE WHEN EXISTS (SELECT 1 FROM all_constraints c WHERE c.owner = t.owner AND c.table_name = t.table_name AND c.constraint_type = 'P') THEN 'Y' ELSE 'N' END AS pk, \
             CASE WHEN EXISTS (SELECT 1 FROM all_constraints c WHERE c.owner = t.owner AND c.table_name = t.table_name AND c.constraint_type = 'U' AND c.status = 'ENABLED') \
               OR EXISTS (SELECT 1 FROM all_indexes i WHERE i.table_owner = t.owner AND i.table_name = t.table_name AND i.uniqueness = 'UNIQUE' AND i.index_type = 'NORMAL' AND i.status IN ('VALID','N/A') \
                 AND NOT EXISTS (SELECT 1 FROM all_constraints p WHERE p.owner = t.owner AND p.table_name = t.table_name AND p.constraint_type = 'P' AND p.index_name = i.index_name)) \
             THEN 'Y' ELSE 'N' END AS uk \
             FROM all_tables t \
             WHERE t.temporary = 'N' AND t.nested = 'NO' AND t.secondary = 'N' AND t.dropped = 'NO' \
             AND (t.iot_type IS NULL OR t.iot_type = 'IOT') \
             AND NOT EXISTS (SELECT 1 FROM all_mviews m WHERE m.owner = t.owner AND m.mview_name = t.table_name) \
             AND {} \
             ORDER BY t.owner, t.table_name",
            user_schema("t.owner", maintained_flag)
        ),
        limit,
    )
}

/// Foreign keys: child owner, child table, parent owner, parent table.
pub(super) fn references_sql(maintained_flag: bool, limit: usize) -> String {
    bounded(
        &format!(
            "SELECT DISTINCT c.owner AS co, c.table_name AS ct, p.owner AS po, p.table_name AS pt \
             FROM all_constraints c JOIN all_constraints p ON p.owner = c.r_owner AND p.constraint_name = c.r_constraint_name \
             WHERE c.constraint_type = 'R' AND {} \
             ORDER BY 1, 2, 3, 4",
            user_schema("c.owner", maintained_flag)
        ),
        limit,
    )
}

/// Services of open PDBs (or of a non-CDB) visible in `V$SERVICES`.
pub(super) fn services_sql(multitenant: bool, limit: usize) -> String {
    let container = if multitenant {
        " AND con_id NOT IN (1, 2)"
    } else {
        ""
    };
    bounded(
        &format!(
            "SELECT DISTINCT name FROM v$services WHERE name NOT LIKE 'SYS$%' AND name NOT LIKE '%XDB'{container} ORDER BY name"
        ),
        limit,
    )
}

const SERVICE_LIMIT: usize = 256;

/// The driver's row sentinel, named for what overflowed.
fn row_limit(e: SourceError, code: &'static str) -> SourceError {
    if e.fact() == Some("query_row_limit") {
        SourceError::budget(code)
    } else {
        e
    }
}

fn text(row: &[Option<String>], i: usize) -> Result<String, SourceError> {
    row.get(i)
        .cloned()
        .flatten()
        .ok_or_else(|| SourceError::catalog("discovery_shape"))
}

pub(super) fn assemble(
    tables: Rows,
    references: Rows,
) -> Result<Vec<DiscoveredTable>, SourceError> {
    let mut targets: BTreeMap<TableRef, Vec<TableRef>> = BTreeMap::new();
    for r in &references {
        targets
            .entry(TableRef {
                schema: text(r, 0)?,
                name: text(r, 1)?,
            })
            .or_default()
            .push(TableRef {
                schema: text(r, 2)?,
                name: text(r, 3)?,
            });
    }
    tables
        .iter()
        .map(|r| {
            let table = TableRef {
                schema: text(r, 0)?,
                name: text(r, 1)?,
            };
            let mut references = targets.remove(&table).unwrap_or_default();
            references.sort();
            references.dedup();
            Ok(DiscoveredTable {
                estimated_rows: r.get(2).cloned().flatten().and_then(|v| v.parse().ok()),
                has_primary_key: r.get(3).cloned().flatten().as_deref() == Some("Y"),
                has_unique_key: r.get(4).cloned().flatten().as_deref() == Some("Y"),
                partition_child: false,
                references,
                table,
            })
        })
        .collect()
}

impl OracleEngine {
    /// Tables the session user can see in non-system schemas. More than the
    /// bound is an error, never a silent cut.
    pub fn discover_catalog(
        &mut self,
        budget: &ReadBudget,
    ) -> Result<Vec<DiscoveredTable>, SourceError> {
        validate_budget(budget)?;
        self.set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        let d = self.ensure_version()?;
        let flag = d.identity_columns; // ORACLE_MAINTAINED is 12.1+ too.
        let limit = budget.max_chunk_rows.min(DISCOVERY_TABLE_LIMIT) as usize;
        let tables = self
            .query(&tables_sql(flag, limit), &[], limit)
            .map_err(|e| row_limit(e, "catalog_table_limit"))?;
        let fk_limit = (limit * 4).min(100_000);
        let references = self
            .query(&references_sql(flag, fk_limit), &[], fk_limit)
            .map_err(|e| row_limit(e, "catalog_reference_limit"))?;
        assemble(tables, references)
    }

    /// The current service plus PDB services visible in `V$SERVICES`
    /// (needs `SELECT_CATALOG_ROLE` or similar; otherwise only the current one).
    pub fn list_services(&mut self, budget: &ReadBudget) -> Result<Vec<String>, SourceError> {
        validate_budget(budget)?;
        self.set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        let d = self.ensure_version()?;
        let mut out: Vec<String> = self
            .scalar(
                "SELECT SYS_CONTEXT('USERENV','SERVICE_NAME') FROM dual",
                &[],
            )?
            .into_iter()
            .collect();
        if self.readable("v$services")? {
            let multitenant = d.multitenant != Multitenant::NonCdbOnly;
            let rows = self.query(
                &services_sql(multitenant, SERVICE_LIMIT),
                &[],
                SERVICE_LIMIT,
            )?;
            for name in rows
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
            {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
                    out.push(name);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_sql_is_bounded_dictionary_only() {
        for sql in [
            tables_sql(true, 100),
            tables_sql(false, 100),
            references_sql(true, 400),
            services_sql(true, 256),
        ] {
            assert!(sql.starts_with("SELECT * FROM (SELECT "), "{sql}");
            assert!(sql.contains("ROWNUM <= "));
            let upper = sql.to_ascii_uppercase();
            for verb in [
                "INSERT", "UPDATE", "DELETE", "MERGE", "ANALYZE", "LOCK", "DBMS_",
            ] {
                assert!(!upper.contains(verb), "{verb} in {sql}");
            }
            for word in upper.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
            {
                if let Some(view) = word.strip_prefix("ALL_") {
                    assert!(
                        ["TABLES", "CONSTRAINTS", "INDEXES", "MVIEWS", "USERS"].contains(&view),
                        "unexpected view ALL_{view}"
                    );
                }
            }
        }
        assert!(tables_sql(true, 100).ends_with("ROWNUM <= 101"));
        // The ROWNUM wrapper selects `*`: every inner column needs a unique name.
        assert!(tables_sql(true, 1).contains("AS o,") && tables_sql(true, 1).contains("AS uk"));
        assert!(
            references_sql(true, 1).contains("c.owner AS co")
                && references_sql(true, 1).contains("p.owner AS po")
        );
        assert!(tables_sql(true, 1).contains("u.oracle_maintained = 'Y'"));
        let old = tables_sql(false, 1);
        assert!(!old.contains("oracle_maintained") && old.contains("'SYSTEM'"));
        assert!(references_sql(true, 1).contains("c.constraint_type = 'R'"));
        assert!(services_sql(true, 1).contains("con_id NOT IN (1, 2)"));
        assert!(!services_sql(false, 1).contains("con_id"));
    }

    fn row(cells: &[&str]) -> Vec<Option<String>> {
        cells
            .iter()
            .map(|c| (!c.is_empty()).then(|| (*c).to_owned()))
            .collect()
    }

    #[test]
    fn assemble_joins_targets_and_keeps_missing_statistics_unknown() {
        let tables = vec![
            row(&["APP", "CUSTOMERS", "4800", "Y", "N"]),
            row(&["APP", "ORDERS", "", "N", "Y"]),
        ];
        let refs = vec![
            row(&["APP", "ORDERS", "APP", "CUSTOMERS"]),
            row(&["APP", "ORDERS", "APP", "CUSTOMERS"]),
            row(&["APP", "ORDERS", "REF", "REGIONS"]),
        ];
        let out = assemble(tables, refs).unwrap();
        assert_eq!(out[0].estimated_rows, Some(4800));
        assert!(out[0].has_primary_key && !out[0].has_unique_key);
        assert!(out[0].references.is_empty());
        assert_eq!(out[1].estimated_rows, None);
        assert_eq!(out[1].references.len(), 2);
        assert_eq!(out[1].references[1].schema, "REF");
        assert!(assemble(vec![row(&["APP"])], vec![]).is_err());
    }
}
