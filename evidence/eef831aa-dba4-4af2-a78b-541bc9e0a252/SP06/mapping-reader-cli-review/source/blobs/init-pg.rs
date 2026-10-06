//! `init-pg` installs one target database for a later `import-pg`.
//!
//! Stdin is one bounded `oo.pipeline.init-pg/v1` document. The password and
//! the CA stay in files named by that document. This command calls
//! [`oo_pipeline::target_installer::install`] and
//! [`oo_pipeline::target_installer::install_full_import`], then
//! [`oo_pipeline::target_installer::verify_readiness`] and
//! [`oo_pipeline::target_installer::verify_full_import_readiness`]. Stdout is
//! those outcomes and the compiled seals. It is not a publication, not an
//! import, and not a runtime grant.

#![forbid(unsafe_code)]

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

use oo_pipeline::target_installer::{
    FullImportInstallOutcome, InstallError, InstallOutcome, ReadinessError, compiled_digests,
    compiled_full_import_digest, install, install_full_import, verify_full_import_readiness,
    verify_readiness,
};

const REQUEST_PROTOCOL: &str = "oo.pipeline.init-pg/v1";
const RESULT_PROTOCOL: &str = "oo.pipeline.init-pg-result/v1";
const DEFAULT_CONNECT_MS: u64 = 30_000;
const MAX_CONNECT_MS: u64 = 120_000;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _argv0 = args.next();
    if args.next().is_some() {
        eprintln!("init-pg: usage");
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
    // The placeholder is not sent. `PgCredentials` is the public username check.
    PgCredentials::new(&request.target.administrator, "x").map_err(|_| Fail::input())?;
    let password = read_password(&request.target.password_file)?;
    let ca = read_ca(&request.target.ca_file)?;
    PgCredentials::new(&request.target.administrator, password.as_str()).map_err(|_| Fail::input())?;
    let timeout = Duration::from_millis(connect_ms);
    let mut conn = connect_target(&endpoint, &request.target.administrator, &password, &ca, timeout).await?;
    expect_principal(&mut conn, &request.target.administrator, timeout).await?;
    expect_administrator(&mut conn, timeout).await?;
    let install_outcome = install(&mut conn).await.map_err(map_install)?;
    let ready = verify_readiness(&mut conn).await.map_err(map_ready)?;
    let compiled = compiled_digests();
    if ready.digests().bootstrap() != compiled.bootstrap()
        || ready.digests().fence() != compiled.fence()
        || ready.digests().preflight() != compiled.preflight()
        || ready.digests().mapping() != compiled.mapping()
    {
        return Err(Fail::not_ready());
    }
    let full_outcome = install_full_import(&mut conn).await.map_err(map_install)?;
    let full = verify_full_import_readiness(&mut conn).await.map_err(map_ready)?;
    let full_digest = compiled_full_import_digest();
    if full.digest() != full_digest {
        return Err(Fail::not_ready());
    }
    Ok(render(
        install_outcome,
        ready.retains_historical_v2(),
        ready.digests(),
        full_outcome,
        &full_digest,
    ))
}

fn render(
    install_outcome: InstallOutcome,
    historical_v2: bool,
    digests: &oo_pipeline::target_installer::CompiledDigests,
    full_outcome: FullImportInstallOutcome,
    full_digest: &str,
) -> String {
    let value = serde_json::json!({
        "full_import": full_outcome.to_string(),
        "full_import_digest": full_digest,
        "historical_v2": historical_v2,
        "install": install_outcome.to_string(),
        "protocol": RESULT_PROTOCOL,
        "seals": {
            "bootstrap": digests.bootstrap(),
            "fence": digests.fence(),
            "mapping": digests.mapping(),
            "preflight": digests.preflight(),
        }
    });
    canonical_json(&value)
}

fn map_install(err: InstallError) -> Fail {
    match err {
        InstallError::NotAdministrator => Fail::new(5, "target administrator"),
        InstallError::ChecksumMismatch { .. } => Fail::new(7, "migration checksum"),
        InstallError::MissingRelation { .. } | InstallError::Refused { .. } => {
            Fail::new(6, "target catalog refused")
        }
        InstallError::Database { sqlstate, .. } if sqlstate == "OO001" => Fail::new(7, "migration checksum"),
        InstallError::Database { sqlstate, .. } if sqlstate == "OO002" => Fail::new(6, "target catalog refused"),
        InstallError::Database { .. } => Fail::new(1, "target database"),
    }
}

fn map_ready(err: ReadinessError) -> Fail {
    match err {
        ReadinessError::NotInstalled | ReadinessError::Incomplete => Fail::not_ready(),
        ReadinessError::ChecksumMismatch { .. } => Fail::new(7, "migration checksum"),
        ReadinessError::Database { sqlstate, .. } if sqlstate == "OO001" => Fail::new(7, "migration checksum"),
        ReadinessError::Database { .. } => Fail::new(1, "target database"),
    }
}

async fn connect_target(
    endpoint: &PgEndpoint,
    administrator: &str,
    password: &str,
    ca: &[u8],
    timeout: Duration,
) -> Result<sqlx::postgres::PgConnection, Fail> {
    let options = PgConnectOptions::from_explicit()
        .host(endpoint.host())
        .port(endpoint.port())
        .username(administrator)
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

async fn expect_administrator(
    conn: &mut sqlx::postgres::PgConnection,
    timeout: Duration,
) -> Result<(), Fail> {
    // `install` probes `_oo` before it can return `NotAdministrator`. A login
    // without `USAGE` on that schema gets a permission error instead. Decide
    // the same administrator rule from `pg_roles` first.
    let query = sqlx::query(
        "SELECT rolsuper, \
                EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') \
         FROM pg_catalog.pg_roles \
         WHERE rolname = current_user",
    );
    let row = match tokio::time::timeout(timeout, query.fetch_one(&mut *conn)).await {
        Ok(Ok(row)) => row,
        _ => return Err(Fail::new(1, "target database")),
    };
    let superuser: bool = row.try_get(0).map_err(|_| Fail::new(1, "target database"))?;
    let owner_exists: bool = row.try_get(1).map_err(|_| Fail::new(1, "target database"))?;
    if superuser {
        return Ok(());
    }
    if !owner_exists {
        return Err(Fail::new(5, "target administrator"));
    }
    let member_query = sqlx::query(
        "SELECT pg_catalog.pg_has_role(current_user, 'oo_target_owner', 'MEMBER')",
    );
    let member_row = match tokio::time::timeout(timeout, member_query.fetch_one(&mut *conn)).await {
        Ok(Ok(row)) => row,
        _ => return Err(Fail::new(1, "target database")),
    };
    let member: bool = member_row.try_get(0).map_err(|_| Fail::new(1, "target database"))?;
    if member { Ok(()) } else { Err(Fail::new(5, "target administrator")) }
}

async fn expect_principal(
    conn: &mut sqlx::postgres::PgConnection,
    administrator: &str,
    timeout: Duration,
) -> Result<(), Fail> {
    let query = sqlx::query("SELECT CURRENT_USER::text, SESSION_USER::text");
    let row = match tokio::time::timeout(timeout, query.fetch_one(&mut *conn)).await {
        Ok(Ok(row)) => row,
        _ => return Err(Fail::new(4, "target principal")),
    };
    let current: String = row.try_get(0).map_err(|_| Fail::new(4, "target principal"))?;
    let session: String = row.try_get(1).map_err(|_| Fail::new(4, "target principal"))?;
    if current == administrator && session == administrator {
        Ok(())
    } else {
        Err(Fail::new(4, "target principal"))
    }
}

fn write_stdout(document: &str) -> Result<(), Fail> {
    let mut payload = Vec::with_capacity(document.len() + 1);
    payload.extend_from_slice(document.as_bytes());
    payload.push(b'\n');
    io::stdout().write_all(&payload).map_err(|_| Fail::new(1, "init output"))
}

fn read_stdin() -> Result<String, Fail> {
    let mut stdin = io::stdin();
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    let limit = oo_core::limits::DOCUMENT_MAX_BYTES;
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| Fail::new(1, "init input"))?;
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

    const fn not_ready() -> Self {
        Self::new(1, "target schema is not ready")
    }

    fn emit(self) {
        eprintln!("init-pg: {}", self.fact);
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
    serde_json::from_value(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    target: TargetIn,
    #[serde(default, deserialize_with = "strict_u64")]
    connect_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetIn {
    host: String,
    port: u16,
    database: String,
    administrator: String,
    password_file: String,
    ca_file: String,
}

