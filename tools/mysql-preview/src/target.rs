//! Sealed full import on the existing PostgreSQL target functions.
//! The runtime connection copies. The admin connection checks, indexes, and analyzes.
//! Reader column grants stay with `install-column-acl`.

use std::collections::BTreeSet;
use std::time::Duration;

use oo_connectors::{stable_ident, PG17_RESERVED};
use oo_pipeline::physical_mapping::{plan_physical_mapping, quote_physical_name, Level, MappingRequest};
use oo_pipeline::full_import::quality_preimage;
use oo_pipeline::target_installer::{self, sha256_hex};
use oo_pipeline::target_statistics::{analyze_mapped_shadows, LogicalTableRef, StatisticsRequest, MAX_TABLES};
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

use crate::config::{PreviewConfig, Stop};
use crate::error::PreviewError;
use crate::source::{sealed_identity, EncodedBatch, SourceRead};

pub struct LoadOutcome {
    pub run_id: String,
    pub target_generation: i64,
    pub source_generation: i64,
    pub state: &'static str,
    pub tables: Vec<Value>,
    pub exhausted_tables: i64,
    pub open_tables: i64,
    pub target_state: String,
    pub quality_stored: bool,
}

pub async fn load_target(config: &PreviewConfig, admin_password: &str, runtime_password: &str, admin_ca: &[u8], runtime_ca: &[u8], source: &SourceRead) -> Result<LoadOutcome, PreviewError> {
    let mut admin = dial(config, &config.target_admin_user, admin_password, admin_ca, true).await?;
    let mut runtime = dial(config, &config.target_runtime_user, runtime_password, runtime_ca, false).await?;
    prepare_target(&mut admin, config).await?;
    let source_generation = i64::try_from(config.generation).map_err(|_| PreviewError::new("budget", "generation"))?;
    if let Some(reused) = reuse_sealed(&mut admin, &mut runtime, config, source, source_generation).await? {
        return Ok(reused);
    }
    let target_generation = adopt(&mut runtime, config).await?;
    let shadows = register(&mut runtime, config, source, source_generation).await?;
    let run_id = match open_run(&mut runtime, config, source, target_generation, source_generation, &shadows).await {
        Ok(run_id) => run_id,
        Err(err) => return Err(err),
    };
    if config.stop == Stop::CancelBeforeBatch {
        retire(&mut runtime, config, target_generation, &run_id).await?;
        let fact = read_run_fact(&mut admin, &run_id).await?;
        return finish(FinishedRun {
            run_id,
            target_generation,
            source_generation,
            state: "retired",
            tables: Vec::new(),
            fact,
            quality_stored: false,
            exhausted: 0,
            require_open: true,
        });
    }
    if let Err(err) = prepare_shadows(&mut runtime, config, source, target_generation, source_generation, &run_id, &shadows).await {
        let _ = retire(&mut runtime, config, target_generation, &run_id).await;
        return Err(err);
    }
    if let Err(err) = apply_batches(&mut runtime, config, source, target_generation, &run_id, &shadows).await {
        let _ = retire(&mut runtime, config, target_generation, &run_id).await;
        return Err(err);
    }
    if config.stop == Stop::FailBeforeSeal {
        retire(&mut runtime, config, target_generation, &run_id).await?;
        let fact = read_run_fact(&mut admin, &run_id).await?;
        return finish(FinishedRun {
            run_id,
            target_generation,
            source_generation,
            state: "retired",
            tables: Vec::new(),
            fact,
            quality_stored: true,
            exhausted: 0,
            require_open: true,
        });
    }
    if config.stop == Stop::PartialAfterFirst {
        let Some(first) = shadows.first() else {
            return Err(PreviewError::new("target", "partial"));
        };
        note_listed(&mut runtime, config, target_generation, &run_id, std::slice::from_ref(first)).await?;
        let fact = read_run_fact(&mut admin, &run_id).await?;
        return finish(FinishedRun {
            run_id,
            target_generation,
            source_generation,
            state: "open",
            tables: shadow_json(std::slice::from_ref(first), source),
            fact,
            quality_stored: true,
            exhausted: 1,
            require_open: true,
        });
    }
    if let Err(err) = check_and_index(&mut admin, &shadows, source).await {
        let _ = retire(&mut runtime, config, target_generation, &run_id).await;
        return Err(err);
    }
    if let Err(err) = analyze(&mut admin, config, source, source_generation, &shadows).await {
        let _ = retire(&mut runtime, config, target_generation, &run_id).await;
        return Err(err);
    }
    if let Err(err) = note_and_seal(&mut runtime, config, source, target_generation, source_generation, &run_id, &shadows).await {
        let _ = retire(&mut runtime, config, target_generation, &run_id).await;
        return Err(err);
    }
    let fact = read_run_fact(&mut admin, &run_id).await?;
    let exhausted = i64::try_from(shadows.len()).map_err(|_| PreviewError::new("config", "tables"))?;
    finish(FinishedRun {
        run_id,
        target_generation,
        source_generation,
        state: "awaiting_publish",
        tables: shadow_json(&shadows, source),
        fact,
        quality_stored: true,
        exhausted,
        require_open: false,
    })
}

struct ShadowColumn {
    logical: String,
    attnum: i32,
    oid: i64,
    typmod: i32,
    not_null: bool,
    physical: String,
    key: bool,
}

struct Shadow {
    index: i32,
    logical_schema: String,
    logical_table: String,
    physical: String,
    key_physical: String,
    columns: Vec<ShadowColumn>,
    rows: u64,
    canonical_digest: String,
}

async fn dial(config: &PreviewConfig, user: &str, password: &str, ca: &[u8], admin: bool) -> Result<PgConnection, PreviewError> {
    let options = PgConnectOptions::from_explicit()
        .host(&config.target_host)
        .port(config.target_port)
        .username(user)
        .password(password)
        .database(&config.target_database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec())
        .disable_statement_logging();
    let connect = tokio::time::timeout(Duration::from_millis(config.connect_timeout_ms), options.connect()).await;
    let mut conn = match connect {
        Ok(Ok(conn)) => conn,
        Ok(Err(_)) => return Err(PreviewError::new("connect", "target")),
        Err(_) => return Err(PreviewError::new("connect", "connect_timeout")),
    };
    // 0000 revokes EXECUTE on pg_catalog.set_config from PUBLIC and grants it
    // only to oo_target_owner. assert_privileges forbids that grant for
    // oo_pipeline_runtime. The SET utility (the local-pg-mirror session
    // recipe) still applies; do not call the function.
    let statement = if admin {
        "SET statement_timeout = '120s'"
    } else {
        "SET statement_timeout = '30s'"
    };
    sqlx::query(statement)
        .execute(&mut conn)
        .await
        .map_err(|_| PreviewError::new("connect", "target"))?;
    sqlx::query("SET lock_timeout = '1s'")
        .execute(&mut conn)
        .await
        .map_err(|_| PreviewError::new("connect", "target"))?;
    sqlx::query("SET search_path = pg_catalog")
        .execute(&mut conn)
        .await
        .map_err(|_| PreviewError::new("connect", "target"))?;
    Ok(conn)
}

async fn prepare_target(conn: &mut PgConnection, config: &PreviewConfig) -> Result<(), PreviewError> {
    require_login(conn, &config.target_runtime_user, &config.target_admin_user).await?;
    target_installer::install(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "install"))?;
    target_installer::install_full_import(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "install"))?;
    target_installer::verify_full_import_readiness(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "not_ready"))?;
    grant_member(conn, &config.target_runtime_user).await?;
    grant_connect(conn, &config.target_database, &config.target_runtime_user).await?;
    Ok(())
}

async fn require_login(conn: &mut PgConnection, name: &str, admin: &str) -> Result<(), PreviewError> {
    if name == admin || matches!(name, "oo_pipeline_runtime" | "oo_target_owner" | "postgres") {
        return Err(PreviewError::new("config", "user"));
    }
    let row = sqlx::query("SELECT rolsuper, rolcanlogin FROM pg_catalog.pg_roles WHERE rolname = $1")
        .bind(name)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "role"))?;
    let Some(row) = row else {
        return Err(PreviewError::new("target", "role_missing"));
    };
    let superuser: bool = row.try_get(0).map_err(|_| PreviewError::new("target", "role"))?;
    let login: bool = row.try_get(1).map_err(|_| PreviewError::new("target", "role"))?;
    if superuser || !login {
        return Err(PreviewError::new("target", "role"));
    }
    Ok(())
}

async fn grant_member(conn: &mut PgConnection, runtime: &str) -> Result<(), PreviewError> {
    // PostgreSQL 16 stores INHERIT on the membership. A grant issued while the
    // login was NOINHERIT keeps inherit_option false after ALTER ROLE INHERIT,
    // so schema USAGE on _oo never applies. Re-grant with INHERIT TRUE.
    // Reader roles are not this login and stay NOINHERIT.
    let sql = format!("GRANT oo_pipeline_runtime TO \"{runtime}\" WITH INHERIT TRUE");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "grant"))?;
    Ok(())
}

async fn grant_connect(conn: &mut PgConnection, database: &str, user: &str) -> Result<(), PreviewError> {
    let allowed: bool = sqlx::query_scalar("SELECT pg_catalog.has_database_privilege($1, $2, 'CONNECT')")
        .bind(user)
        .bind(database)
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "grant"))?;
    if allowed {
        return Ok(());
    }
    let sql = format!("GRANT CONNECT ON DATABASE \"{database}\" TO \"{user}\"");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "grant"))?;
    Ok(())
}

async fn reuse_sealed(
    admin: &mut PgConnection,
    runtime: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    source_generation: i64,
) -> Result<Option<LoadOutcome>, PreviewError> {
    let found = sqlx::query(
        "SELECT run_id::text, target_generation, source_generation FROM _oo.full_import_run WHERE shard_id = $1 AND source_id = $2 AND state = 'awaiting_publish' AND config_digest = $3 AND type_digest = $4 AND input_digest = $5 AND schema_digest = $6 ORDER BY opened_at DESC LIMIT 1",
    )
    .bind(&config.shard)
    .bind(&config.source_id)
    .bind(&source.config_digest)
    .bind(&source.type_digest)
    .bind(&source.input_digest)
    .bind(&source.schema_digest)
    .fetch_optional(&mut *admin)
    .await
    .map_err(|_| PreviewError::new("target", "state"))?;
    let Some(found) = found else {
        return Ok(None);
    };
    let run_id: String = found.try_get(0).map_err(|_| PreviewError::new("target", "state"))?;
    let target_generation: i64 = found.try_get(1).map_err(|_| PreviewError::new("target", "state"))?;
    let stored_generation: i64 = found.try_get(2).map_err(|_| PreviewError::new("target", "state"))?;
    if stored_generation != source_generation {
        return Err(PreviewError::new("target", "state"));
    }
    let stored_tables = sqlx::query(
        "SELECT schema_ident, table_ident, physical_name, applied_rows, canonical_digest, exhausted FROM _oo.full_import_table WHERE run_id = $1::uuid ORDER BY table_index",
    )
    .bind(&run_id)
    .fetch_all(&mut *admin)
    .await
    .map_err(|_| PreviewError::new("target", "state"))?;
    if stored_tables.len() != source.tables.len() {
        return Err(PreviewError::new("target", "state"));
    }
    // The unpublished seal keeps its original target generation. Adopting again
    // would move the shard fence and then refuse to recreate the same shadows.
    let shadows = register(runtime, config, source, source_generation).await?;
    if shadows.len() != stored_tables.len() {
        return Err(PreviewError::new("target", "state"));
    }
    for (shadow, stored) in shadows.iter().zip(stored_tables.iter()) {
        let schema: String = stored.try_get(0).map_err(|_| PreviewError::new("target", "state"))?;
        let table: String = stored.try_get(1).map_err(|_| PreviewError::new("target", "state"))?;
        let physical: String = stored.try_get(2).map_err(|_| PreviewError::new("target", "state"))?;
        let applied: i64 = stored.try_get(3).map_err(|_| PreviewError::new("target", "state"))?;
        let digest: String = stored.try_get(4).map_err(|_| PreviewError::new("target", "state"))?;
        let exhausted: bool = stored.try_get(5).map_err(|_| PreviewError::new("target", "state"))?;
        let rows = i64::try_from(shadow.rows).map_err(|_| PreviewError::new("budget", "max_rows"))?;
        if schema != shadow.logical_schema
            || table != shadow.logical_table
            || physical != shadow.physical
            || applied != rows
            || digest != shadow.canonical_digest
            || !exhausted
        {
            return Err(PreviewError::new("target", "state"));
        }
    }
    let fact = read_run_fact(admin, &run_id).await?;
    let exhausted = i64::try_from(shadows.len()).map_err(|_| PreviewError::new("config", "tables"))?;
    let outcome = finish(FinishedRun {
        run_id,
        target_generation,
        source_generation,
        state: "awaiting_publish",
        tables: shadow_json(&shadows, source),
        fact,
        quality_stored: true,
        exhausted,
        require_open: false,
    })?;
    Ok(Some(outcome))
}

async fn adopt(conn: &mut PgConnection, config: &PreviewConfig) -> Result<i64, PreviewError> {
    let generation: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
        .bind(&config.shard)
        .bind(&config.owner)
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "adopt"))?;
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(&config.shard)
        .bind(&config.owner)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "adopt"))?;
    Ok(generation)
}

async fn register(conn: &mut PgConnection, config: &PreviewConfig, source: &SourceRead, source_generation: i64) -> Result<Vec<Shadow>, PreviewError> {
    let reserved: BTreeSet<String> = PG17_RESERVED.iter().map(|word| (*word).to_string()).collect();
    let mut shadows = Vec::new();
    for table in &source.tables {
        let schema_ident = stable_ident(&table.schema, &reserved).map_err(|_| PreviewError::new("ident", "ident"))?;
        let table_ident = stable_ident(&table.name, &reserved).map_err(|_| PreviewError::new("ident", "ident"))?;
        register_one(
            conn,
            config,
            source,
            RegisterPlace {
                generation: source_generation,
                level: Level::Namespace,
                namespace: &schema_ident,
                table: "",
                column: "",
            },
        )
        .await?;
        let physical = register_one(
            conn,
            config,
            source,
            RegisterPlace {
                generation: source_generation,
                level: Level::Table,
                namespace: &schema_ident,
                table: &table_ident,
                column: "",
            },
        )
        .await?;
        let mut columns = Vec::new();
        let mut key_physical = String::new();
        for column in &table.columns {
            let ident = stable_ident(&column.plan.name, &reserved).map_err(|_| PreviewError::new("ident", "ident"))?;
            let column_physical = register_one(
                conn,
                config,
                source,
                RegisterPlace {
                    generation: source_generation,
                    level: Level::Column,
                    namespace: &schema_ident,
                    table: &table_ident,
                    column: &ident,
                },
            )
            .await?;
            if column.key {
                key_physical = column_physical.clone();
            }
            columns.push(ShadowColumn {
                logical: ident,
                attnum: column.plan.attnum,
                oid: column.oid,
                typmod: column.typmod,
                not_null: column.not_null,
                physical: column_physical,
                key: column.key,
            });
        }
        let mut previous_attnum = 0i32;
        for column in &columns {
            if column.attnum <= previous_attnum {
                return Err(PreviewError::new("catalog", "attnum"));
            }
            previous_attnum = column.attnum;
        }
        let (rows, canonical_digest) = digest_for(&table.batches);
        shadows.push(Shadow {
            index: i32::try_from(table.index).map_err(|_| PreviewError::new("config", "tables"))?,
            logical_schema: schema_ident,
            logical_table: table_ident,
            physical,
            key_physical,
            columns,
            rows,
            canonical_digest,
        });
    }
    Ok(shadows)
}

struct RegisterPlace<'a> {
    generation: i64,
    level: Level,
    namespace: &'a str,
    table: &'a str,
    column: &'a str,
}

async fn register_one(
    conn: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    place: RegisterPlace<'_>,
) -> Result<String, PreviewError> {
    let request = MappingRequest {
        source: config.source_id.clone(),
        config_digest: source.config_digest.clone(),
        run_generation: place.generation,
        level: place.level,
        logical_namespace: place.namespace.to_string(),
        logical_table: place.table.to_string(),
        logical_column: place.column.to_string(),
    };
    let mapped = plan_physical_mapping(&request).map_err(|_| PreviewError::new("target", "physical_name"))?;
    sqlx::query("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(&config.source_id)
        .bind(&source.config_digest)
        .bind(place.generation)
        .bind(place.level.as_str())
        .bind(place.namespace)
        .bind(place.table)
        .bind(place.column)
        .bind(mapped.identity_digest.as_slice())
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "register"))?;
    let stored: String = sqlx::query_scalar("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)")
        .bind(&config.source_id)
        .bind(&source.config_digest)
        .bind(place.generation)
        .bind(place.level.as_str())
        .bind(place.namespace)
        .bind(place.table)
        .bind(place.column)
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "lookup"))?;
    if stored != mapped.physical_name {
        return Err(PreviewError::new("target", "physical_name"));
    }
    Ok(stored)
}

async fn open_run(
    conn: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    generation: i64,
    source_generation: i64,
    shadows: &[Shadow],
) -> Result<String, PreviewError> {
    let schemas: Vec<String> = shadows.iter().map(|table| table.logical_schema.clone()).collect();
    let names: Vec<String> = shadows.iter().map(|table| table.logical_table.clone()).collect();
    let physical: Vec<String> = shadows.iter().map(|table| table.physical.clone()).collect();
    let mut tx = conn.begin().await.map_err(|_| PreviewError::new("target", "open"))?;
    let run_id: String = sqlx::query_scalar(
        "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
    )
    .bind(&config.shard)
    .bind(&config.owner)
    .bind(generation)
    .bind(&config.source_id)
    .bind(source_generation)
    .bind(&source.config_digest)
    .bind(&source.type_digest)
    .bind(&source.input_digest)
    .bind(&source.schema_digest)
    .bind(&schemas)
    .bind(&names)
    .bind(&physical)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| PreviewError::new("target", "open"))?;
    tx.commit().await.map_err(|_| PreviewError::new("target", "open"))?;
    Ok(run_id)
}

async fn prepare_shadows(
    conn: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    generation: i64,
    source_generation: i64,
    run_id: &str,
    shadows: &[Shadow],
) -> Result<(), PreviewError> {
    let mut tx = conn.begin().await.map_err(|_| PreviewError::new("target", "prepare"))?;
    for table in shadows {
        let idents: Vec<String> = table.columns.iter().map(|column| column.logical.clone()).collect();
        let oids: Vec<i64> = table.columns.iter().map(|column| column.oid).collect();
        let typmods: Vec<i32> = table.columns.iter().map(|column| column.typmod).collect();
        let required: Vec<bool> = table.columns.iter().map(|column| column.not_null).collect();
        let physical: String = sqlx::query_scalar(
            "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
        )
        .bind(&config.shard)
        .bind(&config.owner)
        .bind(generation)
        .bind(run_id)
        .bind(table.index)
        .bind(&config.source_id)
        .bind(&source.config_digest)
        .bind(source_generation)
        .bind(&table.logical_schema)
        .bind(&table.logical_table)
        .bind(&idents)
        .bind(&oids)
        .bind(&typmods)
        .bind(&required)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PreviewError::new("target", "prepare"))?;
        if physical != table.physical {
            return Err(PreviewError::new("target", "physical_name"));
        }
    }
    tx.commit().await.map_err(|_| PreviewError::new("target", "prepare"))?;
    Ok(())
}

async fn apply_batches(
    conn: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    generation: i64,
    run_id: &str,
    shadows: &[Shadow],
) -> Result<(), PreviewError> {
    for table in &source.tables {
        let shadow = shadows
            .iter()
            .find(|shadow| shadow.index == i32::try_from(table.index).unwrap_or(-1))
            .ok_or_else(|| PreviewError::new("target", "batch"))?;
        for batch in &table.batches {
            write_one(conn, config, source, generation, run_id, shadow, batch).await?;
        }
    }
    Ok(())
}

async fn write_one(
    conn: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    generation: i64,
    run_id: &str,
    table: &Shadow,
    batch: &EncodedBatch,
) -> Result<(), PreviewError> {
    let rejected = u64::try_from(batch.quality_rejects.len()).map_err(|_| PreviewError::new("budget", "max_rows"))?;
    let quality = quality_preimage(
        &batch.batch_id,
        batch.rows,
        rejected,
        &source.input_digest,
        &batch.quality_rejects,
    );
    let mut mixed = batch.copy_bytes.clone();
    mixed.extend(quality);
    if sha256_hex(&mixed) != batch.payload_digest {
        return Err(PreviewError::new("target", "payload"));
    }
    let statement = copy_statement(table)?;
    let mut tx = conn.begin().await.map_err(|_| PreviewError::new("target", "copy"))?;
    let decision: String = sqlx::query_scalar(
        "SELECT _oo.full_import_stage_decision($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::numeric)",
    )
    .bind(&config.shard)
    .bind(&config.owner)
    .bind(generation)
    .bind(&config.source_id)
    .bind(&table.physical)
    .bind(&batch.batch_id)
    .bind(&batch.payload_digest)
    .bind(&source.config_digest)
    .bind(&source.type_digest)
    .bind(&source.hook_digest)
    .bind(&source.schema_digest)
    .bind(&batch.applied_pos)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| PreviewError::new("target", "stage"))?;
    if decision == "replay" {
        tx.rollback().await.map_err(|_| PreviewError::new("target", "stage"))?;
        return Ok(());
    }
    let mut copy = tx.copy_in_raw(&statement).await.map_err(|_| PreviewError::new("target", "copy"))?;
    if copy.send(batch.copy_bytes.as_slice()).await.is_err() {
        let _ = copy.abort("copy send failed").await;
        return Err(PreviewError::new("target", "copy"));
    }
    if copy.finish().await.is_err() {
        return Err(PreviewError::new("target", "copy"));
    }
    let rows = i64::try_from(batch.rows).map_err(|_| PreviewError::new("budget", "max_rows"))?;
    let rejected_rows = i64::try_from(batch.quality_rejects.len()).map_err(|_| PreviewError::new("budget", "max_rows"))?;
    let reasons: Vec<String> = batch.quality_rejects.iter().map(|reject| reject.reason.clone()).collect();
    let value_lens: Vec<i64> = batch.quality_rejects.iter().map(|reject| reject.value_len).collect();
    let attnums: Vec<i32> = batch.quality_rejects.iter().map(|reject| reject.attnum).collect();
    let row_indexes: Vec<i64> = batch.quality_rejects.iter().map(|reject| reject.row_index).collect();
    let key_digests: Vec<String> = batch.quality_rejects.iter().map(|reject| reject.key_digest.clone()).collect();
    let wrote: String = sqlx::query_scalar(
        "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
    )
    .bind(&config.shard)
    .bind(&config.owner)
    .bind(generation)
    .bind(run_id)
    .bind(table.index)
    .bind(&config.source_id)
    .bind(&table.physical)
    .bind(&batch.batch_id)
    .bind(&batch.payload_digest)
    .bind(&source.config_digest)
    .bind(&source.type_digest)
    .bind(&source.hook_digest)
    .bind(&source.schema_digest)
    .bind(&source.input_digest)
    .bind(&batch.applied_pos)
    .bind(rows)
    .bind(rejected_rows)
    .bind(reasons)
    .bind(value_lens)
    .bind(attnums)
    .bind(row_indexes)
    .bind(key_digests)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| PreviewError::new("target", "write"))?;
    if wrote != "inserted" && wrote != "replay" {
        let _ = tx.rollback().await;
        return Err(PreviewError::new("target", "write"));
    }
    tx.commit().await.map_err(|_| PreviewError::new("target", "write"))?;
    Ok(())
}

fn copy_statement(table: &Shadow) -> Result<String, PreviewError> {
    let quoted = quote_physical_name(&table.physical).map_err(|_| PreviewError::new("target", "physical_name"))?;
    let mut list = String::from("\"_oo_pos\"");
    for column in &table.columns {
        list.push_str(", ");
        list.push_str(&quote_physical_name(&column.physical).map_err(|_| PreviewError::new("target", "physical_name"))?);
    }
    Ok(format!("COPY _oo.{quoted} ({list}) FROM STDIN WITH (FORMAT binary)"))
}

async fn check_and_index(admin: &mut PgConnection, shadows: &[Shadow], source: &SourceRead) -> Result<(), PreviewError> {
    for shadow in shadows {
        let quoted = quote_physical_name(&shadow.physical).map_err(|_| PreviewError::new("target", "physical_name"))?;
        let key = quote_physical_name(&shadow.key_physical).map_err(|_| PreviewError::new("target", "physical_name"))?;
        let sql = format!("SELECT count(*)::int8, count(DISTINCT {key})::int8 FROM \"_oo\".{quoted}");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&mut *admin)
            .await
            .map_err(|_| PreviewError::new("target", "key_coverage"))?;
        let count: i64 = row.try_get(0).map_err(|_| PreviewError::new("target", "key_coverage"))?;
        let distinct: i64 = row.try_get(1).map_err(|_| PreviewError::new("target", "key_coverage"))?;
        if count != distinct || u64::try_from(count).unwrap_or(u64::MAX) != shadow.rows {
            return Err(PreviewError::new("target", "key_coverage"));
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
        .map_err(|_| PreviewError::new("target", "type_mismatch"))?;
        if attrs.len() != shadow.columns.len() + 1 {
            return Err(PreviewError::new("target", "type_mismatch"));
        }
        let position_name: String = attrs[0].try_get(0).map_err(|_| PreviewError::new("target", "type_mismatch"))?;
        let position_oid: i64 = attrs[0].try_get(1).map_err(|_| PreviewError::new("target", "type_mismatch"))?;
        if position_name != "_oo_pos" || position_oid != 1700 {
            return Err(PreviewError::new("target", "type_mismatch"));
        }
        for (attr, column) in attrs.iter().skip(1).zip(shadow.columns.iter()) {
            let oid: i64 = attr.try_get(1).map_err(|_| PreviewError::new("target", "type_mismatch"))?;
            let typmod: i32 = attr.try_get(2).map_err(|_| PreviewError::new("target", "type_mismatch"))?;
            if oid != column.oid || typmod != column.typmod {
                return Err(PreviewError::new("target", "type_mismatch"));
            }
        }
        let prefix: String = shadow.physical.chars().take(60).collect();
        let index = format!("ix_{prefix}");
        let create = format!("CREATE INDEX \"{index}\" ON \"_oo\".{quoted} ({key})");
        sqlx::raw_sql(sqlx::AssertSqlSafe(create))
            .execute(&mut *admin)
            .await
            .map_err(|_| PreviewError::new("target", "index"))?;
        let _ = source;
    }
    Ok(())
}

async fn analyze(
    admin: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    source_generation: i64,
    shadows: &[Shadow],
) -> Result<(), PreviewError> {
    for chunk in shadows.chunks(MAX_TABLES) {
        let request = StatisticsRequest {
            project: config.source_id.clone(),
            source: config.source_id.clone(),
            config_digest: source.config_digest.clone(),
            source_generation,
            tables: chunk
                .iter()
                .map(|table| LogicalTableRef {
                    namespace: table.logical_schema.clone(),
                    name: table.logical_table.clone(),
                })
                .collect(),
            statement_timeout_ms: u32::try_from(config.statement_timeout_ms).unwrap_or(30_000),
            lock_timeout_ms: u32::try_from(config.lock_timeout_ms).unwrap_or(1_000),
        };
        analyze_mapped_shadows(admin, &request)
            .await
            .map_err(|_| PreviewError::new("target", "analyze"))?;
    }
    Ok(())
}

async fn note_listed(conn: &mut PgConnection, config: &PreviewConfig, generation: i64, run_id: &str, shadows: &[Shadow]) -> Result<(), PreviewError> {
    for shadow in shadows {
        let rows = i64::try_from(shadow.rows).map_err(|_| PreviewError::new("budget", "max_rows"))?;
        sqlx::query("SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)")
            .bind(&config.shard)
            .bind(&config.owner)
            .bind(generation)
            .bind(run_id)
            .bind(shadow.index)
            .bind(rows)
            .bind(&shadow.canonical_digest)
            .bind(&config.source_id)
            .execute(&mut *conn)
            .await
            .map_err(|_| PreviewError::new("target", "note"))?;
    }
    Ok(())
}

async fn note_and_seal(
    conn: &mut PgConnection,
    config: &PreviewConfig,
    source: &SourceRead,
    generation: i64,
    source_generation: i64,
    run_id: &str,
    shadows: &[Shadow],
) -> Result<(), PreviewError> {
    sealed_identity(source, config)?;
    note_listed(conn, config, generation, run_id, shadows).await?;
    let schemas: Vec<String> = shadows.iter().map(|table| table.logical_schema.clone()).collect();
    let names: Vec<String> = shadows.iter().map(|table| table.logical_table.clone()).collect();
    let sealed: String = sqlx::query_scalar(
        "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
    )
    .bind(&config.shard)
    .bind(&config.owner)
    .bind(generation)
    .bind(run_id)
    .bind(&config.source_id)
    .bind(source_generation)
    .bind(&source.config_digest)
    .bind(&source.type_digest)
    .bind(&source.input_digest)
    .bind(&source.schema_digest)
    .bind(&schemas)
    .bind(&names)
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| PreviewError::new("target", "seal"))?;
    if sealed != "awaiting_publish" {
        return Err(PreviewError::new("target", "seal"));
    }
    Ok(())
}

async fn retire(conn: &mut PgConnection, config: &PreviewConfig, generation: i64, run_id: &str) -> Result<(), PreviewError> {
    sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
        .bind(&config.shard)
        .bind(&config.owner)
        .bind(generation)
        .bind(run_id)
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "retire"))?;
    Ok(())
}

struct RunFact {
    state: String,
    exhausted: i64,
    open_tables: i64,
}

async fn read_run_fact(admin: &mut PgConnection, run_id: &str) -> Result<RunFact, PreviewError> {
    let state: String = sqlx::query_scalar("SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid")
        .bind(run_id)
        .fetch_one(&mut *admin)
        .await
        .map_err(|_| PreviewError::new("target", "state"))?;
    let row = sqlx::query(
        "SELECT count(*) FILTER (WHERE exhausted)::int8, count(*) FILTER (WHERE NOT exhausted)::int8 FROM _oo.full_import_table WHERE run_id = $1::uuid",
    )
    .bind(run_id)
    .fetch_one(&mut *admin)
    .await
    .map_err(|_| PreviewError::new("target", "state"))?;
    Ok(RunFact {
        state,
        exhausted: row.try_get(0).map_err(|_| PreviewError::new("target", "state"))?,
        open_tables: row.try_get(1).map_err(|_| PreviewError::new("target", "state"))?,
    })
}

struct FinishedRun {
    run_id: String,
    target_generation: i64,
    source_generation: i64,
    state: &'static str,
    tables: Vec<Value>,
    fact: RunFact,
    quality_stored: bool,
    exhausted: i64,
    require_open: bool,
}

fn finish(run: FinishedRun) -> Result<LoadOutcome, PreviewError> {
    let FinishedRun {
        run_id,
        target_generation,
        source_generation,
        state,
        tables,
        fact,
        quality_stored,
        exhausted,
        require_open,
    } = run;
    let open_ok = if require_open { fact.open_tables > 0 } else { fact.open_tables == 0 };
    if fact.state != state || fact.exhausted != exhausted || !open_ok {
        return Err(PreviewError::new("target", "state"));
    }
    Ok(LoadOutcome {
        run_id,
        target_generation,
        source_generation,
        state,
        tables,
        exhausted_tables: fact.exhausted,
        open_tables: fact.open_tables,
        target_state: fact.state,
        quality_stored,
    })
}

fn digest_for(batches: &[EncodedBatch]) -> (u64, String) {
    let mut rows = 0u64;
    let mut hashes = Vec::new();
    for batch in batches {
        rows += batch.rows;
        hashes.extend_from_slice(&batch.row_hashes);
    }
    (rows, sha256_hex(&hashes))
}

fn shadow_json(shadows: &[Shadow], source: &SourceRead) -> Vec<Value> {
    shadows
        .iter()
        .map(|shadow| {
            json!({
                "namespace": shadow.logical_schema,
                "table": shadow.logical_table,
                "physical": shadow.physical,
                "rows": shadow.rows,
                "canonical_digest": shadow.canonical_digest,
                "columns": shadow.columns.iter().map(|column| json!({
                    "name": column.logical,
                    "oid": column.oid,
                    "typmod": column.typmod,
                    "physical": column.physical,
                    "key": column.key
                })).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .zip(source.tables.iter())
        .map(|(mut value, table)| {
            if let Some(object) = value.as_object_mut() {
                object.insert("rejects".to_string(), json!(table.rejects));
            }
            value
        })
        .collect()
}
