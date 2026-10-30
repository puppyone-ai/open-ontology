use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use arrow_ipc::writer::StreamWriter;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row};
use oo_connectors::{
    mysql::{self, MysqlConfig, SelectedTable},
    types::RunBinding,
};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use tokio::time::timeout;

use crate::{PreviewError, capture::arrow_description, source_sql::secret_file as read_secret};

type Result<T> = std::result::Result<T, PreviewError>;

struct Input {
    source: MysqlConfig,
    secret_file: PathBuf,
    audit: Option<(String, PathBuf)>,
    selected: SelectedTable,
    source_id: String,
    generation: u64,
    owner: String,
    plaintext: bool,
    max_rows: u64,
    max_bytes: u64,
    deadline_ms: u64,
    statement_ms: u64,
    chunking: bool,
    type_policy: mysql::values::TypePolicy,
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| PreviewError::new("config", "full_field"))
}

fn bounded(value: &Value, key: &str, high: u64) -> Result<u64> {
    value[key]
        .as_u64()
        .filter(|n| *n > 0 && *n <= high)
        .ok_or_else(|| PreviewError::new("budget", "full_range"))
}

fn absolute(value: &Value, key: &str) -> Result<PathBuf> {
    let path = PathBuf::from(text(value, key)?);
    if !path.is_absolute() {
        return Err(PreviewError::new("config", "path"));
    }
    Ok(path)
}

fn keys(value: &Value, allowed: &[&str]) -> Result<()> {
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "keys"))?;
    if map.len() != allowed.len() || !allowed.iter().all(|k| map.contains_key(*k)) {
        return Err(PreviewError::new("config", "keys"));
    }
    Ok(())
}

fn parse(value: &Value) -> Result<Input> {
    let mut fields = value.clone();
    let type_policy = match fields.as_object_mut().and_then(|m|m.remove("type_policy")) {
        None => mysql::values::TypePolicy::default(),
        Some(policy) => serde_json::from_value(policy).map_err(|_| PreviewError::new("config","type_policy"))?,
    };
    let chunking = match fields.as_object_mut().and_then(|m| m.remove("chunking")) {
        None => false,
        Some(Value::Bool(value)) => value,
        _ => return Err(PreviewError::new("config", "chunking")),
    };
    keys(
        &fields,
        &[
            "protocol",
            "source_id",
            "generation",
            "owner",
            "secret_ref",
            "connection",
            "audit",
            "approved_objects",
            "table",
            "budgets",
        ],
    )?;
    if value["protocol"] != "oo.mysql-full/v1" {
        return Err(PreviewError::new("config", "protocol"));
    }
    let connection = &value["connection"];
    keys(
        connection,
        &[
            "host",
            "port",
            "database",
            "user",
            "secret_file",
            "tls",
            "ca_file",
            "plaintext_authorized",
        ],
    )?;
    let plaintext = match text(connection, "tls")? {
        "required" if connection["plaintext_authorized"] == false => false,
        "disabled"
            if connection["plaintext_authorized"] == true && connection["ca_file"].is_null() =>
        {
            true
        }
        _ => return Err(PreviewError::new("tls", "policy")),
    };
    let audit = &value["audit"];
    let audit_account = if audit == &json!({"mode":"grant_enforced"}) {
        None
    } else {
        keys(audit, &["user", "secret_ref", "secret_file"])?;
        oo_connectors::SecretRef::new(text(audit, "secret_ref")?)
            .map_err(|_| PreviewError::new("config", "secret_ref"))?;
        Some((
            text(audit, "user")?.to_owned(),
            absolute(audit, "secret_file")?,
        ))
    };
    let table = &value["table"];
    keys(table, &["schema", "table", "columns"])?;
    let schema = text(table, "schema")?;
    let name = text(table, "table")?;
    let columns: Vec<String> = table["columns"]
        .as_array()
        .ok_or_else(|| PreviewError::new("config", "columns"))?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| mysql::mysql_ident(s))
                .map(str::to_owned)
                .ok_or_else(|| PreviewError::new("ident", "column"))
        })
        .collect::<Result<_>>()?;
    if !mysql::mysql_ident(schema)
        || !mysql::mysql_ident(name)
        || columns.iter().collect::<BTreeSet<_>>().len() != columns.len()
    {
        return Err(PreviewError::new("ident", "table"));
    }
    let approved = value["approved_objects"]
        .as_array()
        .ok_or_else(|| PreviewError::new("privilege", "unapproved"))?;
    if !approved.iter().any(|a| a == table) {
        return Err(PreviewError::new("privilege", "unapproved"));
    }
    let budget = &value["budgets"];
    keys(
        budget,
        &[
            "connect_timeout_ms",
            "statement_timeout_ms",
            "deadline_ms",
            "max_rows",
            "max_bytes",
        ],
    )?;
    let source_id = text(value, "source_id")?.to_owned();
    if !mysql::mysql_ident(&source_id) {
        return Err(PreviewError::new("config", "source_id"));
    }
    let owner = text(value, "owner")?.to_owned();
    if owner.len() != 36
        || !owner.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err(PreviewError::new("config", "owner"));
    }
    let source = MysqlConfig {
        mysql_host: text(connection, "host")?.to_owned(),
        mysql_port: u16::try_from(bounded(connection, "port", 65535)?)
            .map_err(|_| PreviewError::new("config", "port"))?,
        mysql_database: text(connection, "database")?.to_owned(),
        mysql_user: text(connection, "user")?.to_owned(),
        mysql_ca_file: if plaintext {
            PathBuf::new()
        } else {
            absolute(connection, "ca_file")?
        },
        mysql_tls_server_name: None,
        secret_ref: oo_connectors::SecretRef::new(text(value, "secret_ref")?)
            .map_err(|_| PreviewError::new("config", "secret_ref"))?,
        connect_timeout_ms: bounded(budget, "connect_timeout_ms", 30000)?,
    };
    if !mysql::mysql_ident(&source.mysql_database)
        || source.mysql_host.len() > 253
        || source.mysql_host.chars().any(char::is_whitespace)
    {
        return Err(PreviewError::new("config", "endpoint"));
    }
    Ok(Input {
        type_policy,
        chunking,
        source,
        secret_file: absolute(connection, "secret_file")?,
        audit: audit_account,
        selected: SelectedTable {
            schema: schema.to_owned(),
            name: name.to_owned(),
            columns,
        },
        source_id,
        generation: bounded(value, "generation", i64::MAX as u64)?,
        owner,
        plaintext,
        max_rows: bounded(budget, "max_rows", 1000000)?,
        max_bytes: bounded(budget, "max_bytes", 33554432)?,
        deadline_ms: bounded(
            budget,
            "deadline_ms",
            if chunking { 86_400_000 } else { 300000 },
        )?,
        statement_ms: bounded(budget, "statement_timeout_ms", 30000)?,
    })
}

async fn connect(input: &Input, user: &str, password: &str) -> Result<Conn> {
    let mut conn = if !input.plaintext {
        let mut config = input.source.clone();
        config.mysql_user = user.to_owned();
        mysql::connect(&config, password).await?
    } else {
        let opts = OptsBuilder::default()
            .ip_or_hostname(input.source.mysql_host.clone())
            .tcp_port(input.source.mysql_port)
            .user(Some(user.to_owned()))
            .pass(Some(password.to_owned()))
            .db_name(Some(input.source.mysql_database.clone()))
            .prefer_socket(false)
            .ssl_opts(None::<mysql_async::SslOpts>);
        timeout(
            Duration::from_millis(input.source.connect_timeout_ms),
            Conn::new(opts),
        )
        .await
        .map_err(|_| PreviewError::new("connect", "deadline"))?
        .map_err(|_| PreviewError::new("connect", "source"))?
    };
    conn.query_drop("SET time_zone = '+00:00'")
        .await
        .map_err(|_| PreviewError::new("protocol", "timezone"))?;
    conn.query_drop("SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ZERO_DATE,NO_ZERO_IN_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION'")
        .await
        .map_err(|_| PreviewError::new("protocol", "sql_mode"))?;
    Ok(conn)
}

async fn audit_enabled(conn: &mut Conn) -> Result<bool> {
    let row: Option<(u8, String)> = conn
        .query_first("SELECT @@general_log, @@log_output")
        .await
        .map_err(|_| PreviewError::new("privilege", "source_audit"))?;
    Ok(
        row.is_some_and(|(enabled, output)| {
            enabled == 1 && output.split(',').any(|s| s == "TABLE")
        }),
    )
}

pub async fn capture_full(config: &Path, out: &Path) -> Result<()> {
    let bytes = fs::read(config).map_err(|_| PreviewError::new("config", "path"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let input = parse(&value)?;
    match fs::create_dir(out) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let checkpoint: Value = serde_json::from_slice(
                &fs::read(out.join("chunk-binding.json"))
                    .map_err(|_| PreviewError::new("resume", "binding_missing"))?,
            )
            .map_err(|_| PreviewError::new("resume", "binding_json"))?;
            if checkpoint["config_sha256"] != sha256_hex(&bytes)
                || out.join("capturer.json").exists()
            {
                return Err(PreviewError::new("resume", "binding_mismatch"));
            }
        }
        Err(_) => return Err(PreviewError::new("config", "out_exists")),
    }
    let source_connection = AtomicU64::new(0);
    let audit_connection = AtomicU64::new(0);
    let result = timeout(
        Duration::from_millis(input.deadline_ms),
        capture(&input, &bytes, out, &source_connection, &audit_connection),
    )
    .await
    .map_err(|_| PreviewError::new("budget", "snapshot_deadline"))
    .and_then(|v| v);
    if let Err(error) = &result {
        let mut targets = vec![(
            source_connection.load(Ordering::SeqCst),
            &input.source.mysql_user,
            &input.secret_file,
        )];
        if let Some((user, secret_file)) = &input.audit {
            targets.push((audit_connection.load(Ordering::SeqCst), user, secret_file));
        }
        for (id, user, secret_file) in targets {
            if id == 0 {
                continue;
            }
            let cleanup = async {
                let password = read_secret(secret_file)?;
                let mut control = connect(&input, user, &password).await?;
                let _ = control.query_drop(format!("KILL CONNECTION {id}")).await;
                let _ = control.disconnect().await;
                Ok::<(), PreviewError>(())
            };
            let _ = timeout(Duration::from_secs(5), cleanup).await;
        }
        write(
            out,
            "failure.json",
            &json!({"candidate_valid":false,"restart_table":true,"code":error.code,"fact":error.fact}),
        )?;
    }
    result
}

async fn capture(
    input: &Input,
    config_bytes: &[u8],
    out: &Path,
    source_connection: &AtomicU64,
    audit_connection: &AtomicU64,
) -> Result<()> {
    let mut observer = None;
    let mut start: Option<String> = None;
    if let Some((audit_user, audit_secret_file)) = &input.audit {
        let audit_password = read_secret(audit_secret_file)?;
        let mut conn = connect(input, audit_user, &audit_password).await?;
        audit_connection.store(u64::from(conn.id()), Ordering::SeqCst);
        if !audit_enabled(&mut conn).await? {
            return Err(PreviewError::new("privilege", "source_audit_unavailable"));
        }
        start = conn
            .query_first("SELECT DATE_FORMAT(NOW(6),'%Y-%m-%d %H:%i:%s.%f')")
            .await
            .map_err(|_| PreviewError::new("privilege", "source_audit"))?;
        observer = Some(conn);
    }
    let password = read_secret(&input.secret_file)?;
    let mut conn = connect(input, &input.source.mysql_user, &password).await?;
    let mut connection_id = u64::from(conn.id());
    source_connection.store(connection_id, Ordering::SeqCst);
    write(
        out,
        "connection.json",
        &json!({"connection_id":connection_id}),
    )?;
    let row: Option<(String, String, String, String, u8, u8, String)> = conn.query_first("SELECT @@version, @@version_comment, @@sql_mode, @@time_zone, @@log_bin, @@performance_schema, CURRENT_USER()").await.map_err(|_| PreviewError::new("protocol", "facts"))?;
    let (version, comment, sql_mode, time_zone, log_bin, performance_schema, current_user) =
        row.ok_or_else(|| PreviewError::new("protocol", "facts"))?;
    let cipher: Option<(String, String)> = conn
        .query_first("SHOW STATUS LIKE 'Ssl_cipher'")
        .await
        .map_err(|_| PreviewError::new("tls", "facts"))?;
    full_source_engine(&version, &comment, log_bin)?;
    if !mysql::current_user_is(&current_user, &input.source.mysql_user)
        || !mysql::map::session_mode_ok(&sql_mode)
        || cipher.is_none_or(|(_, c)| c.is_empty() != input.plaintext)
    {
        return Err(PreviewError::new("privilege", "source_facts"));
    }
    if input.plaintext {
        let ssl: Option<String> = conn
            .query_first("SELECT @@have_ssl")
            .await
            .map_err(|_| PreviewError::new("tls", "facts"))?;
        if ssl.as_deref() != Some("DISABLED") {
            return Err(PreviewError::new("tls", "plaintext_facts"));
        }
    }
    let grants: Vec<Row> = conn
        .query("SHOW GRANTS FOR CURRENT_USER()")
        .await
        .map_err(|_| PreviewError::new("privilege", "grants"))?;
    let grants: Vec<String> = grants
        .into_iter()
        .map(|r| {
            r.get(0)
                .ok_or_else(|| PreviewError::new("privilege", "grants"))
        })
        .collect::<Result<_>>()?;
    let privileges = mysql::full_read_grants(&grants)?.privileges;
    let mut notes = Vec::new();
    let catalog = mysql::catalog_columns(&mut conn, &mut notes, &input.selected).await?;
    let reserved = oo_connectors::PG17_RESERVED
        .iter()
        .map(|s| s.to_string())
        .collect();
    let keys =
        mysql::chunk::key_columns(&mut conn, &input.selected.schema, &input.selected.name).await?;
    let mut mapped = catalog
        .iter()
        .map(|column| {
            mysql::map::map_capture_column(
                column,
                false,
                mysql::map::session_utc(&time_zone),
                &reserved,
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for column in &mut mapped {
        column.key = keys.iter().any(|key| key.name == column.plan.name);
        column.plan.key = column.key;
    }
    let mut columns = if input.selected.columns.is_empty() {
        mapped
    } else {
        let mut selected = Vec::new();
        for name in &input.selected.columns {
            selected.push(
                mapped
                    .iter()
                    .find(|column| column.plan.name == *name)
                    .cloned()
                    .ok_or_else(|| PreviewError::new("catalog", "missing_column"))?,
            );
        }
        if keys
            .iter()
            .any(|key| !selected.iter().any(|column| column.plan.name == key.name))
        {
            return Err(PreviewError::new("catalog", "missing_key_column"));
        }
        selected
    };
    for column in &mut columns {
        if column.key { continue; }
        if input.type_policy.zero_date_to_null && matches!(column.store, mysql::map::StoreKind::Date | mysql::map::StoreKind::TimestampNaive | mysql::map::StoreKind::TimestampTz) {
            column.plan.mapping.nullable = true;
            column.not_null = false;
            column.plan.mapping.metadata.push(("mysql_zero_date_to_null".into(), "true".into()));
        }
        if input.type_policy.strip_text_nul && matches!(column.store, mysql::map::StoreKind::Text { .. }) {
            column.plan.mapping.metadata.push(("mysql_strip_text_nul".into(), "true".into()));
        }
    }
    let has_key = !keys.is_empty();
    let requires_conversion = columns.iter().any(|column| {
        column
            .plan
            .mapping
            .metadata
            .iter()
            .any(|(key, _)| key == "mysql_source_charset")
    });
    let schema_sha256=sha256_hex(&serde_json::to_vec(&catalog.iter().map(|c|json!({"name":c.name,"ordinal":c.ordinal,"data_type":c.data_type,"column_type":c.column_type,"nullable":c.nullable,"char_len":c.char_len,"precision":c.precision,"scale":c.scale,"charset":c.charset,"column_key":c.column_key})).collect::<Vec<_>>()).map_err(|_|PreviewError::new("catalog","schema_digest"))?);
    let config_digest = sha256_hex(config_bytes);
    let input_digest =
        std::array::from_fn(|i| u8::from_str_radix(&config_digest[i * 2..i * 2 + 2], 16).unwrap());
    let binding = RunBinding {
        source_id: input.source_id.clone(),
        schema: input.selected.schema.clone(),
        table: input.selected.name.clone(),
        run_generation: input.generation,
        input_digest,
    };
    if out.join("chunk-binding.json").exists() {
        let previous: Value = serde_json::from_slice(
            &fs::read(out.join("chunk-binding.json"))
                .map_err(|_| PreviewError::new("resume", "binding_read"))?,
        )
        .map_err(|_| PreviewError::new("resume", "binding_json"))?;
        if previous != chunk_identity(&binding, &keys, &schema_sha256) {
            return Err(PreviewError::new("resume", "binding_mismatch"));
        }
    }
    let full = || {
        if has_key {
            mysql::FullReader::new(
                &input.selected.schema,
                &input.selected.name,
                columns.clone(),
                binding.clone(),
                input.max_rows,
                input.max_bytes,
                input.deadline_ms.min(300000),
                input.statement_ms,
            )
        } else {
            mysql::FullReader::new_unkeyed(
                &input.selected.schema,
                &input.selected.name,
                columns.clone(),
                binding.clone(),
                input.max_rows,
                input.max_bytes,
                input.deadline_ms.min(300000),
                input.statement_ms,
            )
        }
    };
    let initial = if has_key
        && (input.chunking
            || keys.len() != 1
            || columns
                .iter()
                .find(|c| c.key)
                .is_some_and(|c| !matches!(c.oid, 20 | 21 | 23 | 1700 | 1043))
            || requires_conversion
            || out.join("chunk-binding.json").exists())
    {
        None
    } else {
        Some(
            full()?
                .read_on(&mut conn, &mysql::Cancel::default(), &mut notes)
                .await,
        )
    };
    let chunk_context = || ChunkCapture {
        columns: &columns,
        keys: &keys,
        binding: &binding,
        out,
        privileges: &privileges,
        audit_start: start.as_deref(),
        source_connection,
        schema_sha256: &schema_sha256,
    };
    let (rows, arrow_digest, chunked) = match initial {
        Some(Ok(batch)) => {
            if !batch.rejects.is_empty() {
                record_rejects(out, &binding, &columns, &batch)?;
                if !normalization_only(&batch, input.type_policy) {
                    return Err(PreviewError::new("batch", "quality_rejects"));
                }
            }
            let typed = batch
                .typed
                .record_batch()
                .map_err(|_| PreviewError::new("batch", "arrow"))?;
            let mut arrow = Vec::new();
            {
                let mut writer = StreamWriter::try_new(&mut arrow, &typed.schema())
                    .map_err(|_| PreviewError::new("batch", "arrow"))?;
                writer
                    .write(&typed)
                    .and_then(|_| writer.finish())
                    .map_err(|_| PreviewError::new("batch", "arrow"))?;
            }
            if arrow.len() as u64 > input.max_bytes {
                if !has_key {
                    return Err(PreviewError::new(
                        "excluded",
                        "no_usable_key_requires_larger_full_budget_or_source_unique_index",
                    ));
                }
                conn.disconnect()
                    .await
                    .map_err(|_| PreviewError::new("connect", "disconnect"))?;
                conn = connect(input, &input.source.mysql_user, &password).await?;
                connection_id = u64::from(conn.id());
                source_connection.store(connection_id, Ordering::SeqCst);
                write(
                    out,
                    "connection.json",
                    &json!({"connection_id":connection_id}),
                )?;
                let (returned, rows) =
                    capture_chunks(input, conn, chunk_context(), observer.as_mut()).await?;
                conn = returned;
                (
                    rows,
                    mysql::chunk::file_sha256(&out.join("table-0.arrow"))?,
                    rows > 0,
                )
            } else {
                fs::write(out.join("table-0.arrow"), &arrow)
                    .map_err(|_| PreviewError::new("batch", "arrow_file"))?;
                (typed.num_rows() as u64, sha256_hex(&arrow), false)
            }
        }
        initial => {
            if let Some(Err(error)) = initial {
                if error.code == "budget" && !has_key {
                    write(
                        out,
                        "rejects.json",
                        &json!([{"source_id":binding.source_id,"schema":binding.schema,"table":binding.table,"run_generation":binding.run_generation,"reason":"full_budget_exceeded","budget_fact":error.fact,"key_digest":null,"value_len":null,"raw_value_recorded":false}]),
                    )?;
                    return Err(PreviewError::new(
                        "excluded",
                        "no_usable_key_requires_larger_full_budget_or_source_unique_index",
                    ));
                }
                if error.code != "budget"
                    || !["max_rows", "max_bytes", "batch_bytes"].contains(&error.fact)
                {
                    return Err(error.into());
                }
                conn.disconnect()
                    .await
                    .map_err(|_| PreviewError::new("connect", "disconnect"))?;
                conn = connect(input, &input.source.mysql_user, &password).await?;
                connection_id = u64::from(conn.id());
                source_connection.store(connection_id, Ordering::SeqCst);
                write(
                    out,
                    "connection.json",
                    &json!({"connection_id":connection_id}),
                )?;
            }
            let (returned, rows) =
                capture_chunks(input, conn, chunk_context(), observer.as_mut()).await?;
            conn = returned;
            (
                rows,
                mysql::chunk::file_sha256(&out.join("table-0.arrow"))?,
                rows > 0,
            )
        }
    };
    conn.disconnect()
        .await
        .map_err(|_| PreviewError::new("connect", "disconnect"))?;
    source_connection.store(0, Ordering::SeqCst);
    let mut audit = if chunked {
        if let Some(mut observer) = observer {
            if !audit_enabled(&mut observer).await? {
                return Err(PreviewError::new("privilege", "source_audit_unavailable"));
            }
            observer
                .disconnect()
                .await
                .map_err(|_| PreviewError::new("connect", "audit_disconnect"))?;
            audit_connection.store(0, Ordering::SeqCst);
        }
        let entries = mysql::chunk::load_manifest(out, &input.selected.name)?;
        let chunks = entries
            .iter()
            .map(|entry| {
                serde_json::from_slice::<Value>(
                    &fs::read(out.join(format!("{}.audit.json", entry.chunk_id)))
                        .map_err(|_| PreviewError::new("resume", "audit_read"))?,
                )
                .map_err(|_| PreviewError::new("resume", "audit_json"))
            })
            .collect::<Result<Vec<_>>>()?;
        let audit = json!({"source":"mysql.chunk_audits/v1","schema":input.selected.schema,"table":input.selected.name,"chunks":chunks});
        if !(source_admission::mysql_full_audit_valid(&audit)
            || (input.audit.is_none() && source_admission::mysql_full_grant_audit_valid(&audit)))
        {
            return Err(PreviewError::new("privilege", "source_audit_invalid"));
        }
        audit
    } else {
        match observer {
            Some(mut observer) => {
                let records: Vec<(u64, String, String)> = observer.exec("SELECT thread_id, command_type, argument FROM mysql.general_log WHERE thread_id = ? AND event_time >= ? ORDER BY event_time LIMIT 10001", (connection_id, start.ok_or_else(|| PreviewError::new("privilege", "source_audit"))?)).await.map_err(|_| PreviewError::new("privilege", "source_audit"))?;
                if records.len() >= 10001 {
                    return Err(PreviewError::new("budget", "source_audit"));
                }
                let audit = json!({"source":"mysql.general_log","schema":input.selected.schema,"table":input.selected.name,"connection_id":connection_id,"enabled_before":true,"enabled_after":audit_enabled(&mut observer).await?,"chunked":chunked,"records":records.iter().map(|(id,command,argument)|json!({"thread_id":id,"command_type":command,"argument":argument})).collect::<Vec<_>>()});
                if !source_admission::mysql_full_audit_valid(&audit) {
                    return Err(PreviewError::new("privilege", "source_audit_invalid"));
                }
                observer
                    .disconnect()
                    .await
                    .map_err(|_| PreviewError::new("connect", "audit_disconnect"))?;
                audit_connection.store(0, Ordering::SeqCst);
                audit
            }
            None => {
                let audit = json!({"source":"grant_enforced","schema":input.selected.schema,"table":input.selected.name,"connection_id":connection_id,"read_only_transaction":true,"chunked":chunked,"privileges":privileges});
                if !source_admission::mysql_full_grant_audit_valid(&audit) {
                    return Err(PreviewError::new("privilege", "source_audit_invalid"));
                }
                audit
            }
        }
    };
    if !chunked {
        audit.as_object_mut().unwrap().remove("chunked");
    }
    let mut fields = Vec::new();
    let mut types = Vec::new();
    for c in &columns {
        let (kind, precision, scale) = arrow_description(
            c.plan
                .arrow
                .as_ref()
                .ok_or_else(|| PreviewError::new("batch", "arrow_type"))?,
        )?;
        let field = json!({"name":c.plan.name,"arrow_type":kind,"nullable":c.plan.mapping.nullable,"precision":precision,"scale":scale,"key":c.key,"document":c.oid==114,"char_length":if c.oid==1042 {Some(c.typmod-4)} else {None}});
        fields.push(field.clone());
        let mut typ = field;
        typ["table"] = json!(input.selected.name);
        typ["source_type"] = json!(c.plan.mapping.source_type);
        typ["target_oid"] = json!(c.oid);
        typ["target_typmod"] = json!(c.typmod);
        typ["caveat"] = json!(c.caveat);
        typ.as_object_mut().unwrap().remove("nullable");
        typ.as_object_mut().unwrap().remove("key");
        types.push(typ);
    }
    let driver = std::env::current_exe().map_err(|_| PreviewError::new("config", "driver"))?;
    let arrow_path = out
        .canonicalize()
        .map_err(|_| PreviewError::new("config", "out"))?
        .join("table-0.arrow");
    write(
        out,
        "schema-preimage.json",
        &json!([{"table":input.selected.name,"columns":fields}]),
    )?;
    write(out, "type-preimage.json", &json!(types))?;
    write(
        out,
        "driver.json",
        &json!({"protocol":"oo.mysql.capture-driver/v1","executable_path":driver,"executable_sha256":mysql::chunk::file_sha256(&driver).map_err(|_|PreviewError::new("config","driver"))?}),
    )?;
    write(out, "reader-write-audit.json", &audit)?;
    let mut access = json!({"version":version,"version_comment":comment,"log_bin":log_bin,"performance_schema":performance_schema,"tls":if input.plaintext {"absent; plaintext explicitly authorized; no encrypted tail claim"} else {"verified"},"current_user":current_user,"privileges":privileges});
    if chunked {
        access["chunk_key_order"] = json!("source_collation");
    }
    write(out, "access.json", &access)?;
    let mut capturer = json!({"protocol":"oo.mysql.full-capture/v1","source_engine":"mysql","database":input.selected.schema,"source_id":input.source_id,"source_generation":input.generation,"run_id":input.owner,"label":input.owner,"snapshot":{"kind":"mysql_consistent_snapshot","value":input.owner},"snapshot_token":false,"snapshot_complete":true,"cross_table_consistency":"B","product_reader_write_attempts":0,"source_audit":audit,"observed_privileges":privileges,"max_rows":input.max_rows,"max_bytes":input.max_bytes,"config_sha256":sha256_hex(config_bytes),"tables":[{"name":input.selected.name,"schema":input.selected.schema,"arrow":"table-0.arrow","arrow_path":arrow_path,"arrow_sha256":arrow_digest,"row_count":rows,"route":"C5","consistency":if chunked {"B"} else {"S0"},"delete_mode":"reconcile","cursor_safety":"no_cursor","quality":"FULL_SNAPSHOT","chunk_manifest":if chunked {Some("chunks.json")} else {None}}]});
    if chunked {
        let chunks = serde_json::to_value(
            mysql::chunk::load_manifest(out, &input.selected.name)
                .map_err(|_| PreviewError::new("resume", "manifest"))?,
        )
        .map_err(|_| PreviewError::new("resume", "manifest"))?;
        let entries = chunks.as_array().unwrap();
        let parts = entries.iter().enumerate().map(|(index, entry)| json!({"index":index,"total":entries.len(),"chunk_id":index,"rows":entry["rows"],"arrow_sha256":entry["arrow_sha256"]})).collect::<Vec<_>>();
        if !parts.is_empty() {
            capturer["multipart_scope"] = json!({"protocol":"oo.mysql.c5-multipart/v1","source_id":input.source_id,"source_generation":input.generation,"occurrence":input.owner,"schema":input.selected.schema,"table":input.selected.name,"schema_sha256":mysql::chunk::file_sha256(&out.join("schema-preimage.json")).map_err(|_|PreviewError::new("resume","schema"))?,"capture_config_sha256":sha256_hex(config_bytes),"manifest_sha256":sha256_hex(&serde_json::to_vec(&chunks).map_err(|_|PreviewError::new("resume","manifest"))?),"chunks":chunks,"part_count":parts.len(),"parts":parts});
        }
        capturer["chunk_budget"] = json!({"max_rows":input.max_rows,"max_bytes":input.max_bytes});
        capturer["max_rows"] = json!(rows.max(1));
    }
    if !chunked {
        capturer["tables"][0]
            .as_object_mut()
            .unwrap()
            .remove("chunk_manifest");
    }
    if out.join("failure.json").exists() {
        fs::remove_file(out.join("failure.json"))
            .map_err(|_| PreviewError::new("resume", "failure_marker"))?;
    }
    capturer["type_policy"] = serde_json::to_value(input.type_policy).map_err(|_|PreviewError::new("config","type_policy"))?;
    write(out, "capturer.json", &capturer)?;
    Ok(())
}

fn full_source_engine(version: &str, comment: &str, log_bin: u8) -> Result<()> {
    // Full C5 capture is explicitly read-only, even after the DBA enables logs.
    // Enabling C1 remains a separate admission and handoff operation.
    if comment.to_ascii_lowercase().contains("mariadb")
        || version.contains("MariaDB") || log_bin > 1 {
        return Err(PreviewError::new("privilege", "source_facts"));
    }
    Ok(())
}

struct ChunkCapture<'a> {
    columns: &'a [mysql::map::MappedColumn],
    keys: &'a [mysql::chunk::KeyColumn],
    binding: &'a RunBinding,
    out: &'a Path,
    privileges: &'a BTreeSet<String>,
    audit_start: Option<&'a str>,
    source_connection: &'a AtomicU64,
    schema_sha256: &'a str,
}

fn chunk_identity(
    binding: &RunBinding,
    keys: &[mysql::chunk::KeyColumn],
    schema_sha256: &str,
) -> Value {
    json!({"protocol":"oo.mysql.chunk-binding/v1","source_id":binding.source_id,"schema":binding.schema,"table":binding.table,"run_generation":binding.run_generation,"keys":keys,"schema_sha256":schema_sha256,"config_sha256":binding.input_digest.iter().map(|b|format!("{b:02x}")).collect::<String>()})
}

async fn capture_chunks(
    input: &Input,
    mut conn: Conn,
    context: ChunkCapture<'_>,
    mut observer: Option<&mut Conn>,
) -> Result<(Conn, u64)> {
    let ChunkCapture {
        columns,
        keys,
        binding,
        out,
        privileges,
        audit_start,
        source_connection,
        schema_sha256,
    } = context;
    use mysql::chunk::{ChunkBound, ChunkEntry, ChunkRead};
    let checkpoint = chunk_identity(binding, keys, schema_sha256);
    let binding_path = out.join("chunk-binding.json");
    if binding_path.exists() {
        let previous: Value = serde_json::from_slice(
            &fs::read(&binding_path).map_err(|_| PreviewError::new("resume", "binding_read"))?,
        )
        .map_err(|_| PreviewError::new("resume", "binding_json"))?;
        if previous != checkpoint {
            return Err(PreviewError::new("resume", "binding_mismatch"));
        }
    } else {
        write(out, "chunk-binding.json", &checkpoint)?;
        fs::File::open(&binding_path)
            .and_then(|f| f.sync_all())
            .map_err(|_| PreviewError::new("resume", "binding_sync"))?;
        fs::File::open(out)
            .and_then(|f| f.sync_all())
            .map_err(|_| PreviewError::new("resume", "binding_sync"))?;
    }
    let mut entries = mysql::chunk::load_manifest(out, &input.selected.name)?;
    let mut lo = entries.last().and_then(|e| e.hi.clone());
    let mut total = entries.iter().try_fold(0u64, |total, entry| {
        if entry.rows > input.max_rows || entry.bytes > input.max_bytes {
            return Err(PreviewError::new("resume", "manifest_budget"));
        }
        total
            .checked_add(entry.rows)
            .ok_or_else(|| PreviewError::new("resume", "manifest_rows"))
    })?;
    if out.join("chunks.json.pending").exists() {
        fs::remove_file(out.join("chunks.json.pending"))
            .map_err(|_| PreviewError::new("resume", "manifest_pending"))?;
    }

    let mut sizing = mysql::chunk::AdaptiveRows::new(
        input.max_rows.min(1000),
        input.max_rows.min(100_000),
        input.max_bytes.min(16 * 1024 * 1024),
    )?;
    let mut writer = None;
    for entry in &entries {
        let audit: Value = serde_json::from_slice(
            &fs::read(out.join(format!("{}.audit.json", entry.chunk_id)))
                .map_err(|_| PreviewError::new("resume", "audit_read"))?,
        )
        .map_err(|_| PreviewError::new("resume", "audit_json"))?;
        if audit["schema"] != input.selected.schema
            || audit["table"] != input.selected.name
            || !(source_admission::mysql_full_audit_valid(&audit)
                || (input.audit.is_none()
                    && source_admission::mysql_full_grant_audit_valid(&audit)))
        {
            return Err(PreviewError::new("resume", "audit_invalid"));
        }
        let reader = arrow_ipc::reader::StreamReader::try_new(
            fs::File::open(out.join(format!("{}.arrow", entry.chunk_id)))
                .map_err(|_| PreviewError::new("resume", "arrow_open"))?,
            None,
        )
        .map_err(|_| PreviewError::new("resume", "arrow_read"))?;
        let mut reused_rows = 0u64;
        for batch in reader {
            let batch = batch.map_err(|_| PreviewError::new("resume", "arrow_read"))?;
            reused_rows = reused_rows
                .checked_add(batch.num_rows() as u64)
                .ok_or_else(|| PreviewError::new("resume", "manifest_rows"))?;
            if writer.is_none() {
                writer = Some(
                    StreamWriter::try_new(
                        fs::File::create(out.join("table-0.arrow.pending"))
                            .map_err(|_| PreviewError::new("batch", "arrow_file"))?,
                        &batch.schema(),
                    )
                    .map_err(|_| PreviewError::new("batch", "arrow"))?,
                );
            }
            writer
                .as_mut()
                .unwrap()
                .write(&batch)
                .map_err(|_| PreviewError::new("batch", "arrow"))?;
        }
        if reused_rows != entry.rows {
            return Err(PreviewError::new("resume", "manifest_rows"));
        }
    }
    let mut pending_bound = None;
    let mut disconnect_retries = 0u32;
    let mut budget_retries = 0u64;
    loop {
        let hi = if let Some(bound) = pending_bound.take() {
            bound
        } else {
            let solved = timeout(
                Duration::from_millis(input.statement_ms),
                mysql::chunk::next_boundary(
                    &mut conn,
                    &input.selected.schema,
                    &input.selected.name,
                    keys,
                    lo.clone(),
                    sizing.rows(),
                ),
            )
            .await
            .map_err(|_| PreviewError::new("budget", "statement_timeout"))?;
            match solved {
                Ok(hi) => hi,
                Err(error) if error.code == "protocol" && disconnect_retries < 3 => {
                    disconnect_retries += 1;
                    let _ = timeout(Duration::from_secs(5), conn.disconnect()).await;
                    source_connection.store(0, Ordering::SeqCst);
                    let password = read_secret(&input.secret_file)?;
                    conn = connect(input, &input.source.mysql_user, &password).await?;
                    source_connection.store(u64::from(conn.id()), Ordering::SeqCst);
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let Some(hi) = hi else { break };
        let bound = ChunkBound {
            lo: lo.clone(),
            hi: Some(hi.clone()),
        };
        let started = std::time::Instant::now();
        let attempt = mysql::chunk::read_chunk(
            conn,
            ChunkRead {
                schema: &input.selected.schema,
                table: &input.selected.name,
                keys,
                columns,
                binding,
                bound: &bound,
                max_rows: input.max_rows.min(100_000),
                max_bytes: input.max_bytes.min(16 * 1024 * 1024),
                deadline: Duration::from_millis(input.statement_ms),
            },
        )
        .await;
        let (returned, batch) = match attempt {
            Ok(value) => value,
            Err(error) => {
                source_connection.store(0, Ordering::SeqCst);
                if error.code == "budget"
                    && ["max_rows", "max_bytes", "arrow_bytes", "batch_bytes"].contains(&error.fact)
                {
                    if !sizing.shrink() {
                        write(
                            out,
                            "rejects.json",
                            &json!([{"source_id":binding.source_id,"schema":binding.schema,"table":binding.table,"run_generation":binding.run_generation,"reason":"row_budget_exceeded","budget_fact":error.fact,"key_digest":null,"boundary_sha256":sha256_hex(&serde_json::to_vec(&bound).map_err(|_|PreviewError::new("batch","reject_json"))?),"value_len":error.value_len,"raw_value_recorded":false}]),
                        )?;
                        return Err(PreviewError::new(
                            "excluded",
                            "single_row_budget_requires_larger_budget_or_lob_policy",
                        ));
                    }
                    budget_retries += 1;
                } else if error.code == "protocol" && disconnect_retries < 3 {
                    disconnect_retries += 1;
                    pending_bound = Some(Some(hi.clone()));
                } else {
                    return Err(error.into());
                }
                let password = read_secret(&input.secret_file)?;
                conn = connect(input, &input.source.mysql_user, &password).await?;
                source_connection.store(u64::from(conn.id()), Ordering::SeqCst);
                continue;
            }
        };
        let completed_disconnect_retries = disconnect_retries;
        disconnect_retries = 0;
        let read_elapsed = started.elapsed();
        conn = returned;
        if !batch.rejects.is_empty() {
            record_rejects(out, binding, columns, &batch)?;
            if !normalization_only(&batch, input.type_policy) {
                return Err(PreviewError::new("batch", "quality_rejects"));
            }
        }
        let typed = batch
            .typed
            .record_batch()
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        let count = typed.num_rows() as u64;
        if count == 0 {
            return Err(PreviewError::new("batch", "chunk_changed"));
        }
        let id = entries.len() as u64;
        let path = out.join(format!("{id}.arrow"));
        let mut chunk = StreamWriter::try_new(
            fs::File::create(&path).map_err(|_| PreviewError::new("batch", "arrow_file"))?,
            &typed.schema(),
        )
        .map_err(|_| PreviewError::new("batch", "arrow"))?;
        chunk
            .write(&typed)
            .and_then(|_| chunk.finish())
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        chunk
            .get_ref()
            .sync_all()
            .map_err(|_| PreviewError::new("batch", "arrow_sync"))?;
        if fs::metadata(&path)
            .map_err(|_| PreviewError::new("batch", "arrow_file"))?
            .len()
            > input.max_bytes
        {
            if !sizing.shrink() {
                write(
                    out,
                    "rejects.json",
                    &json!([{"source_id":binding.source_id,"schema":binding.schema,"table":binding.table,"run_generation":binding.run_generation,"reason":"row_budget_exceeded","budget_fact":"arrow_bytes","key_digest":null,"value_len":null,"raw_value_recorded":false}]),
                )?;
                return Err(PreviewError::new(
                    "excluded",
                    "single_row_budget_requires_larger_budget_or_lob_policy",
                ));
            }
            budget_retries += 1;
            conn.disconnect()
                .await
                .map_err(|_| PreviewError::new("connect", "disconnect"))?;
            source_connection.store(0, Ordering::SeqCst);
            let password = read_secret(&input.secret_file)?;
            conn = connect(input, &input.source.mysql_user, &password).await?;
            source_connection.store(u64::from(conn.id()), Ordering::SeqCst);
            continue;
        }
        if writer.is_none() {
            writer = Some(
                StreamWriter::try_new(
                    fs::File::create(out.join("table-0.arrow.pending"))
                        .map_err(|_| PreviewError::new("batch", "arrow_file"))?,
                    &typed.schema(),
                )
                .map_err(|_| PreviewError::new("batch", "arrow"))?,
            );
        }
        writer
            .as_mut()
            .unwrap()
            .write(&typed)
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        let connection_id = u64::from(conn.id());
        conn.disconnect()
            .await
            .map_err(|_| PreviewError::new("connect", "disconnect"))?;
        source_connection.store(0, Ordering::SeqCst);
        let audit = if let Some(observer) = observer.as_deref_mut() {
            let records: Vec<(u64,String,String)>=observer.exec("SELECT thread_id,command_type,argument FROM mysql.general_log WHERE thread_id=? AND event_time>=? ORDER BY event_time LIMIT 10001",(connection_id,audit_start.ok_or_else(||PreviewError::new("privilege","source_audit"))?)).await.map_err(|_|PreviewError::new("privilege","source_audit"))?;
            if records.len() >= 10001 {
                return Err(PreviewError::new("budget", "source_audit"));
            }
            json!({"source":"mysql.general_log","schema":input.selected.schema,"table":input.selected.name,"connection_id":connection_id,"enabled_before":true,"enabled_after":audit_enabled(observer).await?,"chunked":true,"records":records.iter().map(|(id,command,argument)|json!({"thread_id":id,"command_type":command,"argument":argument})).collect::<Vec<_>>()})
        } else {
            json!({"source":"grant_enforced","schema":input.selected.schema,"table":input.selected.name,"connection_id":connection_id,"read_only_transaction":true,"privileges":privileges})
        };
        if !(source_admission::mysql_full_audit_valid(&audit)
            || (input.audit.is_none() && source_admission::mysql_full_grant_audit_valid(&audit)))
        {
            return Err(PreviewError::new("privilege", "source_audit_invalid"));
        }
        write(out, &format!("{id}.audit.json"), &audit)?;
        fs::File::open(out.join(format!("{id}.audit.json")))
            .and_then(|f| f.sync_all())
            .map_err(|_| PreviewError::new("resume", "audit_sync"))?;
        let password = read_secret(&input.secret_file)?;
        conn = connect(input, &input.source.mysql_user, &password).await?;
        source_connection.store(u64::from(conn.id()), Ordering::SeqCst);
        write(
            out,
            &format!("{id}.metrics.json"),
            &json!({"chunk_id":id,"rows":count,"bytes":batch.bytes,"elapsed_ms":started.elapsed().as_secs_f64()*1000.0,"budget_retries":budget_retries,"disconnect_retries":completed_disconnect_retries}),
        )?;
        entries.push(ChunkEntry {
            table: input.selected.name.clone(),
            chunk_id: id,
            lo: lo.clone(),
            hi: Some(hi.clone()),
            rows: count,
            bytes: batch.bytes,
            arrow_sha256: mysql::chunk::file_sha256(&path)?,
        });
        mysql::chunk::save_manifest(out, &input.selected.name, &entries)?;
        total += count;
        sizing.observe(count, batch.bytes, read_elapsed);
        budget_retries = 0;
        lo = Some(hi);
    }
    if let Some(mut writer) = writer {
        writer
            .finish()
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|_| PreviewError::new("batch", "arrow_sync"))?;
        fs::rename(out.join("table-0.arrow.pending"), out.join("table-0.arrow"))
            .map_err(|_| PreviewError::new("batch", "arrow_file"))?;
    } else {
        let composite = columns.iter().filter(|c| c.key).count() > 1;
        let reader = if composite {
            mysql::FullReader::new_composite(
                &input.selected.schema,
                &input.selected.name,
                columns.to_vec(),
                binding.clone(),
                input.max_rows,
                input.max_bytes,
                input.deadline_ms.min(300000),
                input.statement_ms,
            )?
        } else {
            mysql::FullReader::new(
                &input.selected.schema,
                &input.selected.name,
                columns.to_vec(),
                binding.clone(),
                input.max_rows,
                input.max_bytes,
                input.deadline_ms.min(300000),
                input.statement_ms,
            )?
        };
        let batch = reader
        .read_on(&mut conn, &mysql::Cancel::default(), &mut Vec::new())
        .await?;
        let typed = batch
            .typed
            .record_batch()
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        let mut writer = StreamWriter::try_new(
            fs::File::create(out.join("table-0.arrow"))
                .map_err(|_| PreviewError::new("batch", "arrow_file"))?,
            &typed.schema(),
        )
        .map_err(|_| PreviewError::new("batch", "arrow"))?;
        writer
            .write(&typed)
            .and_then(|_| writer.finish())
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        if fs::metadata(out.join("table-0.arrow"))
            .map_err(|_| PreviewError::new("batch", "arrow_file"))?
            .len()
            > input.max_bytes
        {
            return Err(PreviewError::new("budget", "arrow_bytes"));
        }
        mysql::chunk::save_manifest(out, &input.selected.name, &entries)?;
    }
    Ok((conn, total))
}

fn normalization_only(batch: &mysql::Batch, policy: mysql::values::TypePolicy) -> bool {
    batch.rejects.iter().all(|(_,_,note)| match note.reason {
        "zero_date" => policy.zero_date_to_null,
        "nul_byte" => policy.strip_text_nul,
        _ => false,
    })
}

fn record_rejects(
    out: &Path,
    binding: &RunBinding,
    columns: &[mysql::map::MappedColumn],
    batch: &mysql::Batch,
) -> Result<()> {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let rejects=batch.rejects.iter().map(|(row,column,note)|json!({
        "protocol":oo_connectors::types::TYPE_CONVERSION_PROTOCOL,"source_id":binding.source_id,"schema":binding.schema,"table":binding.table,
        "column":columns[*column].plan.name,"attnum":columns[*column].plan.attnum,"path":columns[*column].plan.name,"row_index":row,
        "key_digest":batch.typed.rows.get(*row).and_then(|row|row.key_digest.as_ref()).map(|key|hex(key)),"reason":note.reason,"value_len":note.value_len,
        "run_generation":binding.run_generation,"input_digest":hex(&binding.input_digest),"type_policy_version":oo_core::TYPE_POLICY_V1,
    })).collect::<Vec<_>>();
    let path = out.join("rejects.json");
    let mut previous: Vec<Value> = if path.exists() {
        if fs::metadata(&path).map_err(|_| PreviewError::new("batch","reject_read"))?.len() > 33554432 {
            return Err(PreviewError::new("budget","reject_bytes"));
        }
        serde_json::from_slice(&fs::read(&path).map_err(|_| PreviewError::new("batch","reject_read"))?)
            .map_err(|_| PreviewError::new("batch","reject_json"))?
    } else { Vec::new() };
    previous.extend(rejects);
    if previous.len() > 100000 || serde_json::to_vec(&previous).map_err(|_|PreviewError::new("batch","reject_json"))?.len() > 33554432 {
        return Err(PreviewError::new("budget","reject_bytes"));
    }
    write(out, "rejects.json", &json!(previous))
}

fn write(out: &Path, name: &str, value: &Value) -> Result<()> {
    fs::write(
        out.join(name),
        serde_json::to_vec_pretty(value).map_err(|_| PreviewError::new("config", "json"))?,
    )
    .map_err(|_| PreviewError::new("config", "out"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_full_c5_stays_available_when_binary_logging_is_enabled() {
        for logging in [0, 1] {
            assert!(full_source_engine("5.7.28-log", "MySQL Community Server", logging).is_ok());
        }
        assert!(full_source_engine("5.7.28-log", "MySQL", 2).is_err());
        assert!(full_source_engine("5.7.28-MariaDB", "MySQL", 1).is_err());
        assert!(full_source_engine("10.6.0", "mariadb", 0).is_err());
    }

    fn config() -> Value {
        json!({"protocol":"oo.mysql-full/v1","source_id":"sample","generation":1,"owner":"00000000-0000-0000-0000-000000000001","secret_ref":"secret://sample/reader",
            "connection":{"host":"localhost","port":3306,"database":"sample","user":"reader","secret_file":"/secrets/reader","tls":"disabled","ca_file":null,"plaintext_authorized":true},
            "audit":{"user":"observer","secret_ref":"secret://sample/audit","secret_file":"/secrets/audit"},
            "approved_objects":[{"schema":"sample","table":"items","columns":["id","name"]}],"table":{"schema":"sample","table":"items","columns":["id","name"]},
            "budgets":{"connect_timeout_ms":1000,"statement_timeout_ms":1000,"deadline_ms":5000,"max_rows":10,"max_bytes":1048576}})
    }

    #[tokio::test]
    async fn resume_checks_binding_before_opening_any_source_connection() {
        let dir = std::env::temp_dir().join(format!("oo-wf-chunk-binding-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let c = config();
        let bytes = serde_json::to_vec(&c).unwrap();
        let path = dir.join("capture.json");
        fs::write(&path, &bytes).unwrap();
        let out = dir.join("out");
        fs::create_dir(&out).unwrap();
        write(
            &out,
            "chunk-binding.json",
            &json!({"config_sha256":"incorrect"}),
        )
        .unwrap();
        assert_eq!(
            capture_full(&path, &out).await.unwrap_err().fact,
            "binding_mismatch"
        );
        write(
            &out,
            "chunk-binding.json",
            &json!({"config_sha256":sha256_hex(&bytes)}),
        )
        .unwrap();
        write(&out, "capturer.json", &json!({})).unwrap();
        assert_eq!(
            capture_full(&path, &out).await.unwrap_err().fact,
            "binding_mismatch"
        );
        fs::remove_file(out.join("capturer.json")).unwrap();
        fs::remove_file(out.join("chunk-binding.json")).unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_dir(out).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn installed_normalization_policy_is_explicit_and_closed() {
        assert!(!parse(&config()).unwrap().type_policy.zero_date_to_null);
        let mut c = config();
        c["type_policy"] = json!({"zero_date_to_null":true,"strip_text_nul":true});
        let policy = parse(&c).unwrap().type_policy;
        assert!(policy.zero_date_to_null && policy.strip_text_nul);
        c["type_policy"]["unreviewed"] = json!(true);
        assert!(parse(&c).is_err());
    }

    #[test]
    fn chunk_mode_is_explicit_and_closed() {
        let mut c = config();
        c["chunking"] = json!(true);
        assert!(parse(&c).unwrap().chunking);
        c["budgets"]["deadline_ms"] = json!(900000);
        assert!(parse(&c).is_ok());
        c["chunking"] = json!(false);
        assert!(parse(&c).is_err());
        c["chunking"] = json!("yes");
        assert!(parse(&c).is_err());
    }

    #[test]
    fn same_read_only_principal_can_use_a_separate_audit_connection() {
        let mut c = config();
        c["audit"] = json!({"user":c["connection"]["user"],"secret_ref":c["secret_ref"],"secret_file":c["connection"]["secret_file"]});
        let input = parse(&c).expect("same-principal observer");
        assert_eq!(
            input.audit,
            Some((input.source.mysql_user.clone(), input.secret_file.clone()))
        );
    }

    #[test]
    fn explicit_approval_transport_and_budgets_precede_connection() {
        assert!(parse(&config()).is_ok());
        for (key, value) in [
            ("plaintext_authorized", json!(false)),
            ("tls", json!("auto")),
            ("secret_file", json!("relative")),
        ] {
            let mut c = config();
            c["connection"][key] = value;
            assert!(parse(&c).is_err(), "{key}");
        }
        let mut c = config();
        c["table"]["columns"] = json!([]);
        assert!(parse(&c).is_err());
        let mut c = config();
        c["budgets"]["max_bytes"] = json!(33554433u64);
        assert!(parse(&c).is_err());
        let mut c = config();
        c["connection"]["password"] = json!("forbidden");
        assert!(parse(&c).is_err());
    }

    #[test]
    fn grant_enforced_audit_is_exact_and_needs_no_audit_account() {
        let mut c = config();
        c["audit"] = json!({"mode":"grant_enforced"});
        assert!(parse(&c).unwrap().audit.is_none());
        for extra in [
            json!({"mode":"grant_enforced","user":"u"}),
            json!({"mode":"other"}),
            json!({}),
        ] {
            c["audit"] = extra;
            assert!(parse(&c).is_err());
        }
    }
}
