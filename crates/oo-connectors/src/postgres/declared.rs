use super::*;
use crate::engine::declared::{BYTE_LIMIT, Export};

// Aggregate on the server so oversized comment text never crosses the wire.
// All lookups use public pg_catalog metadata, not information_schema's
// owner-only referential_constraints view.
const TABLE: &str = r#"
SELECT CASE WHEN octet_length(body) <= $3::bigint THEN body END
FROM (
 SELECT json_build_object(
 'schema', n.nspname, 'name', c.relname,
 'comment', pg_catalog.obj_description(c.oid, 'pg_class'),
 'columns', COALESCE((SELECT json_agg(json_build_object(
   'name', a.attname, 'not_null', a.attnotnull,
   'comment', pg_catalog.col_description(c.oid, a.attnum)) ORDER BY a.attnum)
   FROM pg_catalog.pg_attribute a
   WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped), '[]'::json),
 'primary_key', COALESCE((SELECT json_agg(json_build_object(
   'attnum', a.attnum, 'name', a.attname) ORDER BY k.ord)
   FROM pg_catalog.pg_constraint p
   CROSS JOIN LATERAL unnest(p.conkey) WITH ORDINALITY k(num,ord)
   JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.num
   WHERE p.conrelid=c.oid AND p.contype='p'), '[]'::json),
 'foreign_keys', COALESCE((SELECT json_agg(json_build_object(
   'name', f.conname, 'columns', (
     SELECT json_agg(json_build_object('attnum', a.attnum, 'name', a.attname,
       'ref_schema', rn.nspname, 'ref_table', rc.relname,
       'ref_attnum', ra.attnum, 'ref_name', ra.attname) ORDER BY k.ord)
     FROM unnest(f.conkey, f.confkey) WITH ORDINALITY k(num,refnum,ord)
     JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.num
     JOIN pg_catalog.pg_attribute ra ON ra.attrelid=f.confrelid AND ra.attnum=k.refnum
     JOIN pg_catalog.pg_class rc ON rc.oid=f.confrelid
     JOIN pg_catalog.pg_namespace rn ON rn.oid=rc.relnamespace
   )) ORDER BY f.conname)
   FROM pg_catalog.pg_constraint f WHERE f.conrelid=c.oid AND f.contype='f'), '[]'::json)
 )::text AS body
 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname=$1 AND c.relname=$2 AND c.relkind IN ('r','p')
) catalog LIMIT 1
"#;

impl PostgresEngine {
    pub(super) async fn export_declared(
        &mut self,
        tables: &[TableRef],
        budget: &ReadBudget,
    ) -> Result<Option<DeclaredSourceCatalog>, SourceError> {
        if self.family != EngineFamily::Postgres {
            return Ok(None);
        }
        validate_budget(budget)?;
        for table in tables {
            self.selected(table)?;
        }
        let mut s = self.connect().await?;
        guards(&mut s, budget).await?;
        self.protect(&mut s).await?;
        self.transaction(&mut s).await?;
        let started = Instant::now();
        let result = async {
            let mut out = Export::new(s.source_id.clone(), budget)?;
            for table in tables.iter().collect::<std::collections::BTreeSet<_>>() {
                let row = session::fetch_one(
                    &mut s,
                    sqlx::query(TABLE)
                        .bind(&table.schema)
                        .bind(&table.name)
                        .bind(out.remaining as i64),
                )
                .await?;
                let body =
                    cell_opt_string(&row, 0)?.ok_or_else(|| SourceError::budget(BYTE_LIMIT))?;
                let table: DeclaredSourceTable = serde_json::from_str(&body)
                    .map_err(|_| SourceError::catalog("declared_catalog_shape"))?;
                if table.columns.len() as u64 > budget.max_chunk_rows
                    || table
                        .foreign_keys
                        .iter()
                        .map(|fk| fk.columns.len() as u64)
                        .sum::<u64>()
                        > budget.max_chunk_rows
                {
                    return Err(SourceError::budget("declared_catalog_row_limit"));
                }
                out.push(table)?;
            }
            Ok(Some(out.catalog))
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
