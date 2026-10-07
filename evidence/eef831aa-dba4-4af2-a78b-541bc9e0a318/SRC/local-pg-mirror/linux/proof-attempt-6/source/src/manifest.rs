use std::fs;
use std::path::Path;

use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Map, Value};

use crate::error::MirrorError;
use crate::jsonutil::{exact_keys, json_string, parse_object, take_bool, take_i64, take_str, take_u64};
use crate::plan::{write_new_private, Plan};

const MANIFEST_PROTOCOL: &str = "oo.pipeline.local-pg-mirror-manifest/v1";
const TARGET_PROTOCOL: &str = "oo.pipeline.local-pg-mirror-target/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnRecord {
    pub name: String,
    pub attnum: i32,
    pub oid: i64,
    pub physical_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRecord {
    pub schema: String,
    pub name: String,
    pub physical_name: Option<String>,
    pub rows: u64,
    pub columns: Vec<ColumnRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quarantine {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub oid: i64,
    pub type_name: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub mirror_state: String,
    pub customer_complete: bool,
    pub enterprise: String,
    pub consistency: String,
    pub plan_sha256: String,
    pub admitted_sha256: String,
    pub failure: String,
    pub rows: u64,
    pub source_socket_peak: Option<u64>,
    pub source_socket_fact: String,
    pub recheck_connections: u64,
    pub source_count_star: bool,
    pub source_explain: bool,
    pub source_ddl: bool,
    pub connect_timeout_ms: u64,
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub batch_rows: u64,
    pub snapshot_deadline_ms: u64,
    pub loader_protocol: String,
    pub loader_run_id: Option<String>,
    pub loader_state: String,
    pub mapping_algorithm: String,
    pub system_position: String,
    pub target_config_path: String,
    pub target_config_sha256: String,
    pub tables_completed: Vec<TableRecord>,
    pub tables_incomplete: Vec<TableRecord>,
    pub quarantine: Vec<Quarantine>,
    pub manifest_sha256: String,
}

pub fn target_config_bytes(plan: &Plan) -> Result<Vec<u8>, MirrorError> {
    let value = serde_json::json!({
        "protocol": TARGET_PROTOCOL,
        "host": plan.config.target_host,
        "port": plan.config.target_port,
        "database": plan.config.target_database,
        "user": plan.config.target_reader_user,
        "server_name": plan.config.target_server_name,
        "ca_sha256": plan.config.target_ca_sha256,
        "tls": "verify-full",
    });
    let text = serde_json::to_string(&value).map_err(|_| MirrorError::new("config", "json"))?;
    Ok(text.into_bytes())
}

pub fn store_target_config(path: &Path, bytes: &[u8]) -> Result<(), MirrorError> {
    write_new_private(path, bytes, "target_config_exists")
}

pub fn store_manifest(path: &Path, manifest: &mut Manifest) -> Result<(), MirrorError> {
    manifest.customer_complete = false;
    manifest.enterprise = "not_configured".to_string();
    manifest.manifest_sha256 = sha256_hex(canonical(manifest)?.as_bytes());
    let text = manifest_json(manifest)?;
    write_new_private(path, text.as_bytes(), "manifest_conflict")
}

pub fn load_manifest(path: &Path) -> Result<Option<Manifest>, MirrorError> {
    if !path.is_absolute() {
        return Err(MirrorError::new("config", "manifest_path"));
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(MirrorError::new("manifest_conflict", "symlink"));
        }
        Ok(meta) if !meta.is_file() => return Err(MirrorError::new("manifest_conflict", "file")),
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(MirrorError::new("config", "manifest_path")),
    }
    let bytes = fs::read(path).map_err(|_| MirrorError::new("config", "manifest_path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| MirrorError::new("manifest_tamper", "json"))?;
    let map = parse_object(text).map_err(|_| MirrorError::new("manifest_tamper", "json"))?;
    let manifest = manifest_from_map(&map)?;
    let recomputed = sha256_hex(canonical(&manifest)?.as_bytes());
    if recomputed != manifest.manifest_sha256 {
        return Err(MirrorError::new("manifest_tamper", "manifest_sha256"));
    }
    if manifest.customer_complete || manifest.enterprise != "not_configured" {
        return Err(MirrorError::new("manifest_tamper", "customer_complete"));
    }
    if manifest.mirror_state == "complete" {
        if manifest.consistency != "single_snapshot" {
            return Err(MirrorError::new("manifest_tamper", "consistency"));
        }
    } else if manifest.consistency != "not_a_snapshot" {
        return Err(MirrorError::new("manifest_tamper", "consistency"));
    }
    Ok(Some(manifest))
}

pub fn load_target_config(path: &Path, expected_sha: &str) -> Result<Map<String, Value>, MirrorError> {
    let bytes = crate::plan::read_text(path, "config", "target_config")?;
    if sha256_hex(bytes.as_bytes()) != expected_sha {
        return Err(MirrorError::new("config", "target_config_sha256"));
    }
    let map = parse_object(&bytes)?;
    exact_keys(
        &map,
        &[
            "protocol",
            "host",
            "port",
            "database",
            "user",
            "server_name",
            "ca_sha256",
            "tls",
        ],
    )?;
    if take_str(&map, "protocol")? != TARGET_PROTOCOL || take_str(&map, "tls")? != "verify-full" {
        return Err(MirrorError::new("config", "target_config"));
    }
    if take_str(&map, "host")? != take_str(&map, "server_name")? {
        return Err(MirrorError::new("config", "server_name"));
    }
    Ok(map)
}

fn canonical(manifest: &Manifest) -> Result<String, MirrorError> {
    let mut out = String::from("{");
    field(&mut out, "protocol", &json_string(MANIFEST_PROTOCOL)?)?;
    field(&mut out, "mirror_state", &json_string(&manifest.mirror_state)?)?;
    field(&mut out, "customer_complete", "false")?;
    field(&mut out, "enterprise", &json_string("not_configured")?)?;
    field(&mut out, "consistency", &json_string(&manifest.consistency)?)?;
    field(&mut out, "plan_sha256", &json_string(&manifest.plan_sha256)?)?;
    field(&mut out, "admitted_sha256", &json_string(&manifest.admitted_sha256)?)?;
    field(&mut out, "failure", &json_string(&manifest.failure)?)?;
    field(&mut out, "rows", &manifest.rows.to_string())?;
    match manifest.source_socket_peak {
        Some(peak) => field(&mut out, "source_socket_peak", &peak.to_string())?,
        None => field(&mut out, "source_socket_peak", "null")?,
    }
    field(&mut out, "source_socket_fact", &json_string(&manifest.source_socket_fact)?)?;
    field(&mut out, "recheck_connections", &manifest.recheck_connections.to_string())?;
    field(&mut out, "source_count_star", bool_text(manifest.source_count_star))?;
    field(&mut out, "source_explain", bool_text(manifest.source_explain))?;
    field(&mut out, "source_ddl", bool_text(manifest.source_ddl))?;
    field(&mut out, "connect_timeout_ms", &manifest.connect_timeout_ms.to_string())?;
    field(&mut out, "statement_timeout_ms", &manifest.statement_timeout_ms.to_string())?;
    field(&mut out, "lock_timeout_ms", &manifest.lock_timeout_ms.to_string())?;
    field(&mut out, "max_rows", &manifest.max_rows.to_string())?;
    field(&mut out, "max_bytes", &manifest.max_bytes.to_string())?;
    field(&mut out, "batch_rows", &manifest.batch_rows.to_string())?;
    field(&mut out, "snapshot_deadline_ms", &manifest.snapshot_deadline_ms.to_string())?;
    field(&mut out, "loader_protocol", &json_string(&manifest.loader_protocol)?)?;
    match &manifest.loader_run_id {
        Some(run) => field(&mut out, "loader_run_id", &json_string(run)?)?,
        None => field(&mut out, "loader_run_id", "null")?,
    }
    field(&mut out, "loader_state", &json_string(&manifest.loader_state)?)?;
    field(&mut out, "mapping_algorithm", &json_string(&manifest.mapping_algorithm)?)?;
    field(&mut out, "system_position", &json_string(&manifest.system_position)?)?;
    field(&mut out, "target_config_path", &json_string(&manifest.target_config_path)?)?;
    field(&mut out, "target_config_sha256", &json_string(&manifest.target_config_sha256)?)?;
    field(&mut out, "tables_completed", &tables_json(&manifest.tables_completed)?)?;
    field(&mut out, "tables_incomplete", &tables_json(&manifest.tables_incomplete)?)?;
    field(&mut out, "quarantine", &quarantine_json(&manifest.quarantine)?)?;
    if out.ends_with(',') {
        out.pop();
    }
    out.push('}');
    Ok(out)
}

fn field(out: &mut String, key: &str, value: &str) -> Result<(), MirrorError> {
    out.push_str(&json_string(key)?);
    out.push(':');
    out.push_str(value);
    out.push(',');
    Ok(())
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn tables_json(tables: &[TableRecord]) -> Result<String, MirrorError> {
    let mut out = String::from("[");
    for (index, table) in tables.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('{');
        field(&mut out, "schema", &json_string(&table.schema)?)?;
        field(&mut out, "name", &json_string(&table.name)?)?;
        match &table.physical_name {
            Some(name) => field(&mut out, "physical_name", &json_string(name)?)?,
            None => field(&mut out, "physical_name", "null")?,
        }
        field(&mut out, "rows", &table.rows.to_string())?;
        let mut columns = String::from("[");
        for (column_index, column) in table.columns.iter().enumerate() {
            if column_index > 0 {
                columns.push(',');
            }
            columns.push('{');
            field(&mut columns, "name", &json_string(&column.name)?)?;
            field(&mut columns, "attnum", &column.attnum.to_string())?;
            field(&mut columns, "oid", &column.oid.to_string())?;
            field(&mut columns, "physical_name", &json_string(&column.physical_name)?)?;
            if columns.ends_with(',') {
                columns.pop();
            }
            columns.push('}');
        }
        columns.push(']');
        field(&mut out, "columns", &columns)?;
        if out.ends_with(',') {
            out.pop();
        }
        out.push('}');
    }
    out.push(']');
    Ok(out)
}

fn quarantine_json(rows: &[Quarantine]) -> Result<String, MirrorError> {
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('{');
        field(&mut out, "schema", &json_string(&row.schema)?)?;
        field(&mut out, "table", &json_string(&row.table)?)?;
        field(&mut out, "column", &json_string(&row.column)?)?;
        field(&mut out, "oid", &row.oid.to_string())?;
        field(&mut out, "type_name", &json_string(&row.type_name)?)?;
        field(&mut out, "reason", &json_string(&row.reason)?)?;
        if out.ends_with(',') {
            out.pop();
        }
        out.push('}');
    }
    out.push(']');
    Ok(out)
}

fn manifest_json(manifest: &Manifest) -> Result<String, MirrorError> {
    let mut body = canonical(manifest)?;
    body.pop();
    body.push_str(",\"manifest_sha256\":");
    body.push_str(&json_string(&manifest.manifest_sha256)?);
    body.push('}');
    Ok(body)
}

fn manifest_from_map(map: &Map<String, Value>) -> Result<Manifest, MirrorError> {
    exact_keys(
        map,
        &[
            "protocol",
            "mirror_state",
            "customer_complete",
            "enterprise",
            "consistency",
            "plan_sha256",
            "admitted_sha256",
            "failure",
            "rows",
            "source_socket_peak",
            "source_socket_fact",
            "recheck_connections",
            "source_count_star",
            "source_explain",
            "source_ddl",
            "connect_timeout_ms",
            "statement_timeout_ms",
            "lock_timeout_ms",
            "max_rows",
            "max_bytes",
            "batch_rows",
            "snapshot_deadline_ms",
            "loader_protocol",
            "loader_run_id",
            "loader_state",
            "mapping_algorithm",
            "system_position",
            "target_config_path",
            "target_config_sha256",
            "tables_completed",
            "tables_incomplete",
            "quarantine",
            "manifest_sha256",
        ],
    )
    .map_err(|_| MirrorError::new("manifest_tamper", "field"))?;
    if take_str(map, "protocol")? != MANIFEST_PROTOCOL {
        return Err(MirrorError::new("manifest_tamper", "protocol"));
    }
    Ok(Manifest {
        mirror_state: take_str(map, "mirror_state")?.to_string(),
        customer_complete: take_bool(map, "customer_complete")?,
        enterprise: take_str(map, "enterprise")?.to_string(),
        consistency: take_str(map, "consistency")?.to_string(),
        plan_sha256: take_str(map, "plan_sha256")?.to_string(),
        admitted_sha256: take_str(map, "admitted_sha256")?.to_string(),
        failure: take_str(map, "failure")?.to_string(),
        rows: take_u64(map, "rows")?,
        source_socket_peak: match map.get("source_socket_peak") {
            Some(Value::Null) => None,
            Some(Value::Number(number)) => Some(number.as_u64().ok_or_else(|| MirrorError::new("manifest_tamper", "socket"))?),
            _ => return Err(MirrorError::new("manifest_tamper", "socket")),
        },
        source_socket_fact: take_str(map, "source_socket_fact")?.to_string(),
        recheck_connections: take_u64(map, "recheck_connections")?,
        source_count_star: take_bool(map, "source_count_star")?,
        source_explain: take_bool(map, "source_explain")?,
        source_ddl: take_bool(map, "source_ddl")?,
        connect_timeout_ms: take_u64(map, "connect_timeout_ms")?,
        statement_timeout_ms: take_u64(map, "statement_timeout_ms")?,
        lock_timeout_ms: take_u64(map, "lock_timeout_ms")?,
        max_rows: take_u64(map, "max_rows")?,
        max_bytes: take_u64(map, "max_bytes")?,
        batch_rows: take_u64(map, "batch_rows")?,
        snapshot_deadline_ms: take_u64(map, "snapshot_deadline_ms")?,
        loader_protocol: take_str(map, "loader_protocol")?.to_string(),
        loader_run_id: match map.get("loader_run_id") {
            Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value.clone()),
            _ => return Err(MirrorError::new("manifest_tamper", "loader_run_id")),
        },
        loader_state: take_str(map, "loader_state")?.to_string(),
        mapping_algorithm: take_str(map, "mapping_algorithm")?.to_string(),
        system_position: take_str(map, "system_position")?.to_string(),
        target_config_path: take_str(map, "target_config_path")?.to_string(),
        target_config_sha256: take_str(map, "target_config_sha256")?.to_string(),
        tables_completed: parse_tables(map.get("tables_completed"))?,
        tables_incomplete: parse_tables(map.get("tables_incomplete"))?,
        quarantine: parse_quarantine(map.get("quarantine"))?,
        manifest_sha256: take_str(map, "manifest_sha256")?.to_string(),
    })
}

fn parse_tables(value: Option<&Value>) -> Result<Vec<TableRecord>, MirrorError> {
    let Some(Value::Array(items)) = value else {
        return Err(MirrorError::new("manifest_tamper", "tables"));
    };
    let mut tables = Vec::new();
    for item in items {
        let Value::Object(map) = item else {
            return Err(MirrorError::new("manifest_tamper", "tables"));
        };
        exact_keys(map, &["schema", "name", "physical_name", "rows", "columns"])
            .map_err(|_| MirrorError::new("manifest_tamper", "tables"))?;
        let mut columns = Vec::new();
        let Value::Array(raw_columns) = map.get("columns").ok_or_else(|| MirrorError::new("manifest_tamper", "tables"))? else {
            return Err(MirrorError::new("manifest_tamper", "tables"));
        };
        for column in raw_columns {
            let Value::Object(column) = column else {
                return Err(MirrorError::new("manifest_tamper", "tables"));
            };
            exact_keys(column, &["name", "attnum", "oid", "physical_name"])
                .map_err(|_| MirrorError::new("manifest_tamper", "tables"))?;
            columns.push(ColumnRecord {
                name: take_str(column, "name")?.to_string(),
                attnum: i32::try_from(take_i64(column, "attnum")?).map_err(|_| MirrorError::new("manifest_tamper", "attnum"))?,
                oid: take_i64(column, "oid")?,
                physical_name: take_str(column, "physical_name")?.to_string(),
            });
        }
        let physical_name = match map.get("physical_name") {
            Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value.clone()),
            _ => return Err(MirrorError::new("manifest_tamper", "tables")),
        };
        tables.push(TableRecord {
            schema: take_str(map, "schema")?.to_string(),
            name: take_str(map, "name")?.to_string(),
            physical_name,
            rows: take_u64(map, "rows")?,
            columns,
        });
    }
    Ok(tables)
}

fn parse_quarantine(value: Option<&Value>) -> Result<Vec<Quarantine>, MirrorError> {
    let Some(Value::Array(items)) = value else {
        return Err(MirrorError::new("manifest_tamper", "quarantine"));
    };
    let mut rows = Vec::new();
    for item in items {
        let Value::Object(map) = item else {
            return Err(MirrorError::new("manifest_tamper", "quarantine"));
        };
        exact_keys(map, &["schema", "table", "column", "oid", "type_name", "reason"])
            .map_err(|_| MirrorError::new("manifest_tamper", "quarantine"))?;
        rows.push(Quarantine {
            schema: take_str(map, "schema")?.to_string(),
            table: take_str(map, "table")?.to_string(),
            column: take_str(map, "column")?.to_string(),
            oid: take_i64(map, "oid")?,
            type_name: take_str(map, "type_name")?.to_string(),
            reason: take_str(map, "reason")?.to_string(),
        });
    }
    Ok(rows)
}
