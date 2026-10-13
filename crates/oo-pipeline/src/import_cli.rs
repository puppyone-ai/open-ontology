//! `oo-pipeline import-pg`.
//!
//! Stdin is one bounded `oo.pipeline.import-pg/v1` document. The command
//! prepares a snapshot from the configured read-only source and calls
//! [`crate::full_import::import_unpublished`] on one target connection.
//! It does not install the target schema, and it does not publish a run.
//!
//! [`crate::full_import::RunSummary`] and [`crate::full_import::RunState`] have
//! public fields, so a Rust caller can build an in-memory `awaiting_publish`
//! value. This command does not. Stdout is the summary that
//! `import_unpublished` returned. The stored `_oo.full_import_run` row is the
//! seal. `awaiting_publish` is not publication. A later failure to write
//! stdout does not remove that row.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy,
    SecretRef, SelectedTable, SourceError, SourceErrorKind, SourceId, StreamBudget, TableSelection,
};
use oo_core::parse_json_value;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Row};

use crate::copy_encoder::CopyBudgets;
use crate::full_import::{ImportError, ImportRequest, RunState, RunSummary, import_unpublished};
use crate::target_installer::verify_full_import_readiness;

/// Request protocol. The binary accepts this value and no other.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.import-pg/v1";

/// Result protocol written after the import entry returns.
pub const RESULT_PROTOCOL: &str = "oo.pipeline.import-pg-result/v1";

const DEFAULT_DEADLINE_MS: u64 = 60_000;
const MAX_DEADLINE_MS: u64 = 120_000;

/// Read one request, import, and write the returned summary as canonical JSON.
pub async fn import_pg() -> Result<(), u8> {
    match execute().await {
        Ok(document) => write_stdout(&document),
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

async fn execute() -> Result<String, Stop> {
    let text = read_stdin()?;
    let value =
        parse_json_value(&text, REQUEST_PROTOCOL).map_err(|_| Stop::Fixed("invalid input"))?;
    let mut request: Request =
        serde_json::from_value(value).map_err(|_| Stop::Fixed("invalid input"))?;
    if request.protocol != REQUEST_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    if !valid_owner(&request.owner)
        || !valid_shard(&request.shard)
        || request.source.tables.is_empty()
    {
        return Err(Stop::Fixed("invalid input"));
    }
    let policy = type_policy(request.type_policy)?;
    let budget = source_budget(request.source.budget.take())?;
    let stream = stream_budget(request.stream)?;
    let copy = copy_budget(request.copy)?;
    let deadline_ms = request.deadline_ms.unwrap_or(DEFAULT_DEADLINE_MS);
    if !(1..=MAX_DEADLINE_MS).contains(&deadline_ms) {
        return Err(Stop::Fixed("invalid deadline"));
    }
    let source_password = read_password(&request.source.password_file, "source password file")?;
    let source_ca = read_ca(&request.source.ca_file, "source ca file")?;
    let target_password = read_password(&request.target.password_file, "target password file")?;
    let target_ca = read_ca(&request.target.ca_file, "target ca file")?;
    if request.source_generation == 0 || request.source_generation > i64::MAX as u64 {
        return Err(Stop::Fixed("invalid generation"));
    }
    let config = build_source(&request.source, source_password, &source_ca, budget)?;
    PgEndpoint::new(
        &request.target.host,
        request.target.port,
        &request.target.database,
    )
    .map_err(from_source)?;
    PgCredentials::new(&request.target.runtime_principal, target_password.as_str())
        .map_err(from_source)?;
    let deadline = Instant::now() + Duration::from_millis(deadline_ms);
    let mut conn = connect_target(
        &request.target,
        &target_password,
        &target_ca,
        budget.connect_timeout(),
    )
    .await?;
    expect_principal(
        &mut conn,
        &request.target.runtime_principal,
        budget.statement_timeout(),
    )
    .await?;
    verify_full_import_readiness(&mut conn)
        .await
        .map_err(|_| Stop::Fixed("target schema is not ready"))?;
    let snapshot = PgConnector::new(config)
        .prepare_snapshot(policy, request.source_generation, deadline)
        .await
        .map_err(from_source)?;
    let summary = import_unpublished(
        &mut conn,
        ImportRequest {
            shard: request.shard,
            owner: request.owner,
            snapshot,
            stream,
            copy,
        },
    )
    .await
    .map_err(from_import)?;
    render(summary)
}

fn write_stdout(document: &str) -> Result<(), u8> {
    let mut payload = Vec::with_capacity(document.len() + 1);
    payload.extend_from_slice(document.as_bytes());
    payload.push(b'\n');
    match io::stdout().write_all(&payload) {
        Ok(()) => Ok(()),
        Err(_) => {
            eprintln!("oo-pipeline: import output");
            Err(1)
        }
    }
}

fn render(summary: RunSummary) -> Result<String, Stop> {
    if summary.state != RunState::AwaitingPublish || !valid_owner(&summary.run_id) {
        return Err(Stop::Fixed("import output"));
    }
    let mut tables = Vec::with_capacity(summary.tables.len());
    for table in &summary.tables {
        if !valid_table_physical(&table.physical_name) || !valid_digest(&table.canonical_digest) {
            return Err(Stop::Fixed("import output"));
        }
        let mut columns = Vec::with_capacity(table.columns.len());
        for column in &table.columns {
            if !valid_column_physical(&column.physical_name) {
                return Err(Stop::Fixed("import output"));
            }
            columns.push(serde_json::json!({
                "attnum": column.attnum,
                "name": column.name,
                "physical_name": column.physical_name,
                "type_oid": column.type_oid,
            }));
        }
        tables.push(serde_json::json!({
            "canonical_digest": table.canonical_digest,
            "columns": columns,
            "name": table.name,
            "physical_name": table.physical_name,
            "rows": table.rows,
            "schema": table.schema,
        }));
    }
    let table_count = u64::try_from(summary.tables.len()).unwrap_or(u64::MAX);
    let value = serde_json::json!({
        "protocol": RESULT_PROTOCOL,
        "run_id": summary.run_id,
        "source_generation": summary.source_generation,
        "state": "awaiting_publish",
        "table_count": table_count,
        "tables": tables,
        "target_generation": summary.target_generation,
    });
    Ok(canonical_json(&value))
}

pub(crate) fn build_source(
    source: &SourceIn,
    password: String,
    ca: &[u8],
    budget: PgBudget,
) -> Result<PgSourceConfig, Stop> {
    let mut tables = Vec::with_capacity(source.tables.len());
    for table in &source.tables {
        tables.push(SelectedTable::new(&table.schema, &table.table).map_err(from_source)?);
    }
    PgSourceConfig::try_new(
        SourceId::new(&source.source_id).map_err(from_source)?,
        PgEndpoint::new(&source.host, source.port, &source.database).map_err(from_source)?,
        PgCredentials::new(&source.username, password).map_err(from_source)?,
        SecretRef::new(&source.secret_ref).map_err(from_source)?,
        PgTlsConfig::from_ca_pem(ca).map_err(from_source)?,
        budget,
        TableSelection::new(tables).map_err(from_source)?,
    )
    .map_err(from_source)
}

async fn connect_target(
    target: &TargetIn,
    password: &str,
    ca: &[u8],
    timeout: Duration,
) -> Result<sqlx::postgres::PgConnection, Stop> {
    let options = PgConnectOptions::from_explicit()
        .host(&target.host)
        .port(target.port)
        .username(&target.runtime_principal)
        .password(password)
        .database(&target.database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        return Err(Stop::Fixed("target connect"));
    }
    let options = options.disable_statement_logging();
    match tokio::time::timeout(timeout, options.connect()).await {
        Ok(Ok(conn)) => Ok(conn),
        _ => Err(Stop::Fixed("target connect")),
    }
}

async fn expect_principal(
    conn: &mut sqlx::postgres::PgConnection,
    principal: &str,
    timeout: Duration,
) -> Result<(), Stop> {
    let query = sqlx::query("SELECT CURRENT_USER::text, SESSION_USER::text");
    let row = match tokio::time::timeout(timeout, query.fetch_one(&mut *conn)).await {
        Ok(Ok(row)) => row,
        _ => return Err(Stop::Fixed("target principal")),
    };
    let current: String = row
        .try_get(0)
        .map_err(|_| Stop::Fixed("target principal"))?;
    let session: String = row
        .try_get(1)
        .map_err(|_| Stop::Fixed("target principal"))?;
    if current == principal && session == principal {
        Ok(())
    } else {
        Err(Stop::Fixed("target principal"))
    }
}

pub(crate) fn source_budget(budget: Option<BudgetIn>) -> Result<PgBudget, Stop> {
    let (connect_ms, statement_ms, lock_ms, max_rows, max_bytes) = match budget {
        None => (30_000, 60_000, 30_000, 10_000, 8_388_608),
        Some(item) => (
            item.connect_ms,
            item.statement_ms,
            item.lock_ms,
            item.max_rows,
            item.max_bytes,
        ),
    };
    if max_rows == 0 || max_rows > 100_000 || max_bytes == 0 || max_bytes > 32 * 1024 * 1024 {
        return Err(Stop::Fixed("invalid budget"));
    }
    let rows = u32::try_from(max_rows).map_err(|_| Stop::Fixed("invalid budget"))?;
    PgBudget::new(
        timeout_ms(connect_ms)?,
        timeout_ms(statement_ms)?,
        timeout_ms(lock_ms)?,
        rows,
        max_bytes,
    )
    .map_err(|_| Stop::Fixed("invalid budget"))
}

fn stream_budget(stream: Option<StreamIn>) -> Result<StreamBudget, Stop> {
    let (batch_rows, batch_bytes, max_value, max_row) = match stream {
        None => (1_024, 1_048_576, 65_536, 262_144),
        Some(item) => (
            item.batch_rows,
            item.batch_bytes,
            item.max_value_bytes,
            item.max_row_bytes,
        ),
    };
    if batch_rows == 0
        || batch_rows > 65_536
        || batch_bytes == 0
        || batch_bytes > 32 * 1024 * 1024
        || max_value == 0
        || max_value > 1_048_576
        || !(2..=4_194_304).contains(&max_row)
        || max_row < max_value
    {
        return Err(Stop::Fixed("invalid budget"));
    }
    StreamBudget::try_new(
        u32::try_from(batch_rows).map_err(|_| Stop::Fixed("invalid budget"))?,
        batch_bytes,
        u32::try_from(max_value).map_err(|_| Stop::Fixed("invalid budget"))?,
        u32::try_from(max_row).map_err(|_| Stop::Fixed("invalid budget"))?,
    )
    .map_err(|_| Stop::Fixed("invalid budget"))
}

fn copy_budget(copy: Option<CopyIn>) -> Result<CopyBudgets, Stop> {
    let (max_value, max_row, max_chunk, max_rows) = match copy {
        None => (65_536, 262_144, 1_048_576, 1_024),
        Some(item) => (
            item.max_value_bytes,
            item.max_row_bytes,
            item.max_chunk_bytes,
            item.max_rows_per_chunk,
        ),
    };
    if max_value == 0
        || max_value > 1_048_576
        || max_row < max_value
        || max_row > 4_194_304
        || !(19..=8_388_608).contains(&max_chunk)
        || max_chunk < max_row
        || max_rows == 0
        || max_rows > 65_536
    {
        return Err(Stop::Fixed("invalid budget"));
    }
    Ok(CopyBudgets {
        max_value_bytes: u32::try_from(max_value).map_err(|_| Stop::Fixed("invalid budget"))?,
        max_row_bytes: u32::try_from(max_row).map_err(|_| Stop::Fixed("invalid budget"))?,
        max_chunk_bytes: u32::try_from(max_chunk).map_err(|_| Stop::Fixed("invalid budget"))?,
        max_rows_per_chunk: u32::try_from(max_rows).map_err(|_| Stop::Fixed("invalid budget"))?,
    })
}

fn type_policy(policy: Option<TypePolicyIn>) -> Result<PgTypePolicy, Stop> {
    match policy {
        None => Ok(PgTypePolicy::PRESERVE_ROW),
        Some(item) if item.unsupported == "preserve_row" && item.reject == "preserve_row" => {
            Ok(PgTypePolicy::PRESERVE_ROW)
        }
        Some(_) => Err(Stop::Fixed("invalid input")),
    }
}

fn timeout_ms(ms: u64) -> Result<Duration, Stop> {
    if (1..=120_000).contains(&ms) {
        Ok(Duration::from_millis(ms))
    } else {
        Err(Stop::Fixed("invalid budget"))
    }
}

pub(crate) fn from_source(err: SourceError) -> Stop {
    let kind = match err.kind() {
        SourceErrorKind::Config => "config",
        SourceErrorKind::Connect => "connect",
        SourceErrorKind::Auth => "auth",
        SourceErrorKind::Tls => "tls",
        SourceErrorKind::Privilege => "privilege",
        SourceErrorKind::Version => "version",
        SourceErrorKind::Budget => "budget",
        SourceErrorKind::Catalog => "catalog",
        SourceErrorKind::Identifier => "identifier",
        SourceErrorKind::Protocol => "protocol",
    };
    match err.fact() {
        Some(fact) if safe_token(fact) => Stop::Source { kind, fact },
        _ => Stop::Noted(format!("oo-pipeline: source {kind}")),
    }
}

fn from_import(err: ImportError) -> Stop {
    match err {
        ImportError::NotReady => Stop::Fixed("target schema is not ready"),
        ImportError::Unsupported { .. } => Stop::Fixed("unsupported copy type"),
        ImportError::Refused { reason } if safe_token(&reason) => {
            Stop::Noted(format!("oo-pipeline: full import refused {reason}"))
        }
        ImportError::Refused { .. } => Stop::Fixed("full import refused"),
        ImportError::Conflict => Stop::Fixed("import conflict"),
        ImportError::Binding(_) => Stop::Fixed("import binding"),
        ImportError::Source(text) => from_source_text(&text),
        ImportError::Database { sqlstate, .. } => sqlstate_stop(&sqlstate),
    }
}

fn from_source_text(text: &str) -> Stop {
    if text.len() < 200
        && text.starts_with("source error:")
        && text.chars().all(|ch| ch.is_ascii_graphic() || ch == ' ')
    {
        Stop::Noted(format!("oo-pipeline: {text}"))
    } else {
        Stop::Fixed("import source")
    }
}

fn sqlstate_stop(sqlstate: &str) -> Stop {
    if sqlstate.len() == 5 && sqlstate.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Stop::Noted(format!("oo-pipeline: target database {sqlstate}"))
    } else {
        Stop::Fixed("target database")
    }
}

fn safe_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_owner(owner: &str) -> bool {
    let bytes = owner.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        if (hyphen && *byte != b'-') || (!hyphen && !byte.is_ascii_hexdigit()) {
            return false;
        }
    }
    true
}

fn valid_shard(shard: &str) -> bool {
    let mut chars = shard.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() && shard.len() <= 128 => chars.all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | ':' | '-')
        }),
        _ => false,
    }
}

fn valid_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_table_physical(name: &str) -> bool {
    physical_name(name, b"pt_")
}

fn valid_column_physical(name: &str) -> bool {
    physical_name(name, b"pc_")
}

fn physical_name(name: &str, prefix: &[u8]) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 63
        && bytes.starts_with(prefix)
        && bytes[prefix.len()..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn read_stdin() -> Result<String, Stop> {
    read_bounded_stdin("import input")
}

pub(crate) fn read_bounded_stdin(io_label: &'static str) -> Result<String, Stop> {
    let mut stdin = io::stdin();
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    let limit = oo_core::limits::DOCUMENT_MAX_BYTES;
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| Stop::Fixed(io_label))?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > limit {
            return Err(Stop::Fixed("invalid input"));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(buf).map_err(|_| Stop::Fixed("invalid input"))
}

pub(crate) fn read_password(path: &str, label: &'static str) -> Result<String, Stop> {
    let bytes = read_bounded(path, 1026).map_err(|_| Stop::Fixed(label))?;
    let mut text = String::from_utf8(bytes).map_err(|_| Stop::Fixed(label))?;
    if text.ends_with("\r\n") {
        text.truncate(text.len() - 2);
    } else if text.ends_with('\n') {
        text.pop();
    }
    if text.is_empty() || text.len() > 1024 || text.contains('\0') {
        return Err(Stop::Fixed(label));
    }
    Ok(text)
}

pub(crate) fn read_ca(path: &str, label: &'static str) -> Result<Vec<u8>, Stop> {
    let bytes = read_bounded(path, 1024 * 1024).map_err(|_| Stop::Fixed(label))?;
    PgTlsConfig::from_ca_pem(&bytes).map_err(|_| Stop::Fixed(label))?;
    Ok(bytes)
}

fn read_bounded(path: &str, max: usize) -> Result<Vec<u8>, ()> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !meta.file_type().is_file() || meta.len() > u64::try_from(max).unwrap_or(u64::MAX) {
        return Err(());
    }
    let mut file = File::open(Path::new(path)).map_err(|_| ())?;
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = file.read(&mut chunk).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > max {
            return Err(());
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

pub(crate) enum Stop {
    Fixed(&'static str),
    Source {
        kind: &'static str,
        fact: &'static str,
    },
    Noted(String),
}

impl Stop {
    pub(crate) fn emit(self) {
        match self {
            Self::Fixed(message) => eprintln!("oo-pipeline: {message}"),
            Self::Source { kind, fact } => eprintln!("oo-pipeline: source {kind} {fact}"),
            Self::Noted(line) => eprintln!("{line}"),
        }
    }
}

fn strict_some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(serde::de::Error::custom("null"));
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

fn strict_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    strict_some(deserializer)
}

fn strict_budget<'de, D>(deserializer: D) -> Result<Option<BudgetIn>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    strict_some(deserializer)
}

fn strict_stream<'de, D>(deserializer: D) -> Result<Option<StreamIn>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    strict_some(deserializer)
}

fn strict_copy<'de, D>(deserializer: D) -> Result<Option<CopyIn>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    strict_some(deserializer)
}

fn strict_policy<'de, D>(deserializer: D) -> Result<Option<TypePolicyIn>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    strict_some(deserializer)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    source: SourceIn,
    target: TargetIn,
    shard: String,
    owner: String,
    source_generation: u64,
    #[serde(default, deserialize_with = "strict_u64")]
    deadline_ms: Option<u64>,
    #[serde(default, deserialize_with = "strict_stream")]
    stream: Option<StreamIn>,
    #[serde(default, deserialize_with = "strict_copy")]
    copy: Option<CopyIn>,
    #[serde(default, deserialize_with = "strict_policy")]
    type_policy: Option<TypePolicyIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceIn {
    source_id: String,
    host: String,
    port: u16,
    database: String,
    username: String,
    secret_ref: String,
    pub(crate) password_file: String,
    pub(crate) ca_file: String,
    pub(crate) tables: Vec<TableIn>,
    #[serde(default, deserialize_with = "strict_budget")]
    pub(crate) budget: Option<BudgetIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetIn {
    host: String,
    port: u16,
    database: String,
    runtime_principal: String,
    password_file: String,
    ca_file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TableIn {
    schema: String,
    table: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BudgetIn {
    connect_ms: u64,
    statement_ms: u64,
    lock_ms: u64,
    max_rows: u64,
    max_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamIn {
    batch_rows: u64,
    batch_bytes: u64,
    max_value_bytes: u64,
    max_row_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyIn {
    max_value_bytes: u64,
    max_row_bytes: u64,
    max_chunk_bytes: u64,
    max_rows_per_chunk: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypePolicyIn {
    unsupported: String,
    reject: String,
}
