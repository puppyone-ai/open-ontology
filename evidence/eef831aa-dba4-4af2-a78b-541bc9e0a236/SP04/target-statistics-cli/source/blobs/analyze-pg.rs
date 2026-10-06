//! `analyze-pg` measures mapped shadow statistics on one target database.
//!
//! Stdin is one bounded `oo.pipeline.analyze-pg/v1` document. The password and
//! the CA stay in files named by that document. This command calls
//! [`oo_pipeline::target_statistics::analyze_mapped_shadows`] and writes that
//! receipt. It does not import, does not open a source database, and does not
//! publish.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use oo_connectors::{PgCredentials, PgEndpoint, PgTlsConfig};
use oo_core::parse_json_value;
use serde::Deserialize;
use serde_json::{Map, Value};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Row};

use oo_pipeline::physical_mapping::{plan_physical_mapping, Level, MappingRequest, PlanError};
use oo_pipeline::target_statistics::{
    analyze_mapped_shadows, ConnectionDisposition, LogicalTableRef, MaintenanceError,
    StatisticsError, StatisticsRequest, MAX_TABLES, MAX_TIMEOUT_MS, MIN_TIMEOUT_MS, PROTOCOL,
};

const REQUEST_PROTOCOL: &str = "oo.pipeline.analyze-pg/v1";
const RESULT_PROTOCOL: &str = "oo.pipeline.analyze-pg-result/v1";
const DEFAULT_CONNECT_MS: u64 = 30_000;
const MAX_CONNECT_MS: u64 = 120_000;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _argv0 = args.next();
    if args.next().is_some() {
        eprintln!("analyze-pg: usage");
        return ExitCode::from(2);
    }
    match execute() {
        Ok(code) => code,
        Err(fail) => {
            fail.emit();
            ExitCode::from(fail.code)
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn execute() -> Result<ExitCode, Fail> {
    let document = run().await?;
    write_stdout(&document)?;
    Ok(ExitCode::SUCCESS)
}

async fn run() -> Result<String, Fail> {
    let text = read_stdin()?;
    let value = parse_json_value(&text, REQUEST_PROTOCOL).map_err(|_| Fail::input())?;
    let request: Request = serde_json::from_value(value).map_err(|_| Fail::input())?;
    if request.protocol != REQUEST_PROTOCOL {
        return Err(Fail::new(3, "invalid version"));
    }
    let connect_ms = request.connect_ms.unwrap_or(DEFAULT_CONNECT_MS);
    if !(1..=MAX_CONNECT_MS).contains(&connect_ms) {
        return Err(Fail::input());
    }
    let endpoint = PgEndpoint::new(&request.target.host, request.target.port, &request.target.database)
        .map_err(|_| Fail::input())?;
    PgCredentials::new(&request.target.operator, "x").map_err(|_| Fail::input())?;
    closed(&request)?;
    let statistics = statistics_request(&request);
    let password = read_password(&request.target.password_file)?;
    let ca = read_ca(&request.target.ca_file)?;
    PgCredentials::new(&request.target.operator, password.as_str()).map_err(|_| Fail::input())?;
    let timeout = Duration::from_millis(connect_ms);
    let mut conn = connect_target(&endpoint, &request.target.operator, &password, &ca, timeout).await?;
    expect_principal(&mut conn, &request.target.operator, timeout).await?;
    let receipt = analyze_mapped_shadows(&mut conn, &statistics)
        .await
        .map_err(map_maintenance)?;
    if receipt.published || receipt.estimate_is_exact || receipt.protocol != PROTOCOL {
        return Err(Fail::new(1, "target statistics"));
    }
    Ok(render(&receipt))
}

fn closed(request: &Request) -> Result<(), Fail> {
    if request.project != request.source {
        return Err(Fail::new(3, "identity"));
    }
    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&request.statement_timeout_ms)
        || !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&request.lock_timeout_ms)
    {
        return Err(Fail::new(3, "bound"));
    }
    if request.tables.is_empty() || request.tables.len() > MAX_TABLES {
        return Err(Fail::new(3, "bound"));
    }
    let mut seen = BTreeSet::new();
    for table in &request.tables {
        if !seen.insert((table.namespace.clone(), table.name.clone())) {
            return Err(Fail::new(3, "bound"));
        }
        plan_physical_mapping(&MappingRequest {
            source: request.source.clone(),
            config_digest: request.config_digest.clone(),
            run_generation: request.source_generation,
            level: Level::Table,
            logical_namespace: table.namespace.clone(),
            logical_table: table.name.clone(),
            logical_column: String::new(),
        })
        .map_err(map_plan)?;
    }
    Ok(())
}

fn statistics_request(request: &Request) -> StatisticsRequest {
    StatisticsRequest {
        project: request.project.clone(),
        source: request.source.clone(),
        config_digest: request.config_digest.clone(),
        source_generation: request.source_generation,
        tables: request
            .tables
            .iter()
            .map(|table| LogicalTableRef {
                namespace: table.namespace.clone(),
                name: table.name.clone(),
            })
            .collect(),
        statement_timeout_ms: request.statement_timeout_ms,
        lock_timeout_ms: request.lock_timeout_ms,
    }
}

fn map_plan(err: PlanError) -> Fail {
    match err {
        PlanError::ClosedLogicalName | PlanError::NotPhysicalName => Fail::new(3, "bound"),
        PlanError::ClosedSource | PlanError::ClosedConfig | PlanError::ClosedGeneration => {
            Fail::new(3, "identity")
        }
    }
}

fn map_maintenance(err: MaintenanceError) -> Fail {
    match err.kind {
        StatisticsError::Identity => Fail::new(3, "identity"),
        StatisticsError::Bound
        | StatisticsError::TimeoutBound
        | StatisticsError::EmptyTables
        | StatisticsError::DuplicateTable => Fail::new(3, "bound"),
        StatisticsError::OpenTransaction => Fail::new(6, "open transaction"),
        StatisticsError::RuntimeCaller => Fail::new(5, "runtime caller"),
        StatisticsError::ReadOnlyCaller => Fail::new(5, "read-only caller"),
        StatisticsError::RoleDenied => Fail::new(5, "target role"),
        StatisticsError::StaleGeneration => Fail::new(6, "stale generation"),
        StatisticsError::MissingTable => Fail::new(6, "mapping absent"),
        StatisticsError::MissingRelation => Fail::new(6, "relation absent"),
        StatisticsError::ForgedRelation => Fail::new(6, "relation forged"),
        StatisticsError::LockTimeout => Fail::new(7, "lock timeout"),
        StatisticsError::StatementTimeout => Fail::new(7, "statement timeout"),
        StatisticsError::EstimateUnknown => Fail::new(7, "estimate unknown"),
        StatisticsError::EstimateUnusable => Fail::new(7, "estimate unusable"),
        StatisticsError::TransactionIncomplete => Fail::new(1, "target statistics"),
        StatisticsError::Database { .. } => Fail::new(1, "target database"),
    }
}

fn render(receipt: &oo_pipeline::target_statistics::StatisticsReceipt) -> String {
    let tables: Vec<Value> = receipt
        .tables
        .iter()
        .map(|table| {
            serde_json::json!({
                "estimated_rows": table.estimated_rows,
                "name": table.logical_table,
                "namespace": table.logical_namespace,
                "physical_name": table.physical_name,
                "reltuples_bits": table.reltuples_bits,
            })
        })
        .collect();
    let disposition = match receipt.disposition {
        ConnectionDisposition::Reusable => "reusable",
        ConnectionDisposition::FreshConnection => "fresh_connection",
    };
    let value = serde_json::json!({
        "config_digest": receipt.config_digest,
        "disposition": disposition,
        "estimate_is_exact": receipt.estimate_is_exact,
        "lock_timeout_ms": receipt.lock_timeout_ms,
        "project": receipt.project,
        "protocol": RESULT_PROTOCOL,
        "published": receipt.published,
        "source": receipt.source,
        "source_generation": receipt.source_generation,
        "statement_timeout_ms": receipt.statement_timeout_ms,
        "tables": tables,
    });
    canonical_json(&value)
}

async fn connect_target(
    endpoint: &PgEndpoint,
    operator: &str,
    password: &str,
    ca: &[u8],
    timeout: Duration,
) -> Result<sqlx::postgres::PgConnection, Fail> {
    let options = PgConnectOptions::from_explicit()
        .host(endpoint.host())
        .port(endpoint.port())
        .username(operator)
        .password(password)
        .database(endpoint.database())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        return Err(Fail::new(4, "target connect"));
    }
    let options = options.disable_statement_logging();
    match tokio::time::timeout(timeout, options.connect()).await {
        Ok(Ok(conn)) => Ok(conn),
        _ => Err(Fail::new(4, "target connect")),
    }
}

async fn expect_principal(
    conn: &mut sqlx::postgres::PgConnection,
    operator: &str,
    timeout: Duration,
) -> Result<(), Fail> {
    let query = sqlx::query("SELECT CURRENT_USER::text, SESSION_USER::text");
    let row = match tokio::time::timeout(timeout, query.fetch_one(&mut *conn)).await {
        Ok(Ok(row)) => row,
        _ => return Err(Fail::new(4, "target principal")),
    };
    let current: String = row.try_get(0).map_err(|_| Fail::new(4, "target principal"))?;
    let session: String = row.try_get(1).map_err(|_| Fail::new(4, "target principal"))?;
    if current == operator && session == operator {
        Ok(())
    } else {
        Err(Fail::new(4, "target principal"))
    }
}

fn write_stdout(document: &str) -> Result<(), Fail> {
    let mut payload = Vec::with_capacity(document.len() + 1);
    payload.extend_from_slice(document.as_bytes());
    payload.push(b'\n');
    io::stdout()
        .write_all(&payload)
        .map_err(|_| Fail::new(1, "analyze output"))
}

fn read_stdin() -> Result<String, Fail> {
    let mut stdin = io::stdin();
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    let limit = oo_core::limits::DOCUMENT_MAX_BYTES;
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| Fail::new(1, "analyze input"))?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > limit {
            return Err(Fail::input());
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(buf).map_err(|_| Fail::input())
}

fn read_password(path: &str) -> Result<String, Fail> {
    let bytes = read_bounded(path, 1026).map_err(|_| Fail::new(3, "target password file"))?;
    let text = String::from_utf8(bytes).map_err(|_| Fail::new(3, "target password file"))?;
    let text = if let Some(stripped) = text.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = text.strip_suffix('\n') {
        stripped.to_string()
    } else {
        text
    };
    if text.is_empty() || text.len() > 1024 || text.contains('\0') {
        return Err(Fail::new(3, "target password file"));
    }
    Ok(text)
}

fn read_ca(path: &str) -> Result<Vec<u8>, Fail> {
    let bytes = read_bounded(path, 1024 * 1024).map_err(|_| Fail::new(3, "target ca file"))?;
    PgTlsConfig::from_ca_pem(&bytes).map_err(|_| Fail::new(3, "target ca file"))?;
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
            ch if u32::from(ch) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[derive(Clone, Copy)]
struct Fail {
    code: u8,
    fact: &'static str,
}

impl Fail {
    const fn new(code: u8, fact: &'static str) -> Self {
        Self { code, fact }
    }

    const fn input() -> Self {
        Self::new(3, "invalid input")
    }

    fn emit(self) {
        eprintln!("analyze-pg: {}", self.fact);
    }
}

fn strict_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(serde::de::Error::custom("null"));
    }
    let Some(number) = value.as_u64() else {
        return Err(serde::de::Error::custom("integer"));
    };
    Ok(Some(number))
}

fn strict_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    value
        .as_i64()
        .ok_or_else(|| serde::de::Error::custom("integer"))
}

fn strict_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let Some(number) = value.as_u64() else {
        return Err(serde::de::Error::custom("integer"));
    };
    u32::try_from(number).map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    project: String,
    source: String,
    config_digest: String,
    #[serde(deserialize_with = "strict_i64")]
    source_generation: i64,
    tables: Vec<TableIn>,
    #[serde(deserialize_with = "strict_u32")]
    statement_timeout_ms: u32,
    #[serde(deserialize_with = "strict_u32")]
    lock_timeout_ms: u32,
    target: TargetIn,
    #[serde(default, deserialize_with = "strict_u64")]
    connect_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableIn {
    namespace: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetIn {
    host: String,
    port: u16,
    database: String,
    operator: String,
    password_file: String,
    ca_file: String,
}
