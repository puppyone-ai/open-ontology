//! Sealed import on one local PostgreSQL connection. MariaDB is read before the run opens.

use std::time::Duration;

use oo_pipeline::copy_encoder::{DecodedCell, TypedBatch};
use oo_pipeline::copy_sink::{self, SinkError};
use oo_pipeline::full_import::{self, hook_digest, quality_preimage};
use oo_pipeline::physical_mapping::{
    LOOKUP_SQL, Level, MappingRequest, REGISTER_SQL, plan_physical_mapping, quote_physical_name,
};
use oo_pipeline::target_installer;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Acquire, ConnectOptions, Row};

use crate::cell::{self, TableSpec};
use crate::config::{self, Config, Phase, Secret};
use crate::native::{self, NativeRow, NativeTable, OracleColumn, OracleFile, OracleTable};

struct RejectLine {
    column: String,
    reason: String,
    row_index: u64,
    attnum: i32,
    value_len: i64,
}

struct Kept {
    cells: Vec<DecodedCell>,
    text: Vec<Option<String>>,
}

struct BoundColumn {
    logical: String,
    physical: String,
    oid: i64,
    typmod: i32,
    not_null: bool,
}

struct BoundTable {
    schema: String,
    logical: String,
    physical: String,
    columns: Vec<BoundColumn>,
    key_physical: String,
}

struct Pipe<'a> {
    config: &'a Config,
    generation: i64,
    config_digest: &'a str,
    type_digest: &'a str,
    hook: &'a str,
    schema_digest: &'a str,
    input_hex: &'a str,
    input_bytes: [u8; 32],
    secrets: &'a [&'a str],
}

struct Reg<'a> {
    digest: &'a str,
    generation: i64,
    level: Level,
    namespace: &'a str,
    table: &'a str,
    column: &'a str,
}

pub async fn run(config: &Config) -> Phase {
    if config.selection.is_none() {
        return done(
            config,
            false,
            json!({
                "step": "approved_objects",
                "error": "approved_objects required",
                "selection": false,
                "connection_id": Json::Null,
                "catalog_written": false,
                "sealed": "",
            }),
        );
    }
    run_selected(config).await
}

pub async fn run_fixture(config: &Config) -> Phase {
    if config.selection.is_some() {
        return done(
            config,
            false,
            json!({
                "step": "fixture",
                "error": "fixture command refuses a customer selection",
                "selection": true,
                "connection_id": Json::Null,
                "catalog_written": false,
                "sealed": "",
            }),
        );
    }
    let fixture = cell::fixture_tables();
    let native = match native::read_fixture(config).await {
        Ok(tables) => tables,
        Err(err) => return done(config, false, json!({ "step": "native", "error": err })),
    };
    let preimage = native::input_preimage(&native);
    let _ = config::write_bytes(
        &config.evidence.join("input-preimage.txt"),
        preimage.as_bytes(),
    );
    let digest = Sha256::digest(preimage.as_bytes());
    let mut input_bytes = [0u8; 32];
    input_bytes.copy_from_slice(&digest);
    let input_hex = config::hex_bytes(&input_bytes);
    let schema_digest = cell::schema_digest(&fixture);
    let type_digest = cell::type_digest(&fixture);
    let config_digest = config::config_digest(&config.image_digest);
    let hook = hook_digest();
    let pg_user = match Secret::load(&config.pg_user_file) {
        Ok(secret) => secret,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "step": "pg-user", "error": err, "input_digest": input_hex }),
            );
        }
    };
    let pg_password = match Secret::load(&config.pg_password_file) {
        Ok(secret) => secret,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "step": "pg-password", "error": err, "input_digest": input_hex }),
            );
        }
    };
    let secrets = [pg_password.expose()];
    let mut conn = match connect_pg(config, pg_user.expose(), pg_password.expose()).await {
        Ok(conn) => conn,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "step": "pg-connect", "error": config::redact(&err, &secrets), "input_digest": input_hex }),
            );
        }
    };
    if let Err(err) = install(&mut conn).await {
        return done(
            config,
            false,
            json!({ "step": "install", "error": config::redact(&err, &secrets), "input_digest": input_hex }),
        );
    }
    let target_generation = match adopt(&mut conn, &config.owner_token).await {
        Ok(generation) => generation,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "step": "adopt", "error": config::redact(&err, &secrets) }),
            );
        }
    };
    let pipe = Pipe {
        config,
        generation: target_generation,
        config_digest: &config_digest,
        type_digest: &type_digest,
        hook: &hook,
        schema_digest: &schema_digest,
        input_hex: &input_hex,
        input_bytes,
        secrets: &secrets,
    };
    let bound = match register(&mut conn, &config_digest, 1, &secrets, &fixture).await {
        Ok(bound) => bound,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "step": "register", "error": err, "target_generation": target_generation }),
            );
        }
    };
    if let Err(err) =
        bind_session(&mut conn, &config.owner_token, target_generation, &secrets).await
    {
        return done(config, false, json!({ "step": "bind", "error": err }));
    }
    let run_id = match open_run(&mut conn, &pipe, &bound).await {
        Ok(run_id) => run_id,
        Err(err) => return done(config, false, json!({ "step": "open", "error": err })),
    };
    let mut stored = Vec::new();
    let mut faults = Vec::new();
    for (index, table) in native.iter().enumerate() {
        match store_table(
            &mut conn,
            &pipe,
            &run_id,
            index as i32,
            table,
            &bound[index],
        )
        .await
        {
            Ok(item) => stored.push(item),
            Err(err) => {
                faults.push(json!({ "table": table.spec.name, "error": err }));
                break;
            }
        }
    }
    let sealed = if faults.is_empty() && stored.len() == bound.len() {
        match seal_run(&mut conn, &pipe, &run_id, &bound).await {
            Ok(state) => state,
            Err(err) => {
                faults.push(json!({ "step": "seal", "error": err }));
                String::new()
            }
        }
    } else {
        String::new()
    };
    let mut oracle_ok = false;
    let mut mismatches = Vec::new();
    if sealed == "awaiting_publish" {
        match compare_stored(&mut conn, &stored, &secrets).await {
            Ok(found) => {
                mismatches = found;
                oracle_ok = mismatches.is_empty();
            }
            Err(err) => faults.push(json!({ "step": "oracle", "error": err })),
        }
        if let Err(err) = write_catalog(&pipe, &stored, &run_id, &fixture) {
            faults.push(json!({ "step": "catalog", "error": err }));
        }
        if let Err(err) = write_oracle(config, &stored) {
            faults.push(json!({ "step": "oracle-file", "error": err }));
        }
    }
    let pg_gates = pg_gates(
        &mut conn,
        config,
        target_generation,
        &config_digest,
        &bound,
        &secrets,
    )
    .await;
    if !pg_gates.ok {
        faults.push(pg_gates.detail.clone());
    }
    let ok = faults.is_empty() && oracle_ok && sealed == "awaiting_publish";
    done(
        config,
        ok,
        json!({
            "run_id": run_id,
            "target_generation": target_generation,
            "input_digest": input_hex,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "config_digest": config_digest,
            "sealed": sealed,
            "oracle_ok": oracle_ok,
            "mismatches": mismatches,
            "faults": faults,
            "tables": stored.iter().map(|item| json!({
                "table": item.logical,
                "physical": item.physical,
                "accepted": item.accepted,
                "rejects": item.rejects.len(),
                "unplanned": item.unplanned,
            })).collect::<Vec<_>>(),
        }),
    )
}

struct StoredTable {
    logical: String,
    physical: String,
    namespace: String,
    key_physical: String,
    columns: Vec<OracleColumn>,
    rows: Vec<Vec<Option<String>>>,
    accepted: u64,
    digest: String,
    rejects: Vec<RejectLine>,
    unplanned: bool,
    batch_id: String,
}

async fn connect_pg(config: &Config, user: &str, password: &str) -> Result<PgConnection, String> {
    let ca = std::fs::read(&config.pg_ca_file).map_err(|err| format!("ca: {err}"))?;
    let options = PgConnectOptions::from_explicit()
        .host(&config.pg_host)
        .port(config.pg_port)
        .username(user)
        .password(password)
        .database(&config.pg_database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca)
        .disable_statement_logging();
    let mut conn = tokio::time::timeout(Duration::from_secs(20), options.connect())
        .await
        .map_err(|_| "pg connect timeout".to_string())?
        .map_err(|err| err.to_string())?;
    sqlx::query("SET statement_timeout = '180s'")
        .execute(&mut conn)
        .await
        .map_err(|err| err.to_string())?;
    Ok(conn)
}

async fn install(conn: &mut PgConnection) -> Result<(), String> {
    target_installer::install(conn)
        .await
        .map_err(|err| err.to_string())?;
    target_installer::install_full_import(conn)
        .await
        .map_err(|err| err.to_string())?;
    target_installer::install_sealed_catalog(conn)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

async fn adopt(conn: &mut PgConnection, owner: &str) -> Result<i64, String> {
    full_import::adopt_generation(conn, config::SHARD, owner)
        .await
        .map_err(|err| err.to_string())
}

async fn register(
    conn: &mut PgConnection,
    config_digest: &str,
    generation: i64,
    secrets: &[&str],
    specs: &[TableSpec],
) -> Result<Vec<BoundTable>, String> {
    let mut seen = Vec::new();
    for spec in specs {
        if !seen.iter().any(|name: &String| name == &spec.schema) {
            register_one(
                conn,
                Reg {
                    digest: config_digest,
                    generation,
                    level: Level::Namespace,
                    namespace: &spec.schema,
                    table: "",
                    column: "",
                },
                secrets,
            )
            .await?;
            seen.push(spec.schema.clone());
        }
    }
    let mut tables = Vec::new();
    for spec in specs {
        let physical = register_one(
            conn,
            Reg {
                digest: config_digest,
                generation,
                level: Level::Table,
                namespace: &spec.schema,
                table: &spec.name,
                column: "",
            },
            secrets,
        )
        .await?;
        let mut columns = Vec::new();
        let mut key_physical = String::new();
        for column in &spec.columns {
            let column_physical = register_one(
                conn,
                Reg {
                    digest: config_digest,
                    generation,
                    level: Level::Column,
                    namespace: &spec.schema,
                    table: &spec.name,
                    column: &column.name,
                },
                secrets,
            )
            .await?;
            if column.key {
                key_physical = column_physical.clone();
            }
            columns.push(BoundColumn {
                logical: column.name.clone(),
                physical: column_physical,
                oid: column.oid(),
                typmod: column.typmod(),
                not_null: !column.nullable,
            });
        }
        tables.push(BoundTable {
            schema: spec.schema.clone(),
            logical: spec.name.clone(),
            physical,
            columns,
            key_physical,
        });
    }
    Ok(tables)
}

async fn register_one(
    conn: &mut PgConnection,
    reg: Reg<'_>,
    secrets: &[&str],
) -> Result<String, String> {
    let config_digest = reg.digest;
    let generation = reg.generation;
    let level = reg.level;
    let namespace = reg.namespace;
    let table = reg.table;
    let column = reg.column;
    let request = MappingRequest {
        source: config::SOURCE_ID.to_string(),
        config_digest: config_digest.to_string(),
        run_generation: generation,
        level,
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    };
    let plan = plan_physical_mapping(&request).map_err(|err| err.to_string())?;
    // register_physical_mapping returns inserted or replay, not the stored name.
    sqlx::query(REGISTER_SQL)
        .bind(&request.source)
        .bind(&request.config_digest)
        .bind(request.run_generation)
        .bind(level.as_str())
        .bind(&request.logical_namespace)
        .bind(&request.logical_table)
        .bind(&request.logical_column)
        .bind(plan.identity_digest.as_slice())
        .execute(&mut *conn)
        .await
        .map_err(|err| sql_err("register", &err, secrets))?;
    let stored: String = sqlx::query_scalar(LOOKUP_SQL)
        .bind(&request.source)
        .bind(&request.config_digest)
        .bind(request.run_generation)
        .bind(level.as_str())
        .bind(&request.logical_namespace)
        .bind(&request.logical_table)
        .bind(&request.logical_column)
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| sql_err("lookup", &err, secrets))?;
    if stored != plan.physical_name {
        return Err(format!("mapping name drift {level:?}"));
    }
    Ok(stored)
}

async fn bind_session(
    conn: &mut PgConnection,
    owner: &str,
    generation: i64,
    secrets: &[&str],
) -> Result<(), String> {
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(config::SHARD)
        .bind(owner)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .map_err(|err| sql_err("bind", &err, secrets))?;
    Ok(())
}

async fn open_run(
    conn: &mut PgConnection,
    pipe: &Pipe<'_>,
    bound: &[BoundTable],
) -> Result<String, String> {
    let config = pipe.config;
    let generation = pipe.generation;
    let config_digest = pipe.config_digest;
    let type_digest = pipe.type_digest;
    let input_hex = pipe.input_hex;
    let schema_digest = pipe.schema_digest;
    let secrets = pipe.secrets;
    let schemas = bound
        .iter()
        .map(|table| table.schema.clone())
        .collect::<Vec<_>>();
    let tables = bound
        .iter()
        .map(|table| table.logical.clone())
        .collect::<Vec<_>>();
    let physical = bound
        .iter()
        .map(|table| table.physical.clone())
        .collect::<Vec<_>>();
    let run_id: String = sqlx::query_scalar(
        "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)::text",
    )
    .bind(config::SHARD)
    .bind(&config.owner_token)
    .bind(generation)
    .bind(config::SOURCE_ID)
    .bind(1_i64)
    .bind(config_digest)
    .bind(type_digest)
    .bind(input_hex)
    .bind(schema_digest)
    .bind(&schemas)
    .bind(&tables)
    .bind(&physical)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| sql_err("open", &err, secrets))?;
    if !config::uuid_text(&run_id) {
        return Err("run id is not a lowercase uuid".to_string());
    }
    Ok(run_id)
}

async fn store_table(
    conn: &mut PgConnection,
    pipe: &Pipe<'_>,
    run_id: &str,
    index: i32,
    table: &NativeTable,
    bound: &BoundTable,
) -> Result<StoredTable, String> {
    let (mut kept, mut rejects, mut unplanned) = split_rows(table);
    let batch_id = format!("t{index}.b0");
    let copied = match copy_kept(conn, pipe, run_id, index, table, bound, &kept).await {
        Ok(done) => done,
        Err(err) => {
            unplanned = true;
            rejects.push(RejectLine {
                column: table.spec.columns[0].name.to_string(),
                reason: "encode_fallback".to_string(),
                row_index: 0,
                attnum: table.spec.columns[0].attnum,
                value_len: err.len() as i64,
            });
            kept.clear();
            copy_kept(conn, pipe, run_id, index, table, bound, &kept).await?
        }
    };
    note_table(conn, pipe, run_id, index, copied.accepted, &copied.digest).await?;
    let spec = &table.spec;
    let columns = spec
        .columns
        .iter()
        .zip(&bound.columns)
        .map(|(column, bound_column)| OracleColumn {
            name: column.name.to_string(),
            physical: bound_column.physical.clone(),
            compare: native::compare_label(column).to_string(),
            scale: native::compare_scale(column),
        })
        .collect();
    Ok(StoredTable {
        logical: spec.name.to_string(),
        physical: bound.physical.clone(),
        namespace: copied.namespace,
        key_physical: bound.key_physical.clone(),
        columns,
        rows: kept.into_iter().map(|row| row.text).collect(),
        accepted: copied.accepted,
        digest: copied.digest,
        rejects,
        unplanned,
        batch_id,
    })
}

fn split_rows(table: &NativeTable) -> (Vec<Kept>, Vec<RejectLine>, bool) {
    let mut kept = Vec::new();
    let mut rejects = Vec::new();
    let mut unplanned = false;
    for row in &table.rows {
        match decode_row(table, row) {
            Ok(cells) => kept.push(Kept {
                cells,
                text: row.text.clone(),
            }),
            Err(line) => {
                if line.reason != "time_range" && line.reason != "zero_date" {
                    unplanned = true;
                }
                rejects.push(line);
            }
        }
    }
    (kept, rejects, unplanned)
}

fn decode_row(table: &NativeTable, row: &NativeRow) -> Result<Vec<DecodedCell>, RejectLine> {
    let mut cells = Vec::with_capacity(table.spec.columns.len());
    for (column, value) in table.spec.columns.iter().zip(&row.values) {
        match cell::decode_cell(column, value) {
            Ok(cell) => cells.push(cell),
            Err(fault) => {
                return Err(RejectLine {
                    column: fault.column.to_string(),
                    reason: fault.reason.to_string(),
                    row_index: row.index as u64,
                    attnum: column.attnum,
                    value_len: fault.value_len,
                });
            }
        }
    }
    Ok(cells)
}

struct Copied {
    accepted: u64,
    digest: String,
    namespace: String,
}

async fn copy_kept(
    conn: &mut PgConnection,
    pipe: &Pipe<'_>,
    run_id: &str,
    index: i32,
    table: &NativeTable,
    bound: &BoundTable,
    kept: &[Kept],
) -> Result<Copied, String> {
    let config = pipe.config;
    let generation = pipe.generation;
    let config_digest = pipe.config_digest;
    let type_digest = pipe.type_digest;
    let hook = pipe.hook;
    let schema_digest = pipe.schema_digest;
    let input_hex = pipe.input_hex;
    let input_bytes = pipe.input_bytes;
    let secrets = pipe.secrets;
    let batch_id = format!("t{index}.b0");
    let rows = kept.iter().map(|row| row.cells.clone()).collect();
    let batch = cell::assemble_table(&table.spec, rows, input_bytes, 1)
        .map_err(|err| config::redact(&err, secrets))?;
    let digest = canonical_digest(&batch);
    let columns = cell::copy_columns(&table.spec);
    let positions = cell::positions(batch.rows.len()).map_err(|err| err.to_string())?;
    let applied_pos = if positions.is_empty() {
        "0".to_string()
    } else {
        positions[positions.len() - 1].to_plain_string()
    };
    let quality = quality_preimage(
        &batch_id,
        batch.rows.len() as u64,
        0,
        input_hex,
        &[] as &[full_import::QualityReject],
    );
    let hashed = copy_sink::hash_batch(&batch, &columns, &positions, cell::copy_budgets())
        .map_err(|err| sink_err(&err))?;
    let mut payload_hasher = hashed.hasher.clone();
    payload_hasher.update(&quality);
    let payload_digest = config::hex_bytes(&payload_hasher.finalize());
    let mut tx = conn
        .begin()
        .await
        .map_err(|err| sql_err("begin", &err, secrets))?;
    let prepared = prepare_shadow(&mut tx, pipe, run_id, index, bound).await?;
    if prepared != bound.physical {
        let _ = tx.rollback().await;
        return Err("shadow name drift".to_string());
    }
    let statement = copy_statement(bound)?;
    let sent = match copy_sink::send_batch(
        &mut tx,
        &statement,
        &batch,
        &columns,
        &positions,
        cell::copy_budgets(),
    )
    .await
    {
        Ok(sent) => sent,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(sink_err(&err));
        }
    };
    if sent.hasher.clone().finalize().as_slice() != hashed.hasher.clone().finalize().as_slice() {
        let _ = tx.rollback().await;
        return Err("copy bytes changed between passes".to_string());
    }
    let wrote: String = sqlx::query_scalar(
        "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
    )
    .bind(config::SHARD)
    .bind(&config.owner_token)
    .bind(generation)
    .bind(run_id)
    .bind(index)
    .bind(config::SOURCE_ID)
    .bind(&bound.physical)
    .bind(batch_id)
    .bind(&payload_digest)
    .bind(config_digest)
    .bind(type_digest)
    .bind(hook)
    .bind(schema_digest)
    .bind(input_hex)
    .bind(&applied_pos)
    .bind(batch.rows.len() as i64)
    .bind(0_i64)
    .bind(Vec::<String>::new())
    .bind(Vec::<i64>::new())
    .bind(Vec::<i32>::new())
    .bind(Vec::<i64>::new())
    .bind(Vec::<String>::new())
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| sql_err("write_batch", &err, secrets))?;
    if wrote == "replay" {
        let _ = tx.rollback().await;
        return Err("unexpected replay".to_string());
    }
    tx.commit()
        .await
        .map_err(|err| sql_err("commit", &err, secrets))?;
    let namespace: String = sqlx::query_scalar(LOOKUP_SQL)
        .bind(config::SOURCE_ID)
        .bind(config_digest)
        .bind(1_i64)
        .bind("namespace")
        .bind(&bound.schema)
        .bind("")
        .bind("")
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| sql_err("namespace", &err, secrets))?;
    Ok(Copied {
        accepted: batch.rows.len() as u64,
        digest,
        namespace,
    })
}

async fn prepare_shadow(
    conn: &mut PgConnection,
    pipe: &Pipe<'_>,
    run_id: &str,
    index: i32,
    bound: &BoundTable,
) -> Result<String, String> {
    let config = pipe.config;
    let generation = pipe.generation;
    let config_digest = pipe.config_digest;
    let secrets = pipe.secrets;
    let names = bound
        .columns
        .iter()
        .map(|column| column.logical.clone())
        .collect::<Vec<_>>();
    let oids = bound
        .columns
        .iter()
        .map(|column| column.oid)
        .collect::<Vec<_>>();
    let typmods = bound
        .columns
        .iter()
        .map(|column| column.typmod)
        .collect::<Vec<_>>();
    let not_null = bound
        .columns
        .iter()
        .map(|column| column.not_null)
        .collect::<Vec<_>>();
    sqlx::query_scalar(
        "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(config::SHARD)
    .bind(&config.owner_token)
    .bind(generation)
    .bind(run_id)
    .bind(index)
    .bind(config::SOURCE_ID)
    .bind(config_digest)
    .bind(1_i64)
    .bind(&bound.schema)
    .bind(&bound.logical)
    .bind(&names)
    .bind(&oids)
    .bind(&typmods)
    .bind(&not_null)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| sql_err("prepare", &err, secrets))
}

fn copy_statement(bound: &BoundTable) -> Result<String, String> {
    let table = quote_physical_name(&bound.physical).map_err(|err| err.to_string())?;
    let mut list = String::from("\"_oo_pos\"");
    for column in &bound.columns {
        list.push_str(", ");
        list.push_str(&quote_physical_name(&column.physical).map_err(|err| err.to_string())?);
    }
    Ok(format!(
        "COPY _oo.{table} ({list}) FROM STDIN WITH (FORMAT binary)"
    ))
}

fn canonical_digest(batch: &TypedBatch) -> String {
    let mut hasher = Sha256::new();
    for row in &batch.rows {
        hasher.update(row.canonical_hash);
    }
    config::hex_bytes(&hasher.finalize())
}

async fn note_table(
    conn: &mut PgConnection,
    pipe: &Pipe<'_>,
    run_id: &str,
    index: i32,
    rows: u64,
    digest: &str,
) -> Result<(), String> {
    let config = pipe.config;
    let generation = pipe.generation;
    let secrets = pipe.secrets;
    sqlx::query("SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)")
        .bind(config::SHARD)
        .bind(&config.owner_token)
        .bind(generation)
        .bind(run_id)
        .bind(index)
        .bind(rows as i64)
        .bind(digest)
        .bind(config::SOURCE_ID)
        .execute(&mut *conn)
        .await
        .map_err(|err| sql_err("note", &err, secrets))?;
    Ok(())
}

async fn seal_run(
    conn: &mut PgConnection,
    pipe: &Pipe<'_>,
    run_id: &str,
    bound: &[BoundTable],
) -> Result<String, String> {
    let config = pipe.config;
    let generation = pipe.generation;
    let config_digest = pipe.config_digest;
    let type_digest = pipe.type_digest;
    let input_hex = pipe.input_hex;
    let schema_digest = pipe.schema_digest;
    let secrets = pipe.secrets;
    let schemas = bound
        .iter()
        .map(|table| table.schema.clone())
        .collect::<Vec<_>>();
    let tables = bound
        .iter()
        .map(|table| table.logical.clone())
        .collect::<Vec<_>>();
    let state: String = sqlx::query_scalar(
        "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(config::SHARD)
    .bind(&config.owner_token)
    .bind(generation)
    .bind(run_id)
    .bind(config::SOURCE_ID)
    .bind(1_i64)
    .bind(config_digest)
    .bind(type_digest)
    .bind(input_hex)
    .bind(schema_digest)
    .bind(&schemas)
    .bind(&tables)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| sql_err("seal", &err, secrets))?;
    Ok(state)
}

fn write_catalog(
    pipe: &Pipe<'_>,
    stored: &[StoredTable],
    run_id: &str,
    specs: &[TableSpec],
) -> Result<(), String> {
    let config = pipe.config;
    let target_generation = pipe.generation;
    let config_digest = pipe.config_digest;
    let input_hex = pipe.input_hex;
    let type_digest = pipe.type_digest;
    let schema_digest = pipe.schema_digest;
    let hook = pipe.hook;
    let mut tables = Vec::new();
    let mut quality_rows = Vec::new();
    let mut manifest = 0_u64;
    for item in stored {
        manifest += item.rejects.len() as u64;
        let spec = specs
            .iter()
            .find(|spec| spec.name == item.logical)
            .ok_or_else(|| format!("table {}", item.logical))?;
        let columns = spec
            .columns
            .iter()
            .zip(&item.columns)
            .map(|(column, stored_column)| {
                json!({
                    "logical_name": column.name,
                    "oid": column.oid(),
                    "typmod": column.typmod(),
                    "caveat": column.caveat,
                    "source_type": column.source_type,
                    "key": column.key,
                    "stored_physical": stored_column.physical,
                })
            })
            .collect::<Vec<_>>();
        let rejects = item
            .rejects
            .iter()
            .map(|reject| {
                json!({
                    "column": reject.column,
                    "reason": reject.reason,
                    "row_index": reject.row_index,
                    "attnum": reject.attnum,
                    "value_len": reject.value_len,
                })
            })
            .collect::<Vec<_>>();
        tables.push(json!({
            "logical_schema": spec.schema,
            "logical_table": item.logical,
            "accepted_rows": item.accepted,
            "canonical_digest": item.digest,
            "stored_namespace": item.namespace,
            "stored_physical": item.physical,
            "columns": columns,
            "rejects": rejects,
        }));
        quality_rows.push(json!({
            "physical_name": item.physical,
            "batch_id": item.batch_id,
            "kind": "batch",
            "reason": Json::Null,
            "value_len": Json::Null,
            "attnum": Json::Null,
            "row_index": Json::Null,
            "key_digest": Json::Null,
            "accepted_rows": item.accepted as i64,
            "rejected_rows": 0,
        }));
    }
    let document = json!({
        "protocol": oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL,
        "capture": "sealed_import",
        "engine": "mariadb",
        "source_id": config::SOURCE_ID,
        "config_digest": config_digest,
        "input_digest": input_hex,
        "type_digest": type_digest,
        "schema_digest": schema_digest,
        "tool_hook_digest": hook,
        "run_id": run_id,
        "source_generation": 1,
        "target_generation": target_generation,
        "state": "awaiting_publish",
        "hook_checked": false,
        "shadows_queried": false,
        "source_introspection": "not_used",
        "tables": tables,
        "quality": {
            "loss_concealed": false,
            "manifest_reject_count": manifest,
            "payload_includes_reject_lines": false,
            "stored_omits_manifest_rejects": manifest > 0,
            "stored_rejected_rows": 0,
            "stored_rows": quality_rows,
        },
    });
    config::write_json(&config.evidence.join("catalog.json"), &document)?;
    oo_builder::compile_sealed_import_catalog(
        &serde_json::to_string(&document).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn write_oracle(config: &Config, stored: &[StoredTable]) -> Result<(), String> {
    let file = OracleFile {
        tables: stored
            .iter()
            .map(|item| OracleTable {
                logical_table: item.logical.clone(),
                physical: item.physical.clone(),
                namespace: item.namespace.clone(),
                key_physical: item.key_physical.clone(),
                columns: item.columns.clone(),
                rows: item.rows.clone(),
            })
            .collect(),
    };
    config::write_json(&config.evidence.join("oracle-native.json"), &file)
}

async fn compare_stored(
    conn: &mut PgConnection,
    stored: &[StoredTable],
    secrets: &[&str],
) -> Result<Vec<Json>, String> {
    sqlx::query("SET TIME ZONE 'UTC'")
        .execute(&mut *conn)
        .await
        .map_err(|err| sql_err("time zone", &err, secrets))?;
    let mut mismatches = Vec::new();
    for item in stored {
        let read = read_shadow(
            conn,
            &item.columns,
            &item.physical,
            &item.key_physical,
            secrets,
        )
        .await?;
        if read.len() != item.rows.len() {
            mismatches.push(json!({ "table": item.logical, "native_rows": item.rows.len(), "stored_rows": read.len() }));
            continue;
        }
        for (row_index, (native_row, stored_row)) in item.rows.iter().zip(&read).enumerate() {
            for (column, (native_cell, stored_cell)) in
                item.columns.iter().zip(native_row.iter().zip(stored_row))
            {
                if !native::stored_match(
                    &column.compare,
                    column.scale,
                    native_cell.as_deref(),
                    stored_cell.as_deref(),
                ) {
                    mismatches.push(json!({
                        "table": item.logical,
                        "column": column.name,
                        "row": row_index,
                        "native": clip(native_cell),
                        "stored": clip(stored_cell),
                    }));
                }
            }
        }
    }
    Ok(mismatches)
}

async fn read_shadow(
    conn: &mut PgConnection,
    columns: &[OracleColumn],
    physical: &str,
    key_physical: &str,
    secrets: &[&str],
) -> Result<Vec<Vec<Option<String>>>, String> {
    let table = quote_physical_name(physical).map_err(|err| err.to_string())?;
    let key = quote_physical_name(key_physical).map_err(|err| err.to_string())?;
    let mut parts = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let quoted = quote_physical_name(&column.physical).map_err(|err| err.to_string())?;
        let expr = match column.compare.as_str() {
            "hex" => {
                format!("CASE WHEN {quoted} IS NULL THEN NULL ELSE encode({quoted}, 'hex') END")
            }
            "date" => format!(
                "CASE WHEN {quoted} IS NULL THEN NULL ELSE to_char({quoted}, 'YYYY-MM-DD') END"
            ),
            "time" => format!(
                "CASE WHEN {quoted} IS NULL THEN NULL ELSE to_char({quoted}, 'HH24:MI:SS.US') END"
            ),
            "timestamp" | "timestamptz" => format!(
                "CASE WHEN {quoted} IS NULL THEN NULL ELSE to_char({quoted}, 'YYYY-MM-DD HH24:MI:SS.US') END"
            ),
            _ => format!("CASE WHEN {quoted} IS NULL THEN NULL ELSE {quoted}::text END"),
        };
        // A cast keeps the source column name. PostgreSQL then sorts that text
        // output, so a numeric key orders as text. The alias leaves ORDER BY
        // on the table column.
        parts.push(format!("{expr} AS c{index}"));
    }
    let sql = format!(
        "SELECT {} FROM _oo.{table} ORDER BY {key}",
        parts.join(", ")
    );
    // quote_physical_name accepts only pn_/pt_/pc_ plus lowercase hex.
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| sql_err("shadow read", &err, secrets))?;
    let mut out = Vec::new();
    for row in rows {
        let mut cells = Vec::new();
        for index in 0..columns.len() {
            cells.push(
                row.try_get::<Option<String>, _>(index)
                    .map_err(|err| err.to_string())?,
            );
        }
        out.push(cells);
    }
    Ok(out)
}

async fn pg_gates(
    conn: &mut PgConnection,
    config: &Config,
    generation: i64,
    config_digest: &str,
    bound: &[BoundTable],
    secrets: &[&str],
) -> Phase {
    let mut detail = json!({});
    let lookup = sqlx::query_scalar::<_, String>(LOOKUP_SQL)
        .bind(config::SOURCE_ID)
        .bind(config_digest)
        .bind(1_i64)
        .bind("column")
        .bind(&bound[0].schema)
        .bind(&bound[0].logical)
        .bind(&bound[0].columns[0].logical)
        .fetch_one(&mut *conn)
        .await;
    let lookup_ok = lookup
        .as_ref()
        .map(|name| name == &bound[0].columns[0].physical)
        .unwrap_or(false);
    detail["mapping_lookup"] = json!({ "ok": lookup_ok, "error": lookup.as_ref().err().map(|err| sql_err("lookup", err, secrets)) });
    let missing = sqlx::query_scalar::<_, String>(LOOKUP_SQL)
        .bind(config::SOURCE_ID)
        .bind(config_digest)
        .bind(1_i64)
        .bind("column")
        .bind(&bound[0].schema)
        .bind(&bound[0].logical)
        .bind("no_such_column")
        .fetch_one(&mut *conn)
        .await;
    let identity_ok = missing
        .as_ref()
        .err()
        .map(|err| sql_err("identity", err, secrets))
        .is_some_and(|text| text.contains("OO024"));
    detail["identity"] = json!({
        "ok": identity_ok,
        "error": missing.as_ref().err().map(|err| sql_err("identity", err, secrets)),
    });
    let retire = retire_run(conn, config, generation, config_digest, secrets).await;
    detail["retire"] = retire.clone();
    let ok = lookup_ok && identity_ok && retire.get("ok").and_then(Json::as_bool) == Some(true);
    let phase = Phase::new("pg-gates", ok, detail);
    let _ = phase.save(&config.evidence);
    phase
}

async fn retire_run(
    conn: &mut PgConnection,
    config: &Config,
    generation: i64,
    config_digest: &str,
    secrets: &[&str],
) -> Json {
    // The probe records OO030, then rolls back. A committed generation-2 mapping
    // would make the sealed generation stale for the operator catalog.
    if let Err(err) = sqlx::query("BEGIN").execute(&mut *conn).await {
        return json!({ "ok": false, "error": sql_err("retire-begin", &err, secrets) });
    }
    let registered = async {
        register_one(
            conn,
            Reg {
                digest: config_digest,
                generation: 2,
                level: Level::Namespace,
                namespace: config::SCHEMA,
                table: "",
                column: "",
            },
            secrets,
        )
        .await?;
        let physical = register_one(
            conn,
            Reg {
                digest: config_digest,
                generation: 2,
                level: Level::Table,
                namespace: config::SCHEMA,
                table: "k_retire",
                column: "",
            },
            secrets,
        )
        .await?;
        let column = register_one(
            conn,
            Reg {
                digest: config_digest,
                generation: 2,
                level: Level::Column,
                namespace: config::SCHEMA,
                table: "k_retire",
                column: "id",
            },
            secrets,
        )
        .await?;
        let digest = oo_core::api::payload_sha256(b"oo_synth.k_retire\nid\t23\t-1\t\n");
        let run_id: String = sqlx::query_scalar(
            "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)::text",
        )
        .bind(config::SHARD)
        .bind(&config.owner_token)
        .bind(generation)
        .bind(config::SOURCE_ID)
        .bind(2_i64)
        .bind(config_digest)
        .bind(&digest)
        .bind(&digest)
        .bind(&digest)
        .bind(vec![config::SCHEMA.to_string()])
        .bind(vec!["k_retire".to_string()])
        .bind(vec![physical])
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| sql_err("retire-open", &err, secrets))?;
        let prepared: String = sqlx::query_scalar(
            "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(config::SHARD)
        .bind(&config.owner_token)
        .bind(generation)
        .bind(&run_id)
        .bind(0_i32)
        .bind(config::SOURCE_ID)
        .bind(config_digest)
        .bind(2_i64)
        .bind(config::SCHEMA)
        .bind("k_retire")
        .bind(vec!["id".to_string()])
        .bind(vec![23_i64])
        .bind(vec![-1_i32])
        .bind(vec![true])
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| sql_err("retire-prepare", &err, secrets))?;
        let _ = (column, prepared);
        sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
            .bind(config::SHARD)
            .bind(&config.owner_token)
            .bind(generation)
            .bind(&run_id)
            .execute(&mut *conn)
            .await
            .map_err(|err| sql_err("retire", &err, secrets))?;
        let sealed = sqlx::query_scalar::<_, String>(
            "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(config::SHARD)
        .bind(&config.owner_token)
        .bind(generation)
        .bind(&run_id)
        .bind(config::SOURCE_ID)
        .bind(2_i64)
        .bind(config_digest)
        .bind(&digest)
        .bind(&digest)
        .bind(&digest)
        .bind(vec![config::SCHEMA.to_string()])
        .bind(vec!["k_retire".to_string()])
        .fetch_one(&mut *conn)
        .await;
        match sealed {
            Ok(state) => Err(format!("seal returned {state}")),
            Err(err) => {
                let text = sql_err("retire-seal", &err, secrets);
                if text.contains("OO030") {
                    Ok(text)
                } else {
                    Err(text)
                }
            }
        }
    };
    let registered = registered.await;
    let rolled = sqlx::query("ROLLBACK").execute(&mut *conn).await;
    match (registered, rolled) {
        (Ok(text), Ok(_)) => json!({ "ok": true, "seal": text, "probe": "rolled_back" }),
        (Ok(_), Err(err)) => {
            json!({ "ok": false, "error": sql_err("retire-rollback", &err, secrets) })
        }
        (Err(err), Ok(_)) => json!({ "ok": false, "error": err }),
        (Err(err), Err(rollback)) => json!({
            "ok": false,
            "error": err,
            "rollback_error": sql_err("retire-rollback", &rollback, secrets)
        }),
    }
}

fn detach_schema(config: &Config) -> String {
    config
        .selection
        .as_ref()
        .and_then(|selection| selection.requested.first())
        .map(|object| object.schema.clone())
        .unwrap_or_else(|| config::SCHEMA.to_string())
}

fn detach_digest(config: &Config) -> String {
    match config.selection.as_ref() {
        Some(selection) => {
            let names = selection
                .requested
                .iter()
                .map(|object| object.schema.clone())
                .collect::<Vec<_>>();
            config::config_digest_for(&config.image_digest, &config::schema_label(&names))
        }
        None => config::config_digest(&config.image_digest),
    }
}

pub async fn detach(config: &Config) -> Phase {
    let oracle = match std::fs::read(config.evidence.join("oracle-native.json")) {
        Ok(bytes) => match serde_json::from_slice::<OracleFile>(&bytes) {
            Ok(file) => file,
            Err(err) => {
                return done(
                    config,
                    false,
                    json!({ "step": "oracle", "error": err.to_string(), "label": "source_detached_local_read" }),
                );
            }
        },
        Err(err) => {
            return done(
                config,
                false,
                json!({ "step": "oracle", "error": err.to_string(), "label": "source_detached_local_read" }),
            );
        }
    };
    let pg_user = match Secret::load(&config.pg_user_file) {
        Ok(secret) => secret,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "error": err, "label": "source_detached_local_read" }),
            );
        }
    };
    let pg_password = match Secret::load(&config.pg_password_file) {
        Ok(secret) => secret,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "error": err, "label": "source_detached_local_read" }),
            );
        }
    };
    let secrets = [pg_password.expose()];
    let mut conn = match connect_pg(config, pg_user.expose(), pg_password.expose()).await {
        Ok(conn) => conn,
        Err(err) => {
            return done(
                config,
                false,
                json!({ "error": config::redact(&err, &secrets), "label": "source_detached_local_read" }),
            );
        }
    };
    let compared = compare_stored(&mut conn, &oracle_as_stored(&oracle), &secrets).await;
    let lookup = if let Some(table) = oracle.tables.first() {
        sqlx::query_scalar::<_, String>(LOOKUP_SQL)
            .bind(config::SOURCE_ID)
            .bind(detach_digest(config))
            .bind(1_i64)
            .bind("table")
            .bind(detach_schema(config))
            .bind(&table.logical_table)
            .bind("")
            .fetch_one(&mut conn)
            .await
            .map(|name| name == table.physical)
            .unwrap_or(false)
    } else {
        false
    };
    match compared {
        Ok(mismatches) => done(
            config,
            mismatches.is_empty() && lookup,
            json!({
                "label": "source_detached_local_read",
                "mismatches": mismatches,
                "mapping_lookup": lookup,
                "tables": oracle.tables.len(),
            }),
        ),
        Err(err) => done(
            config,
            false,
            json!({ "label": "source_detached_local_read", "error": err, "mapping_lookup": lookup }),
        ),
    }
}

fn oracle_as_stored(file: &OracleFile) -> Vec<StoredTable> {
    file.tables
        .iter()
        .map(|table| StoredTable {
            logical: table.logical_table.clone(),
            physical: table.physical.clone(),
            namespace: table.namespace.clone(),
            key_physical: table.key_physical.clone(),
            columns: table.columns.clone(),
            rows: table.rows.clone(),
            accepted: table.rows.len() as u64,
            digest: String::new(),
            rejects: Vec::new(),
            unplanned: false,
            batch_id: String::new(),
        })
        .collect()
}

fn clip(value: &Option<String>) -> Json {
    match value {
        None => Json::Null,
        Some(text) => {
            let mut clipped = text.chars().take(120).collect::<String>();
            if clipped.len() < text.len() {
                clipped.push('…');
            }
            Json::String(clipped)
        }
    }
}

fn sink_err(err: &SinkError) -> String {
    match err {
        SinkError::Encode(error) => format!("encode {} {}", error.code, error.detail),
        SinkError::Driver(detail) => format!("driver {detail}"),
    }
}

fn sql_err(step: &str, err: &sqlx::Error, secrets: &[&str]) -> String {
    let code = err
        .as_database_error()
        .and_then(|db| db.code().map(|code| code.to_string()))
        .unwrap_or_default();
    config::redact(&format!("{step} {code} {err}"), secrets)
}

fn requested_allowed(config: &Config) -> Result<(), String> {
    let selection = config
        .selection
        .as_ref()
        .ok_or_else(|| "approved_objects required".to_string())?;
    for object in &selection.requested {
        if !config::object_allowed(&selection.approved, object) {
            return Err(format!("unapproved: {}.{}", object.schema, object.table));
        }
    }
    Ok(())
}

struct CaptureNote<'a> {
    started: &'a str,
    finished: &'a str,
    role: &'a str,
    secret_ref: &'a str,
    cancel: Option<&'a str>,
}

fn write_capture(
    config: &Config,
    connection_id: u64,
    control_id: u64,
    note: CaptureNote<'_>,
) -> Result<(), String> {
    config::write_json(
        &config.evidence.join("capture-session.json"),
        &json!({
            "connection_id": connection_id,
            "control_connection_id": control_id,
            "started": note.started,
            "finished": note.finished,
            "role": note.role,
            "secret_ref": note.secret_ref,
            "account": "oo_reader",
            "cancel": note.cancel,
        }),
    )
}

fn refuse(config: &Config, err: &str, connection_id: Option<u64>) -> Phase {
    refuse_with(config, err, connection_id, json!({}))
}

fn refuse_with(config: &Config, err: &str, connection_id: Option<u64>, extra: Json) -> Phase {
    let step = err.split(':').next().unwrap_or("native").trim();
    let mut detail = json!({
        "step": step,
        "error": err,
        "selection": true,
        "connection_id": connection_id,
        "catalog_written": false,
        "sealed": "",
    });
    if let (Some(object), Some(more)) = (detail.as_object_mut(), extra.as_object()) {
        for (key, value) in more {
            object.insert(key.clone(), value.clone());
        }
    }
    done(config, false, detail)
}

fn stop_fields(stop: Option<&native::StreamStop>, control_id: u64) -> Json {
    match stop {
        Some(item) => json!({
            "control_connection_id": control_id,
            "killed": item.killed,
            "rows_seen": item.rows_seen,
            "rows_accepted": item.rows_accepted,
            "cancel": item.cancel,
        }),
        None => json!({ "control_connection_id": control_id }),
    }
}

async fn run_selected(config: &Config) -> Phase {
    if let Err(err) = requested_allowed(config) {
        return refuse(config, &err, None);
    }
    let selection = config.selection.as_ref().expect("selection");
    let role = selection.role.clone();
    let secret_ref = selection.secret_ref.clone();
    let budget = selection.budget.clone();
    let mut reader = match native::Reader::open(config).await {
        Ok(opened) => opened,
        Err(err) => return refuse(config, &err, None),
    };
    let connection_id = reader.business_id;
    let control_id = reader.control_id;
    let started = reader.started.clone();
    let specs = match native::load_specs(&mut reader.business, config).await {
        Ok(specs) => specs,
        Err(err) => {
            let finished = native::server_now(&mut reader.control)
                .await
                .unwrap_or_default();
            let _ = write_capture(
                config,
                connection_id,
                control_id,
                CaptureNote {
                    started: &started,
                    finished: &finished,
                    role: &role,
                    secret_ref: &secret_ref,
                    cancel: None,
                },
            );
            reader.disconnect().await;
            return refuse_with(
                config,
                &err,
                Some(connection_id),
                stop_fields(None, control_id),
            );
        }
    };
    let mut tally = native::Tally { rows: 0, bytes: 0 };
    let mut captured = Vec::new();
    let mut stop = None;
    for spec in &specs {
        match native::read_bounded(&mut reader, spec, &budget, &mut tally).await {
            Ok(table) => captured.push(table),
            Err(item) => {
                stop = Some(item);
                break;
            }
        }
    }
    let finished = native::server_now(&mut reader.control)
        .await
        .unwrap_or_default();
    let cancel = stop.as_ref().and_then(|item| item.cancel.clone());
    let _ = write_capture(
        config,
        connection_id,
        control_id,
        CaptureNote {
            started: &started,
            finished: &finished,
            role: &role,
            secret_ref: &secret_ref,
            cancel: cancel.as_deref(),
        },
    );
    reader.disconnect().await;
    if let Some(item) = stop {
        if captured.is_empty() {
            return refuse_with(
                config,
                &item.error,
                Some(connection_id),
                stop_fields(Some(&item), control_id),
            );
        }
        return selected_pg(
            config,
            connection_id,
            control_id,
            &captured,
            Some(item.error.clone()),
            Some(&item),
        )
        .await;
    }
    if tally.rows == 0 {
        return refuse_with(
            config,
            "no_data: selection returned no rows",
            Some(connection_id),
            stop_fields(None, control_id),
        );
    }
    selected_pg(config, connection_id, control_id, &captured, None, None).await
}

async fn selected_pg(
    config: &Config,
    connection_id: u64,
    control_id: u64,
    native: &[NativeTable],
    retire_reason: Option<String>,
    stop: Option<&native::StreamStop>,
) -> Phase {
    let specs: Vec<TableSpec> = native.iter().map(|table| table.spec.clone()).collect();
    let preimage = native::input_preimage(native);
    let digest = Sha256::digest(preimage.as_bytes());
    let mut input_bytes = [0u8; 32];
    input_bytes.copy_from_slice(&digest);
    let input_hex = config::hex_bytes(&input_bytes);
    let schema_digest = cell::schema_digest(&specs);
    let type_digest = cell::type_digest(&specs);
    let schema_names = specs
        .iter()
        .map(|spec| spec.schema.clone())
        .collect::<Vec<_>>();
    let config_digest =
        config::config_digest_for(&config.image_digest, &config::schema_label(&schema_names));
    let hook = hook_digest();
    if retire_reason.is_none() {
        let _ = config::write_bytes(
            &config.evidence.join("input-preimage.txt"),
            preimage.as_bytes(),
        );
    }
    let pg_user = match Secret::load(&config.pg_user_file) {
        Ok(secret) => secret,
        Err(err) => return refuse(config, &format!("pg-user: {err}"), Some(connection_id)),
    };
    let pg_password = match Secret::load(&config.pg_password_file) {
        Ok(secret) => secret,
        Err(err) => return refuse(config, &format!("pg-password: {err}"), Some(connection_id)),
    };
    let secrets = [pg_password.expose()];
    let mut conn = match connect_pg(config, pg_user.expose(), pg_password.expose()).await {
        Ok(conn) => conn,
        Err(err) => {
            return refuse(
                config,
                &format!("pg-connect: {}", config::redact(&err, &secrets)),
                Some(connection_id),
            );
        }
    };
    if let Err(err) = install(&mut conn).await {
        return refuse(
            config,
            &format!("install: {}", config::redact(&err, &secrets)),
            Some(connection_id),
        );
    }
    let target_generation = match adopt(&mut conn, &config.owner_token).await {
        Ok(generation) => generation,
        Err(err) => {
            return refuse(
                config,
                &format!("adopt: {}", config::redact(&err, &secrets)),
                Some(connection_id),
            );
        }
    };
    let pipe = Pipe {
        config,
        generation: target_generation,
        config_digest: &config_digest,
        type_digest: &type_digest,
        hook: &hook,
        schema_digest: &schema_digest,
        input_hex: &input_hex,
        input_bytes,
        secrets: &secrets,
    };
    let bound = match register(&mut conn, &config_digest, 1, &secrets, &specs).await {
        Ok(bound) => bound,
        Err(err) => return refuse(config, &format!("register: {err}"), Some(connection_id)),
    };
    if let Err(err) =
        bind_session(&mut conn, &config.owner_token, target_generation, &secrets).await
    {
        return refuse(config, &format!("bind: {err}"), Some(connection_id));
    }
    let run_id = match open_run(&mut conn, &pipe, &bound).await {
        Ok(run_id) => run_id,
        Err(err) => return refuse(config, &format!("open: {err}"), Some(connection_id)),
    };
    let mut stored = Vec::new();
    let mut faults = Vec::new();
    for (index, table) in native.iter().enumerate() {
        match store_table(
            &mut conn,
            &pipe,
            &run_id,
            index as i32,
            table,
            &bound[index],
        )
        .await
        {
            Ok(item) => stored.push(item),
            Err(err) => {
                faults.push(json!({ "table": table.spec.name, "error": err }));
                break;
            }
        }
    }
    let must_retire = retire_reason.is_some() || !faults.is_empty() || stored.len() != bound.len();
    if must_retire {
        let retired = retire_open(&mut conn, config, target_generation, &run_id, &secrets).await;
        let seal_text = match seal_run(&mut conn, &pipe, &run_id, &bound).await {
            Ok(state) => format!("seal returned {state}"),
            Err(err) => err,
        };
        let seal_blocked = seal_text.contains("OO030");
        let reason = retire_reason.unwrap_or_else(|| "partial cleanup".to_string());
        let step = reason
            .split(':')
            .next()
            .unwrap_or("partial")
            .trim()
            .to_string();
        let retire_error = retired.as_ref().err().cloned();
        return done(
            config,
            false,
            json!({
                "step": step,
                "error": reason,
                "selection": true,
                "connection_id": connection_id,
                "control_connection_id": control_id,
                "killed": stop.map(|item| item.killed),
                "rows_seen": stop.map(|item| item.rows_seen),
                "rows_accepted": stop.map(|item| item.rows_accepted),
                "cancel": stop.and_then(|item| item.cancel.clone()),
                "run_id": run_id,
                "catalog_written": false,
                "sealed": "",
                "retired": retired.is_ok(),
                "retire_error": retire_error,
                "seal_after_retire": seal_text,
                "seal_blocked": seal_blocked,
                "stored_tables": stored.iter().map(|item| item.logical.clone()).collect::<Vec<_>>(),
                "faults": faults,
                "input_digest": input_hex,
                "schema_digest": schema_digest,
                "type_digest": type_digest,
                "config_digest": config_digest,
                "target_generation": target_generation,
            }),
        );
    }
    let sealed = match seal_run(&mut conn, &pipe, &run_id, &bound).await {
        Ok(state) => state,
        Err(err) => {
            let retired =
                retire_open(&mut conn, config, target_generation, &run_id, &secrets).await;
            return done(
                config,
                false,
                json!({
                    "step": "seal",
                    "error": err,
                    "selection": true,
                    "connection_id": connection_id,
                    "control_connection_id": control_id,
                    "killed": stop.map(|item| item.killed),
                    "rows_seen": stop.map(|item| item.rows_seen),
                    "cancel": stop.and_then(|item| item.cancel.clone()),
                    "run_id": run_id,
                    "catalog_written": false,
                    "sealed": "",
                    "retired": retired.is_ok(),
                    "input_digest": input_hex,
                }),
            );
        }
    };
    let mut oracle_ok = false;
    let mut mismatches = Vec::new();
    let mut catalog_written = false;
    if sealed == "awaiting_publish" {
        match compare_stored(&mut conn, &stored, &secrets).await {
            Ok(found) => {
                mismatches = found;
                oracle_ok = mismatches.is_empty();
            }
            Err(err) => faults.push(json!({ "step": "oracle", "error": err })),
        }
        if let Err(err) = write_catalog(&pipe, &stored, &run_id, &specs) {
            faults.push(json!({ "step": "catalog", "error": err }));
        } else {
            catalog_written = true;
        }
        if let Err(err) = write_oracle(config, &stored) {
            faults.push(json!({ "step": "oracle-file", "error": err }));
        }
    }
    let pg_gates = pg_gates(
        &mut conn,
        config,
        target_generation,
        &config_digest,
        &bound,
        &secrets,
    )
    .await;
    if !pg_gates.ok {
        faults.push(pg_gates.detail.clone());
    }
    let ok = faults.is_empty() && oracle_ok && sealed == "awaiting_publish" && catalog_written;
    done(
        config,
        ok,
        json!({
            "selection": true,
            "connection_id": connection_id,
            "control_connection_id": control_id,
            "run_id": run_id,
            "target_generation": target_generation,
            "input_digest": input_hex,
            "schema_digest": schema_digest,
            "type_digest": type_digest,
            "config_digest": config_digest,
            "sealed": sealed,
            "oracle_ok": oracle_ok,
            "catalog_written": catalog_written,
            "mismatches": mismatches,
            "faults": faults,
            "tables": stored.iter().map(|item| json!({
                "table": item.logical,
                "physical": item.physical,
                "accepted": item.accepted,
                "rejects": item.rejects.len(),
                "unplanned": item.unplanned,
            })).collect::<Vec<_>>(),
        }),
    )
}

async fn retire_open(
    conn: &mut PgConnection,
    config: &Config,
    generation: i64,
    run_id: &str,
    secrets: &[&str],
) -> Result<(), String> {
    sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
        .bind(config::SHARD)
        .bind(&config.owner_token)
        .bind(generation)
        .bind(run_id)
        .execute(&mut *conn)
        .await
        .map_err(|err| sql_err("retire", &err, secrets))?;
    Ok(())
}

fn done(config: &Config, ok: bool, detail: Json) -> Phase {
    let name = detail
        .get("label")
        .and_then(Json::as_str)
        .map(|_| "detach")
        .unwrap_or("import");
    let phase = Phase::new(name, ok, detail);
    let _ = phase.save(&config.evidence);
    phase
}
