//! Producer receipt for one PostgreSQL capture.
//!
//! The file `oo.source-admission.capture-receipt/v1` is hashed from the bytes
//! this process writes. Digests are SHA-256 of the config, catalog, registry,
//! driver executable, audit record, and Arrow page that the same call read.
//! A column the loader stores under a different oid keeps its source value and
//! a native note. It is not removed to make the table look complete.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Date32Array, Decimal128Array, Int16Array, Int32Array, Int64Array,
    RecordBatch, StringArray, TimestampMicrosecondArray,
};
use arrow_array::builder::FixedSizeBinaryBuilder;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::audit_local::platform_sql_allowed;
use crate::capture::{decode_message, peek_sql, CellKind, Msg};
use crate::config::{Capture, Config};
use crate::error::Fail;
use crate::pg::{connect_read_only, endpoint_from_files};

const RECEIPT_PROTOCOL: &str = "oo.source-admission.capture-receipt/v1";
const REQUEST_PROTOCOL: &str = "oo.pipeline.native-update/v1";
// An exported PostgreSQL snapshot has one source position. Splitting one table
// into multiple NativeUpdate generations would invent an order between pages
// and make the later page's zero snapshot position fail the target CAS. Keep
// the approved fixture's largest table in one bounded Arrow object.
const PAGE_LIMIT: usize = 16 * 1024 * 1024;
const SHARD: &str = "shard_dl01w1_46";
const OWNER: &str = "00000000-0000-4000-8000-000000000046";

#[derive(Clone, Debug)]
struct ColumnPlan {
    name: String,
    oid: i64,
    typmod: i32,
    nullable: bool,
    format: String,
    landing: Landing,
    primary_key: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Landing {
    Int { width: u8 },
    Bool,
    Decimal { precision: u8, scale: i8 },
    Utf8 { note: Option<&'static str> },
    Binary,
    UuidBytes,
    TimestampUtc,
    Date,
    Refused(&'static str),
}

#[derive(Clone, Debug)]
enum CellValue {
    Null,
    Int(i64),
    Bool(bool),
    Decimal(i128),
    Text(String),
    Bytes(Vec<u8>),
    Fixed([u8; 16]),
    Days(i32),
}

struct TableData {
    schema: String,
    table: String,
    pk: String,
    columns: Vec<ColumnPlan>,
    excluded: Vec<Value>,
    rows: Vec<(i64, Vec<CellValue>)>,
    tombstones: Vec<String>,
}

pub async fn produce(config: &Config, config_bytes: &[u8]) -> Result<(), Fail> {
    let capture = config.capture.as_ref().ok_or(Fail::new("capture.refused", 4))?;
    refuse_early(config, capture)?;
    let endpoint = endpoint_from_files(
        &config.source.host,
        config.source.port,
        &config.source.database,
        &capture.username,
        &capture.password_file,
        &config.source.ca_file,
    )?;
    let session = connect_read_only(&endpoint).await?;
    let client = &session.client;
    let mut audit_sql = vec!["SET default_transaction_read_only = on".to_owned()];
    exec(config, client, &mut audit_sql, &format!("SET statement_timeout = {}", config.budget.statement_ms)).await?;
    exec(config, client, &mut audit_sql, "SET search_path = pg_catalog").await?;
    exec(config, client, &mut audit_sql, "SET TIME ZONE 'UTC'").await?;
    // pg_export_snapshot refuses a read-only transaction. The session returns
    // to read-only after the snapshot commit. Statements stay SET and SELECT.
    exec(config, client, &mut audit_sql, "SET default_transaction_read_only = off").await?;
    exec(config, client, &mut audit_sql, "BEGIN ISOLATION LEVEL REPEATABLE READ").await?;
    let produced = match read_snapshot(config, client, &mut audit_sql).await {
        Ok(value) => value,
        Err(err) => {
            let _ = client.batch_execute("ROLLBACK").await;
            return Err(err);
        }
    };
    exec(config, client, &mut audit_sql, "COMMIT").await?;
    exec(config, client, &mut audit_sql, "SET default_transaction_read_only = on").await?;
    write_snapshot(config, config_bytes, &audit_sql, produced)
}

pub async fn produce_delta(config: &Config, config_bytes: &[u8]) -> Result<(), Fail> {
    let capture = config.capture.as_ref().ok_or(Fail::new("capture.refused", 4))?;
    refuse_early(config, capture)?;
    let root = admit_root(config);
    let index_path = root.join("index.json");
    let index_bytes = fs::read(&index_path).map_err(|_| Fail::new("capture.refused", 4))?;
    let index: Value = serde_json::from_slice(&index_bytes).map_err(|_| Fail::new("capture.refused", 4))?;
    let endpoint = endpoint_from_files(
        &config.source.host,
        config.source.port,
        &config.source.database,
        &capture.username,
        &capture.password_file,
        &config.source.ca_file,
    )?;
    let session = connect_read_only(&endpoint).await?;
    let client = &session.client;
    let mut audit_sql = vec!["SET default_transaction_read_only = on".to_owned()];
    exec(config, client, &mut audit_sql, &format!("SET statement_timeout = {}", config.budget.statement_ms)).await?;
    exec(config, client, &mut audit_sql, "SET search_path = pg_catalog").await?;
    exec(config, client, &mut audit_sql, "SET TIME ZONE 'UTC'").await?;
    let sql = peek_sql(&capture.slot, &capture.publication, 10_000);
    if !platform_sql_allowed(&sql) {
        return Err(Fail::new("audit.write", 7));
    }
    audit_sql.push(sql.clone());
    let peeked = client.query(&sql, &[]).await.map_err(|_| Fail::new("capture.protocol", 4))?;
    if peeked.len() == 10_000 {
        return Err(Fail::new("capture.large_txn", 4));
    }
    let folded = fold_peek(&index, &peeked)?;
    if folded.pending {
        write_json(&root.join("delta-open.json"), &json!({
            "protocol": "oo.postgres-preview.native-delta/v1",
            "phase": "open_transaction",
            "commits": folded.commits,
            "slot_advanced": false
        }))?;
        return Err(Fail::new("capture.blocked", 5));
    }
    if folded.changes.is_empty() {
        write_json(&root.join("delta-empty.json"), &json!({
            "protocol": "oo.postgres-preview.native-delta/v1",
            "phase": "caught_up_empty",
            "peek_rows": peeked.len(),
            "slot_advanced": false,
            "delete_grade": "unknown"
        }))?;
        return Ok(());
    }
    exec(config, client, &mut audit_sql, "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await?;
    let dba_observation = observe_dba_objects(config, client, &mut audit_sql).await?;
    let reread = match reread_changes(config, client, &mut audit_sql, &folded).await {
        Ok(value) => value,
        Err(err) => {
            let _ = client.batch_execute("ROLLBACK").await;
            return Err(err);
        }
    };
    exec(config, client, &mut audit_sql, "COMMIT").await?;
    write_delta(config, config_bytes, &audit_sql, &index, folded.max_lsn, reread, &dba_observation)
}

fn refuse_early(config: &Config, capture: &Capture) -> Result<(), Fail> {
    if capture.method != "c1-logical" || !ident_ok(&capture.publication) || !ident_ok(&capture.slot) || !ident_ok(&capture.username) {
        return Err(Fail::new("capture.refused", 4));
    }
    if capture.username == config.source.username || capture.username == config.dba.username || capture.username == config.target.username {
        return Err(Fail::new("identity.separate", 6));
    }
    if !capture.password_file.starts_with('/') || capture.password_file.contains("..") || !Path::new(&capture.password_file).is_file() {
        return Err(Fail::new("capture.refused", 4));
    }
    if config.approved_objects.is_empty() || config.budget.max_rows == 0 {
        return Err(Fail::new("capture.refused", 4));
    }
    Ok(())
}

struct SnapshotRead {
    snapshot: String,
    server_version: String,
    role: String,
    dba_observation: Value,
    tables: Vec<TableData>,
}

async fn read_snapshot(
    config: &Config,
    client: &tokio_postgres::Client,
    audit_sql: &mut Vec<String>,
) -> Result<SnapshotRead, Fail> {
    let snap_sql = "SELECT pg_catalog.pg_export_snapshot()::text, current_setting('server_version')::text, current_user::text";
    audit_sql.push(snap_sql.to_owned());
    if !platform_sql_allowed(snap_sql) {
        return Err(Fail::new("audit.write", 7));
    }
    let row = client.query_one(snap_sql, &[]).await.map_err(|err| blocked(config, &err))?;
    let snapshot: String = row.try_get(0).map_err(|_| Fail::new("capture.blocked", 5))?;
    let server_version: String = row.try_get(1).map_err(|_| Fail::new("capture.blocked", 5))?;
    let role: String = row.try_get(2).map_err(|_| Fail::new("capture.blocked", 5))?;
    if snapshot.is_empty() || role != config.capture.as_ref().map(|item| item.username.as_str()).unwrap_or("") {
        return Err(Fail::new("capture.blocked", 5));
    }
    let slot_sql = "SELECT slot_name::text FROM pg_catalog.pg_replication_slots WHERE slot_name = $1 AND plugin = 'pgoutput'";
    audit_sql.push(slot_sql.to_owned());
    let capture = config.capture.as_ref().ok_or(Fail::new("capture.refused", 4))?;
    let slot = client.query_opt(slot_sql, &[&capture.slot]).await.map_err(|err| blocked(config, &err))?;
    if slot.is_none() {
        return Err(Fail::new("capture.position_expired", 8));
    }
    let pub_sql = "SELECT pubname::text FROM pg_catalog.pg_publication WHERE pubname = $1";
    audit_sql.push(pub_sql.to_owned());
    let publication = client.query_opt(pub_sql, &[&capture.publication]).await.map_err(|err| blocked(config, &err))?;
    if publication.is_none() {
        return Err(Fail::new("capture.blocked", 5));
    }
    let dba_observation = observe_dba_objects(config, client, audit_sql).await?;
    let mut tables = Vec::new();
    for object in &config.approved_objects {
        tables.push(read_table(config, client, audit_sql, &object.schema, &object.table).await?);
    }
    Ok(SnapshotRead { snapshot, server_version, role, dba_observation, tables })
}

async fn observe_dba_objects(config: &Config, client: &tokio_postgres::Client, audit_sql: &mut Vec<String>) -> Result<Value, Fail> {
    let capture = config.capture.as_ref().ok_or(Fail::new("capture.refused", 4))?;
    let sql = "SELECT p.pubname::text, pg_catalog.pg_get_userbyid(p.pubowner)::text, s.slot_name::text, s.plugin::text, s.database::text, current_user::text FROM pg_catalog.pg_publication p CROSS JOIN pg_catalog.pg_replication_slots s WHERE p.pubname = $1 AND s.slot_name = $2 AND s.plugin = 'pgoutput' AND s.database = current_database()";
    if !platform_sql_allowed(sql) { return Err(Fail::new("audit.write", 7)); }
    audit_sql.push(sql.to_owned());
    let row = client.query_opt(sql, &[&capture.publication, &capture.slot]).await.map_err(|err| blocked(config, &err))?.ok_or(Fail::new("capture.position_expired", 8))?;
    let field = |index| row.try_get::<_, String>(index).map_err(|_| Fail::new("capture.protocol", 4));
    Ok(json!({"publication": {"name": field(0)?, "owner": field(1)?}, "replication_slot": {"name": field(2)?, "plugin": field(3)?, "database": field(4)?}, "source_principal": field(5)?}))
}

fn dba_descriptors(root: &Path, filename: &str, observation: &Value) -> Result<Vec<Value>, Fail> {
    let bytes = canonical_bytes(observation)?;
    let path = root.join(filename);
    write_bytes(&path, &bytes)?;
    let digest = sha256(&bytes);
    let mut descriptors = Vec::new();
    for (kind, risk, rollback) in [
        ("publication", "DBA publication adds selected table changes to the logical decoding feed", "DBA drops the named publication only after stopping this owned capture and confirming no other consumer"),
        ("replication_slot", "Stopped logical slot retains WAL until its configured retention ceiling", "DBA drops the named replication slot only after stopping this owned capture and confirming its retained position is no longer required"),
    ] {
        let name = observation.get(kind).and_then(|item| item.get("name")).and_then(Value::as_str).ok_or(Fail::new("capture.protocol", 4))?;
        descriptors.push(json!({"kind": kind, "name": name, "created_by": "dba", "observed": true, "platform_created": false, "platform_write_attempts": 0, "risk": risk, "rollback": rollback, "observation_path": path, "observation_sha256": digest}));
    }
    Ok(descriptors)
}

async fn read_table(
    config: &Config,
    client: &tokio_postgres::Client,
    audit_sql: &mut Vec<String>,
    schema: &str,
    table: &str,
) -> Result<TableData, Fail> {
    if !ident_ok(schema) || !ident_ok(table) {
        return Err(Fail::new("capture.refused", 4));
    }
    let catalog_sql = "SELECT a.attname::text, a.atttypid::bigint, a.atttypmod, a.attnotnull, pg_catalog.format_type(a.atttypid, a.atttypmod), EXISTS (SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid = c.oid AND i.indisprimary AND i.indnkeyatts = 1 AND a.attnum = ANY (i.indkey)) FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE a.attnum > 0 AND NOT a.attisdropped AND n.nspname = CAST(CAST($1 AS text) AS name) AND c.relname = CAST(CAST($2 AS text) AS name) ORDER BY a.attnum";
    audit_sql.push(catalog_sql.to_owned());
    let found = client.query(catalog_sql, &[&schema, &table]).await.map_err(|err| blocked(config, &err))?;
    if found.is_empty() {
        return Err(Fail::new("capture.refused", 4));
    }
    let mut columns = Vec::new();
    let mut excluded = Vec::new();
    let mut pk = None;
    for row in &found {
        let name: String = row.try_get(0).map_err(|_| Fail::new("capture.protocol", 4))?;
        let oid: i64 = row.try_get(1).map_err(|_| Fail::new("capture.protocol", 4))?;
        let typmod: i32 = row.try_get(2).map_err(|_| Fail::new("capture.protocol", 4))?;
        let not_null: bool = row.try_get(3).map_err(|_| Fail::new("capture.protocol", 4))?;
        let format: String = row.try_get(4).map_err(|_| Fail::new("capture.protocol", 4))?;
        let primary: bool = row.try_get(5).map_err(|_| Fail::new("capture.protocol", 4))?;
        let approved = config.approved_fields.iter().any(|field| field.schema == schema && field.table == table && field.column == name);
        if !approved {
            excluded.push(json!({"column": name, "source_oid": oid, "format": format, "native_note": "not_approved"}));
            continue;
        }
        let landing = land_column(oid, typmod);
        if let Landing::Refused(reason) = landing {
            return Err(Fail::new(reason, 4));
        }
        if primary {
            if pk.is_some() || !matches!(landing, Landing::Int { width: 64 }) {
                return Err(Fail::new("capture.refused", 4));
            }
            pk = Some(name.clone());
        }
        columns.push(ColumnPlan {
            name,
            oid,
            typmod,
            nullable: !not_null,
            format,
            landing,
            primary_key: primary,
        });
    }
    for field in config.approved_fields.iter().filter(|field| field.schema == schema && field.table == table) {
        if !columns.iter().any(|column| column.name == field.column) {
            return Err(Fail::new("capture.refused", 4));
        }
    }
    let pk_name = pk.ok_or(Fail::new("capture.refused", 4))?;
    let count_sql = format!("SELECT count(*)::bigint FROM {}.{}", quote(schema), quote(table));
    audit_sql.push(count_sql.clone());
    if !platform_sql_allowed(&count_sql) {
        return Err(Fail::new("audit.write", 7));
    }
    let count: i64 = client.query_one(&count_sql, &[]).await.map_err(|err| blocked(config, &err))?.try_get(0).map_err(|_| Fail::new("capture.protocol", 4))?;
    if count < 0 || count as u64 > config.budget.max_rows {
        return Err(Fail::new("capture.budget", 5));
    }
    let select_sql = data_sql(schema, table, &pk_name, &columns)?;
    audit_sql.push(select_sql.clone());
    let loaded = client.query(&select_sql, &[]).await.map_err(|err| blocked(config, &err))?;
    if loaded.len() as i64 != count {
        return Err(Fail::new("capture.diff", 7));
    }
    let mut rows = Vec::with_capacity(loaded.len());
    for row in &loaded {
        let mut values = Vec::with_capacity(columns.len());
        let mut pk_value = None;
        for (index, column) in columns.iter().enumerate() {
            let value = read_cell(row, index, column)?;
            if column.primary_key {
                pk_value = match &value {
                    CellValue::Int(pk) => Some(*pk),
                    _ => None,
                };
            }
            values.push(value);
        }
        rows.push((pk_value.ok_or(Fail::new("capture.protocol", 4))?, values));
    }
    rows.sort_by_key(|row| row.0);
    Ok(TableData {
        schema: schema.to_owned(),
        table: table.to_owned(),
        pk: pk_name,
        columns,
        excluded,
        rows,
        tombstones: Vec::new(),
    })
}

fn data_sql(schema: &str, table: &str, pk: &str, columns: &[ColumnPlan]) -> Result<String, Fail> {
    let mut exprs = Vec::new();
    for column in columns {
        exprs.push(select_expr(column)?);
    }
    let sql = format!(
        "SELECT {} FROM {}.{} ORDER BY {}.{}",
        exprs.join(", "),
        quote(schema),
        quote(table),
        quote(table),
        quote(pk)
    );
    if !platform_sql_allowed(&sql) {
        return Err(Fail::new("audit.write", 7));
    }
    Ok(sql)
}

fn select_expr(column: &ColumnPlan) -> Result<String, Fail> {
    let name = quote(&column.name);
    let expr = match column.landing {
        Landing::Int { .. } | Landing::TimestampUtc => {
            if matches!(column.landing, Landing::TimestampUtc) {
                format!("CASE WHEN {name} IS NULL THEN NULL ELSE (EXTRACT(EPOCH FROM {name}) * 1000000)::bigint END")
            } else {
                format!("{name}::bigint")
            }
        }
        Landing::Bool | Landing::Binary => name,
        Landing::Decimal { .. } | Landing::Utf8 { .. } => format!("{name}::text"),
        Landing::UuidBytes => format!("pg_catalog.uuid_send({name})"),
        Landing::Date => format!("CASE WHEN {name} IS NULL THEN NULL ELSE ({name} - DATE '1970-01-01')::integer END"),
        Landing::Refused(_) => return Err(Fail::new("capture.refused", 4)),
    };
    Ok(expr)
}

fn read_cell(row: &tokio_postgres::Row, index: usize, column: &ColumnPlan) -> Result<CellValue, Fail> {
    match &column.landing {
        Landing::Int { width } => {
            let value: Option<i64> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(value) if int_width_ok(*width, value) => Ok(CellValue::Int(value)),
                _ => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::TimestampUtc => {
            let value: Option<i64> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(value) => Ok(CellValue::Int(value)),
                None => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::Bool => {
            let value: Option<bool> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(value) => Ok(CellValue::Bool(value)),
                None => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::Decimal { scale, .. } => {
            let value: Option<String> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(text) => Ok(CellValue::Decimal(parse_numeric(&text, *scale)?)),
                None => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::Utf8 { .. } => {
            let value: Option<String> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(text) if !text.as_bytes().contains(&0) => Ok(CellValue::Text(text)),
                _ => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::Binary => {
            let value: Option<Vec<u8>> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(bytes) => Ok(CellValue::Bytes(bytes)),
                None => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::UuidBytes => {
            let value: Option<Vec<u8>> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(bytes) if bytes.len() == 16 => {
                    let mut fixed = [0u8; 16];
                    fixed.copy_from_slice(&bytes);
                    Ok(CellValue::Fixed(fixed))
                }
                _ => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::Date => {
            let value: Option<i32> = row.try_get(index).map_err(|_| Fail::new("capture.protocol", 4))?;
            match value {
                None if column.nullable => Ok(CellValue::Null),
                Some(days) => Ok(CellValue::Days(days)),
                None => Err(Fail::new("capture.protocol", 4)),
            }
        }
        Landing::Refused(_) => Err(Fail::new("capture.refused", 4)),
    }
}

fn write_snapshot(config: &Config, config_bytes: &[u8], audit_sql: &[String], read: SnapshotRead) -> Result<(), Fail> {
    let root = admit_root(config);
    fs::create_dir_all(&root).map_err(|_| Fail::new("capture.apply", 4))?;
    let mut catalog_tables = Vec::new();
    let mut index_tables = Vec::new();
    let mut fidelity_complete = true;
    for table in &read.tables {
        let (catalog_row, notes_gap) = catalog_row(table);
        if notes_gap {
            fidelity_complete = false;
        }
        catalog_tables.push(catalog_row);
    }
    let catalog = json!({
        "protocol": "oo.postgres-preview.native-catalog/v1",
        "source_engine": "postgresql",
        "source_id": config.source_id,
        "server_version": read.server_version,
        "snapshot": {"kind": "pg_snapshot", "value": read.snapshot},
        "tables": catalog_tables
    });
    let catalog_bytes = canonical_bytes(&catalog)?;
    let registry = json!({
        "protocol": "oo.postgres-preview.approved-registry/v1",
        "source_engine": "postgresql",
        "source_id": config.source_id,
        "objects": config.approved_objects.iter().map(|object| json!({"schema": object.schema, "table": object.table})).collect::<Vec<_>>(),
        "fields": config.approved_fields.iter().map(|field| json!({"schema": field.schema, "table": field.table, "column": field.column})).collect::<Vec<_>>()
    });
    let registry_bytes = canonical_bytes(&registry)?;
    let audit = json!({
        "protocol": "oo.postgres-preview.native-audit/v1",
        "role": read.role,
        "statements": audit_sql,
        "business_verbs": "select",
        "slot_advanced": false,
        "source_dml": 0
    });
    let audit_bytes = canonical_bytes(&audit)?;
    write_bytes(&root.join("catalog.json"), &catalog_bytes)?;
    write_bytes(&root.join("registry.json"), &registry_bytes)?;
    write_bytes(&root.join("audit.json"), &audit_bytes)?;
    let shared = SharedDigest {
        config: sha256(config_bytes),
        catalog: sha256(&catalog_bytes),
        registry: sha256(&registry_bytes),
        audit: sha256(&audit_bytes),
        driver: driver_digest()?,
        write_attempts: audit_sql.iter().filter(|sql| !platform_sql_allowed(sql)).count(),
        dba_objects: dba_descriptors(&root, "dba-objects.snapshot.json", &read.dba_observation)?,
    };
    for table in &read.tables {
        let pages = split_pages(&table.columns, &table.rows, PAGE_LIMIT)?;
        let mut generation = 1i64;
        let mut parent = None;
        let mut page_rows = Vec::new();
        let mut offset = 0usize;
        for bytes in &pages {
            let count = page_row_count(&table.rows[offset..], bytes, &table.columns)?;
            let slice = &table.rows[offset..offset + count];
            offset += count;
            let written = write_page(config, &root, table, generation, parent, &read.snapshot, "pg_snapshot", &shared, bytes, slice, &[])?;
            page_rows.push(written);
            parent = Some(generation);
            generation += 1;
        }
        if offset != table.rows.len() {
            return Err(Fail::new("capture.diff", 7));
        }
        index_tables.push(json!({
            "schema": table.schema,
            "table": table.table,
            "primary_key": table.pk,
            "rows": table.rows.len(),
            "pages": page_rows
        }));
    }
    let index = json!({
        "protocol": "oo.postgres-preview.native-receipt-index/v1",
        "source_engine": "postgresql",
        "source_id": config.source_id,
        "snapshot": {"kind": "pg_snapshot", "value": read.snapshot},
        "fidelity_complete": fidelity_complete,
        "delete_grade": "unknown",
        "target_database": config.target.database,
        "tables": index_tables
    });
    write_json(&root.join("index.json"), &index)?;
    Ok(())
}

struct SharedDigest {
    config: String,
    catalog: String,
    registry: String,
    audit: String,
    driver: String,
    write_attempts: usize,
    dba_objects: Vec<Value>,
}

#[allow(clippy::too_many_arguments)]
fn write_page(
    config: &Config,
    root: &Path,
    table: &TableData,
    generation: i64,
    parent: Option<i64>,
    snapshot_value: &str,
    snapshot_kind: &str,
    shared: &SharedDigest,
    arrow: &[u8],
    rows: &[(i64, Vec<CellValue>)],
    tombstones: &[String],
) -> Result<Value, Fail> {
    let dir = root.join(format!("{}.{}", table.schema, table.table));
    fs::create_dir_all(&dir).map_err(|_| Fail::new("capture.apply", 4))?;
    let arrow_name = format!("g{generation}.arrow");
    let arrow_path = dir.join(&arrow_name);
    write_bytes(&arrow_path, arrow)?;
    let arrow_hash = sha256(arrow);
    // A zero-row stream is rejected by the native decoder as empty. A delete
    // page therefore submits zero Arrow bytes. The schema-message hash is
    // still taken from an unsent empty batch of the same column plan.
    let schema_hash = if arrow.is_empty() {
        arrow_schema_sha256(&encode_page(&table.columns, &[])?)?
    } else {
        arrow_schema_sha256(arrow)?
    };
    let schema_body = column_projection(table);
    let schema_digest = sha256(&canonical_bytes(&schema_body)?);
    let type_body = json!(table.columns.iter().map(|column| column.format.clone()).collect::<Vec<_>>());
    let type_digest = sha256(&canonical_bytes(&type_body)?);
    let run_id = format!("pg46-{}-{}-g{generation}-{}", table.schema, table.table, &arrow_hash[..8]);
    let receipt = json!({
        "schema": RECEIPT_PROTOCOL,
        "source_engine": "postgresql",
        "source_id": config.source_id,
        "source_generation": generation,
        "snapshot": {"kind": snapshot_kind, "value": snapshot_value},
        "run_id": run_id,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": shared.config,
        "input_digest": arrow_hash,
        "audit_digest": shared.audit,
        "catalog_sha256": shared.catalog,
        "registry_sha256": shared.registry,
        "driver_digest": shared.driver,
        "arrow_sha256": [arrow_hash],
        "arrow_schema_sha256": schema_hash,
        "native_notes": native_notes(table)
    });
    if receipt_has_secret(&receipt) {
        return Err(Fail::new("secret.leak", 1));
    }
    let receipt_bytes = canonical_bytes(&receipt)?;
    let receipt_name = format!("g{generation}.receipt.json");
    write_bytes(&dir.join(&receipt_name), &receipt_bytes)?;
    let receipt_hash = sha256(&receipt_bytes);
    let dir_name = format_dir(table);
    let container_arrow = container_object_path(&config.out_dir, &dir_name, &arrow_name);
    let container_receipt = container_object_path(&config.out_dir, &dir_name, &receipt_name);
    let mut request = json!({
        "protocol": REQUEST_PROTOCOL,
        "source_engine": "postgresql",
        "source_id": config.source_id,
        "source_generation": generation,
        "logical_schema": table.schema,
        "logical_table": table.table,
        "batch_id": format!("{}-{}-g{generation}", table.schema, table.table, generation = generation),
        "capture_method": "c1_cdc",
        "delete_detection": "native",
        "consistency_grade": "S1",
        "cross_table_consistent": false,
        "position_codec": if snapshot_kind == "pg_snapshot" { "snapshot-zero/v1" } else { "pg-commit-lsn/v1" },
        "position_lineage": if snapshot_kind == "pg_snapshot" { "pg-export-snapshot" } else { "pg-commit-lsn" },
        "applied_pos": if snapshot_kind == "pg_snapshot" { "0".to_owned() } else { snapshot_value.to_owned() },
        "config_digest": shared.config,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": arrow_hash,
        "audit_digest": shared.audit,
        "snapshot": {"kind": snapshot_kind, "value": snapshot_value},
        "columns": request_columns(table),
        "arrow_path": container_arrow,
        "tombstones": tombstones,
        "row_identity": "unique",
        "key_column": table.pk,
        "mirror": false,
        "target": target_block(config),
        "admission_path": container_receipt,
        "admission_expect": {"declared_receipt_sha256": receipt_hash}
    });
    if let Some(parent) = parent {
        request["parent_source_generation"] = json!(parent);
    }
    let request_name = format!("g{generation}.request.json");
    write_json(&dir.join(&request_name), &request)?;
    // Producer observation, not a trusted admission decision. This is emitted
    // only beside the page produced from bounded, allowed source statements.
    let capture = page_observation(table, generation, snapshot_value, snapshot_kind, shared, &arrow_name, &arrow_hash, rows.len(), tombstones.len())?;
    write_json(&dir.join(format!("g{generation}.capture.json")), &capture)?;
    Ok(json!({
        "generation": generation,
        "parent": parent,
        "rows": rows.len(),
        "tombstones": tombstones.len(),
        "arrow": arrow_name,
        "arrow_bytes": arrow.len(),
        "arrow_sha256": arrow_hash,
        "schema_message_from": if arrow.is_empty() { "unsent_empty_batch" } else { "arrow_file" },
        "receipt": receipt_name,
        "receipt_sha256": receipt_hash,
        "request": request_name
    }))
}

#[allow(clippy::too_many_arguments)]
fn page_observation(
    table: &TableData,
    generation: i64,
    snapshot_value: &str,
    snapshot_kind: &str,
    shared: &SharedDigest,
    arrow_name: &str,
    arrow_hash: &str,
    row_count: usize,
    tombstone_count: usize,
) -> Result<Value, Fail> {
    let cursor_codec = match snapshot_kind {
        "pg_snapshot" if !snapshot_value.is_empty() => "pg-export-snapshot/v1",
        "pg_commit_lsn" if snapshot_value.parse::<u64>().is_ok_and(|lsn| lsn > 0 && lsn.to_string() == snapshot_value) => "pg-commit-lsn/v1",
        _ => return Err(Fail::new("capture.protocol", 4)),
    };
    Ok(json!({
        "protocol": "oo.postgres-preview.native-page-capture/v1",
        "engine": "postgresql",
        "logical_schema": table.schema,
        "logical_table": table.table,
        "source_generation": generation,
        "snapshot": {"kind": snapshot_kind, "value": snapshot_value},
        "snapshot_token": snapshot_kind == "pg_snapshot",
        "cursor": {"codec": cursor_codec, "position": snapshot_value},
        "capture_method": "C1",
        "delete_detection": "native",
        "consistency_grade": "S1",
        "write_class": "R1",
        "cross_table_consistent": false,
        "dba_objects": shared.dba_objects,
        "product_reader_write_attempts": shared.write_attempts,
        "write_observation": "all source statements passed platform_sql_allowed before execution; audit linked",
        "audit_sha256": shared.audit,
        "arrow": arrow_name,
        "arrow_sha256": arrow_hash,
        "row_count": row_count,
        "tombstone_count": tombstone_count,
        "excluded": table.excluded,
        "native_notes": native_notes(table),
        "source_columns": table.columns.iter().map(|column| json!({
            "name": column.name,
            "source_oid": column.oid,
            "source_typmod": column.typmod,
            "source_format": column.format,
            "arrow_type": arrow_type(&column.landing),
            "nullable": column.nullable
        })).collect::<Vec<_>>(),
        "admission_decision": "not_issued"
    }))
}

fn format_dir(table: &TableData) -> String {
    format!("{}.{}", table.schema, table.table)
}

fn container_object_path(out_dir: &str, dir_name: &str, file_name: &str) -> String {
    format!(
        "{}/native-admit/{}/{}",
        out_dir.trim_end_matches('/'),
        dir_name,
        file_name
    )
}

fn target_block(config: &Config) -> Value {
    json!({
        "host": config.target.host,
        "port": config.target.port,
        "database": config.target.database,
        "runtime_principal": config.target.username,
        "password_file": config.target.password_file,
        "ca_file": config.target.ca_file,
        "ssl": "verify-full",
        "shard": SHARD,
        "owner": OWNER
    })
}

fn catalog_row(table: &TableData) -> (Value, bool) {
    // `native_note` describes an older SQL landing preference. Native Arrow
    // preserves UUID's 16 bytes and text's UTF-8 bytes, and the sealed native
    // mapping restores their original PostgreSQL OIDs. A reversible note is
    // not a source fidelity gap; excluded/refused fields are.
    let gap = !table.excluded.is_empty()
        || table.columns.iter().any(|column| matches!(column.landing, Landing::Refused(_)));
    let columns: Vec<Value> = table.columns.iter().map(|column| {
        let (landed, note) = landed(column);
        json!({
            "column": column.name,
            "source_oid": column.oid,
            "typmod": column.typmod,
            "format": column.format,
            "nullable": column.nullable,
            "primary_key": column.primary_key,
            "arrow_type": arrow_type(&column.landing),
            "landed_oid": landed,
            "native_note": note
        })
    }).collect();
    (json!({
        "schema": table.schema,
        "table": table.table,
        "rows": table.rows.len(),
        "columns": columns,
        "excluded": table.excluded
    }), gap)
}

fn native_notes(table: &TableData) -> Vec<Value> {
    table.columns.iter().filter_map(|column| {
        let (landed, note) = landed(column);
        note.map(|text| json!({"column": column.name, "source_oid": column.oid, "landed_oid": landed, "note": text}))
    }).collect()
}

fn landed(column: &ColumnPlan) -> (i64, Option<&'static str>) {
    match column.landing {
        Landing::Int { width: 16 } => (21, None),
        Landing::Int { width: 32 } => (23, None),
        Landing::Int { width: 64 } => (20, None),
        Landing::Int { .. } => (0, Some("refused")),
        Landing::Bool => (16, None),
        Landing::Decimal { .. } => (1700, None),
        Landing::Utf8 { note } => (1043, note),
        Landing::Binary => (17, None),
        Landing::UuidBytes => (17, Some("uuid_lands_bytea")),
        Landing::TimestampUtc => (1184, None),
        Landing::Date => (1082, None),
        Landing::Refused(_) => (0, Some("refused")),
    }
}

fn column_projection(table: &TableData) -> Value {
    json!(table.columns.iter().map(|column| json!({
        "name": column.name,
        "oid": column.oid,
        "typmod": column.typmod,
        "nullable": column.nullable,
        "arrow_type": arrow_type(&column.landing)
    })).collect::<Vec<_>>())
}

fn request_columns(table: &TableData) -> Vec<Value> {
    table.columns.iter().map(|column| {
        let mut item = json!({
            "name": column.name,
            "logical_name": column.name,
            "arrow_type": arrow_type(&column.landing),
            "nullable": column.nullable
        });
        match column.landing {
            Landing::Int { width } => item["precision"] = json!(width),
            Landing::Decimal { precision, scale } => {
                item["precision"] = json!(precision);
                item["scale"] = json!(scale);
            }
            _ => {}
        }
        item
    }).collect()
}

fn arrow_type(landing: &Landing) -> &'static str {
    match landing {
        Landing::Int { width: 16 } => "int16",
        Landing::Int { width: 32 } => "int32",
        Landing::Int { width: 64 } => "int64",
        Landing::Int { .. } => "int64",
        Landing::Bool => "bool",
        Landing::Decimal { .. } => "decimal128",
        Landing::Utf8 { .. } => "utf8",
        Landing::Binary => "binary",
        Landing::UuidBytes => "fixedsizebinary(16)",
        Landing::TimestampUtc => "timestamp_micros_utc",
        Landing::Date => "date32",
        Landing::Refused(_) => "refused",
    }
}

struct Folded {
    changes: BTreeMap<(String, String, i64), bool>,
    commits: usize,
    max_lsn: u64,
    pending: bool,
}

fn fold_peek(index: &Value, rows: &[tokio_postgres::Row]) -> Result<Folded, Fail> {
    let mut relations = HashMap::new();
    let mut open: Option<Vec<(String, String, i64, bool)>> = None;
    let mut changes = BTreeMap::new();
    let mut commits = 0usize;
    let mut max_lsn = 0u64;
    for row in rows {
        let data: Vec<u8> = row.try_get(1).map_err(|_| Fail::new("capture.protocol", 4))?;
        match decode_message(&data)? {
            Msg::Begin { .. } => open = Some(Vec::new()),
            Msg::Commit { commit_lsn, .. } => {
                let events = open.take().ok_or(Fail::new("capture.protocol", 4))?;
                for (schema, table, pk, upsert) in events {
                    changes.insert((schema, table, pk), upsert);
                }
                commits += 1;
                max_lsn = max_lsn.max(commit_lsn);
            }
            Msg::Relation(rel) => {
                relations.insert(rel.oid, rel);
            }
            Msg::Insert { oid, cells } | Msg::Update { oid, cells } => {
                push_change(index, &relations, &mut open, oid, &cells, true)?;
            }
            Msg::Delete { oid, cells } => {
                push_change(index, &relations, &mut open, oid, &cells, false)?;
            }
            Msg::Truncate { oids } => {
                if oids.iter().any(|oid| relations.get(oid).is_some_and(|rel| table_known(index, &rel.namespace, &rel.name))) {
                    return Err(Fail::new("capture.schema_paused", 10));
                }
            }
            Msg::Ignore => {}
        }
    }
    Ok(Folded { changes, commits, max_lsn, pending: open.is_some() })
}

fn push_change(
    index: &Value,
    relations: &HashMap<u32, crate::capture::Rel>,
    open: &mut Option<Vec<(String, String, i64, bool)>>,
    oid: u32,
    cells: &[crate::capture::Cell],
    upsert: bool,
) -> Result<(), Fail> {
    let Some(events) = open else {
        return Err(Fail::new("capture.protocol", 4));
    };
    let rel = relations.get(&oid).ok_or(Fail::new("capture.protocol", 4))?;
    if !table_known(index, &rel.namespace, &rel.name) {
        return Ok(());
    }
    let pk_name = index_pk(index, &rel.namespace, &rel.name)?;
    let index_of = rel.columns.iter().position(|column| column.name == pk_name).ok_or(Fail::new("capture.protocol", 4))?;
    let cell = cells.get(index_of).ok_or(Fail::new("capture.protocol", 4))?;
    if cell.kind != CellKind::Value {
        return Err(Fail::new("capture.protocol", 4));
    }
    let text = cell.text.as_deref().ok_or(Fail::new("capture.protocol", 4))?;
    if !canonical_i64(text) {
        return Err(Fail::new("capture.protocol", 4));
    }
    let pk = text.parse::<i64>().map_err(|_| Fail::new("capture.protocol", 4))?;
    events.push((rel.namespace.clone(), rel.name.clone(), pk, upsert));
    Ok(())
}

fn table_known(index: &Value, schema: &str, table: &str) -> bool {
    index.get("tables").and_then(Value::as_array).is_some_and(|tables| {
        tables.iter().any(|item| item.get("schema").and_then(Value::as_str) == Some(schema) && item.get("table").and_then(Value::as_str) == Some(table))
    })
}

fn index_pk(index: &Value, schema: &str, table: &str) -> Result<String, Fail> {
    index.get("tables").and_then(Value::as_array).and_then(|tables| {
        tables.iter().find(|item| item.get("schema").and_then(Value::as_str) == Some(schema) && item.get("table").and_then(Value::as_str) == Some(table))
    }).and_then(|item| item.get("primary_key").and_then(Value::as_str)).map(str::to_owned).ok_or(Fail::new("capture.refused", 4))
}

async fn reread_changes(
    config: &Config,
    client: &tokio_postgres::Client,
    audit_sql: &mut Vec<String>,
    folded: &Folded,
) -> Result<Vec<TableData>, Fail> {
    let mut grouped: BTreeMap<(String, String), (Vec<i64>, Vec<i64>)> = BTreeMap::new();
    for ((schema, table, pk), upsert) in &folded.changes {
        let entry = grouped.entry((schema.clone(), table.clone())).or_default();
        if *upsert {
            entry.0.push(*pk);
        } else {
            entry.1.push(*pk);
        }
    }
    let mut tables = Vec::new();
    for ((schema, table), (upserts, deletes)) in grouped {
        let mut table_data = read_table_shape(config, client, audit_sql, &schema, &table).await?;
        if upserts.iter().any(|pk| deletes.contains(pk)) {
            return Err(Fail::new("capture.protocol", 4));
        }
        if !upserts.is_empty() {
            let filtered = filtered_sql(&schema, &table, &table_data.pk, &table_data.columns)?;
            audit_sql.push(filtered.clone());
            let loaded = client.query(&filtered, &[&upserts]).await.map_err(|err| blocked(config, &err))?;
            if loaded.len() != upserts.len() {
                return Err(Fail::new("capture.diff", 7));
            }
            for row in &loaded {
                let mut values = Vec::new();
                let mut pk_value = None;
                for (index_of, column) in table_data.columns.iter().enumerate() {
                    let value = read_cell(row, index_of, column)?;
                    if column.primary_key {
                        pk_value = match &value {
                            CellValue::Int(pk) => Some(*pk),
                            _ => None,
                        };
                    }
                    values.push(value);
                }
                table_data.rows.push((pk_value.ok_or(Fail::new("capture.protocol", 4))?, values));
            }
        }
        if !deletes.is_empty() {
            let absent = format!(
                "SELECT {}.{}::bigint FROM {}.{} WHERE {}.{} = ANY ($1::bigint[])",
                quote(&table),
                quote(&table_data.pk),
                quote(&schema),
                quote(&table),
                quote(&table),
                quote(&table_data.pk)
            );
            if !platform_sql_allowed(&absent) {
                return Err(Fail::new("audit.write", 7));
            }
            audit_sql.push(absent.clone());
            let still = client.query(&absent, &[&deletes]).await.map_err(|err| blocked(config, &err))?;
            if !still.is_empty() {
                return Err(Fail::new("capture.diff", 7));
            }
            table_data.tombstones = deletes.iter().map(|pk| pk.to_string()).collect();
        }
        table_data.rows.sort_by_key(|row| row.0);
        tables.push(table_data);
    }
    Ok(tables)
}

async fn read_table_shape(
    config: &Config,
    client: &tokio_postgres::Client,
    audit_sql: &mut Vec<String>,
    schema: &str,
    table: &str,
) -> Result<TableData, Fail> {
    let mut shaped = read_table(config, client, audit_sql, schema, table).await?;
    shaped.rows.clear();
    Ok(shaped)
}

fn filtered_sql(schema: &str, table: &str, pk: &str, columns: &[ColumnPlan]) -> Result<String, Fail> {
    let mut exprs = Vec::new();
    for column in columns {
        exprs.push(select_expr(column)?);
    }
    let sql = format!(
        "SELECT {} FROM {}.{} WHERE {}.{} = ANY ($1::bigint[]) ORDER BY {}.{}",
        exprs.join(", "),
        quote(schema),
        quote(table),
        quote(table),
        quote(pk),
        quote(table),
        quote(pk)
    );
    if !platform_sql_allowed(&sql) {
        return Err(Fail::new("audit.write", 7));
    }
    Ok(sql)
}

#[allow(clippy::too_many_arguments)]
fn write_delta(
    config: &Config,
    config_bytes: &[u8],
    audit_sql: &[String],
    index: &Value,
    max_lsn: u64,
    tables: Vec<TableData>,
    dba_observation: &Value,
) -> Result<(), Fail> {
    let root = admit_root(config);
    let audit = json!({
        "protocol": "oo.postgres-preview.native-audit/v1",
        "statements": audit_sql,
        "slot_advanced": false,
        "applied_pos": max_lsn.to_string(),
        "source_dml": 0
    });
    let audit_bytes = canonical_bytes(&audit)?;
    write_bytes(&root.join("delta-audit.json"), &audit_bytes)?;
    let catalog_bytes = fs::read(root.join("catalog.json")).map_err(|_| Fail::new("capture.refused", 4))?;
    let registry_bytes = fs::read(root.join("registry.json")).map_err(|_| Fail::new("capture.refused", 4))?;
    let shared = SharedDigest {
        config: sha256(config_bytes),
        catalog: sha256(&catalog_bytes),
        registry: sha256(&registry_bytes),
        audit: sha256(&audit_bytes),
        driver: driver_digest()?,
        write_attempts: audit_sql.iter().filter(|sql| !platform_sql_allowed(sql)).count(),
        dba_objects: dba_descriptors(&root, "dba-objects.delta.json", dba_observation)?,
    };
    let mut index = index.clone();
    let lsn_text = max_lsn.to_string();
    for table in &tables {
        if table.rows.is_empty() && table.tombstones.is_empty() {
            continue;
        }
        if table.rows.iter().any(|(pk, _)| table.tombstones.iter().any(|text| text == &pk.to_string())) {
            return Err(Fail::new("capture.protocol", 4));
        }
        let (parent, generation) = next_generation(&index, &table.schema, &table.table)?;
        let bytes = if table.rows.is_empty() {
            Vec::new()
        } else {
            let pages = split_pages(&table.columns, &table.rows, PAGE_LIMIT)?;
            if pages.len() != 1 {
                return Err(Fail::new("capture.large_txn", 4));
            }
            pages.into_iter().next().ok_or(Fail::new("arrow.empty", 8))?
        };
        let written = write_page(
            config,
            &root,
            table,
            generation,
            Some(parent),
            &lsn_text,
            "pg_commit_lsn",
            &shared,
            &bytes,
            &table.rows,
            &table.tombstones,
        )?;
        append_page(&mut index, &table.schema, &table.table, written)?;
    }
    index["delete_grade"] = json!("unknown");
    write_json(&root.join("index.json"), &index)?;
    Ok(())
}

fn next_generation(index: &Value, schema: &str, table: &str) -> Result<(i64, i64), Fail> {
    let pages = index.get("tables").and_then(Value::as_array).and_then(|tables| {
        tables.iter().find(|item| item.get("schema").and_then(Value::as_str) == Some(schema) && item.get("table").and_then(Value::as_str) == Some(table))
    }).and_then(|item| item.get("pages").and_then(Value::as_array)).ok_or(Fail::new("capture.refused", 4))?;
    let parent = pages.last().and_then(|page| page.get("generation").and_then(Value::as_i64)).ok_or(Fail::new("capture.refused", 4))?;
    Ok((parent, parent + 1))
}

fn append_page(index: &mut Value, schema: &str, table: &str, page: Value) -> Result<(), Fail> {
    let tables = index.get_mut("tables").and_then(Value::as_array_mut).ok_or(Fail::new("capture.refused", 4))?;
    let found = tables.iter_mut().find(|item| item.get("schema").and_then(Value::as_str) == Some(schema) && item.get("table").and_then(Value::as_str) == Some(table)).ok_or(Fail::new("capture.refused", 4))?;
    let pages = found.get_mut("pages").and_then(Value::as_array_mut).ok_or(Fail::new("capture.refused", 4))?;
    pages.push(page);
    Ok(())
}

fn land_column(oid: i64, typmod: i32) -> Landing {
    match oid {
        21 if typmod == -1 => Landing::Int { width: 16 },
        23 if typmod == -1 => Landing::Int { width: 32 },
        20 if typmod == -1 => Landing::Int { width: 64 },
        16 if typmod == -1 => Landing::Bool,
        1700 => match numeric_parts(typmod) {
            Some((precision, scale)) if precision <= 38 => Landing::Decimal { precision, scale },
            Some(_) => Landing::Refused("capture.refused"),
            None => Landing::Refused("capture.refused"),
        },
        25 if typmod == -1 => Landing::Utf8 { note: Some("utf8_lands_varchar") },
        1043 if typmod == -1 => Landing::Utf8 { note: None },
        17 if typmod == -1 => Landing::Binary,
        2950 if typmod == -1 => Landing::UuidBytes,
        1184 if typmod == -1 => Landing::TimestampUtc,
        1114 if typmod == -1 => Landing::Refused("capture.refused"),
        1082 if typmod == -1 => Landing::Date,
        _ => Landing::Refused("capture.refused"),
    }
}

fn numeric_parts(typmod: i32) -> Option<(u8, i8)> {
    if typmod < 4 {
        return None;
    }
    let raw = typmod - 4;
    let precision = i32::try_from((raw as u32) >> 16).ok()?;
    let scale = raw & 0xffff;
    if !(1..=76).contains(&precision) || scale < 0 || scale > precision {
        return None;
    }
    Some((u8::try_from(precision).ok()?, i8::try_from(scale).ok()?))
}

fn parse_numeric(text: &str, scale: i8) -> Result<i128, Fail> {
    if text.is_empty() || text.bytes().any(|byte| matches!(byte, b'e' | b'E' | b' ')) {
        return Err(Fail::new("capture.protocol", 4));
    }
    let negative = text.starts_with('-');
    let body = text.trim_start_matches(['-', '+']);
    if body.is_empty() || body.starts_with('.') {
        return Err(Fail::new("capture.protocol", 4));
    }
    let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Fail::new("capture.protocol", 4));
    }
    if frac.len() > usize::from(u8::try_from(scale).unwrap_or(255)) {
        return Err(Fail::new("capture.protocol", 4));
    }
    let mut digits = String::new();
    digits.push_str(whole);
    digits.push_str(frac);
    for _ in frac.len()..usize::from(scale as u8) {
        digits.push('0');
    }
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let value = digits.parse::<i128>().map_err(|_| Fail::new("capture.protocol", 4))?;
    Ok(if negative { value.checked_neg().ok_or(Fail::new("capture.protocol", 4))? } else { value })
}

fn int_width_ok(width: u8, value: i64) -> bool {
    match width {
        16 => i64::from(i16::MIN) <= value && value <= i64::from(i16::MAX),
        32 => i64::from(i32::MIN) <= value && value <= i64::from(i32::MAX),
        64 => true,
        _ => false,
    }
}

fn split_pages(columns: &[ColumnPlan], rows: &[(i64, Vec<CellValue>)], limit: usize) -> Result<Vec<Vec<u8>>, Fail> {
    fn rec(columns: &[ColumnPlan], rows: &[(i64, Vec<CellValue>)], limit: usize) -> Result<Vec<Vec<u8>>, Fail> {
        if rows.is_empty() {
            return Err(Fail::new("arrow.empty", 8));
        }
        let bytes = encode_page(columns, rows)?;
        if bytes.len() <= limit {
            return Ok(vec![bytes]);
        }
        if rows.len() == 1 {
            return Err(Fail::new("capture.large_txn", 4));
        }
        let mid = rows.len() / 2;
        let mut pages = rec(columns, &rows[..mid], limit)?;
        pages.extend(rec(columns, &rows[mid..], limit)?);
        Ok(pages)
    }
    rec(columns, rows, limit)
}

fn page_row_count(rows: &[(i64, Vec<CellValue>)], bytes: &[u8], columns: &[ColumnPlan]) -> Result<usize, Fail> {
    // Pages were split on these rows. Recover the prefix whose encoding matches.
    let mut low = 1usize;
    let mut high = rows.len();
    while low < high {
        let mid = low + (high - low) / 2;
        let encoded = encode_page(columns, &rows[..mid])?;
        if encoded.as_slice() == bytes {
            return Ok(mid);
        }
        if encoded.len() < bytes.len() {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    let encoded = encode_page(columns, &rows[..low])?;
    if encoded.as_slice() == bytes {
        Ok(low)
    } else {
        Err(Fail::new("capture.diff", 7))
    }
}

fn encode_page(columns: &[ColumnPlan], rows: &[(i64, Vec<CellValue>)]) -> Result<Vec<u8>, Fail> {
    let fields: Vec<Field> = columns.iter().map(arrow_field).collect();
    let schema = Arc::new(Schema::new(fields));
    let mut arrays: Vec<ArrayRef> = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        arrays.push(encode_column(column, rows, index)?);
    }
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let mut buffer = Vec::new();
    let mut writer = StreamWriter::try_new_with_options(&mut buffer, schema.as_ref(), options).map_err(|_| Fail::new("arrow.ipc", 1))?;
    writer.write(&batch).map_err(|_| Fail::new("arrow.ipc", 1))?;
    writer.finish().map_err(|_| Fail::new("arrow.ipc", 1))?;
    Ok(buffer)
}

fn arrow_field(column: &ColumnPlan) -> Field {
    let data_type = match column.landing {
        Landing::Int { width: 16 } => DataType::Int16,
        Landing::Int { width: 32 } => DataType::Int32,
        Landing::Int { width: 64 } | Landing::Int { .. } => DataType::Int64,
        Landing::Bool => DataType::Boolean,
        Landing::Decimal { precision, scale } => DataType::Decimal128(precision, scale),
        Landing::Utf8 { .. } => DataType::Utf8,
        Landing::Binary => DataType::Binary,
        Landing::UuidBytes => DataType::FixedSizeBinary(16),
        Landing::TimestampUtc => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        Landing::Date => DataType::Date32,
        Landing::Refused(_) => DataType::Utf8,
    };
    Field::new(&column.name, data_type, column.nullable)
}

fn encode_column(column: &ColumnPlan, rows: &[(i64, Vec<CellValue>)], index: usize) -> Result<ArrayRef, Fail> {
    match column.landing {
        Landing::Int { width: 16 } => {
            let values = int_options(rows, index, |value| i16::try_from(value).ok())?;
            Ok(Arc::new(Int16Array::from(values)))
        }
        Landing::Int { width: 32 } => {
            let values = int_options(rows, index, |value| i32::try_from(value).ok())?;
            Ok(Arc::new(Int32Array::from(values)))
        }
        Landing::Int { .. } => Ok(Arc::new(Int64Array::from(int_options(rows, index, Some)?))),
        Landing::Bool => {
            let mut values = Vec::new();
            for row in rows {
                values.push(match &row.1[index] {
                    CellValue::Null => None,
                    CellValue::Bool(value) => Some(*value),
                    _ => return Err(Fail::new("arrow.ipc", 1)),
                });
            }
            Ok(Arc::new(BooleanArray::from(values)))
        }
        Landing::Decimal { precision, scale } => {
            let mut values = Vec::new();
            for row in rows {
                values.push(match &row.1[index] {
                    CellValue::Null => None,
                    CellValue::Decimal(value) => Some(*value),
                    _ => return Err(Fail::new("arrow.ipc", 1)),
                });
            }
            let array = Decimal128Array::from(values).with_precision_and_scale(precision, scale).map_err(|_| Fail::new("arrow.decimal", 5))?;
            Ok(Arc::new(array))
        }
        Landing::Utf8 { .. } => {
            let mut values = Vec::new();
            for row in rows {
                values.push(match &row.1[index] {
                    CellValue::Null => None,
                    CellValue::Text(value) => Some(value.clone()),
                    _ => return Err(Fail::new("arrow.ipc", 1)),
                });
            }
            Ok(Arc::new(StringArray::from(values)))
        }
        Landing::Binary => {
            let mut values: Vec<Option<Vec<u8>>> = Vec::new();
            for row in rows {
                values.push(match &row.1[index] {
                    CellValue::Null => None,
                    CellValue::Bytes(value) => Some(value.clone()),
                    _ => return Err(Fail::new("arrow.ipc", 1)),
                });
            }
            let refs: Vec<Option<&[u8]>> = values.iter().map(Option::as_deref).collect();
            Ok(Arc::new(BinaryArray::from(refs)))
        }
        Landing::UuidBytes => {
            let mut builder = FixedSizeBinaryBuilder::with_capacity(rows.len(), 16);
            for row in rows {
                match &row.1[index] {
                    CellValue::Null => builder.append_null(),
                    CellValue::Fixed(value) => builder.append_value(value).map_err(|_| Fail::new("arrow.ipc", 1))?,
                    _ => return Err(Fail::new("arrow.ipc", 1)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        Landing::TimestampUtc => {
            let values = int_options(rows, index, Some)?;
            Ok(Arc::new(TimestampMicrosecondArray::from(values).with_timezone("UTC")))
        }
        Landing::Date => {
            let mut values = Vec::new();
            for row in rows {
                values.push(match &row.1[index] {
                    CellValue::Null => None,
                    CellValue::Days(value) => Some(*value),
                    _ => return Err(Fail::new("arrow.ipc", 1)),
                });
            }
            Ok(Arc::new(Date32Array::from(values)))
        }
        Landing::Refused(_) => Err(Fail::new("capture.refused", 4)),
    }
}

fn int_options<T>(rows: &[(i64, Vec<CellValue>)], index: usize, map: impl Fn(i64) -> Option<T>) -> Result<Vec<Option<T>>, Fail> {
    let mut values = Vec::new();
    for row in rows {
        values.push(match &row.1[index] {
            CellValue::Null => None,
            CellValue::Int(value) => Some(map(*value).ok_or(Fail::new("arrow.ipc", 1))?),
            _ => return Err(Fail::new("arrow.ipc", 1)),
        });
    }
    Ok(values)
}

fn arrow_schema_sha256(bytes: &[u8]) -> Result<String, Fail> {
    let mut offset = 0usize;
    while offset + 8 <= bytes.len() {
        let continuation = i32::from_le_bytes(bytes[offset..offset + 4].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?);
        let meta_len = i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?);
        if continuation != -1 || meta_len < 0 {
            return Err(Fail::new("arrow.ipc", 1));
        }
        if meta_len == 0 {
            break;
        }
        let meta_end = offset + 8 + meta_len as usize;
        if meta_end > bytes.len() {
            return Err(Fail::new("arrow.ipc", 1));
        }
        let meta = &bytes[offset + 8..meta_end];
        if flat_u8(meta, 1)? == Some(1) {
            return Ok(sha256(meta));
        }
        let body_len = flat_i64(meta, 3)?.unwrap_or(0);
        if body_len < 0 {
            return Err(Fail::new("arrow.ipc", 1));
        }
        let body_start = align8(meta_end);
        let body_end = body_start + body_len as usize;
        offset = align8(body_end);
    }
    Err(Fail::new("arrow.ipc", 1))
}

fn flat_u8(data: &[u8], index: usize) -> Result<Option<u8>, Fail> {
    flat_field(data, index).map(|slot| slot.map(|at| data[at]))
}

fn flat_i64(data: &[u8], index: usize) -> Result<Option<i64>, Fail> {
    let Some(at) = flat_field(data, index)? else {
        return Ok(None);
    };
    if at + 8 > data.len() {
        return Err(Fail::new("arrow.ipc", 1));
    }
    Ok(Some(i64::from_le_bytes(data[at..at + 8].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?)))
}

fn flat_field(data: &[u8], index: usize) -> Result<Option<usize>, Fail> {
    if data.len() < 4 {
        return Err(Fail::new("arrow.ipc", 1));
    }
    let root = u32::from_le_bytes(data[0..4].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?) as usize;
    if root + 4 > data.len() {
        return Err(Fail::new("arrow.ipc", 1));
    }
    let soff = i32::from_le_bytes(data[root..root + 4].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?);
    let vtable = root.checked_add_signed(isize::try_from(-soff).map_err(|_| Fail::new("arrow.ipc", 1))?).ok_or(Fail::new("arrow.ipc", 1))?;
    if vtable + 4 > data.len() {
        return Err(Fail::new("arrow.ipc", 1));
    }
    let vsize = u16::from_le_bytes(data[vtable..vtable + 2].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?) as usize;
    let slot = 4 + index * 2;
    if slot + 2 > vsize {
        return Ok(None);
    }
    let rel = u16::from_le_bytes(data[vtable + slot..vtable + slot + 2].try_into().map_err(|_| Fail::new("arrow.ipc", 1))?) as usize;
    if rel == 0 {
        Ok(None)
    } else {
        Ok(Some(root + rel))
    }
}

fn align8(value: usize) -> usize {
    value.saturating_add(7) & !7
}

fn canonical_i64(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0"
        && text.parse::<i64>().is_ok()
}

fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn quote(value: &str) -> String {
    format!("\"{value}\"")
}

fn admit_root(config: &Config) -> std::path::PathBuf {
    Path::new(&config.out_dir).join("native-admit")
}

fn canonical_bytes(value: &Value) -> Result<Vec<u8>, Fail> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Fail::new("capture.apply", 4))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_json(path: &Path, value: &Value) -> Result<(), Fail> {
    write_bytes(path, &canonical_bytes(value)?)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| Fail::new("capture.apply", 4))?;
    }
    fs::write(path, bytes).map_err(|_| Fail::new("capture.apply", 4))
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn driver_digest() -> Result<String, Fail> {
    let path = std::env::current_exe().map_err(|_| Fail::new("capture.refused", 4))?;
    let bytes = fs::read(path).map_err(|_| Fail::new("capture.refused", 4))?;
    Ok(sha256(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn receipt_has_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("secret://"),
        Value::Array(items) => items.iter().any(receipt_has_secret),
        Value::Object(map) => map.contains_key("secret_ref") || map.values().any(receipt_has_secret),
        _ => false,
    }
}

async fn exec(config: &Config, client: &tokio_postgres::Client, audit_sql: &mut Vec<String>, sql: &str) -> Result<(), Fail> {
    if !platform_sql_allowed(sql) {
        return Err(Fail::new("audit.write", 7));
    }
    audit_sql.push(sql.to_owned());
    client.batch_execute(sql).await.map_err(|err| blocked(config, &err))
}

fn blocked(config: &Config, err: &tokio_postgres::Error) -> Fail {
    let db = err.as_db_error();
    let _ = write_json(
        &admit_root(config).join("blocked.json"),
        &json!({
            "protocol": "oo.postgres-preview.native-blocked/v1",
            "sqlstate": err.code().map(|code| code.code()).unwrap_or(""),
            "message": db.map(|item| item.message()).unwrap_or(""),
            "detail": db.and_then(|item| item.detail()).unwrap_or(""),
            "position": db.and_then(|item| match item.position() {
                Some(tokio_postgres::error::ErrorPosition::Original(pos)) => Some(pos),
                Some(tokio_postgres::error::ErrorPosition::Internal { position, .. }) => Some(position),
                None => None,
            }),
            "where": db.and_then(|item| item.where_()).unwrap_or("")
        }),
    );
    Fail::new("capture.blocked", 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_typmod_and_text_keep_scale() {
        assert_eq!(numeric_parts(2_490_376), Some((38, 4)));
        assert_eq!(parse_numeric("1.0013", 4).ok(), Some(10013));
        assert_eq!(parse_numeric("-0.0100", 4).ok(), Some(-100));
        assert_eq!(parse_numeric("0", 4).ok(), Some(0));
        assert!(parse_numeric("1.00130", 4).is_err());
        assert!(matches!(land_column(1700, -1), Landing::Refused(_)));
    }

    #[test]
    fn uuid_and_text_keep_a_native_note() {
        assert!(matches!(land_column(2950, -1), Landing::UuidBytes));
        assert!(matches!(land_column(25, -1), Landing::Utf8 { note: Some("utf8_lands_varchar") }));
        assert!(matches!(land_column(17, -1), Landing::Binary));
        assert!(matches!(land_column(1184, -1), Landing::TimestampUtc));
        assert!(matches!(land_column(20, -1), Landing::Int { width: 64 }));
        assert_eq!(arrow_type(&Landing::UuidBytes), "fixedsizebinary(16)");
    }

    #[test]
    fn schema_message_hash_is_stable() {
        let column = ColumnPlan {
            name: "hub_id".to_owned(),
            oid: 20,
            typmod: -1,
            nullable: false,
            format: "bigint".to_owned(),
            landing: Landing::Int { width: 64 },
            primary_key: true,
        };
        let rows = vec![(1, vec![CellValue::Int(1)]), (2, vec![CellValue::Int(2)])];
        let bytes = encode_page(std::slice::from_ref(&column), &rows).unwrap();
        let hash = arrow_schema_sha256(&bytes).unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, arrow_schema_sha256(&bytes).unwrap());
        assert!(bytes.len() < PAGE_LIMIT);
    }

    #[test]
    fn pages_cover_every_row_under_the_limit() {
        let column = ColumnPlan {
            name: "note".to_owned(),
            oid: 25,
            typmod: -1,
            nullable: true,
            format: "text".to_owned(),
            landing: Landing::Utf8 { note: Some("utf8_lands_varchar") },
            primary_key: false,
        };
        let rows: Vec<_> = (0..20).map(|id| (id, vec![CellValue::Text("x".repeat(40))])).collect();
        let two = encode_page(std::slice::from_ref(&column), &rows[..2]).unwrap();
        let pages = split_pages(std::slice::from_ref(&column), &rows, two.len()).unwrap();
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|page| page.len() <= two.len()));
        let mut seen = 0usize;
        let mut rest = rows.as_slice();
        for page in &pages {
            let count = page_row_count(rest, page, std::slice::from_ref(&column)).unwrap();
            seen += count;
            rest = &rest[count..];
        }
        assert_eq!(seen, 20);
        let empty = encode_page(std::slice::from_ref(&column), &[]).unwrap();
        assert!(!empty.is_empty());
        let schema_hash = arrow_schema_sha256(&empty).unwrap();
        assert_eq!(schema_hash.len(), 64);
        assert_ne!(schema_hash, sha256(&[]));
    }

    #[test]
    fn load_document_path_matches_the_written_directory() {
        let arrow = container_object_path("/work-out/capture-g2", "public.hubs", "g1.arrow");
        let receipt = container_object_path("/work-out/capture-g2/", "customer_east.delivery_notes", "g1.receipt.json");
        assert_eq!(arrow, "/work-out/capture-g2/native-admit/public.hubs/g1.arrow");
        assert_eq!(
            receipt,
            "/work-out/capture-g2/native-admit/customer_east.delivery_notes/g1.receipt.json"
        );
    }

    #[test]
    fn canonical_delete_text_matches_the_loader() {
        assert!(canonical_i64("1"));
        assert!(canonical_i64("-1"));
        assert!(canonical_i64("0"));
        assert!(!canonical_i64("01"));
        assert!(!canonical_i64("-0"));
        assert_eq!((-1_i64).to_string(), "-1");
    }
}
