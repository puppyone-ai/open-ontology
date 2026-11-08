use super::*;
use crate::engine::SourceColumnShape;

impl MysqlEngine {
    pub(super) async fn export_shape(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<Option<Vec<SourceColumnShape>>> {
        if self.family != EngineFamily::Mysql {
            return Ok(None);
        }
        self.apply_budget(budget).await?;
        self.ensure_basics().await?;
        self.gate().await?;
        let started = Instant::now();
        let result = self.shape_inner(table, budget).await;
        self.finish_op(started);
        result.map(Some)
    }

    async fn shape_inner(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<Vec<SourceColumnShape>> {
        if !mysql_ident(&table.schema) || !mysql_ident(&table.name) {
            return Err(SourceError::identifier("table_ident"));
        }
        self.exec(
            &format!(
                "SELECT * FROM `{}`.`{}` WHERE 1=0",
                table.schema, table.name
            ),
            vec![],
        )
        .await?;
        self.tables.remove(table);
        self.meta(table).await?;
        let mut remaining = budget.max_chunk_bytes;
        let rows = self.declared_rows(
            "SELECT COLUMN_NAME,ORDINAL_POSITION,COLUMN_TYPE,IS_NULLABLE,CHARACTER_SET_NAME,COLLATION_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",
            table, &mut remaining, budget.max_chunk_rows).await?;
        let keys = self.declared_rows(
            "SELECT COLUMN_NAME,INDEX_NAME,SEQ_IN_INDEX FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND NON_UNIQUE=0 ORDER BY INDEX_NAME,SEQ_IN_INDEX",
            table, &mut remaining, budget.max_chunk_rows).await?;
        let meta = self.meta_ref(table)?;
        let mut out = Vec::new();
        for row in rows {
            let name = row_text(&row, 0).ok_or_else(|| SourceError::catalog("column_name"))?;
            let mapped = meta
                .columns
                .iter()
                .find(|c| c.plan.name == name)
                .ok_or_else(|| SourceError::catalog("column_shape_changed"))?;
            let mut column = SourceColumnShape {
                name,
                ordinal: row_u64(&row, 1)
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| SourceError::catalog("column_ordinal"))?,
                source_type: row_text(&row, 2)
                    .ok_or_else(|| SourceError::catalog("column_type"))?,
                target_type: crate::engine::shape::target_type(
                    mapped
                        .plan
                        .arrow
                        .as_ref()
                        .ok_or_else(|| SourceError::catalog("unsupported_target_type"))?,
                )?,
                nullable: row_text(&row, 3).as_deref() == Some("YES"),
                charset: row_text(&row, 4),
                collation: row_text(&row, 5),
                primary_key_ordinal: None,
                unique_key_ordinals: BTreeMap::new(),
            };
            for key in &keys {
                if row_text(key, 0).as_deref() == Some(column.name.as_str()) {
                    let index = row_text(key, 1).ok_or_else(|| SourceError::catalog("key_name"))?;
                    let ordinal = row_u64(key, 2)
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or_else(|| SourceError::catalog("key_ordinal"))?;
                    if index == "PRIMARY" {
                        column.primary_key_ordinal = Some(ordinal);
                    } else {
                        column.unique_key_ordinals.insert(index, ordinal);
                    }
                }
            }
            out.push(column);
        }
        if out.is_empty() {
            return Err(SourceError::catalog("missing_table"));
        }
        Ok(out)
    }
}
