use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use oo_connectors::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::error::MirrorError;
use crate::jsonutil::{exact_keys, json_string, parse_object, take_bool, take_i64, take_str, take_u64};

const PROTOCOL: &str = "oo.pipeline.local-pg-mirror/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnShape {
    pub attnum: i32,
    pub name: String,
    pub oid: i64,
    pub typmod: i32,
    pub not_null: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableShape {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnShape>,
    pub primary_key: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputConfig {
    pub source_id: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub server_name: String,
    pub ca_sha256: String,
    pub secret_ref: String,
    pub tables: Vec<(String, String)>,
    pub connect_timeout_ms: u64,
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub batch_rows: u64,
    pub snapshot_deadline_ms: u64,
    pub generation: u64,
    pub shard: String,
    pub owner: String,
    pub target_host: String,
    pub target_port: u16,
    pub target_database: String,
    pub target_admin_user: String,
    pub target_runtime_user: String,
    pub target_reader_user: String,
    pub target_server_name: String,
    pub target_ca_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub config: InputConfig,
    pub admitted: Vec<TableShape>,
    pub admitted_sha256: String,
    pub expected_principal: String,
    pub expected_database: String,
    pub expected_in_recovery: bool,
    pub expected_postmaster: String,
    pub expected_postmaster_sha256: String,
    pub plan_sha256: String,
}

pub fn parse_config_text(text: &str) -> Result<InputConfig, MirrorError> {
    let map = parse_object(text)?;
    let engine = take_str(&map, "engine")?;
    if engine != "postgresql" {
        return Err(MirrorError::new("unsupported_engine", "engine"));
    }
    exact_keys(
        &map,
        &[
            "protocol",
            "engine",
            "source_id",
            "host",
            "port",
            "database",
            "user",
            "server_name",
            "ca_sha256",
            "secret_ref",
            "tables",
            "connect_timeout_ms",
            "statement_timeout_ms",
            "lock_timeout_ms",
            "max_rows",
            "max_bytes",
            "batch_rows",
            "snapshot_deadline_ms",
            "generation",
            "shard",
            "owner",
            "target_host",
            "target_port",
            "target_database",
            "target_admin_user",
            "target_runtime_user",
            "target_reader_user",
            "target_server_name",
            "target_ca_sha256",
        ],
    )?;
    if take_str(&map, "protocol")? != PROTOCOL {
        return Err(MirrorError::new("config", "protocol"));
    }
    let mut config = InputConfig {
        source_id: take_str(&map, "source_id")?.to_string(),
        host: take_str(&map, "host")?.to_string(),
        port: port_of(take_u64(&map, "port")?)?,
        database: take_str(&map, "database")?.to_string(),
        user: take_str(&map, "user")?.to_string(),
        server_name: take_str(&map, "server_name")?.to_string(),
        ca_sha256: take_str(&map, "ca_sha256")?.to_string(),
        secret_ref: take_str(&map, "secret_ref")?.to_string(),
        tables: Vec::new(),
        connect_timeout_ms: take_u64(&map, "connect_timeout_ms")?,
        statement_timeout_ms: take_u64(&map, "statement_timeout_ms")?,
        lock_timeout_ms: take_u64(&map, "lock_timeout_ms")?,
        max_rows: take_u64(&map, "max_rows")?,
        max_bytes: take_u64(&map, "max_bytes")?,
        batch_rows: take_u64(&map, "batch_rows")?,
        snapshot_deadline_ms: take_u64(&map, "snapshot_deadline_ms")?,
        generation: take_u64(&map, "generation")?,
        shard: take_str(&map, "shard")?.to_string(),
        owner: take_str(&map, "owner")?.to_string(),
        target_host: take_str(&map, "target_host")?.to_string(),
        target_port: port_of(take_u64(&map, "target_port")?)?,
        target_database: take_str(&map, "target_database")?.to_string(),
        target_admin_user: take_str(&map, "target_admin_user")?.to_string(),
        target_runtime_user: take_str(&map, "target_runtime_user")?.to_string(),
        target_reader_user: take_str(&map, "target_reader_user")?.to_string(),
        target_server_name: take_str(&map, "target_server_name")?.to_string(),
        target_ca_sha256: take_str(&map, "target_ca_sha256")?.to_string(),
    };
    config.tables = parse_tables(map.get("tables").ok_or_else(|| MirrorError::new("config", "json"))?)?;
    validate_config(&config)?;
    Ok(config)
}

pub fn seal_plan(
    config: InputConfig,
    admitted: Vec<TableShape>,
    expected_principal: &str,
    expected_database: &str,
    expected_in_recovery: bool,
    expected_postmaster: &str,
) -> Result<Plan, MirrorError> {
    validate_config(&config)?;
    validate_admitted(&config, &admitted)?;
    if !simple_ident(expected_principal) || expected_principal != config.user {
        return Err(MirrorError::new("config", "principal"));
    }
    if !simple_ident(expected_database) || expected_database != config.database {
        return Err(MirrorError::new("config", "database"));
    }
    if !postmaster_ok(expected_postmaster) {
        return Err(MirrorError::new("config", "postmaster"));
    }
    let admitted_canonical = admitted_canonical(&admitted)?;
    let admitted_sha256 = sha256_hex(admitted_canonical.as_bytes());
    let expected_postmaster_sha256 = sha256_hex(expected_postmaster.as_bytes());
    let mut plan = Plan {
        config,
        admitted,
        admitted_sha256,
        expected_principal: expected_principal.to_string(),
        expected_database: expected_database.to_string(),
        expected_in_recovery,
        expected_postmaster: expected_postmaster.to_string(),
        expected_postmaster_sha256,
        plan_sha256: String::new(),
    };
    plan.plan_sha256 = sha256_hex(plan_preimage(&plan)?.as_bytes());
    Ok(plan)
}

pub fn store_plan(path: &Path, plan: &Plan) -> Result<(), MirrorError> {
    let body = plan_json(plan)?;
    write_new_private(path, body.as_bytes(), "plan_exists")
}

pub fn load_plan(path: &Path) -> Result<Plan, MirrorError> {
    let bytes = read_bytes(path, "config", "plan_path")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| MirrorError::new("plan_tamper", "json"))?;
    let map = parse_object(text).map_err(|_| MirrorError::new("plan_tamper", "json"))?;
    let plan = plan_from_map(&map)?;
    let recomputed = sha256_hex(plan_preimage(&plan)?.as_bytes());
    if recomputed != plan.plan_sha256 || recomputed != take_str(&map, "plan_sha256")? {
        return Err(MirrorError::new("plan_tamper", "plan_sha256"));
    }
    let postmaster_sha = sha256_hex(plan.expected_postmaster.as_bytes());
    if postmaster_sha != plan.expected_postmaster_sha256 {
        return Err(MirrorError::new("plan_tamper", "postmaster"));
    }
    let admitted_sha = sha256_hex(admitted_canonical(&plan.admitted)?.as_bytes());
    if admitted_sha != plan.admitted_sha256 {
        return Err(MirrorError::new("plan_tamper", "admitted"));
    }
    Ok(plan)
}

pub fn source_config(config: &InputConfig, password: &str, ca_pem: &[u8]) -> Result<PgSourceConfig, MirrorError> {
    let source_id = SourceId::new(&config.source_id).map_err(|_| MirrorError::new("config", "source_id"))?;
    let endpoint = PgEndpoint::new(&config.host, config.port, &config.database)
        .map_err(|_| MirrorError::new("config", "endpoint"))?;
    let credentials = PgCredentials::new(&config.user, password.to_string())
        .map_err(|_| MirrorError::new("config", "credentials"))?;
    let secret_ref = SecretRef::new(&config.secret_ref).map_err(|_| MirrorError::new("config", "secret_ref"))?;
    let tls = PgTlsConfig::from_ca_pem(ca_pem).map_err(|_| MirrorError::new("config", "ca_pem"))?;
    let budget = PgBudget::new(
        Duration::from_millis(config.connect_timeout_ms),
        Duration::from_millis(config.statement_timeout_ms),
        Duration::from_millis(config.lock_timeout_ms),
        u32::try_from(config.max_rows).map_err(|_| MirrorError::new("budget", "max_rows"))?,
        config.max_bytes,
    )
    .map_err(|_| MirrorError::new("budget", "budget"))?;
    let mut selected = Vec::new();
    for (schema, table) in &config.tables {
        selected.push(SelectedTable::new(schema.clone(), table.clone()).map_err(|_| MirrorError::new("config", "selection"))?);
    }
    let selection = TableSelection::new(selected).map_err(|_| MirrorError::new("config", "selection"))?;
    PgSourceConfig::try_new(source_id, endpoint, credentials, secret_ref, tls, budget, selection)
        .map_err(|_| MirrorError::new("config", "source"))
}

pub fn read_secret(path: &Path) -> Result<Zeroizing<String>, MirrorError> {
    let bytes = read_bytes(path, "credential_path", "file")?;
    let mut text = String::from_utf8(bytes).map_err(|_| MirrorError::new("credential_path", "encoding"))?;
    if text.ends_with('\n') {
        text.pop();
    }
    if text.is_empty() || text.contains('\0') || text.len() > 1024 {
        return Err(MirrorError::new("credential_path", "rejected"));
    }
    Ok(Zeroizing::new(text))
}

pub fn read_ca(path: &Path) -> Result<Vec<u8>, MirrorError> {
    read_bytes(path, "credential_path", "ca")
}

pub fn read_text(path: &Path, code: &'static str, fact: &'static str) -> Result<String, MirrorError> {
    let bytes = read_bytes(path, code, fact)?;
    String::from_utf8(bytes).map_err(|_| MirrorError::new(code, "encoding"))
}

pub fn write_new_private(path: &Path, bytes: &[u8], exists_code: &'static str) -> Result<(), MirrorError> {
    if !path.is_absolute() {
        return Err(MirrorError::new("config", "path"));
    }
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(MirrorError::new(exists_code, "symlink"));
        }
        return Err(MirrorError::new(exists_code, "exists"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| MirrorError::new(exists_code, "create"))?;
    std::io::Write::write_all(&mut file, bytes).map_err(|_| MirrorError::new("config", "io"))?;
    file.sync_all().map_err(|_| MirrorError::new("config", "io"))?;
    drop(file);
    fs::set_permissions(path, fs::Permissions::from_mode(0o400)).map_err(|_| MirrorError::new("config", "io"))?;
    Ok(())
}

pub fn admitted_canonical(tables: &[TableShape]) -> Result<String, MirrorError> {
    let mut out = String::from("{\"tables\":[");
    for (index, table) in tables.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"schema\":");
        out.push_str(&json_string(&table.schema)?);
        out.push_str(",\"name\":");
        out.push_str(&json_string(&table.name)?);
        out.push_str(",\"columns\":[");
        for (column_index, column) in table.columns.iter().enumerate() {
            if column_index > 0 {
                out.push(',');
            }
            out.push_str("{\"attnum\":");
            out.push_str(&column.attnum.to_string());
            out.push_str(",\"name\":");
            out.push_str(&json_string(&column.name)?);
            out.push_str(",\"oid\":");
            out.push_str(&column.oid.to_string());
            out.push_str(",\"typmod\":");
            out.push_str(&column.typmod.to_string());
            out.push_str(",\"not_null\":");
            out.push_str(if column.not_null { "true" } else { "false" });
            out.push('}');
        }
        out.push_str("],\"primary_key\":[");
        for (key_index, key) in table.primary_key.iter().enumerate() {
            if key_index > 0 {
                out.push(',');
            }
            out.push_str(&json_string(key)?);
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    Ok(out)
}

fn plan_preimage(plan: &Plan) -> Result<String, MirrorError> {
    let mut body = String::from("local-pg-mirror/v1 plan\n");
    push(&mut body, "engine", "postgresql")?;
    push(&mut body, "source_id", &plan.config.source_id)?;
    push(&mut body, "host", &plan.config.host)?;
    push(&mut body, "port", &plan.config.port.to_string())?;
    push(&mut body, "database", &plan.config.database)?;
    push(&mut body, "user", &plan.config.user)?;
    push(&mut body, "server_name", &plan.config.server_name)?;
    push(&mut body, "ca_sha256", &plan.config.ca_sha256)?;
    push(&mut body, "secret_ref", &plan.config.secret_ref)?;
    for (schema, table) in &plan.config.tables {
        let mut line = String::from("table=");
        line.push_str(schema);
        line.push('.');
        line.push_str(table);
        line.push('\n');
        body.push_str(&line);
    }
    push(&mut body, "connect_timeout_ms", &plan.config.connect_timeout_ms.to_string())?;
    push(&mut body, "statement_timeout_ms", &plan.config.statement_timeout_ms.to_string())?;
    push(&mut body, "lock_timeout_ms", &plan.config.lock_timeout_ms.to_string())?;
    push(&mut body, "max_rows", &plan.config.max_rows.to_string())?;
    push(&mut body, "max_bytes", &plan.config.max_bytes.to_string())?;
    push(&mut body, "batch_rows", &plan.config.batch_rows.to_string())?;
    push(&mut body, "snapshot_deadline_ms", &plan.config.snapshot_deadline_ms.to_string())?;
    push(&mut body, "generation", &plan.config.generation.to_string())?;
    push(&mut body, "shard", &plan.config.shard)?;
    push(&mut body, "owner", &plan.config.owner)?;
    push(&mut body, "target_host", &plan.config.target_host)?;
    push(&mut body, "target_port", &plan.config.target_port.to_string())?;
    push(&mut body, "target_database", &plan.config.target_database)?;
    push(&mut body, "target_admin_user", &plan.config.target_admin_user)?;
    push(&mut body, "target_runtime_user", &plan.config.target_runtime_user)?;
    push(&mut body, "target_reader_user", &plan.config.target_reader_user)?;
    push(&mut body, "target_server_name", &plan.config.target_server_name)?;
    push(&mut body, "target_ca_sha256", &plan.config.target_ca_sha256)?;
    push(&mut body, "admitted_sha256", &plan.admitted_sha256)?;
    push(&mut body, "expected_principal", &plan.expected_principal)?;
    push(&mut body, "expected_database", &plan.expected_database)?;
    push(
        &mut body,
        "expected_in_recovery",
        if plan.expected_in_recovery { "true" } else { "false" },
    )?;
    push(&mut body, "expected_postmaster_sha256", &plan.expected_postmaster_sha256)?;
    Ok(body)
}

fn push(body: &mut String, key: &str, value: &str) -> Result<(), MirrorError> {
    if value.contains('\n') || value.contains('\r') {
        return Err(MirrorError::new("config", "preimage"));
    }
    body.push_str(key);
    body.push('=');
    body.push_str(value);
    body.push('\n');
    Ok(())
}

fn plan_json(plan: &Plan) -> Result<String, MirrorError> {
    let mut tables = Vec::new();
    for (schema, table) in &plan.config.tables {
        tables.push(serde_json::json!({"schema": schema, "table": table}));
    }
    let mut admitted_tables = Vec::new();
    for table in &plan.admitted {
        let mut columns = Vec::new();
        for column in &table.columns {
            columns.push(serde_json::json!({
                "attnum": column.attnum,
                "name": column.name,
                "oid": column.oid,
                "typmod": column.typmod,
                "not_null": column.not_null,
            }));
        }
        admitted_tables.push(serde_json::json!({
            "schema": table.schema,
            "name": table.name,
            "columns": columns,
            "primary_key": table.primary_key,
        }));
    }
    let value = serde_json::json!({
        "protocol": PROTOCOL,
        "engine": "postgresql",
        "source_id": plan.config.source_id,
        "host": plan.config.host,
        "port": plan.config.port,
        "database": plan.config.database,
        "user": plan.config.user,
        "server_name": plan.config.server_name,
        "ca_sha256": plan.config.ca_sha256,
        "secret_ref": plan.config.secret_ref,
        "tables": tables,
        "connect_timeout_ms": plan.config.connect_timeout_ms,
        "statement_timeout_ms": plan.config.statement_timeout_ms,
        "lock_timeout_ms": plan.config.lock_timeout_ms,
        "max_rows": plan.config.max_rows,
        "max_bytes": plan.config.max_bytes,
        "batch_rows": plan.config.batch_rows,
        "snapshot_deadline_ms": plan.config.snapshot_deadline_ms,
        "generation": plan.config.generation,
        "shard": plan.config.shard,
        "owner": plan.config.owner,
        "target_host": plan.config.target_host,
        "target_port": plan.config.target_port,
        "target_database": plan.config.target_database,
        "target_admin_user": plan.config.target_admin_user,
        "target_runtime_user": plan.config.target_runtime_user,
        "target_reader_user": plan.config.target_reader_user,
        "target_server_name": plan.config.target_server_name,
        "target_ca_sha256": plan.config.target_ca_sha256,
        "admitted": {"tables": admitted_tables},
        "admitted_sha256": plan.admitted_sha256,
        "expected_principal": plan.expected_principal,
        "expected_database": plan.expected_database,
        "expected_in_recovery": plan.expected_in_recovery,
        "expected_postmaster": plan.expected_postmaster,
        "expected_postmaster_sha256": plan.expected_postmaster_sha256,
        "plan_sha256": plan.plan_sha256,
    });
    serde_json::to_string_pretty(&value).map_err(|_| MirrorError::new("config", "json"))
}

fn plan_from_map(map: &Map<String, Value>) -> Result<Plan, MirrorError> {
    exact_keys(
        map,
        &[
            "protocol",
            "engine",
            "source_id",
            "host",
            "port",
            "database",
            "user",
            "server_name",
            "ca_sha256",
            "secret_ref",
            "tables",
            "connect_timeout_ms",
            "statement_timeout_ms",
            "lock_timeout_ms",
            "max_rows",
            "max_bytes",
            "batch_rows",
            "snapshot_deadline_ms",
            "generation",
            "shard",
            "owner",
            "target_host",
            "target_port",
            "target_database",
            "target_admin_user",
            "target_runtime_user",
            "target_reader_user",
            "target_server_name",
            "target_ca_sha256",
            "admitted",
            "admitted_sha256",
            "expected_principal",
            "expected_database",
            "expected_in_recovery",
            "expected_postmaster",
            "expected_postmaster_sha256",
            "plan_sha256",
        ],
    )
    .map_err(|_| MirrorError::new("plan_tamper", "field"))?;
    if take_str(map, "engine")? != "postgresql" || take_str(map, "protocol")? != PROTOCOL {
        return Err(MirrorError::new("plan_tamper", "engine"));
    }
    let config = InputConfig {
        source_id: take_str(map, "source_id")?.to_string(),
        host: take_str(map, "host")?.to_string(),
        port: port_of(take_u64(map, "port")?)?,
        database: take_str(map, "database")?.to_string(),
        user: take_str(map, "user")?.to_string(),
        server_name: take_str(map, "server_name")?.to_string(),
        ca_sha256: take_str(map, "ca_sha256")?.to_string(),
        secret_ref: take_str(map, "secret_ref")?.to_string(),
        tables: parse_tables(map.get("tables").ok_or_else(|| MirrorError::new("plan_tamper", "tables"))?)?,
        connect_timeout_ms: take_u64(map, "connect_timeout_ms")?,
        statement_timeout_ms: take_u64(map, "statement_timeout_ms")?,
        lock_timeout_ms: take_u64(map, "lock_timeout_ms")?,
        max_rows: take_u64(map, "max_rows")?,
        max_bytes: take_u64(map, "max_bytes")?,
        batch_rows: take_u64(map, "batch_rows")?,
        snapshot_deadline_ms: take_u64(map, "snapshot_deadline_ms")?,
        generation: take_u64(map, "generation")?,
        shard: take_str(map, "shard")?.to_string(),
        owner: take_str(map, "owner")?.to_string(),
        target_host: take_str(map, "target_host")?.to_string(),
        target_port: port_of(take_u64(map, "target_port")?)?,
        target_database: take_str(map, "target_database")?.to_string(),
        target_admin_user: take_str(map, "target_admin_user")?.to_string(),
        target_runtime_user: take_str(map, "target_runtime_user")?.to_string(),
        target_reader_user: take_str(map, "target_reader_user")?.to_string(),
        target_server_name: take_str(map, "target_server_name")?.to_string(),
        target_ca_sha256: take_str(map, "target_ca_sha256")?.to_string(),
    };
    validate_config(&config).map_err(|_| MirrorError::new("plan_tamper", "config"))?;
    let admitted = parse_admitted(map.get("admitted").ok_or_else(|| MirrorError::new("plan_tamper", "admitted"))?)?;
    validate_admitted(&config, &admitted).map_err(|_| MirrorError::new("plan_tamper", "admitted"))?;
    Ok(Plan {
        config,
        admitted,
        admitted_sha256: take_str(map, "admitted_sha256")?.to_string(),
        expected_principal: take_str(map, "expected_principal")?.to_string(),
        expected_database: take_str(map, "expected_database")?.to_string(),
        expected_in_recovery: take_bool(map, "expected_in_recovery")?,
        expected_postmaster: take_str(map, "expected_postmaster")?.to_string(),
        expected_postmaster_sha256: take_str(map, "expected_postmaster_sha256")?.to_string(),
        plan_sha256: take_str(map, "plan_sha256")?.to_string(),
    })
}

fn parse_tables(value: &Value) -> Result<Vec<(String, String)>, MirrorError> {
    let Value::Array(items) = value else {
        return Err(MirrorError::new("config", "selection"));
    };
    if items.is_empty() {
        return Err(MirrorError::new("config", "selection"));
    }
    let mut tables = Vec::new();
    for item in items {
        let Value::Object(map) = item else {
            return Err(MirrorError::new("config", "selection"));
        };
        exact_keys(map, &["schema", "table"])?;
        tables.push((take_str(map, "schema")?.to_string(), take_str(map, "table")?.to_string()));
    }
    Ok(tables)
}

fn parse_admitted(value: &Value) -> Result<Vec<TableShape>, MirrorError> {
    let Value::Object(map) = value else {
        return Err(MirrorError::new("plan_tamper", "admitted"));
    };
    exact_keys(map, &["tables"])?;
    let Value::Array(items) = map.get("tables").ok_or_else(|| MirrorError::new("plan_tamper", "admitted"))? else {
        return Err(MirrorError::new("plan_tamper", "admitted"));
    };
    let mut tables = Vec::new();
    for item in items {
        let Value::Object(table) = item else {
            return Err(MirrorError::new("plan_tamper", "admitted"));
        };
        exact_keys(table, &["schema", "name", "columns", "primary_key"])?;
        let Value::Array(columns) = table.get("columns").ok_or_else(|| MirrorError::new("plan_tamper", "admitted"))? else {
            return Err(MirrorError::new("plan_tamper", "admitted"));
        };
        let mut shapes = Vec::new();
        for column in columns {
            let Value::Object(column) = column else {
                return Err(MirrorError::new("plan_tamper", "admitted"));
            };
            exact_keys(column, &["attnum", "name", "oid", "typmod", "not_null"])?;
            shapes.push(ColumnShape {
                attnum: i32::try_from(take_i64(column, "attnum")?).map_err(|_| MirrorError::new("plan_tamper", "attnum"))?,
                name: take_str(column, "name")?.to_string(),
                oid: take_i64(column, "oid")?,
                typmod: i32::try_from(take_i64(column, "typmod")?).map_err(|_| MirrorError::new("plan_tamper", "typmod"))?,
                not_null: take_bool(column, "not_null")?,
            });
        }
        let Value::Array(keys) = table.get("primary_key").ok_or_else(|| MirrorError::new("plan_tamper", "admitted"))? else {
            return Err(MirrorError::new("plan_tamper", "admitted"));
        };
        let mut primary_key = Vec::new();
        for key in keys {
            let Value::String(key) = key else {
                return Err(MirrorError::new("plan_tamper", "admitted"));
            };
            primary_key.push(key.clone());
        }
        tables.push(TableShape {
            schema: take_str(table, "schema")?.to_string(),
            name: take_str(table, "name")?.to_string(),
            columns: shapes,
            primary_key,
        });
    }
    Ok(tables)
}

fn validate_config(config: &InputConfig) -> Result<(), MirrorError> {
    check_budget(config)?;
    SourceId::new(&config.source_id).map_err(|_| MirrorError::new("config", "source_id"))?;
    PgEndpoint::new(&config.host, config.port, &config.database).map_err(|_| MirrorError::new("config", "endpoint"))?;
    PgEndpoint::new(&config.target_host, config.target_port, &config.target_database)
        .map_err(|_| MirrorError::new("config", "endpoint"))?;
    if config.host != config.server_name || config.target_host != config.target_server_name {
        return Err(MirrorError::new("config", "server_name"));
    }
    if !simple_ident(&config.user)
        || !simple_ident(&config.target_admin_user)
        || !simple_ident(&config.target_runtime_user)
        || !simple_ident(&config.target_reader_user)
    {
        return Err(MirrorError::new("config", "user"));
    }
    if config.target_runtime_user == config.target_admin_user
        || config.target_reader_user == config.target_admin_user
        || config.target_reader_user == config.target_runtime_user
    {
        return Err(MirrorError::new("config", "user"));
    }
    if !hex64(&config.ca_sha256) || !hex64(&config.target_ca_sha256) {
        return Err(MirrorError::new("config", "ca_sha256"));
    }
    SecretRef::new(&config.secret_ref).map_err(|_| MirrorError::new("config", "secret_ref"))?;
    if config.generation == 0 || i64::try_from(config.generation).is_err() {
        return Err(MirrorError::new("config", "generation"));
    }
    if !shard_ok(&config.shard) {
        return Err(MirrorError::new("config", "shard"));
    }
    if !uuid_ok(&config.owner) {
        return Err(MirrorError::new("config", "owner"));
    }
    if config.tables.is_empty() {
        return Err(MirrorError::new("config", "selection"));
    }
    let mut seen = Vec::<(String, String)>::new();
    for (schema, table) in &config.tables {
        if !simple_ident(schema) || !simple_ident(table) {
            return Err(MirrorError::new("config", "selection"));
        }
        let key = (schema.clone(), table.clone());
        if seen.contains(&key) {
            return Err(MirrorError::new("config", "selection"));
        }
        seen.push(key);
    }
    Ok(())
}

fn validate_admitted(config: &InputConfig, admitted: &[TableShape]) -> Result<(), MirrorError> {
    if admitted.len() != config.tables.len() {
        return Err(MirrorError::new("config", "admitted"));
    }
    for (index, table) in admitted.iter().enumerate() {
        if table.schema != config.tables[index].0 || table.name != config.tables[index].1 {
            return Err(MirrorError::new("config", "admitted"));
        }
        if table.columns.is_empty() {
            return Err(MirrorError::new("config", "admitted"));
        }
        for column in &table.columns {
            if !simple_ident(&column.name) || column.attnum <= 0 || column.oid <= 0 {
                return Err(MirrorError::new("config", "admitted"));
            }
        }
        for key in &table.primary_key {
            if !table.columns.iter().any(|column| column.name == *key) {
                return Err(MirrorError::new("config", "admitted"));
            }
        }
    }
    Ok(())
}

fn check_budget(config: &InputConfig) -> Result<(), MirrorError> {
    range(config.connect_timeout_ms, 1, 30_000, "connect_timeout")?;
    range(config.statement_timeout_ms, 1, 30_000, "statement_timeout")?;
    range(config.lock_timeout_ms, 1, 1_000, "lock_timeout")?;
    range(config.batch_rows, 1, 1_000, "batch_rows")?;
    range(config.snapshot_deadline_ms, 1, 300_000, "snapshot_deadline")?;
    range(config.max_rows, 1, 100_000, "max_rows")?;
    range(config.max_bytes, 1, 33_554_432, "max_bytes")?;
    Ok(())
}

fn range(value: u64, low: u64, high: u64, fact: &'static str) -> Result<(), MirrorError> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(MirrorError::new("budget", fact))
    }
}

fn port_of(value: u64) -> Result<u16, MirrorError> {
    u16::try_from(value).ok().filter(|port| *port != 0).ok_or_else(|| MirrorError::new("config", "port"))
}

pub fn simple_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn shard_ok(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && value.len() <= 128
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | ':' | '-'))
}

fn uuid_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let groups = [8usize, 4, 4, 4, 12];
    let mut index = 0;
    for (group, len) in groups.iter().enumerate() {
        if group > 0 {
            if bytes[index] != b'-' {
                return false;
            }
            index += 1;
        }
        for _ in 0..*len {
            if !bytes[index].is_ascii_hexdigit() || bytes[index].is_ascii_uppercase() {
                return false;
            }
            index += 1;
        }
    }
    index == 36
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn postmaster_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.chars().all(|ch| !ch.is_control())
}

fn read_bytes(path: &Path, code: &'static str, fact: &'static str) -> Result<Vec<u8>, MirrorError> {
    if !path.is_absolute() {
        return Err(MirrorError::new(code, fact));
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return Err(MirrorError::new(code, fact)),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(MirrorError::new(code, fact));
    }
    fs::read(path).map_err(|_| MirrorError::new(code, fact))
}
