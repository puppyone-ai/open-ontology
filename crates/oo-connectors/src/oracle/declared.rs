use super::*;
use crate::engine::declared::{BYTE_LIMIT, Export};
use crate::engine::{DeclaredSourceCatalog, DeclaredSourceColumn, DeclaredSourceTable};
use crate::{ForeignKey, ForeignKeyColumn, KeyColumn};

fn text(row: &[Option<String>], index: usize) -> Result<String, SourceError> {
    row.get(index)
        .cloned()
        .flatten()
        .ok_or_else(|| SourceError::catalog("declared_catalog_shape"))
}

fn ordinal(row: &[Option<String>], index: usize) -> Result<i32, SourceError> {
    text(row, index)?
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| SourceError::catalog("declared_catalog_shape"))
}

impl OracleEngine {
    pub(super) fn declared_rows(
        &mut self,
        sql: &str,
        table: &TableRef,
        remaining: &mut u64,
        rows: u64,
    ) -> Result<Rows, SourceError> {
        // Oracle comments are VARCHAR2(4000); bound each fetch to one row and
        // charge before collecting the next, including on the worker route.
        let limit = rows.min(100_000) as usize;
        let sql = format!("SELECT * FROM ({sql}) WHERE ROWNUM <= {}", limit + 1);
        let sql = serial_query(&sql);
        let mut result = Vec::new();
        let mut collect = |row: Vec<Option<String>>| {
            if result.len() >= limit {
                return Err(SourceError::budget("declared_catalog_row_limit"));
            }
            let bytes = row.iter().flatten().map(|s| s.len() as u64).sum::<u64>();
            *remaining = remaining
                .checked_sub(bytes)
                .ok_or_else(|| SourceError::budget(BYTE_LIMIT))?;
            result.push(row);
            Ok(())
        };
        if let Some(worker) = &self.worker {
            let binds = [
                Bind::Text(table.schema.clone()),
                Bind::Text(table.name.clone()),
            ];
            for row in worker
                .query(&sql, &binds, limit + 1)
                .map_err(|e| map_driver(&e))?
            {
                collect(row.map_err(|e| map_driver(&e))?)?;
            }
        } else {
            let statement = self
                .conn
                .as_ref()
                .unwrap()
                .statement(&sql)
                .map_err(|e| map_ora(&e))?
                .fetch_array_size(1)
                .prefetch_rows(0)
                .build()
                .map_err(|e| map_ora(&e))?;
            let cursor = statement
                .query(&[&table.schema, &table.name])
                .map_err(|e| map_ora(&e))?;
            let width = cursor.columns().len();
            for row in cursor {
                let row = row.map_err(|e| map_ora(&e))?;
                let cells = (0..width)
                    .map(|i| row.get::<Option<String>>(i))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| map_ora(&e))?;
                collect(cells)?;
            }
        }
        Ok(result)
    }

    pub(super) async fn export_declared(
        &mut self,
        tables: &[TableRef],
        budget: &ReadBudget,
    ) -> Result<Option<DeclaredSourceCatalog>, SourceError> {
        validate_budget(budget)?;
        self.set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        self.ensure_version()?;
        self.protect().await?;
        let started = Instant::now();
        let result = self.declared_inner(tables, budget);
        self.schedule_next(started.elapsed(), budget);
        self.pace().await;
        result.map(Some)
    }

    fn declared_inner(
        &mut self,
        tables: &[TableRef],
        budget: &ReadBudget,
    ) -> Result<DeclaredSourceCatalog, SourceError> {
        let mut out = Export::new("oracle".into(), budget)?;
        for table in tables.iter().collect::<std::collections::BTreeSet<_>>() {
            if !self.cfg.scope.contains(table) {
                return Err(SourceError::config("selected_table"));
            }
            let mut remaining = out.remaining;
            let rows = self.declared_rows(
                "SELECT comments FROM all_tab_comments WHERE owner=:b1 AND table_name=:b2 AND table_type='TABLE'",
                table, &mut remaining, 1)?;
            let row = rows
                .first()
                .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
            let mut result = DeclaredSourceTable {
                schema: table.schema.clone(),
                name: table.name.clone(),
                comment: row[0].clone(),
                columns: vec![],
                primary_key: vec![],
                foreign_keys: vec![],
            };
            let rows = self.declared_rows(
                "SELECT c.column_name, c.nullable, m.comments FROM all_tab_cols c \
                 LEFT JOIN all_col_comments m ON m.owner=c.owner AND m.table_name=c.table_name AND m.column_name=c.column_name \
                 WHERE c.owner=:b1 AND c.table_name=:b2 AND c.hidden_column='NO' ORDER BY c.column_id",
                table, &mut remaining, budget.max_chunk_rows)?;
            if rows.is_empty() {
                return Err(SourceError::catalog("selected_table_missing"));
            }
            for row in rows {
                result.columns.push(DeclaredSourceColumn {
                    name: text(&row, 0)?,
                    not_null: text(&row, 1)? == "N",
                    comment: row[2].clone(),
                });
            }
            let rows = self.declared_rows(
                "SELECT k.constraint_type, k.constraint_name, c.column_name, TO_CHAR(a.column_id) column_id, r.owner, r.table_name, rc.column_name ref_column_name, TO_CHAR(ra.column_id) ref_column_id \
                 FROM all_constraints k JOIN all_cons_columns c ON c.owner=k.owner AND c.constraint_name=k.constraint_name AND c.table_name=k.table_name \
                 JOIN all_tab_cols a ON a.owner=c.owner AND a.table_name=c.table_name AND a.column_name=c.column_name \
                 LEFT JOIN all_constraints r ON r.owner=k.r_owner AND r.constraint_name=k.r_constraint_name \
                 LEFT JOIN all_cons_columns rc ON rc.owner=r.owner AND rc.constraint_name=r.constraint_name AND rc.table_name=r.table_name AND rc.position=c.position \
                 LEFT JOIN all_tab_cols ra ON ra.owner=rc.owner AND ra.table_name=rc.table_name AND ra.column_name=rc.column_name \
                 WHERE k.owner=:b1 AND k.table_name=:b2 AND k.constraint_type IN ('P','R') ORDER BY k.constraint_name, c.position",
                table, &mut remaining, budget.max_chunk_rows)?;
            let mut foreign = BTreeMap::<String, Vec<ForeignKeyColumn>>::new();
            for row in rows {
                if text(&row, 0)? == "P" {
                    result.primary_key.push(KeyColumn {
                        name: text(&row, 2)?,
                        attnum: ordinal(&row, 3)?,
                    });
                } else {
                    foreign
                        .entry(text(&row, 1)?)
                        .or_default()
                        .push(ForeignKeyColumn {
                            name: text(&row, 2)?,
                            attnum: ordinal(&row, 3)?,
                            ref_schema: text(&row, 4)?,
                            ref_table: text(&row, 5)?,
                            ref_name: text(&row, 6)?,
                            ref_attnum: ordinal(&row, 7)?,
                        });
                }
            }
            result.foreign_keys = foreign
                .into_iter()
                .map(|(name, columns)| ForeignKey { name, columns })
                .collect();
            out.push(result)?;
        }
        Ok(out.catalog)
    }
}
