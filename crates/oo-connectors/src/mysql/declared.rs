use super::*;
use crate::engine::declared::{BYTE_LIMIT, Export};
use crate::engine::{DeclaredSourceCatalog, DeclaredSourceColumn, DeclaredSourceTable};
use crate::{ForeignKey, ForeignKeyColumn, KeyColumn};

fn text(row: &Row, index: usize) -> Result<String> {
    row_text(row, index).ok_or_else(|| SourceError::catalog("declared_catalog_shape"))
}

fn ordinal(row: &Row, index: usize) -> Result<i32> {
    row_u64(row, index)
        .and_then(|n| i32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| SourceError::catalog("declared_catalog_shape"))
}

impl MysqlEngine {
    pub(super) async fn declared_rows(
        &mut self,
        sql: &str,
        table: &TableRef,
        remaining: &mut u64,
        rows: u64,
    ) -> Result<Vec<Row>> {
        let sql = format!("{sql} LIMIT {}", rows.min(100_000) + 1);
        let (result, bytes, over) = self
            .exec_page(
                &sql,
                vec![
                    Value::from(table.schema.as_str()),
                    Value::from(table.name.as_str()),
                ],
                *remaining,
            )
            .await?;
        if over {
            return Err(SourceError::budget(BYTE_LIMIT));
        }
        *remaining -= bytes;
        if result.len() as u64 > rows.min(100_000) {
            return Err(SourceError::budget("declared_catalog_row_limit"));
        }
        Ok(result)
    }

    pub(super) async fn export_declared(
        &mut self,
        tables: &[TableRef],
        budget: &ReadBudget,
    ) -> Result<Option<DeclaredSourceCatalog>> {
        if self.family != EngineFamily::Mysql {
            return Ok(None);
        }
        self.apply_budget(budget).await?;
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        let result = self.declared_inner(tables, budget).await;
        self.finish_op(started);
        result.map(Some)
    }

    async fn declared_inner(
        &mut self,
        tables: &[TableRef],
        budget: &ReadBudget,
    ) -> Result<DeclaredSourceCatalog> {
        let mut out = Export::new(self.options.source_id.clone(), budget)?;
        for table in tables.iter().collect::<BTreeSet<_>>() {
            let mut remaining = out.remaining;
            let rows = self.declared_rows(
                "SELECT TABLE_COMMENT FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND TABLE_TYPE='BASE TABLE'",
                table, &mut remaining, 1).await?;
            let row = rows
                .first()
                .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
            let mut result = DeclaredSourceTable {
                schema: table.schema.clone(),
                name: table.name.clone(),
                comment: Some(text(row, 0)?),
                columns: vec![],
                primary_key: vec![],
                foreign_keys: vec![],
            };
            let rows = self.declared_rows(
                "SELECT COLUMN_NAME, IS_NULLABLE, COLUMN_COMMENT FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",
                table, &mut remaining, budget.max_chunk_rows).await?;
            if rows.is_empty() {
                return Err(SourceError::catalog("selected_table_missing"));
            }
            for row in rows {
                result.columns.push(DeclaredSourceColumn {
                    name: text(&row, 0)?,
                    not_null: text(&row, 1)? == "NO",
                    comment: Some(text(&row, 2)?),
                });
            }
            let rows = self.declared_rows(
                "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, c.ORDINAL_POSITION, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME, r.ORDINAL_POSITION, f.CONSTRAINT_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE k \
                 JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA=k.TABLE_SCHEMA AND c.TABLE_NAME=k.TABLE_NAME AND c.COLUMN_NAME=k.COLUMN_NAME \
                 LEFT JOIN information_schema.COLUMNS r ON r.TABLE_SCHEMA=k.REFERENCED_TABLE_SCHEMA AND r.TABLE_NAME=k.REFERENCED_TABLE_NAME AND r.COLUMN_NAME=k.REFERENCED_COLUMN_NAME \
                 LEFT JOIN information_schema.REFERENTIAL_CONSTRAINTS f ON f.CONSTRAINT_SCHEMA=k.CONSTRAINT_SCHEMA AND f.TABLE_NAME=k.TABLE_NAME AND f.CONSTRAINT_NAME=k.CONSTRAINT_NAME \
                 WHERE k.TABLE_SCHEMA=? AND k.TABLE_NAME=? AND (k.CONSTRAINT_NAME='PRIMARY' OR k.REFERENCED_TABLE_NAME IS NOT NULL) \
                 ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
                table, &mut remaining, budget.max_chunk_rows).await?;
            let mut foreign = BTreeMap::<String, Vec<ForeignKeyColumn>>::new();
            for row in rows {
                if text(&row, 0)? == "PRIMARY" {
                    result.primary_key.push(KeyColumn {
                        name: text(&row, 1)?,
                        attnum: ordinal(&row, 2)?,
                    });
                } else {
                    // Missing referenced metadata is not an empty FK declaration.
                    text(&row, 7)?;
                    foreign
                        .entry(text(&row, 0)?)
                        .or_default()
                        .push(ForeignKeyColumn {
                            name: text(&row, 1)?,
                            attnum: ordinal(&row, 2)?,
                            ref_schema: text(&row, 3)?,
                            ref_table: text(&row, 4)?,
                            ref_name: text(&row, 5)?,
                            ref_attnum: ordinal(&row, 6)?,
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
