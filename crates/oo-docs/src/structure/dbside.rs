//! The database side of the merged ontology (D93): the structure of the tables of the Agent DB, read from its catalog,
//! and the names its link rules match. Read only; nothing is written to the Agent DB.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgConnection, Row};

use super::yaml::{DbColumn, DbForeignKey, DbLinkRule, DbSide, DbTable};
use crate::publish::projector::StoredRule;
use crate::resolve::normalize::norm;

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn split_table(t: &str) -> Option<(&str, &str)> {
    let (a, b) = t.split_once('.')?;
    (is_ident(a) && is_ident(b)).then_some((a, b))
}

const USER_TABLES: &str = "n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE '\\_oo%' AND n.nspname NOT LIKE 'pg\\_%'";

/// Read the tables, columns, primary keys and foreign keys of the user schemas, and the names of the link rules.
pub async fn load(db: &mut PgConnection, rules: &[StoredRule]) -> Result<DbSide, sqlx::Error> {
    let mut tables: BTreeMap<String, DbTable> = BTreeMap::new();
    let cols = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT n.nspname AS s, c.relname AS t, a.attname AS col, format_type(a.atttypid, a.atttypmod) AS typ, a.attnotnull AS notnull \
           FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
          WHERE c.relkind IN ('r', 'p') AND a.attnum > 0 AND NOT a.attisdropped AND {USER_TABLES} ORDER BY n.nspname, c.relname, a.attnum"
    )))
    .fetch_all(&mut *db)
    .await?;
    for r in cols {
        let name = format!("{}.{}", r.get::<String, _>("s"), r.get::<String, _>("t"));
        let t = tables.entry(name.clone()).or_insert_with(|| DbTable {
            name,
            rows: None,
            columns: Vec::new(),
            foreign_keys: Vec::new(),
        });
        t.columns.push(DbColumn {
            name: r.get("col"),
            data_type: r.get("typ"),
            nullable: !r.get::<bool, _>("notnull"),
            primary_key: false,
        });
    }
    let pks = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT n.nspname AS s, c.relname AS t, a.attname AS col FROM pg_index i JOIN pg_class c ON c.oid = i.indrelid \
           JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey) \
          WHERE i.indisprimary AND {USER_TABLES}"
    )))
    .fetch_all(&mut *db)
    .await?;
    for r in pks {
        let name = format!("{}.{}", r.get::<String, _>("s"), r.get::<String, _>("t"));
        if let Some(t) = tables.get_mut(&name) {
            let col: String = r.get("col");
            if let Some(c) = t.columns.iter_mut().find(|c| c.name == col) {
                c.primary_key = true;
            }
        }
    }
    let fks = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT n.nspname AS s, c.relname AS t, a.attname AS col, fn.nspname AS fs, fc.relname AS ft, fa.attname AS fcol \
           FROM pg_constraint k JOIN pg_class c ON c.oid = k.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
           JOIN pg_class fc ON fc.oid = k.confrelid JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
           JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.conkey[1] \
           JOIN pg_attribute fa ON fa.attrelid = fc.oid AND fa.attnum = k.confkey[1] \
          WHERE k.contype = 'f' AND cardinality(k.conkey) = 1 AND {USER_TABLES} ORDER BY n.nspname, c.relname, a.attname"
    )))
    .fetch_all(&mut *db)
    .await?;
    for r in fks {
        let name = format!("{}.{}", r.get::<String, _>("s"), r.get::<String, _>("t"));
        if let Some(t) = tables.get_mut(&name) {
            t.foreign_keys.push(DbForeignKey {
                column: r.get("col"),
                ref_table: format!("{}.{}", r.get::<String, _>("fs"), r.get::<String, _>("ft")),
                ref_column: r.get("fcol"),
            });
        }
    }
    for t in tables.values_mut() {
        if let Some((s, n)) = split_table(&t.name) {
            let q = format!(
                "SELECT count(*) FROM (SELECT 1 FROM {}.{} LIMIT 1000001) x",
                quote(s),
                quote(n)
            );
            t.rows = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(q))
                .fetch_one(&mut *db)
                .await
                .ok();
        }
    }
    let mut side = DbSide {
        tables: tables.into_values().collect(),
        rules: Vec::new(),
        names: BTreeMap::new(),
        projected: BTreeMap::new(),
    };
    for r in rules {
        let Some((s, n)) = split_table(&r.db_table) else {
            continue;
        };
        let mut cols = vec![r.name_col.clone()];
        cols.extend(r.alias_cols.iter().cloned());
        if !cols.iter().all(|c| is_ident(c)) || !is_ident(&r.key_col) {
            continue;
        }
        let sel = cols
            .iter()
            .map(|c| format!("{}::text", quote(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let q = format!("SELECT {sel} FROM {}.{} LIMIT 100000", quote(s), quote(n));
        let Ok(rows) = sqlx::query(sqlx::AssertSqlSafe(q))
            .fetch_all(&mut *db)
            .await
        else {
            continue;
        };
        let set: &mut BTreeSet<String> = side.names.entry(r.db_table.clone()).or_default();
        for row in rows {
            for i in 0..cols.len() {
                if let Ok(Some(v)) = row.try_get::<Option<String>, _>(i) {
                    set.insert(norm(&v));
                }
            }
        }
        side.rules.push(DbLinkRule {
            table: r.db_table.clone(),
            key_col: r.key_col.clone(),
            name_col: r.name_col.clone(),
            entity_type: r.entity_type.clone(),
        });
    }
    // What the projection already linked (the Agent DB may not have its tables yet).
    if let Ok(rows) = sqlx::query(
        "SELECT db_table, count(*) AS n FROM _oo_docproj.entity_link WHERE status <> 'rejected' GROUP BY db_table",
    )
    .fetch_all(&mut *db)
    .await
    {
        for r in rows {
            side.projected.insert(r.get("db_table"), r.get("n"));
        }
    }
    Ok(side)
}
