//! Sealed full import on one PostgreSQL session.

use std::fs;
use std::path::Path;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::ClientConfig;
use serde_json::{json, Value};
use tokio_postgres::Client;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::catalog::{MappedTable, NAMESPACE, SOURCE_GENERATION, SOURCE_ID};
use crate::names::{quality_preimage, row_line, sha256_hex, sha256_raw};

pub struct PgTarget {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: String,
    pub ca_file: String,
}

pub struct ImportOutput {
    pub run_id: String,
    pub target_generation: i64,
    pub accepted: Vec<u64>,
    pub canonical: Vec<String>,
    pub payload: Vec<String>,
    pub identity: Value,
    pub quality_rows: Vec<Value>,
}

pub struct ImportRequest<'a> {
    pub target: &'a PgTarget,
    pub config_digest: &'a str,
    pub type_digest: &'a str,
    pub input_digest: &'a str,
    pub schema_digest: &'a str,
    pub hook_digest: &'a str,
    pub namespace_physical: &'a str,
    pub namespace_digest: &'a [u8; 32],
    pub mapped: &'a [MappedTable],
    pub decoded: &'a [Vec<Vec<Option<String>>>],
    pub login: &'a str,
    pub login_password_file: &'a str,
}

pub async fn import_tables(request: ImportRequest<'_>) -> Result<ImportOutput, String> {
    let ImportRequest {
        target,
        config_digest,
        type_digest,
        input_digest,
        schema_digest,
        hook_digest,
        namespace_physical,
        namespace_digest,
        mapped,
        decoded,
        login,
        login_password_file,
    } = request;
    let client = connect(target).await?;
    let generation: i64 = scalar(
        &client,
        "SELECT _oo.adopt_target_generation($1, $2::text::uuid)",
        &["sr_w45", "2c023948-0862-4dae-ae8f-6fc30241db62"],
    )
    .await?;
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, $2::text::uuid, $3)",
            &[&"sr_w45", &"2c023948-0862-4dae-ae8f-6fc30241db62", &generation],
        )
        .await
        .map_err(|err| show_err(&err))?;
    register(
        &client,
        Register {
            config_digest,
            level: "namespace",
            namespace: NAMESPACE,
            table: "",
            column: "",
            digest: namespace_digest,
            expected: namespace_physical,
        },
    )
    .await?;
    for table in mapped {
        register(
            &client,
            Register {
                config_digest,
                level: "table",
                namespace: NAMESPACE,
                table: table.plan.name,
                column: "",
                digest: &table.digest,
                expected: &table.physical,
            },
        )
        .await?;
        for column in &table.columns {
            register(
                &client,
                Register {
                    config_digest,
                    level: "column",
                    namespace: NAMESPACE,
                    table: table.plan.name,
                    column: column.plan.name,
                    digest: &column.digest,
                    expected: &column.physical,
                },
            )
            .await?;
        }
    }
    let schemas: Vec<String> = mapped.iter().map(|_| NAMESPACE.to_string()).collect();
    let names: Vec<String> = mapped.iter().map(|table| table.plan.name.to_string()).collect();
    let physicals: Vec<String> = mapped.iter().map(|table| table.physical.clone()).collect();
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &[
                &"sr_w45",
                &"2c023948-0862-4dae-ae8f-6fc30241db62",
                &generation,
                &SOURCE_ID,
                &SOURCE_GENERATION,
                &config_digest,
                &type_digest,
                &input_digest,
                &schema_digest,
                &schemas,
                &names,
                &physicals,
            ],
        )
        .await
        .map_err(|err| show_err(&err))?
        .get(0);
    let mut accepted = Vec::new();
    let mut canonical = Vec::new();
    let mut payload = Vec::new();
    for (index, table) in mapped.iter().enumerate() {
        let rows = decoded.get(index).ok_or_else(|| format!("missing rows {}", table.plan.name))?;
        let oids: Vec<i64> = table.columns.iter().map(|column| column.plan.oid).collect();
        let typmods: Vec<i32> = table.columns.iter().map(|column| column.plan.typmod).collect();
        let not_null: Vec<bool> = table.columns.iter().map(|column| column.plan.not_null).collect();
        let column_names: Vec<String> = table.columns.iter().map(|column| column.plan.name.to_string()).collect();
        let table_index = i32::try_from(index).map_err(|err| show_err(&err))?;
        let prepared: String = client
            .query_one(
                "SELECT _oo.full_import_prepare_shadow($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
                &[
                    &"sr_w45",
                    &"2c023948-0862-4dae-ae8f-6fc30241db62",
                    &generation,
                    &run_id,
                    &table_index,
                    &SOURCE_ID,
                    &config_digest,
                    &SOURCE_GENERATION,
                    &NAMESPACE,
                    &table.plan.name,
                    &column_names,
                    &oids,
                    &typmods,
                    &not_null,
                ],
            )
            .await
            .map_err(|err| format!("prepare {}: {}", table.plan.name, show_err(&err)))?
            .get(0);
        if prepared != table.physical {
            return Err(format!("shadow {} != {}", prepared, table.physical));
        }
        let (count, canonical_digest, payload_digest, applied_pos) =
            write_rows(&client, table, rows, input_digest, index).await?;
        let empty_text: Vec<String> = Vec::new();
        let empty_i64: Vec<i64> = Vec::new();
        let empty_i32: Vec<i32> = Vec::new();
        let receipt: String = client
            .query_one(
                "SELECT _oo.full_import_write_batch($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::text::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
                &[
                    &"sr_w45",
                    &"2c023948-0862-4dae-ae8f-6fc30241db62",
                    &generation,
                    &run_id,
                    &table_index,
                    &SOURCE_ID,
                    &table.physical,
                    &format!("t{index}.b0"),
                    &payload_digest,
                    &config_digest,
                    &type_digest,
                    &hook_digest,
                    &schema_digest,
                    &input_digest,
                    &applied_pos,
                    &(count as i64),
                    &0_i64,
                    &empty_text,
                    &empty_i64,
                    &empty_i32,
                    &empty_i64,
                    &empty_text,
                ],
            )
            .await
            .map_err(|err| format!("write_batch {}: {}", table.plan.name, show_err(&err)))?
            .get(0);
        if receipt.is_empty() {
            return Err(format!("empty receipt {}", table.plan.name));
        }
        client
            .execute(
                "SELECT _oo.full_import_note_table($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8)",
                &[
                    &"sr_w45",
                    &"2c023948-0862-4dae-ae8f-6fc30241db62",
                    &generation,
                    &run_id,
                    &table_index,
                    &(count as i64),
                    &canonical_digest,
                    &SOURCE_ID,
                ],
            )
            .await
            .map_err(|err| format!("note {}: {}", table.plan.name, show_err(&err)))?;
        accepted.push(count);
        canonical.push(canonical_digest);
        payload.push(payload_digest);
    }
    let state: String = client
        .query_one(
            "SELECT _oo.full_import_seal_run($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
            &[
                &"sr_w45",
                &"2c023948-0862-4dae-ae8f-6fc30241db62",
                &generation,
                &run_id,
                &SOURCE_ID,
                &SOURCE_GENERATION,
                &config_digest,
                &type_digest,
                &input_digest,
                &schema_digest,
                &schemas,
                &names,
            ],
        )
        .await
        .map_err(|err| format!("seal: {}", show_err(&err)))?
        .get(0);
    if state != "awaiting_publish" {
        return Err(format!("seal state {state}"));
    }
    let identity_text: String = client
        .query_one(
            "SELECT _oo.full_import_sealed_identity($1, $2, $3)::text",
            &[&SOURCE_ID, &config_digest, &SOURCE_GENERATION],
        )
        .await
        .map_err(|err| format!("identity: {}", show_err(&err)))?
        .get(0);
    let identity: Value = serde_json::from_str(&identity_text).map_err(|err| show_err(&err))?;
    let quality_rows = reshape_quality(identity.get("quality").unwrap_or(&Value::Null))?;
    create_login(&client, login, login_password_file, &target.database).await?;
    Ok(ImportOutput {
        run_id,
        target_generation: generation,
        accepted,
        canonical,
        payload,
        identity,
        quality_rows,
    })
}

pub async fn retire_open(target: &PgTarget, generation: i64, run_id: &str) -> Result<(), String> {
    let client = connect(target).await?;
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, $2::text::uuid, $3)",
            &[&"sr_w45", &"2c023948-0862-4dae-ae8f-6fc30241db62", &generation],
        )
        .await
        .map_err(|err| show_err(&err))?;
    client
        .execute(
            "SELECT _oo.full_import_retire_run($1, $2::text::uuid, $3, $4::text::uuid)",
            &[&"sr_w45", &"2c023948-0862-4dae-ae8f-6fc30241db62", &generation, &run_id],
        )
        .await
        .map_err(|err| show_err(&err))?;
    Ok(())
}

fn reshape_quality(value: &Value) -> Result<Vec<Value>, String> {
    let rows = value.as_array().ok_or("quality is not an array")?;
    let mut out = Vec::new();
    for row in rows {
        if row.get("kind").and_then(Value::as_str) != Some("batch") {
            return Err(format!("unexpected quality row {row}"));
        }
        out.push(json!({
            "physical_name": row.get("physical_name").cloned().unwrap_or(Value::Null),
            "batch_id": row.get("batch_id").cloned().unwrap_or(Value::Null),
            "kind": "batch",
            "reason": null,
            "value_len": null,
            "attnum": null,
            "row_index": null,
            "key_digest": null,
            "accepted_rows": row.get("accepted_rows").cloned().unwrap_or(Value::Null),
            "rejected_rows": row.get("rejected_rows").cloned().unwrap_or(Value::Null)
        }));
    }
    Ok(out)
}

async fn write_rows(
    client: &Client,
    table: &MappedTable,
    rows: &[Vec<Option<String>>],
    input_digest: &str,
    index: usize,
) -> Result<(u64, String, String, String), String> {
    let mut payload = String::new();
    let mut row_hashes = Vec::new();
    let quoted_cols = table
        .columns
        .iter()
        .map(|column| format!("\"{}\"", column.physical))
        .collect::<Vec<_>>()
        .join(", ");
    for (offset, row) in rows.iter().enumerate() {
        if row.len() != table.columns.len() {
            return Err(format!("{} row width", table.plan.name));
        }
        let position = offset as u64 + 1;
        let cells = row.iter().map(|cell| cell.as_deref()).collect::<Vec<_>>();
        let line = row_line(position, &cells);
        row_hashes.extend_from_slice(&sha256_raw(line.as_bytes()));
        payload.push_str(&line);
        let mut values = vec![position.to_string()];
        for (cell, column) in row.iter().zip(table.columns.iter()) {
            values.push(match cell {
                None => "NULL".to_string(),
                Some(text) => format!("{}::{}", quote_literal(text), column.plan.pg_type),
            });
        }
        let sql = format!(
            "INSERT INTO _oo.\"{}\" (\"_oo_pos\", {quoted_cols}) VALUES ({})",
            table.physical,
            values.join(", ")
        );
        client.batch_execute(&sql).await.map_err(|err| format!("insert {}: {}", table.plan.name, show_err(&err)))?;
    }
    let batch_id = format!("t{index}.b0");
    let count = rows.len() as u64;
    let mut mixed = payload.into_bytes();
    mixed.extend(quality_preimage(&batch_id, count, 0, input_digest));
    let applied = if count == 0 { "0".to_string() } else { count.to_string() };
    Ok((count, sha256_hex(&row_hashes), sha256_hex(&mixed), applied))
}

fn quote_literal(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

struct Register<'a> {
    config_digest: &'a str,
    level: &'a str,
    namespace: &'a str,
    table: &'a str,
    column: &'a str,
    digest: &'a [u8],
    expected: &'a str,
}

async fn register(client: &Client, item: Register<'_>) -> Result<(), String> {
    let Register {
        config_digest,
        level,
        namespace,
        table,
        column,
        digest,
        expected,
    } = item;
    let status: String = client
        .query_one(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &SOURCE_ID,
                &config_digest,
                &SOURCE_GENERATION,
                &level,
                &namespace,
                &table,
                &column,
                &digest,
            ],
        )
        .await
        .map_err(|err| format!("register {level} {table}.{column}: {}", show_err(&err)))?
        .get(0);
    if status != "inserted" && status != "replay" {
        return Err(format!("register status {status}"));
    }
    let found: String = client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &[&SOURCE_ID, &config_digest, &SOURCE_GENERATION, &level, &namespace, &table, &column],
        )
        .await
        .map_err(|err| show_err(&err))?
        .get(0);
    if found != expected {
        return Err(format!("lookup {found} != {expected}"));
    }
    Ok(())
}

async fn create_login(client: &Client, login: &str, password_file: &str, database: &str) -> Result<(), String> {
    if !login.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return Err("login name".to_string());
    }
    let password = read_secret(Path::new(password_file))?;
    let sql = format!(
        "CREATE ROLE {login} LOGIN PASSWORD {} NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION",
        quote_literal(&password)
    );
    client.batch_execute(&sql).await.map_err(|err| format!("login: {}", show_err(&err)))?;
    client
        .batch_execute(&format!("GRANT CONNECT ON DATABASE {database} TO {login}"))
        .await
        .map_err(|err| format!("connect grant: {}", show_err(&err)))?;
    Ok(())
}

fn show_err(err: &impl std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    text
}

async fn scalar(client: &Client, sql: &str, owner: &[&str; 2]) -> Result<i64, String> {
    client
        .query_one(sql, &[&owner[0], &owner[1]])
        .await
        .map_err(|err| show_err(&err))
        .map(|row| row.get(0))
}

pub async fn lock_table(target: &PgTarget, physical: &str) -> Result<Client, String> {
    let client = connect(target).await?;
    client.batch_execute("BEGIN").await.map_err(|err| show_err(&err))?;
    client
        .batch_execute(&format!("LOCK TABLE _oo.\"{physical}\" IN ACCESS EXCLUSIVE MODE"))
        .await
        .map_err(|err| show_err(&err))?;
    Ok(client)
}

pub async fn unlock(client: Client) {
    let _ = client.batch_execute("ROLLBACK").await;
}

async fn connect(target: &PgTarget) -> Result<Client, String> {
    let password = read_secret(Path::new(&target.password_file))?;
    let ca = fs::read(&target.ca_file).map_err(|err| show_err(&err))?;
    let certificate = CertificateDer::from_pem_slice(&ca).map_err(|err| show_err(&err))?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).map_err(|err| show_err(&err))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|err| show_err(&err))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = MakeRustlsConnect::new(config);
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&target.host);
    cfg.port(target.port);
    cfg.dbname(&target.database);
    cfg.user(&target.user);
    cfg.password(password);
    cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
    cfg.application_name("starrocks-preview");
    cfg.connect_timeout(Duration::from_secs(30));
    let (client, connection) = cfg.connect(connector).await.map_err(|err| show_err(&err))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute("SET statement_timeout = '120s'")
        .await
        .map_err(|err| show_err(&err))?;
    Ok(client)
}

fn read_secret(path: &Path) -> Result<String, String> {
    let mut bytes = fs::read(path).map_err(|err| show_err(&err))?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|err| show_err(&err))
}
