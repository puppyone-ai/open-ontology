use super::*;
use crate::types::{MapDecision, map_column};

impl PostgresEngine {
    pub(super) async fn export_shape(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<Option<Vec<SourceColumnShape>>, SourceError> {
        if !matches!(self.family, EngineFamily::Postgres | EngineFamily::Kingbase) {
            return Ok(None);
        }
        validate_budget(budget)?;
        self.selected(table)?;
        let mut s = self.connect_table(table).await?;
        guards(&mut s, budget).await?;
        self.protect(&mut s).await?;
        self.transaction(&mut s).await?;
        let started = Instant::now();
        let result =
            async {
                let schema = s.quote_one(&table.schema).await?;
                let name = s.quote_one(&table.name).await?;
                // Catalog visibility alone is not proof of SELECT permission.
                session::fetch_all(
                    &mut s,
                    sqlx::query(sqlx_core::sql_str::AssertSqlSafe(format!(
                        "SELECT * FROM {schema}.{name} WHERE false"
                    ))),
                )
                .await?;
                let catalog = s.collect_catalog().await?;
                let source = catalog
                    .tables
                    .iter()
                    .find(|t| t.schema == table.schema && t.name == table.name)
                    .ok_or_else(|| SourceError::catalog("selected_table_missing"))?;
                let graph = s.load_type_graph(&source.columns).await?;
                let complex_key: bool = session::fetch_control(
                    &mut s,
                    sqlx::query(
                        "SELECT EXISTS(SELECT 1 FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid
                 JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2
                 AND i.indisunique AND (i.indpred IS NOT NULL OR i.indexprs IS NOT NULL
                 OR NOT i.indisvalid OR NOT i.indisready))",
                    )
                    .bind(&table.schema)
                    .bind(&table.name),
                )
                .await?
                .try_get(0)
                .map_err(|_| SourceError::catalog("column_shape"))?;
                if complex_key {
                    return Ok(None);
                }
                s.arm_rows(2048);
                let rows = session::fetch_all(&mut s, sqlx::query(
                "SELECT a.attname::text,a.attnum::bigint,format_type(a.atttypid,a.atttypmod),
                 current_setting('server_encoding'),co.collname::text
                 FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid
                 JOIN pg_namespace n ON n.oid=c.relnamespace
                 LEFT JOIN pg_collation co ON co.oid=a.attcollation
                 WHERE n.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped
                 ORDER BY a.attnum LIMIT 2048"
            ).bind(&table.schema).bind(&table.name)).await?;
                let keys = session::fetch_all(
                    &mut s,
                    sqlx::query(
                        "SELECT a.attname::text,ic.relname::text,k.ord::bigint,i.indisprimary
                 FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid
                 JOIN pg_namespace n ON n.oid=c.relnamespace
                 JOIN pg_class ic ON ic.oid=i.indexrelid
                 CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,ord)
                 JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.attnum
                 WHERE n.nspname=$1 AND c.relname=$2 AND i.indisunique
                 AND i.indisvalid AND i.indisready AND k.ord<=i.indnkeyatts
                 ORDER BY ic.relname,k.ord LIMIT 2048",
                    )
                    .bind(&table.schema)
                    .bind(&table.name),
                )
                .await?;
                if rows.len() >= 2048 || keys.len() >= 2048 {
                    return Err(SourceError::budget("column_count"));
                }
                let mut out = Vec::new();
                for row in rows {
                    let name = cell_string(&row, 0)?;
                    let c = source
                        .columns
                        .iter()
                        .find(|c| c.name == name)
                        .ok_or_else(|| SourceError::catalog("column_shape_changed"))?;
                    let plan = map_column(c, &graph, false);
                    if plan.mapping.decision != MapDecision::Lossless {
                        return Err(SourceError::catalog("unsupported_target_type"));
                    }
                    let mut column = SourceColumnShape {
                        name,
                        ordinal: u32::try_from(cell_i64(&row, 1)?)
                            .map_err(|_| SourceError::catalog("column_ordinal"))?,
                        source_type: if matches!(c.typtype.as_str(), "e" | "d") {
                            format!(
                                "{}#catalog:{}",
                                cell_string(&row, 2)?,
                                serde_json::to_string(&(
                                    &c.enum_labels,
                                    &c.domain_checks,
                                    c.base_type_oid
                                ))
                                .map_err(|_| SourceError::catalog("column_shape"))?
                            )
                        } else {
                            cell_string(&row, 2)?
                        },
                        target_type: crate::engine::shape::target_type(
                            plan.arrow
                                .as_ref()
                                .ok_or_else(|| SourceError::catalog("unsupported_target_type"))?,
                        )?,
                        nullable: !c.not_null,
                        charset: (c.collation_oid != 0)
                            .then(|| cell_string(&row, 3))
                            .transpose()?,
                        collation: cell_opt_string(&row, 4)?,
                        primary_key_ordinal: None,
                        unique_key_ordinals: BTreeMap::new(),
                    };
                    for key in &keys {
                        if cell_string(key, 0)? == column.name {
                            let ordinal = u32::try_from(cell_i64(key, 2)?)
                                .map_err(|_| SourceError::catalog("key_ordinal"))?;
                            if cell_bool(key, 3)? {
                                column.primary_key_ordinal = Some(ordinal);
                            } else {
                                column
                                    .unique_key_ordinals
                                    .insert(cell_string(key, 1)?, ordinal);
                            }
                        }
                    }
                    out.push(column);
                }
                if out.is_empty()
                    || out.len() as u64 > budget.max_chunk_rows
                    || serde_json::to_vec(&out)
                        .map_err(|_| SourceError::catalog("column_shape"))?
                        .len() as u64
                        > budget.max_chunk_bytes
                {
                    return Err(SourceError::budget("column_count"));
                }
                Ok(Some(out))
            }
            .await;
        let result = s.finish(result).await;
        if result.is_ok() {
            self.session = Some(s);
        }
        duty(started, budget.duty_cycle_percent).await;
        result
    }
}
