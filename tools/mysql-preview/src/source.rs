//! One read-only MySQL business connection. No SET and no DML.

use arrow_ipc::writer::StreamWriter;
use std::time::{Duration, Instant};
use std::{fs::File, path::Path};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts, Value};
use oo_connectors::PG17_RESERVED;
use oo_connectors::types::{RunBinding, TypedBatch, assemble_batch};
use oo_pipeline::catalog_import::{push_schema_column, push_schema_table};
use oo_pipeline::copy_encoder::{
    CopyBudgets, CopyColumn, CopyEncoder, CopyPiece, ExactDecimal, MapDecision, UnavailablePolicy,
    arrow_dimensions,
};
use oo_pipeline::copy_sink::hash_batch;
use oo_pipeline::full_import::{QualityReject, hook_digest, quality_preimage};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value as JsonValue, json};
use tokio::time::timeout;

use crate::config::{PreviewConfig, SelectedTable, mysql_ident};
use crate::error::{PreviewError, redact};
use crate::map::{
    MappedColumn, MysqlColumn, grant_is_read_only, map_table, mariadb_comment, session_mode_ok,
    session_utc,
};
use crate::values::{self, RawCell};

#[derive(Clone, Debug)]
pub struct StatementNote {
    pub verb: String,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct TableBatch {
    pub index: u32,
    pub schema: String,
    pub name: String,
    pub logical_schema: String,
    pub logical_table: String,
    pub columns: Vec<MappedColumn>,
    pub batches: Vec<EncodedBatch>,
    pub rejects: Vec<JsonValue>,
}

#[derive(Clone, Debug)]
pub struct EncodedBatch {
    pub batch_id: String,
    pub rows: u64,
    pub payload_digest: String,
    pub applied_pos: String,
    pub row_hashes: Vec<u8>,
    pub copy_bytes: Vec<u8>,
    pub quality_rejects: Vec<QualityReject>,
}

pub struct SourceRead {
    pub version: String,
    pub version_comment: String,
    pub sql_mode: String,
    pub time_zone: String,
    pub ssl_cipher: String,
    pub current_user: String,
    pub statements: Vec<StatementNote>,
    pub tables: Vec<TableBatch>,
    pub config_digest: String,
    pub type_digest: String,
    pub input_digest: String,
    pub input_bytes: [u8; 32],
    pub schema_digest: String,
    pub hook_digest: String,
}

pub(crate) async fn open_mysql(
    config: &PreviewConfig,
    password: &str,
) -> Result<Conn, PreviewError> {
    connect(config, password).await
}

pub async fn read_source(
    config: &PreviewConfig,
    password: &str,
    config_bytes: &[u8],
) -> Result<SourceRead, PreviewError> {
    read_source_export(config, password, config_bytes, None).await
}

pub async fn read_source_export(
    config: &PreviewConfig,
    password: &str,
    config_bytes: &[u8],
    export_dir: Option<&Path>,
) -> Result<SourceRead, PreviewError> {
    let started = Instant::now();
    let mut conn = connect(config, password).await?;
    let mut statements = Vec::new();
    let version = scalar(&mut conn, &mut statements, "SELECT VERSION()").await?;
    let comment = scalar(&mut conn, &mut statements, "SELECT @@version_comment").await?;
    if mariadb_comment(&comment) {
        drop(conn);
        return Err(PreviewError::new("version", "mariadb"));
    }
    crate::version::ServerVersion::parse(&version)?;
    let sql_mode = scalar(&mut conn, &mut statements, "SELECT @@session.sql_mode").await?;
    if !session_mode_ok(&sql_mode) {
        drop(conn);
        return Err(PreviewError::new("session", "sql_mode"));
    }
    let time_zone = scalar(&mut conn, &mut statements, "SELECT @@session.time_zone").await?;
    let cipher = show_cipher(&mut conn, &mut statements).await?;
    if cipher.is_empty() {
        drop(conn);
        return Err(PreviewError::new("tls", "ssl_cipher"));
    }
    let current_user = scalar(&mut conn, &mut statements, "SELECT CURRENT_USER()").await?;
    if !current_user_is(&current_user, &config.mysql_user) {
        drop(conn);
        return Err(PreviewError::new("privilege", "current_user"));
    }
    let grants = show_grants(&mut conn, &mut statements).await?;
    if grants.is_empty()
        || grants.iter().any(|line| !grant_is_read_only(line))
        || !grants
            .iter()
            .any(|line| line.to_ascii_uppercase().contains("SELECT"))
    {
        drop(conn);
        return Err(PreviewError::new("privilege", "grants"));
    }
    let reserved: std::collections::BTreeSet<String> = PG17_RESERVED
        .iter()
        .map(|word| (*word).to_string())
        .collect();
    let utc = session_utc(&time_zone);
    let mut tables = Vec::new();
    for (index, selected) in config.tables.iter().enumerate() {
        let columns = catalog_columns(&mut conn, &mut statements, selected).await?;
        let mapped = keep_requested(
            map_table(&columns, config.boolean_tinyint1, utc, &reserved)?,
            &selected.columns,
        )?;
        let logical_schema = oo_connectors::stable_ident(&selected.schema, &reserved)
            .map_err(|_| PreviewError::new("ident", "ident"))?;
        let logical_table = oo_connectors::stable_ident(&selected.name, &reserved)
            .map_err(|_| PreviewError::new("ident", "ident"))?;
        tables.push(TableBatch {
            index: u32::try_from(index).map_err(|_| PreviewError::new("config", "tables"))?,
            schema: selected.schema.clone(),
            name: selected.name.clone(),
            logical_schema,
            logical_table,
            columns: mapped,
            batches: Vec::new(),
            rejects: Vec::new(),
        });
    }
    let digests = digests_of(config, config_bytes, &tables);
    if config.stop != crate::config::Stop::Access {
        query_drop(
            &mut conn,
            &mut statements,
            "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
        )
        .await?;
        for table in &mut tables {
            if matches!(config.stop, crate::config::Stop::PartialAfterFirst) && table.index > 0 {
                continue;
            }
            read_table(
                config,
                &mut conn,
                &mut statements,
                table,
                &digests,
                export_dir,
            )
            .await?;
        }
        query_drop(&mut conn, &mut statements, "COMMIT").await?;
    }
    drop(conn);
    past_snapshot(config, started)?;
    Ok(SourceRead {
        version,
        version_comment: comment,
        sql_mode,
        time_zone,
        ssl_cipher: cipher,
        current_user,
        statements,
        tables,
        config_digest: digests.config_digest,
        type_digest: digests.type_digest,
        input_digest: digests.input_digest,
        input_bytes: digests.input_bytes,
        schema_digest: digests.schema_digest,
        hook_digest: digests.hook_digest,
    })
}

struct Digests {
    config_digest: String,
    type_digest: String,
    input_digest: String,
    input_bytes: [u8; 32],
    schema_digest: String,
    hook_digest: String,
}

fn past_snapshot(config: &PreviewConfig, started: Instant) -> Result<(), PreviewError> {
    if started.elapsed() > Duration::from_millis(config.snapshot_deadline_ms) {
        return Err(PreviewError::new("budget", "snapshot_deadline"));
    }
    Ok(())
}

pub(crate) fn sealed_identity(
    source: &SourceRead,
    config: &PreviewConfig,
) -> Result<(), PreviewError> {
    if !current_user_is(&source.current_user, &config.mysql_user) {
        return Err(PreviewError::new("privilege", "current_user"));
    }
    let rendered = source
        .input_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if rendered != source.input_digest {
        return Err(PreviewError::new("target", "input_digest"));
    }
    Ok(())
}

fn digests_of(config: &PreviewConfig, config_bytes: &[u8], tables: &[TableBatch]) -> Digests {
    let config_digest = sha256_hex(config_bytes);
    let mut schema = String::new();
    for table in tables {
        push_schema_table(&mut schema, &table.logical_schema, &table.logical_table);
        for column in &table.columns {
            push_schema_column(
                &mut schema,
                &column.logical_name,
                column.oid,
                column.typmod,
                column.caveat,
            );
        }
    }
    let schema_digest = sha256_hex(schema.as_bytes());
    let type_digest = sha256_hex(b"oo.mysql-preview.types/v1");
    let input_material = format!(
        "{schema_digest}\n{}\n{}",
        config.generation, config.source_id
    );
    let input_digest = sha256_hex(input_material.as_bytes());
    let mut input_bytes = [0u8; 32];
    for (index, byte) in input_digest.as_bytes().chunks(2).enumerate() {
        let hex = std::str::from_utf8(byte).unwrap_or("00");
        input_bytes[index] = u8::from_str_radix(hex, 16).unwrap_or(0);
    }
    Digests {
        config_digest,
        type_digest,
        input_digest,
        input_bytes,
        schema_digest,
        hook_digest: hook_digest(),
    }
}

async fn connect(config: &PreviewConfig, password: &str) -> Result<Conn, PreviewError> {
    let ssl = tls_opts(&config.mysql_ca_file)?;
    // mysql_async 0.36 OptsBuilder has no TCP connect timeout. Conn::new is bounded below.
    let opts = OptsBuilder::default()
        .ip_or_hostname(config.mysql_host.clone())
        .tcp_port(config.mysql_port)
        .user(Some(config.mysql_user.clone()))
        .pass(Some(password.to_string()))
        .db_name(Some(config.mysql_database.clone()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    let connect = timeout(
        Duration::from_millis(config.connect_timeout_ms),
        Conn::new(opts),
    )
    .await;
    match connect {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => Err(driver_error(&err.to_string(), password, "connect")),
        Err(_) => Err(PreviewError::new("connect", "connect_timeout")),
    }
}

fn tls_opts(ca: &std::path::Path) -> Result<SslOpts, PreviewError> {
    let path = ca.to_path_buf();
    Ok(SslOpts::default()
        .with_root_certs(vec![path.into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true))
}

fn driver_error(text: &str, secret: &str, fact: &'static str) -> PreviewError {
    if redact(text, secret) {
        PreviewError::new("connect", "redacted")
    } else {
        let _ = text;
        PreviewError::new("connect", fact)
    }
}

async fn scalar(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    sql: &str,
) -> Result<String, PreviewError> {
    note(notes, sql);
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    row_string(&row, 0)
}

async fn query_drop(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    sql: &str,
) -> Result<(), PreviewError> {
    note(notes, sql);
    timeout(Duration::from_secs(30), conn.query_drop(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    Ok(())
}

async fn show_cipher(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
) -> Result<String, PreviewError> {
    let sql = "SHOW STATUS LIKE 'Ssl_cipher'";
    note(notes, sql);
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("tls", "ssl_cipher"))?;
    let row = row.ok_or_else(|| PreviewError::new("tls", "ssl_cipher"))?;
    row_string(&row, 1)
}

async fn show_grants(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
) -> Result<Vec<String>, PreviewError> {
    let sql = "SHOW GRANTS FOR CURRENT_USER()";
    note(notes, sql);
    let rows: Vec<Row> = timeout(Duration::from_secs(30), conn.query(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("privilege", "grants"))?;
    let mut lines = Vec::new();
    for row in rows {
        lines.push(row_string(&row, 0)?);
    }
    Ok(lines)
}

pub(crate) async fn catalog_columns(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    selected: &SelectedTable,
) -> Result<Vec<MysqlColumn>, PreviewError> {
    if !mysql_ident(&selected.schema) || !mysql_ident(&selected.name) {
        return Err(PreviewError::new("ident", "ident"));
    }
    let sql = "SELECT COLUMN_NAME, ORDINAL_POSITION, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE, \
               CHARACTER_MAXIMUM_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, CHARACTER_SET_NAME, COLUMN_KEY \
               FROM information_schema.COLUMNS \
               WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
    note(notes, sql);
    let rows: Vec<Row> = timeout(
        Duration::from_secs(30),
        conn.exec(sql, (selected.schema.as_str(), selected.name.as_str())),
    )
    .await
    .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
    .map_err(|_| PreviewError::new("catalog", "columns"))?;
    if rows.is_empty() {
        return Err(PreviewError::new("catalog", "missing_table"));
    }
    let mut columns = Vec::new();
    for row in rows {
        columns.push(MysqlColumn {
            name: row_string(&row, 0)?,
            ordinal: i32::try_from(row_i64(&row, 1)?)
                .map_err(|_| PreviewError::new("catalog", "ordinal"))?,
            data_type: row_string(&row, 2)?,
            column_type: row_string(&row, 3)?,
            nullable: row_string(&row, 4)? == "YES",
            char_len: row_opt_i64(&row, 5)?,
            precision: row_opt_i64(&row, 6)?.map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
            scale: row_opt_i64(&row, 7)?.map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
            charset: row_opt_string(&row, 8)?,
            column_key: row_string(&row, 9)?,
        });
        if !mysql_ident(&columns.last().expect("pushed").name) {
            return Err(PreviewError::new("ident", "ident"));
        }
    }
    Ok(columns)
}

async fn read_table(
    config: &PreviewConfig,
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    table: &mut TableBatch,
    digests: &Digests,
    export_dir: Option<&Path>,
) -> Result<(), PreviewError> {
    let export_path = export_dir.map(|dir| dir.join(format!("table-{}.arrow", table.index)));
    let mut arrow_writer: Option<StreamWriter<File>> = None;
    let key = table
        .columns
        .iter()
        .find(|column| column.key)
        .ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    let key_name = key.plan.name.clone();
    let list = select_list(table)?;
    let mut sent = 0u64;
    let mut batch_index = 0u64;
    let mut last_key: Option<String> = None;
    let mut bytes = 0u64;
    loop {
        if sent >= config.max_rows {
            return Err(PreviewError::new("budget", "max_rows"));
        }
        let limit = config.batch_rows.min(config.max_rows - sent);
        let (sql, args_note) = page_sql(
            &table.schema,
            &table.name,
            &list,
            &key_name,
            last_key.as_deref(),
        );
        note(notes, &sql);
        let rows = fetch_page(
            conn,
            &sql,
            last_key.as_deref(),
            limit,
            table.columns.len(),
            bytes,
            config.max_bytes,
        )
        .await?;
        let short = (rows.len() as u64) < limit;
        if rows.is_empty() {
            if table.batches.is_empty() {
                let encoded = encode_page(
                    config,
                    table,
                    digests,
                    &[],
                    batch_index,
                    &mut bytes,
                    export_path.as_deref(),
                    &mut arrow_writer,
                )?;
                table.batches.push(encoded);
            }
            break;
        }
        let encoded = encode_page(
            config,
            table,
            digests,
            &rows,
            batch_index,
            &mut bytes,
            export_path.as_deref(),
            &mut arrow_writer,
        )?;
        sent += encoded.rows;
        last_key = page_last_key(&rows, table);
        table.batches.push(encoded);
        batch_index += 1;
        let _ = args_note;
        if short || last_key.is_none() {
            break;
        }
    }
    if let Some(writer) = arrow_writer.as_mut() {
        writer
            .finish()
            .map_err(|_| PreviewError::new("batch", "arrow_finish"))?;
    }
    Ok(())
}

fn select_list(table: &TableBatch) -> Result<String, PreviewError> {
    let mut list = String::new();
    for column in &table.columns {
        if !list.is_empty() {
            list.push_str(", ");
        }
        list.push('`');
        list.push_str(&column.plan.name);
        list.push('`');
    }
    Ok(list)
}

fn page_sql(
    schema: &str,
    name: &str,
    list: &str,
    key: &str,
    last: Option<&str>,
) -> (String, &'static str) {
    let predicate = if last.is_some() {
        format!(" WHERE `{key}` > ?")
    } else {
        String::new()
    };
    (
        format!("SELECT {list} FROM `{schema}`.`{name}`{predicate} ORDER BY `{key}` LIMIT ?"),
        "page",
    )
}

fn keep_requested(
    mapped: Vec<MappedColumn>,
    requested: &[String],
) -> Result<Vec<MappedColumn>, PreviewError> {
    if requested.is_empty() {
        return Ok(mapped);
    }
    let mut kept = Vec::with_capacity(requested.len());
    for name in requested {
        let Some(column) = mapped
            .iter()
            .find(|column| column.plan.name == *name || column.logical_name == *name)
        else {
            return Err(PreviewError::new("catalog", "missing_column"));
        };
        if kept
            .iter()
            .any(|item: &MappedColumn| item.plan.name == column.plan.name)
        {
            return Err(PreviewError::new("catalog", "missing_column"));
        }
        kept.push(column.clone());
    }
    if kept.iter().filter(|column| column.key).count() != 1 {
        return Err(PreviewError::new("catalog", "primary_key"));
    }
    Ok(kept)
}

/// Pull at most one page. Byte accounting happens as each row arrives.
/// Rows past `max_bytes` are not buffered, and the rest of the result is not read.
async fn fetch_page(
    conn: &mut Conn,
    sql: &str,
    last: Option<&str>,
    limit: u64,
    column_count: usize,
    bytes_so_far: u64,
    max_bytes: u64,
) -> Result<Vec<Row>, PreviewError> {
    let started = if let Some(last) = last {
        timeout(Duration::from_secs(30), conn.exec_iter(sql, (last, limit))).await
    } else {
        timeout(Duration::from_secs(30), conn.exec_iter(sql, (limit,))).await
    };
    let mut result = started
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let mut rows = Vec::new();
    let mut pending_bytes = bytes_so_far;
    loop {
        if rows.len() as u64 >= limit {
            break;
        }
        let next = timeout(Duration::from_secs(30), result.next())
            .await
            .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
            .map_err(|_| PreviewError::new("protocol", "query"))?;
        let Some(row) = next else {
            return Ok(rows);
        };
        for index in 0..column_count {
            let add = match raw_cell(&row, index)? {
                RawCell::Bytes(value) => value.len() as u64,
                _ => 8,
            };
            if pending_bytes.saturating_add(add) > max_bytes {
                // The overflowing row is not stored. The caller drops the connection.
                return Err(PreviewError::new("budget", "max_bytes"));
            }
            pending_bytes = pending_bytes.saturating_add(add);
        }
        rows.push(row);
    }
    // LIMIT already bounds the server. Drain the terminator so the next page can run.
    loop {
        let next = timeout(Duration::from_secs(30), result.next())
            .await
            .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
            .map_err(|_| PreviewError::new("protocol", "query"))?;
        if next.is_none() {
            break;
        }
    }
    Ok(rows)
}

fn encode_page(
    config: &PreviewConfig,
    table: &mut TableBatch,
    digests: &Digests,
    rows: &[Row],
    batch_index: u64,
    bytes: &mut u64,
    export_path: Option<&Path>,
    arrow_writer: &mut Option<StreamWriter<File>>,
) -> Result<EncodedBatch, PreviewError> {
    let mut decoded = Vec::with_capacity(rows.len());
    let mut quality_rejects = Vec::new();
    let mut pending_rejects = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(table.columns.len());
        for (column_index, column) in table.columns.iter().enumerate() {
            let raw = raw_cell(row, column_index)?;
            *bytes = bytes.saturating_add(match &raw {
                RawCell::Bytes(value) => value.len() as u64,
                _ => 8,
            });
            if *bytes > config.max_bytes {
                return Err(PreviewError::new("budget", "max_bytes"));
            }
            let (cell, reject) =
                values::decode_cell(column.store, &raw, column.plan.mapping.nullable)?;
            if let Some(note) = reject {
                let refused = values::reject_for(&note);
                let value_len = i64::try_from(refused.value_len)
                    .map_err(|_| PreviewError::new("value", "value_len"))?;
                let row_number = i64::try_from(row_index)
                    .map_err(|_| PreviewError::new("budget", "max_rows"))?;
                pending_rejects.push((
                    column.logical_name.clone(),
                    refused.reason,
                    column.plan.attnum,
                    row_number,
                    value_len,
                ));
                quality_rejects.push(QualityReject {
                    reason: refused.reason.to_string(),
                    value_len,
                    attnum: column.plan.attnum,
                    row_index: row_number,
                    key_digest: String::new(),
                });
            }
            cells.push(cell);
        }
        decoded.push(cells);
    }
    let binding = RunBinding {
        source_id: config.source_id.clone(),
        schema: table.schema.clone(),
        table: table.name.clone(),
        run_generation: config.generation,
        input_digest: digests.input_bytes,
    };
    let key_attnum = table
        .columns
        .iter()
        .find(|column| column.key)
        .map(|column| column.plan.attnum)
        .ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    let plans = table
        .columns
        .iter()
        .map(|column| column.plan.clone())
        .collect();
    let typed = assemble_batch(&binding, plans, &[key_attnum], decoded)
        .map_err(|_| PreviewError::new("batch", "assemble"))?;
    if let Some(path) = export_path {
        let batch = typed
            .record_batch()
            .map_err(|_| PreviewError::new("batch", "arrow"))?;
        if arrow_writer.is_none() {
            let file = File::create(path).map_err(|_| PreviewError::new("batch", "arrow_file"))?;
            *arrow_writer = Some(
                StreamWriter::try_new(file, &batch.schema())
                    .map_err(|_| PreviewError::new("batch", "arrow_schema"))?,
            );
        }
        if let Some(writer) = arrow_writer.as_mut() {
            writer
                .write(&batch)
                .map_err(|_| PreviewError::new("batch", "arrow_write"))?;
        }
    }
    fill_key_digests(&typed, &mut quality_rejects);
    for (pending, quality) in pending_rejects.iter().zip(quality_rejects.iter()) {
        table.rejects.push(json!({
            "attnum": pending.2,
            "column": pending.0,
            "key_digest": quality.key_digest,
            "reason": pending.1,
            "row": pending.3,
            "table": table.logical_table,
            "value_len": pending.4
        }));
    }
    let (rows_n, cols_n) =
        arrow_dimensions(&typed).map_err(|_| PreviewError::new("batch", "arrow"))?;
    if rows_n != typed.rows.len() || cols_n != table.columns.len() {
        return Err(PreviewError::new("batch", "arrow"));
    }
    encode_typed(table, digests, &typed, batch_index, config, quality_rejects)
}

fn encode_typed(
    table: &TableBatch,
    digests: &Digests,
    typed: &TypedBatch,
    batch_index: u64,
    config: &PreviewConfig,
    quality_rejects: Vec<QualityReject>,
) -> Result<EncodedBatch, PreviewError> {
    let start = table.batches.iter().map(|batch| batch.rows).sum::<u64>();
    let mut positions = Vec::with_capacity(typed.rows.len());
    for offset in 0..typed.rows.len() {
        let index = start + offset as u64 + 1;
        positions.push(
            ExactDecimal::new(false, index.to_string(), 0)
                .map_err(|_| PreviewError::new("batch", "position"))?,
        );
    }
    let mut columns = Vec::with_capacity(typed.plans.len() + 1);
    columns.push(CopyColumn::SystemPosition);
    for column in &typed.plans {
        if column.mapping.decision == MapDecision::Lossless {
            columns.push(CopyColumn::User {
                attnum: column.attnum,
            });
        }
    }
    let budgets = copy_budgets(config)?;
    let first = pull_bytes(typed, &columns, &positions, budgets)?;
    let second = pull_bytes(typed, &columns, &positions, budgets)?;
    if first != second {
        return Err(PreviewError::new("batch", "encode"));
    }
    let hashed = hash_batch(typed, &columns, &positions, budgets)
        .map_err(|_| PreviewError::new("batch", "encode"))?;
    if hashed.rows != typed.rows.len() as u64 {
        return Err(PreviewError::new("batch", "encode"));
    }
    let batch_id = format!("t{}.b{batch_index}", table.index);
    let applied_pos = positions
        .last()
        .map(ExactDecimal::to_plain_string)
        .unwrap_or_else(|| "0".to_string());
    let rejected = u64::try_from(quality_rejects.len())
        .map_err(|_| PreviewError::new("budget", "max_rows"))?;
    let quality = quality_preimage(
        &batch_id,
        typed.rows.len() as u64,
        rejected,
        &digests.input_digest,
        &quality_rejects,
    );
    let mut mixed = first.clone();
    mixed.extend(quality);
    let mut row_hashes = Vec::with_capacity(typed.rows.len() * 32);
    for row in &typed.rows {
        row_hashes.extend_from_slice(&row.canonical_hash);
    }
    Ok(EncodedBatch {
        batch_id,
        rows: typed.rows.len() as u64,
        payload_digest: sha256_hex(&mixed),
        applied_pos,
        row_hashes,
        copy_bytes: first,
        quality_rejects,
    })
}

fn fill_key_digests(typed: &TypedBatch, rejects: &mut [QualityReject]) {
    for reject in rejects {
        let Ok(index) = usize::try_from(reject.row_index) else {
            continue;
        };
        let Some(row) = typed.rows.get(index) else {
            continue;
        };
        let Some(digest) = row.key_digest else {
            continue;
        };
        reject.key_digest = hex32(&digest);
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Access document for one captured read. It has no physical names.
pub fn access_document(source: &SourceRead) -> JsonValue {
    json!({
        "protocol": "oo.mysql-preview/v1",
        "state": "access",
        "mysql_version": source.version,
        "version_comment": source.version_comment,
        "sql_mode": source.sql_mode,
        "time_zone": source.time_zone,
        "ssl_cipher_present": !source.ssl_cipher.is_empty(),
        "config_digest": source.config_digest,
        "statements": source.statements.iter().map(|note| json!({"verb": note.verb, "sha256": note.sha256})).collect::<Vec<_>>(),
        "tables": source.tables.iter().map(|table| json!({
            "schema": table.logical_schema,
            "table": table.logical_table,
            "columns": table.columns.iter().map(|column| json!({
                "name": column.logical_name,
                "oid": column.oid,
                "typmod": column.typmod,
                "caveat": column.caveat
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

fn pull_bytes(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    budgets: CopyBudgets,
) -> Result<Vec<u8>, PreviewError> {
    let mut encoder = CopyEncoder::open(
        batch,
        columns,
        positions,
        budgets,
        UnavailablePolicy::Refuse,
    )
    .map_err(|_| PreviewError::new("batch", "encode"))?;
    let mut out = Vec::new();
    loop {
        match encoder
            .pull()
            .map_err(|_| PreviewError::new("batch", "encode"))?
        {
            CopyPiece::Done => break,
            CopyPiece::Header { bytes }
            | CopyPiece::Trailer { bytes }
            | CopyPiece::Rows { bytes, .. } => {
                out.extend_from_slice(&bytes);
            }
        }
    }
    Ok(out)
}

fn copy_budgets(config: &PreviewConfig) -> Result<CopyBudgets, PreviewError> {
    let max_value =
        u32::try_from(config.max_bytes).map_err(|_| PreviewError::new("budget", "max_bytes"))?;
    let max_row = max_value
        .checked_mul(2)
        .ok_or_else(|| PreviewError::new("budget", "max_bytes"))?;
    let max_chunk = max_row
        .checked_mul(2)
        .ok_or_else(|| PreviewError::new("budget", "max_bytes"))?;
    let batch_rows =
        u32::try_from(config.batch_rows).map_err(|_| PreviewError::new("budget", "batch_rows"))?;
    Ok(CopyBudgets {
        max_value_bytes: max_value,
        max_row_bytes: max_row.max(64),
        max_chunk_bytes: max_chunk.max(64),
        max_rows_per_chunk: batch_rows,
    })
}

fn page_last_key(rows: &[Row], table: &TableBatch) -> Option<String> {
    let key_index = table.columns.iter().position(|column| column.key)?;
    let row = rows.last()?;
    row_string(row, key_index).ok()
}

fn raw_cell(row: &Row, index: usize) -> Result<RawCell, PreviewError> {
    let value: Value = row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?;
    Ok(match value {
        Value::NULL => RawCell::Null,
        Value::Int(value) => RawCell::Int(value),
        Value::UInt(value) => RawCell::Uint(value),
        Value::Float(value) => RawCell::Float(value),
        Value::Double(value) => RawCell::Double(value),
        Value::Bytes(value) => RawCell::Bytes(value),
        Value::Date(year, month, day, hour, minute, second, micros) => RawCell::Date {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
        Value::Time(negative, days, hours, minutes, seconds, micros) => RawCell::Time {
            negative,
            days,
            hours,
            minutes,
            seconds,
            micros,
        },
    })
}

fn row_string(row: &Row, index: usize) -> Result<String, PreviewError> {
    match raw_cell(row, index)? {
        RawCell::Bytes(bytes) => {
            String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))
        }
        RawCell::Int(value) => Ok(value.to_string()),
        RawCell::Uint(value) => Ok(value.to_string()),
        RawCell::Null => Err(PreviewError::new("protocol", "cell")),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn row_opt_string(row: &Row, index: usize) -> Result<Option<String>, PreviewError> {
    match raw_cell(row, index)? {
        RawCell::Null => Ok(None),
        RawCell::Bytes(bytes) => Ok(Some(
            String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))?,
        )),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn row_i64(row: &Row, index: usize) -> Result<i64, PreviewError> {
    match raw_cell(row, index)? {
        RawCell::Int(value) => Ok(value),
        RawCell::Uint(value) => {
            i64::try_from(value).map_err(|_| PreviewError::new("protocol", "cell"))
        }
        RawCell::Bytes(bytes) => {
            let text =
                String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))?;
            text.parse()
                .map_err(|_| PreviewError::new("protocol", "cell"))
        }
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn row_opt_i64(row: &Row, index: usize) -> Result<Option<i64>, PreviewError> {
    match raw_cell(row, index)? {
        RawCell::Null => Ok(None),
        _ => row_i64(row, index).map(Some),
    }
}

fn current_user_is(current: &str, user: &str) -> bool {
    let account = current.split('@').next().unwrap_or("");
    !account.is_empty() && account.eq_ignore_ascii_case(user)
}

fn note(notes: &mut Vec<StatementNote>, sql: &str) {
    let verb = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    notes.push(StatementNote {
        verb,
        sha256: sha256_hex(sql.as_bytes()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_quality_preimage_matches_the_historical_line() {
        let bytes = quality_preimage("t0.b0", 5, 0, "abc", &[]);
        assert_eq!(bytes, b"full-import/v1 quality\nbatch\tt0.b0\t5\t0\tabc\n");
    }

    #[test]
    fn a_masked_reject_is_a_quality_line_without_the_value() {
        let bytes = quality_preimage(
            "t0.b0",
            5,
            1,
            "abc",
            &[QualityReject {
                reason: "zero_date".to_string(),
                value_len: 10,
                attnum: 12,
                row_index: 1,
                key_digest: String::new(),
            }],
        );
        let text = String::from_utf8(bytes).expect("utf8");
        assert_eq!(
            text,
            "full-import/v1 quality\nbatch\tt0.b0\t5\t1\tabc\nreject\tzero_date\t10\t12\t1\t\tabc\n"
        );
        assert!(!text.contains("0000-00-00"));
    }

    #[test]
    fn key_digest_text_is_lowercase_hex() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xab;
        bytes[31] = 0x0f;
        let text = hex32(&bytes);
        assert_eq!(text.len(), 64);
        assert!(
            text.chars()
                .all(|char| char.is_ascii_hexdigit() && !char.is_ascii_uppercase())
        );
        assert!(text.starts_with("ab"));
        assert!(text.ends_with("0f"));
    }
}
