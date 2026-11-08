use super::*;
use crate::engine::SourceColumnShape;
use crate::types::ArrowPhysical;

impl OracleEngine {
    pub(super) async fn export_shape(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<Option<Vec<SourceColumnShape>>, SourceError> {
        validate_budget(budget)?;
        if !self.cfg.scope.contains(table) {
            return Err(SourceError::config("selected_table"));
        }
        self.set_timeout(Duration::from_millis(budget.statement_timeout_ms))?;
        self.ensure_version()?;
        self.protect().await?;
        let started = Instant::now();
        let result = self
            .shape_inner(table, budget)
            .map_err(|e| self.recover_timed_out_call(e, started, budget));
        self.schedule_next(started.elapsed(), budget);
        self.pace().await;
        result.map(Some)
    }

    fn shape_inner(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<Vec<SourceColumnShape>, SourceError> {
        self.query(
            &format!(
                "SELECT * FROM {}.{} WHERE 1=0",
                keyset::ident(&table.schema)?,
                keyset::ident(&table.name)?
            ),
            &[],
            1,
        )?;
        let meta = self.load_meta(table)?;
        let mut remaining = budget.max_chunk_bytes;
        let rows = self.declared_rows(
            "SELECT column_name,TO_CHAR(column_id),character_set_name,TO_CHAR(char_length),char_used FROM all_tab_cols WHERE owner=:b1 AND table_name=:b2 AND hidden_column='NO' ORDER BY column_id",
            table, &mut remaining, budget.max_chunk_rows)?;
        let keys = self.declared_rows(
            "SELECT ic.column_name,i.index_name,TO_CHAR(ic.column_position),CASE WHEN EXISTS (SELECT 1 FROM all_constraints c WHERE c.owner=i.table_owner AND c.table_name=i.table_name AND c.index_name=i.index_name AND c.constraint_type='P') THEN 'P' ELSE 'U' END FROM all_indexes i JOIN all_ind_columns ic ON ic.index_owner=i.owner AND ic.index_name=i.index_name WHERE i.table_owner=:b1 AND i.table_name=:b2 AND i.uniqueness='UNIQUE' ORDER BY i.index_name,ic.column_position",
            table, &mut remaining, budget.max_chunk_rows)?;
        let mut out = Vec::new();
        for row in rows {
            let name = row[0].clone().ok_or(SourceError::catalog("column_name"))?;
            let c = meta
                .columns
                .iter()
                .find(|c| c.name == name)
                .ok_or(SourceError::catalog("column_shape_changed"))?;
            let physical = match super::super::typed::data_type(&c.data_type, c.precision, c.scale)
            {
                DataType::Int64 => ArrowPhysical::Int64,
                DataType::Float64 => ArrowPhysical::Float64,
                DataType::Binary => ArrowPhysical::Binary,
                DataType::Utf8 => ArrowPhysical::Utf8,
                DataType::Decimal128(precision, scale) => {
                    ArrowPhysical::Decimal128 { precision, scale }
                }
                DataType::Timestamp(_, None) => ArrowPhysical::TimestampMicrosNaive,
                DataType::Timestamp(_, Some(_)) => ArrowPhysical::TimestampMicrosUtc,
                _ => return Err(SourceError::catalog("unsupported_target_type")),
            };
            let source_type = match c.data_type.as_str() {
                "NUMBER" | "INTEGER" => match (c.precision, c.scale) {
                    (Some(p), Some(s)) => format!("{}({p},{s})", c.data_type),
                    (None, Some(s)) => format!("{}(*,{s})", c.data_type),
                    _ => c.data_type.clone(),
                },
                "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" => format!(
                    "{}({} {})",
                    c.data_type,
                    if row[4].as_deref() == Some("C") {
                        row[3].as_deref().unwrap_or("0").to_owned()
                    } else {
                        c.data_length.to_string()
                    },
                    if row[4].as_deref() == Some("C") {
                        "CHAR"
                    } else {
                        "BYTE"
                    }
                ),
                "RAW" => format!("RAW({})", c.data_length),
                _ => c.data_type.clone(),
            };
            let mut column = SourceColumnShape {
                name,
                ordinal: row[1]
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .ok_or(SourceError::catalog("column_ordinal"))?,
                source_type,
                target_type: crate::engine::shape::target_type(&physical)?,
                nullable: c.nullable,
                charset: row[2].clone(),
                collation: c.collation.clone(),
                primary_key_ordinal: None,
                unique_key_ordinals: BTreeMap::new(),
            };
            for key in &keys {
                if key[0].as_deref() == Some(column.name.as_str()) {
                    let ordinal = key[2]
                        .as_deref()
                        .and_then(|v| v.parse().ok())
                        .ok_or(SourceError::catalog("key_ordinal"))?;
                    if key[3].as_deref() == Some("P") {
                        column.primary_key_ordinal = Some(ordinal);
                    } else {
                        column.unique_key_ordinals.insert(
                            key[1].clone().ok_or(SourceError::catalog("key_name"))?,
                            ordinal,
                        );
                    }
                }
            }
            out.push(column);
        }
        if out.is_empty() {
            return Err(SourceError::catalog("table_not_visible"));
        }
        Ok(out)
    }
}
