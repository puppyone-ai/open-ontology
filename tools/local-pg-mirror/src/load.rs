//! Target shadow load. The runtime connection that binds the generation is the
//! connection that copies, notes, and seals. Admin checks, indexes, and analyzes.

use std::collections::BTreeSet;

use oo_connectors::{stable_ident, PG17_RESERVED};
use oo_pipeline::physical_mapping::{plan_physical_mapping, quote_physical_name, Level, MappingRequest};
use oo_pipeline::target_installer::sha256_hex;
use oo_pipeline::target_statistics::{analyze_mapped_shadows, LogicalTableRef, StatisticsRequest, MAX_TABLES};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};

use crate::error::MirrorError;
use crate::ident::quote_ident;
use crate::manifest::{ColumnRecord, TableRecord};
use crate::pg;
use crate::plan::Plan;
use crate::session::Digests;
use crate::spool::BatchBlob;

struct ShadowColumn {
    name: String,
    logical: String,
    attnum: i32,
    oid: i64,
    typmod: i32,
    not_null: bool,
    physical: String,
}

struct Shadow {
    index: i32,
    schema: String,
    name: String,
    schema_ident: String,
    table_ident: String,
    physical: String,
    key_physical: String,
    columns: Vec<ShadowColumn>,
}

pub struct TargetOutcome {
    pub run_id: String,
    pub generation: i64,
    pub tables: Vec<TableRecord>,
    pub index_count: u64,
    pub analyze_tables: u64,
    pub sealed: bool,
}

pub async fn apply(
    runtime: &mut PgConnection,
    admin: &mut PgConnection,
    plan: &Plan,
    digests: &Digests,
    batches: &[BatchBlob],
    seal: bool,
) -> Result<TargetOutcome, MirrorError> {
    let generation = adopt(runtime, plan).await?;
    let shadows = register(runtime, plan, digests).await?;
    let run_id = match open_run(runtime, plan, digests, generation, &shadows).await {
        Ok(run_id) => run_id,
        Err(err) => return Err(err),
    };
    if let Err(err) = prepare(runtime, plan, digests, generation, &run_id, &shadows).await {
        let _ = retire(runtime, plan, generation, &run_id).await;
        return Err(err);
    }
    if let Err(err) = apply_batches(runtime, plan, digests, generation, &run_id, &shadows, batches).await {
        let _ = retire(runtime, plan, generation, &run_id).await;
        return Err(err);
    }
    let tables = records(&shadows, batches);
    if !seal {
        retire(runtime, plan, generation, &run_id).await?;
        return Ok(TargetOutcome {
            run_id,
            generation,
            tables,
            index_count: 0,
            analyze_tables: 0,
            sealed: false,
        });
    }
    if let Err(err) = check_and_index(admin, &shadows, &tables).await {
        let _ = retire(runtime, plan, generation, &run_id).await;
        return Err(err);
    }
    let analyzed = match analyze(admin, plan, digests, &shadows).await {
        Ok(count) => count,
        Err(err) => {
            let _ = retire(runtime, plan, generation, &run_id).await;
            return Err(err);
        }
    };
    if let Err(err) = note_and_seal(runtime, plan, digests, generation, &run_id, &shadows, &tables).await {
        let _ = retire(runtime, plan, generation, &run_id).await;
        return Err(err);
    }
    Ok(TargetOutcome {
        run_id,
        generation,
        tables,
        index_count: shadows.len() as u64,
        analyze_tables: analyzed,
        sealed: true,
    })
}

async fn adopt(conn: &mut PgConnection, plan: &Plan) -> Result<i64, MirrorError> {
    let generation: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
        .bind(&plan.config.shard)
        .bind(&plan.config.owner)
        .fetch_one(&mut *conn)
        .await
        .map_err(pg::map_sqlx)?;
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(&plan.config.shard)
        .bind(&plan.config.owner)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .map_err(pg::map_sqlx)?;
    Ok(generation)
}

async fn register(conn: &mut PgConnection, plan: &Plan, digests: &Digests) -> Result<Vec<Shadow>, MirrorError> {
    let reserved = reserved_words();
    let source_generation = i64::try_from(plan.config.generation).map_err(|_| MirrorError::new("budget", "generation"))?;
    let mut shadows = Vec::new();
    for (index, table) in plan.admitted.iter().enumerate() {
        let schema_ident = stable_ident(&table.schema, &reserved).map_err(|_| MirrorError::new("ident", "ident"))?;
        let table_ident = stable_ident(&table.name, &reserved).map_err(|_| MirrorError::new("ident", "ident"))?;
        register_one(
            conn,
            plan,
            digests,
            source_generation,
            Level::Namespace,
            &schema_ident,
            "",
            "",
        )
        .await?;
        let physical = register_one(
            conn,
            plan,
            digests,
            source_generation,
            Level::Table,
            &schema_ident,
            &table_ident,
            "",
        )
        .await?;
        let mut columns = Vec::new();
        let mut key_physical = String::new();
        for column in &table.columns {
            let ident = stable_ident(&column.name, &reserved).map_err(|_| MirrorError::new("ident", "ident"))?;
            let column_physical = register_one(
                conn,
                plan,
                digests,
                source_generation,
                Level::Column,
                &schema_ident,
                &table_ident,
                &ident,
            )
            .await?;
            if table.primary_key.first().is_some_and(|name| name == &column.name) {
                key_physical = column_physical.clone();
            }
            columns.push(ShadowColumn {
                name: column.name.clone(),
                logical: ident,
                attnum: column.attnum,
                oid: column.oid,
                typmod: column.typmod,
                not_null: column.not_null,
                physical: column_physical,
            });
        }
        shadows.push(Shadow {
            index: i32::try_from(index).map_err(|_| MirrorError::new("config", "selection"))?,
            schema: table.schema.clone(),
            name: table.name.clone(),
            schema_ident,
            table_ident,
            physical,
            key_physical,
            columns,
        });
    }
    Ok(shadows)
}

#[allow(clippy::too_many_arguments)]
async fn register_one(
    conn: &mut PgConnection,
    plan: &Plan,
    digests: &Digests,
    source_generation: i64,
    level: Level,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<String, MirrorError> {
    let request = MappingRequest {
        source: plan.config.source_id.clone(),
        config_digest: digests.config_digest.clone(),
        run_generation: source_generation,
        level,
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    };
    let mapped = plan_physical_mapping(&request).map_err(|_| MirrorError::new("config", "physical_name"))?;
    sqlx::query("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(&plan.config.source_id)
        .bind(&digests.config_digest)
        .bind(source_generation)
        .bind(level.as_str())
        .bind(namespace)
        .bind(table)
        .bind(column)
        .bind(mapped.identity_digest.as_slice())
        .execute(&mut *conn)
        .await
        .map_err(pg::map_sqlx)?;
    let stored: String = sqlx::query_scalar("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)")
        .bind(&plan.config.source_id)
        .bind(&digests.config_digest)
        .bind(source_generation)
        .bind(level.as_str())
        .bind(namespace)
        .bind(table)
        .bind(column)
        .fetch_one(&mut *conn)
        .await
        .map_err(pg::map_sqlx)?;
    if stored != mapped.physical_name {
        return Err(MirrorError::new("refused", "physical_name"));
    }
    Ok(stored)
}

async fn open_run(
    conn: &mut PgConnection,
    plan: &Plan,
    digests: &Digests,
    generation: i64,
    shadows: &[Shadow],
) -> Result<String, MirrorError> {
    let source_generation = i64::try_from(plan.config.generation).map_err(|_| MirrorError::new("budget", "generation"))?;
    let schemas: Vec<String> = shadows.iter().map(|table| table.schema_ident.clone()).collect();
    let names: Vec<String> = shadows.iter().map(|table| table.table_ident.clone()).collect();
    let physical: Vec<String> = shadows.iter().map(|table| table.physical.clone()).collect();
    let mut tx = conn.begin().await.map_err(pg::map_sqlx)?;
    let run_id: String = sqlx::query_scalar(
        "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
    )
    .bind(&plan.config.shard)
    .bind(&plan.config.owner)
    .bind(generation)
    .bind(&plan.config.source_id)
    .bind(source_generation)
    .bind(&digests.config_digest)
    .bind(&digests.type_digest)
    .bind(&digests.input_digest)
    .bind(&digests.schema_digest)
    .bind(&schemas)
    .bind(&names)
    .bind(&physical)
    .fetch_one(&mut *tx)
    .await
    .map_err(pg::map_sqlx)?;
    tx.commit().await.map_err(pg::map_sqlx)?;
    Ok(run_id)
}

async fn prepare(
    conn: &mut PgConnection,
    plan: &Plan,
    digests: &Digests,
    generation: i64,
    run_id: &str,
    shadows: &[Shadow],
) -> Result<(), MirrorError> {
    let source_generation = i64::try_from(plan.config.generation).map_err(|_| MirrorError::new("budget", "generation"))?;
    let mut tx = conn.begin().await.map_err(pg::map_sqlx)?;
    for table in shadows {
        let idents: Vec<String> = table.columns.iter().map(|column| column.logical.clone()).collect();
        let oids: Vec<i64> = table.columns.iter().map(|column| column.oid).collect();
        let typmods: Vec<i32> = table.columns.iter().map(|column| column.typmod).collect();
        let required: Vec<bool> = table.columns.iter().map(|column| column.not_null).collect();
        let physical: String = sqlx::query_scalar(
            "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
        )
        .bind(&plan.config.shard)
        .bind(&plan.config.owner)
        .bind(generation)
        .bind(run_id)
        .bind(table.index)
        .bind(&plan.config.source_id)
        .bind(&digests.config_digest)
        .bind(source_generation)
        .bind(&table.schema_ident)
        .bind(&table.table_ident)
        .bind(&idents)
        .bind(&oids)
        .bind(&typmods)
        .bind(&required)
        .fetch_one(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
        if physical != table.physical {
            return Err(MirrorError::new("refused", "physical_name"));
        }
    }
    tx.commit().await.map_err(pg::map_sqlx)?;
    Ok(())
}

async fn apply_batches(
    conn: &mut PgConnection,
    plan: &Plan,
    digests: &Digests,
    generation: i64,
    run_id: &str,
    shadows: &[Shadow],
    batches: &[BatchBlob],
) -> Result<(), MirrorError> {
    for batch in batches {
        let table = shadows
            .get(batch.table_index as usize)
            .ok_or_else(|| MirrorError::new("refused", "batch"))?;
        let quality = crate::spool::quality_bytes(&batch.batch_id, batch.rows, &digests.input_digest);
        let mut mixed = batch.copy_bytes.clone();
        mixed.extend_from_slice(&quality);
        if sha256_hex(&mixed) != batch.payload_digest {
            return Err(MirrorError::new("manifest_tamper", "spool"));
        }
        let statement = copy_statement(table)?;
        let mut tx = conn.begin().await.map_err(pg::map_sqlx)?;
        let decision: String = sqlx::query_scalar(
            "SELECT _oo.full_import_stage_decision($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::numeric)",
        )
        .bind(&plan.config.shard)
        .bind(&plan.config.owner)
        .bind(generation)
        .bind(&plan.config.source_id)
        .bind(&table.physical)
        .bind(&batch.batch_id)
        .bind(&batch.payload_digest)
        .bind(&digests.config_digest)
        .bind(&digests.type_digest)
        .bind(&digests.hook_digest)
        .bind(&digests.schema_digest)
        .bind(&batch.applied_pos)
        .fetch_one(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
        if decision == "replay" {
            tx.rollback().await.map_err(pg::map_sqlx)?;
            continue;
        }
        let mut copy = tx
            .copy_in_raw(&statement)
            .await
            .map_err(|_| MirrorError::new("database", "copy"))?;
        if copy.send(batch.copy_bytes.as_slice()).await.is_err() {
            let _ = copy.abort("copy send failed").await;
            return Err(MirrorError::new("database", "copy"));
        }
        if copy.finish().await.is_err() {
            return Err(MirrorError::new("database", "copy"));
        }
        let rows = i64::try_from(batch.rows).map_err(|_| MirrorError::new("budget", "max_rows"))?;
        let wrote: String = sqlx::query_scalar(
            "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
        )
        .bind(&plan.config.shard)
        .bind(&plan.config.owner)
        .bind(generation)
        .bind(run_id)
        .bind(table.index)
        .bind(&plan.config.source_id)
        .bind(&table.physical)
        .bind(&batch.batch_id)
        .bind(&batch.payload_digest)
        .bind(&digests.config_digest)
        .bind(&digests.type_digest)
        .bind(&digests.hook_digest)
        .bind(&digests.schema_digest)
        .bind(&digests.input_digest)
        .bind(&batch.applied_pos)
        .bind(rows)
        .bind(0_i64)
        .bind(Vec::<String>::new())
        .bind(Vec::<i64>::new())
        .bind(Vec::<i32>::new())
        .bind(Vec::<i64>::new())
        .bind(Vec::<String>::new())
        .fetch_one(&mut *tx)
        .await
        .map_err(pg::map_sqlx)?;
        if wrote == "replay" {
            tx.rollback().await.map_err(pg::map_sqlx)?;
            continue;
        }
        if wrote != "inserted" {
            let _ = tx.rollback().await;
            return Err(MirrorError::new("refused", "batch"));
        }
        tx.commit().await.map_err(pg::map_sqlx)?;
    }
    Ok(())
}

fn copy_statement(table: &Shadow) -> Result<String, MirrorError> {
    let quoted = quote_physical_name(&table.physical).map_err(|_| MirrorError::new("config", "physical_name"))?;
    let mut list = String::from("\"_oo_pos\"");
    for column in &table.columns {
        list.push_str(", ");
        list.push_str(&quote_physical_name(&column.physical).map_err(|_| MirrorError::new("config", "physical_name"))?);
    }
    Ok(format!("COPY _oo.{quoted} ({list}) FROM STDIN WITH (FORMAT binary)"))
}

async fn check_and_index(admin: &mut PgConnection, shadows: &[Shadow], tables: &[TableRecord]) -> Result<(), MirrorError> {
    for (shadow, table) in shadows.iter().zip(tables.iter()) {
        let quoted = quote_physical_name(&shadow.physical).map_err(|_| MirrorError::new("config", "physical_name"))?;
        let key = quote_physical_name(&shadow.key_physical).map_err(|_| MirrorError::new("config", "physical_name"))?;
        let sql = format!("SELECT count(*)::int8, count(DISTINCT {key})::int8 FROM \"_oo\".{quoted}");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&mut *admin)
            .await
            .map_err(pg::map_sqlx)?;
        let count: i64 = row.try_get(0).map_err(pg::map_sqlx)?;
        let distinct: i64 = row.try_get(1).map_err(pg::map_sqlx)?;
        if count != distinct || u64::try_from(count).unwrap_or(u64::MAX) != table.rows {
            return Err(MirrorError::new("refused", "key_coverage"));
        }
        let attrs = sqlx::query(
            "SELECT a.attname::text, a.atttypid::int8, a.atttypmod::int4 \
             FROM pg_catalog.pg_attribute AS a \
             JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = '_oo' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(&shadow.physical)
        .fetch_all(&mut *admin)
        .await
        .map_err(pg::map_sqlx)?;
        if attrs.len() != shadow.columns.len() + 1 {
            return Err(MirrorError::new("refused", "type_mismatch"));
        }
        let position_name: String = attrs[0].try_get(0).map_err(pg::map_sqlx)?;
        let position_oid: i64 = attrs[0].try_get(1).map_err(pg::map_sqlx)?;
        if position_name != "_oo_pos" || position_oid != 1700 {
            return Err(MirrorError::new("refused", "type_mismatch"));
        }
        for (attr, column) in attrs.iter().skip(1).zip(shadow.columns.iter()) {
            let oid: i64 = attr.try_get(1).map_err(pg::map_sqlx)?;
            let typmod: i32 = attr.try_get(2).map_err(pg::map_sqlx)?;
            if oid != column.oid || typmod != column.typmod {
                return Err(MirrorError::new("refused", "type_mismatch"));
            }
        }
        let prefix: String = shadow.physical.chars().take(60).collect();
        let index = quote_ident(&format!("ix_{prefix}"))?;
        let create = format!("CREATE INDEX {index} ON \"_oo\".{quoted} ({key})");
        sqlx::raw_sql(sqlx::AssertSqlSafe(create))
            .execute(&mut *admin)
            .await
            .map_err(pg::map_sqlx)?;
    }
    Ok(())
}

async fn analyze(admin: &mut PgConnection, plan: &Plan, digests: &Digests, shadows: &[Shadow]) -> Result<u64, MirrorError> {
    let source_generation = i64::try_from(plan.config.generation).map_err(|_| MirrorError::new("budget", "generation"))?;
    let mut done = 0u64;
    for chunk in shadows.chunks(MAX_TABLES) {
        let request = StatisticsRequest {
            project: plan.config.source_id.clone(),
            source: plan.config.source_id.clone(),
            config_digest: digests.config_digest.clone(),
            source_generation,
            tables: chunk
                .iter()
                .map(|table| LogicalTableRef {
                    namespace: table.schema_ident.clone(),
                    name: table.table_ident.clone(),
                })
                .collect(),
            statement_timeout_ms: u32::try_from(plan.config.statement_timeout_ms).unwrap_or(30_000),
            lock_timeout_ms: u32::try_from(plan.config.lock_timeout_ms).unwrap_or(1_000),
        };
        analyze_mapped_shadows(admin, &request)
            .await
            .map_err(|_| MirrorError::new("refused", "analyze"))?;
        done += chunk.len() as u64;
    }
    Ok(done)
}

async fn note_and_seal(
    conn: &mut PgConnection,
    plan: &Plan,
    digests: &Digests,
    generation: i64,
    run_id: &str,
    shadows: &[Shadow],
    tables: &[TableRecord],
) -> Result<(), MirrorError> {
    let source_generation = i64::try_from(plan.config.generation).map_err(|_| MirrorError::new("budget", "generation"))?;
    for (shadow, table) in shadows.iter().zip(tables.iter()) {
        let rows = i64::try_from(table.rows).map_err(|_| MirrorError::new("budget", "max_rows"))?;
        sqlx::query("SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)")
            .bind(&plan.config.shard)
            .bind(&plan.config.owner)
            .bind(generation)
            .bind(run_id)
            .bind(shadow.index)
            .bind(rows)
            .bind(&table.canonical_digest)
            .bind(&plan.config.source_id)
            .execute(&mut *conn)
            .await
            .map_err(pg::map_sqlx)?;
    }
    let schemas: Vec<String> = shadows.iter().map(|table| table.schema_ident.clone()).collect();
    let names: Vec<String> = shadows.iter().map(|table| table.table_ident.clone()).collect();
    let sealed: String = sqlx::query_scalar(
        "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
    )
    .bind(&plan.config.shard)
    .bind(&plan.config.owner)
    .bind(generation)
    .bind(run_id)
    .bind(&plan.config.source_id)
    .bind(source_generation)
    .bind(&digests.config_digest)
    .bind(&digests.type_digest)
    .bind(&digests.input_digest)
    .bind(&digests.schema_digest)
    .bind(&schemas)
    .bind(&names)
    .fetch_one(&mut *conn)
    .await
    .map_err(pg::map_sqlx)?;
    if sealed != "awaiting_publish" {
        return Err(MirrorError::new("refused", "seal"));
    }
    Ok(())
}

async fn retire(conn: &mut PgConnection, plan: &Plan, generation: i64, run_id: &str) -> Result<(), MirrorError> {
    sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
        .bind(&plan.config.shard)
        .bind(&plan.config.owner)
        .bind(generation)
        .bind(run_id)
        .execute(&mut *conn)
        .await
        .map_err(pg::map_sqlx)?;
    Ok(())
}

fn records(shadows: &[Shadow], batches: &[BatchBlob]) -> Vec<TableRecord> {
    shadows
        .iter()
        .map(|shadow| {
            let (rows, canonical_digest) = digest_for(batches, shadow.index as u32);
            TableRecord {
                schema: shadow.schema.clone(),
                name: shadow.name.clone(),
                physical_name: Some(shadow.physical.clone()),
                rows,
                canonical_digest,
                columns: shadow
                    .columns
                    .iter()
                    .map(|column| ColumnRecord {
                        name: column.name.clone(),
                        attnum: column.attnum,
                        oid: column.oid,
                        typmod: column.typmod,
                        physical_name: column.physical.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn digest_for(batches: &[BatchBlob], index: u32) -> (u64, String) {
    let mut rows = 0u64;
    let mut hashes = Vec::new();
    let mut any = false;
    for batch in batches.iter().filter(|batch| batch.table_index == index) {
        any = true;
        rows += batch.rows;
        hashes.extend_from_slice(&batch.row_hashes);
    }
    if any {
        (rows, sha256_hex(&hashes))
    } else {
        (0, String::new())
    }
}

fn reserved_words() -> BTreeSet<String> {
    PG17_RESERVED.iter().map(|word| (*word).to_string()).collect()
}
