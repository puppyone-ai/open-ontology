//! Local trusted `SELECT` command.
//!
//! The request file is an operator document on this machine. It is not an HTTP
//! body, a token, or an authentication claim. The password is read from the
//! file named by the request. It is not taken from the arguments or the
//! environment, and it is not written to stdout, stderr, or the audit file.
//!
//! One TLS session loads `load_policy_catalog` and then runs
//! `DedicatedSession::execute_trusted_select` as the restricted `NOLOGIN`
//! role. This command does not install grants and does not open a source
//! database. The audit file is a local JSONL receipt (`flush` and `sync_all`
//! on admission and again on completion). It is not a production audit WAL.
//! Untrusted parser admission stays blocked. Row filters stay refused.

#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use oo_core::parse_json_value;
use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::query_executor::{
    Admission, AuditAdmission, Cell, CompletionRecord, CompletionStatus, DedicatedSession,
    ExecError, QuerySummary, SelectRequest, StopClass, MAX_QUERY_BYTES, MAX_QUERY_ROWS,
    MAX_QUERY_WALL,
};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, CompiledPolicy, LogicalTable, PiiClass,
    PolicyIdentity, RowFilter, TlsConnection, TlsEndpoint, compile_column_acl, connect_require_tls,
    rustls_connector, SQL_BYTE_BUDGET,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

const REQUEST_VERSION: &str = "oo.gateway.query-pg/v1";
const TURN_VERSION: &str = "oo.gateway.query-pg-turn/v1";
const AUDIT_VERSION: &str = "oo.gateway.query-pg-audit/v1";
const REQUEST_BYTES: usize = 1_048_576;
const STDIN_BYTES: usize = 256 * 1024;
const MAX_TURNS: usize = 8;
const PASSWORD_FILE_BYTES: usize = 1026;
const PASSWORD_BYTES: usize = 1024;
const CA_BYTES: usize = 1_048_576;
const MAX_POLICIES: usize = 32;
const MAX_COLUMNS: usize = 512;
const MIN_OPERATORS: u32 = 54;
const MAX_OPERATORS: u32 = 128;
const MAX_FUNCTIONS: u32 = 8;
const MAX_TABLES: u32 = 32;

const EXIT_IO: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_REQUEST: u8 = 3;
const EXIT_TLS: u8 = 4;
const EXIT_POLICY: u8 = 5;
const EXIT_GUARD: u8 = 6;
const EXIT_AUDIT: u8 = 7;
const EXIT_QUERY: u8 = 8;

fn main() -> ExitCode {
    match std::panic::catch_unwind(run) {
        Ok(code) => code,
        Err(_) => {
            eprintln!("query-pg: query");
            ExitCode::from(EXIT_QUERY)
        }
    }
}

fn run() -> ExitCode {
    let mode = match parse_args(env::args_os()) {
        Ok(mode) => mode,
        Err(fail) => return fail.exit(),
    };
    if mode.help {
        let mut out = io::stdout();
        if out.write_all(HELP.as_bytes()).is_err() || out.flush().is_err() {
            return Fail::Io.exit();
        }
        return ExitCode::SUCCESS;
    }
    let loaded = match load_request(&mode.request) {
        Ok(loaded) => loaded,
        Err(fail) => return fail.exit(),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return Fail::Io.exit(),
    };
    match runtime.block_on(execute(loaded)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fail) => fail.exit(),
    }
}

struct Mode {
    help: bool,
    request: PathBuf,
}

fn parse_args<I>(args: I) -> Result<Mode, Fail>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _argv0 = args.next();
    let Some(flag) = args.next() else {
        return Err(Fail::Usage);
    };
    if flag == "--help" {
        if args.next().is_some() {
            return Err(Fail::Usage);
        }
        return Ok(Mode {
            help: true,
            request: PathBuf::new(),
        });
    }
    if flag != "--request" {
        return Err(Fail::Usage);
    }
    let Some(path) = args.next() else {
        return Err(Fail::Usage);
    };
    if args.next().is_some() {
        return Err(Fail::Usage);
    }
    let path = path.into_string().map_err(|_| Fail::Usage)?;
    if path.is_empty() || path.contains('\0') {
        return Err(Fail::Usage);
    }
    Ok(Mode {
        help: false,
        request: PathBuf::from(path),
    })
}

struct Loaded {
    endpoint: TlsEndpoint,
    password: String,
    ca: Vec<u8>,
    audit_path: PathBuf,
    identity: PolicyIdentity,
    policies: Vec<CompiledPolicy>,
    limits: CatalogLimits,
    statements: Vec<String>,
    max_rows: u32,
    max_result_bytes: u64,
    wall: Duration,
}

fn load_request(path: &Path) -> Result<Loaded, Fail> {
    let bytes = read_regular(path, REQUEST_BYTES)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Fail::Request)?;
    let value = parse_json_value(text, REQUEST_VERSION).map_err(|_| Fail::Request)?;
    reject_null(&value)?;
    let doc: RequestDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
    if doc.version != REQUEST_VERSION {
        return Err(Fail::Request);
    }
    let endpoint = endpoint_from(&doc.endpoint)?;
    let audit_path = PathBuf::from(&doc.audit_file);
    audit_available(&audit_path)?;
    let password = read_password(Path::new(&doc.password_file))?;
    if !password.is_empty() && text.contains(&password) {
        return Err(Fail::Request);
    }
    let ca = read_ca(Path::new(&doc.ca_file))?;
    let policies = compile_policies(&doc)?;
    let identity = PolicyIdentity {
        project: doc.identity.project.clone(),
        role: doc.identity.role.clone(),
        policy_id: doc.identity.policy_id.clone(),
    };
    // CompiledPolicy fields stay private. The public compiler names the
    // database role `oo_r_` plus the slug, and the loader checks that name.
    ensure_identity(&doc, &identity)?;
    let limits = limits_from(&doc.catalog)?;
    let (statements, max_rows, max_result_bytes, wall) = statements_from(&doc.statement)?;
    Ok(Loaded {
        endpoint,
        password,
        ca,
        audit_path,
        identity,
        policies,
        limits,
        statements,
        max_rows,
        max_result_bytes,
        wall,
    })
}

fn ensure_identity(doc: &RequestDoc, identity: &PolicyIdentity) -> Result<(), Fail> {
    let Some(first) = doc.policies.first() else {
        return Err(Fail::Request);
    };
    let role = format!("oo_r_{}", first.role);
    if identity.project != first.project
        || identity.policy_id != first.config_digest
        || identity.role != role
    {
        return Err(Fail::Policy);
    }
    for policy in &doc.policies {
        if policy.project != first.project
            || policy.config_digest != first.config_digest
            || policy.generation != first.generation
            || policy.login != first.login
            || policy.role != first.role
        {
            return Err(Fail::Policy);
        }
    }
    Ok(())
}

async fn execute(loaded: Loaded) -> Result<(), Fail> {
    let tls = rustls_connector(&loaded.ca).map_err(|_| Fail::Tls)?;
    let (mut client, mut connection) =
        connect_require_tls(&loaded.endpoint, &loaded.password, &loaded.ca)
            .await
            .map_err(|_| Fail::Tls)?;
    // Poll the same connection the catalog load needs, then move it into bind.
    // `TlsConnection` has to be `Unpin` for that move to be sound.
    assert_connection_unpin(&connection);
    let catalog = {
        let mut load = std::pin::pin!(load_policy_catalog(
            &mut client,
            &loaded.identity,
            &loaded.policies,
            &loaded.limits,
        ));
        tokio::select! {
            biased;
            result = &mut load => result.map_err(|_| Fail::Policy)?,
            _ = &mut connection => return Err(Fail::Tls),
        }
    };
    let mut session = DedicatedSession::bind(client, connection, tls)
        .await
        .map_err(exec_fail)?;
    let hub = Arc::new(AuditHub {
        path: loaded.audit_path,
        opened: AtomicBool::new(false),
        notes: Mutex::new(Vec::new()),
    });
    for (turn, sql) in loaded.statements.iter().enumerate() {
        let turn_u = u32::try_from(turn).map_err(|_| Fail::Request)?;
        let hook = FileAudit {
            hub: Arc::clone(&hub),
            file: None,
        };
        let request = SelectRequest {
            identity: &loaded.identity,
            catalog: &catalog,
            original_sql: sql,
            role: &loaded.identity.role,
            max_rows: loaded.max_rows,
            max_result_bytes: loaded.max_result_bytes,
            wall_clock: loaded.wall,
        };
        let mut stream = match session.execute_trusted_select(request, hook).await {
            Ok(stream) => stream,
            Err(err) => return Err(exec_fail(err)),
        };
        let mut failed = false;
        loop {
            match stream.next_row().await {
                Ok(Some(cells)) => write_row(turn_u, &cells)?,
                Ok(None) => break,
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        drop(stream);
        session.wait_idle().await.map_err(|_| Fail::Query)?;
        let summary = session.summary().ok_or(Fail::Query)?;
        let note = {
            let notes = lock_notes(&hub.notes);
            notes.get(turn).cloned().ok_or(Fail::Query)?
        };
        write_summary(turn_u, &summary, &note)?;
        if failed || summary.status() != CompletionStatus::Succeeded {
            return Err(Fail::Query);
        }
        if !summary.end_audit_ok() {
            return Err(Fail::Audit);
        }
    }
    Ok(())
}

fn assert_connection_unpin(connection: &TlsConnection) {
    fn assert_unpin<T: Unpin>(_: &T) {}
    assert_unpin(connection);
}

fn exec_fail(err: ExecError) -> Fail {
    match err {
        ExecError::Guard(_) => Fail::Guard,
        ExecError::AuditUnavailable => Fail::Audit,
        _ => Fail::Query,
    }
}

fn write_row(turn: u32, cells: &[Cell]) -> Result<(), Fail> {
    let line = RowLine {
        record: "row",
        turn,
        cells: cells.iter().map(out_cell).collect(),
    };
    write_json(&line)
}

fn out_cell(cell: &Cell) -> OutCell {
    match cell {
        Cell::Null => OutCell::Null,
        Cell::Bool(value) => OutCell::Bool { value: *value },
        Cell::Int(value) => OutCell::Int {
            value: value.clone(),
        },
        Cell::Text(value) => OutCell::Text {
            value: value.clone(),
        },
        Cell::Decimal(value) => OutCell::Decimal {
            negative: value.is_negative(),
            coefficient: value.coefficient().to_string(),
            scale: value.scale(),
        },
    }
}

fn write_summary(turn: u32, summary: &QuerySummary, note: &AuditNote) -> Result<(), Fail> {
    let line = SummaryLine {
        record: "summary",
        turn,
        status: summary.status().as_str(),
        truncated: summary.truncated(),
        returned_rows: summary.returned_rows(),
        result_bytes: summary.result_bytes(),
        rollback_confirmed: summary.rollback_confirmed(),
        socket_dropped: summary.socket_dropped(),
        end_audit_ok: summary.end_audit_ok(),
        original_sha256: note.original_sha256.clone(),
        executed_sha256: note.executed_sha256.clone(),
        stop: summary.stop().map(stop_name),
        sqlstate: clean_sqlstate(summary.sqlstate()),
    };
    write_json(&line)
}

fn write_json(value: &impl Serialize) -> Result<(), Fail> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Fail::Io)?;
    bytes.push(b'\n');
    let mut out = io::stdout().lock();
    out.write_all(&bytes).map_err(|_| Fail::Io)?;
    out.flush().map_err(|_| Fail::Io)?;
    Ok(())
}

fn stop_name(stop: StopClass) -> &'static str {
    match stop {
        StopClass::Timeout => "timeout",
        StopClass::Canceled => "canceled",
        StopClass::TypeRefused => "type_refused",
        StopClass::Database => "database",
        StopClass::Protocol => "protocol",
    }
}

fn clean_sqlstate(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.len() == 5
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Some(value.to_string())
    } else {
        None
    }
}

struct AuditHub {
    path: PathBuf,
    opened: AtomicBool,
    notes: Mutex<Vec<AuditNote>>,
}

#[derive(Clone)]
struct AuditNote {
    original_sha256: String,
    executed_sha256: String,
    fingerprint_hex: String,
    backend_pid: i32,
    backend_start: String,
}

struct FileAudit {
    hub: Arc<AuditHub>,
    file: Option<File>,
}

impl AuditAdmission for FileAudit {
    async fn admit(&mut self, admission: &Admission<'_>) -> Result<(), ExecError> {
        let note = note_from_admission(admission)?;
        if !closed_audit_text(admission.role())
            || !closed_audit_text(admission.project())
            || !closed_audit_text(admission.policy_id())
        {
            return Err(ExecError::AuditUnavailable);
        }
        let first = !self.hub.opened.load(Ordering::SeqCst);
        let mut options = OpenOptions::new();
        options.write(true).append(true);
        if first {
            options.create_new(true);
        }
        let mut file = options
            .open(&self.hub.path)
            .map_err(|_| ExecError::AuditUnavailable)?;
        let line = AdmitLine {
            protocol: AUDIT_VERSION,
            record: "admit",
            original_sha256: note.original_sha256.clone(),
            executed_sha256: note.executed_sha256.clone(),
            fingerprint_hex: note.fingerprint_hex.clone(),
            role: admission.role().to_string(),
            project: admission.project().to_string(),
            policy_id: admission.policy_id().to_string(),
            backend_pid: note.backend_pid,
            backend_start: note.backend_start.clone(),
        };
        write_synced(&mut file, &line)?;
        if first {
            self.hub.opened.store(true, Ordering::SeqCst);
        }
        self.file = Some(file);
        lock_notes(&self.hub.notes).push(note);
        Ok(())
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        let note = {
            let notes = lock_notes(&self.hub.notes);
            let note = notes.last().ok_or(ExecError::AuditUnavailable)?.clone();
            if note.original_sha256 != completion.original_sha256()
                || note.executed_sha256 != completion.executed_sha256()
                || note.fingerprint_hex != completion.fingerprint_hex()
                || note.backend_pid != completion.backend_pid()
                || note.backend_start != completion.backend_start()
            {
                return Err(ExecError::AuditUnavailable);
            }
            note
        };
        let line = CompleteLine {
            protocol: AUDIT_VERSION,
            record: "complete",
            status: completion.status().as_str(),
            truncated: completion.truncated(),
            returned_rows: completion.returned_rows(),
            result_bytes: completion.result_bytes(),
            rollback_confirmed: completion.rollback_confirmed(),
            socket_dropped: completion.socket_dropped(),
            stop: completion.stop().map(stop_name),
            sqlstate: clean_sqlstate(completion.sqlstate()),
            original_sha256: note.original_sha256,
            executed_sha256: note.executed_sha256,
            fingerprint_hex: note.fingerprint_hex,
            role: completion.role().to_string(),
            project: completion.project().to_string(),
            policy_id: completion.policy_id().to_string(),
            backend_pid: note.backend_pid,
            backend_start: note.backend_start,
        };
        let file = self.file.as_mut().ok_or(ExecError::AuditUnavailable)?;
        write_synced(file, &line)
    }
}

fn note_from_admission(admission: &Admission<'_>) -> Result<AuditNote, ExecError> {
    if !sha256_ok(admission.original_sha256())
        || !sha256_ok(admission.executed_sha256())
        || !fingerprint_ok(admission.fingerprint_hex())
        || !backend_start_ok(admission.backend_start())
        || admission.backend_pid() <= 0
    {
        return Err(ExecError::AuditUnavailable);
    }
    Ok(AuditNote {
        original_sha256: admission.original_sha256().to_string(),
        executed_sha256: admission.executed_sha256().to_string(),
        fingerprint_hex: admission.fingerprint_hex().to_string(),
        backend_pid: admission.backend_pid(),
        backend_start: admission.backend_start().to_string(),
    })
}

fn write_synced(file: &mut File, value: &impl Serialize) -> Result<(), ExecError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| ExecError::AuditUnavailable)?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .map_err(|_| ExecError::AuditUnavailable)?;
    file.flush().map_err(|_| ExecError::AuditUnavailable)?;
    file.sync_all().map_err(|_| ExecError::AuditUnavailable)?;
    Ok(())
}

fn lock_notes(mutex: &Mutex<Vec<AuditNote>>) -> MutexGuard<'_, Vec<AuditNote>> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn sha256_ok(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn fingerprint_ok(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn backend_start_ok(value: &str) -> bool {
    (1..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn closed_audit_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|char| char.is_control() || char == '\0')
}

fn compile_policies(doc: &RequestDoc) -> Result<Vec<CompiledPolicy>, Fail> {
    if doc.policies.is_empty() || doc.policies.len() > MAX_POLICIES {
        return Err(Fail::Request);
    }
    let mut compiled = Vec::with_capacity(doc.policies.len());
    for policy in &doc.policies {
        if policy.columns_len() > MAX_COLUMNS {
            return Err(Fail::Request);
        }
        let input = policy.to_acl()?;
        compiled.push(compile_column_acl(&input).map_err(|_| Fail::Policy)?);
    }
    Ok(compiled)
}

fn endpoint_from(doc: &EndpointDoc) -> Result<TlsEndpoint, Fail> {
    if !host_ok(&doc.host) || doc.port == 0 || !plain_ident(&doc.database) || !plain_ident(&doc.user)
    {
        return Err(Fail::Request);
    }
    Ok(TlsEndpoint {
        host: doc.host.clone(),
        port: doc.port,
        database: doc.database.clone(),
        user: doc.user.clone(),
    })
}

fn limits_from(doc: &CatalogDoc) -> Result<CatalogLimits, Fail> {
    if doc.max_tables == 0
        || doc.max_tables > MAX_TABLES
        || doc.max_columns == 0
        || doc.max_columns > MAX_COLUMNS as u32
        || doc.max_functions == 0
        || doc.max_functions > MAX_FUNCTIONS
        || doc.max_operators < MIN_OPERATORS
        || doc.max_operators > MAX_OPERATORS
    {
        return Err(Fail::Request);
    }
    let unknown_rows = match doc.unknown_rows.as_str() {
        "refuse" => UnknownRows::Refuse,
        "zero" => UnknownRows::Zero,
        "saturate" => UnknownRows::Saturate,
        _ => return Err(Fail::Request),
    };
    Ok(CatalogLimits {
        max_tables: doc.max_tables,
        max_columns: doc.max_columns,
        max_functions: doc.max_functions,
        max_operators: doc.max_operators,
        large_table_row_threshold: doc.large_table_row_threshold,
        unknown_rows,
    })
}

fn statements_from(doc: &StatementDoc) -> Result<(Vec<String>, u32, u64, Duration), Fail> {
    if doc.max_rows > MAX_QUERY_ROWS || doc.max_result_bytes > MAX_QUERY_BYTES {
        return Err(Fail::Request);
    }
    let millis = u128::from(doc.wall_clock_ms);
    if millis == 0 || millis > MAX_QUERY_WALL.as_millis() {
        return Err(Fail::Request);
    }
    let wall = Duration::from_millis(doc.wall_clock_ms);
    let statements = match doc.source.as_str() {
        "request" => {
            let sql = doc.sql.as_deref().ok_or(Fail::Request)?;
            validate_sql(sql)?;
            vec![sql.to_string()]
        }
        "stdin" => {
            if doc.sql.is_some() {
                return Err(Fail::Request);
            }
            read_turns()?
        }
        _ => return Err(Fail::Request),
    };
    Ok((statements, doc.max_rows, doc.max_result_bytes, wall))
}

fn validate_sql(sql: &str) -> Result<(), Fail> {
    if sql.is_empty() || sql.len() > SQL_BYTE_BUDGET || sql.contains('\0') {
        Err(Fail::Request)
    } else {
        Ok(())
    }
}

fn read_turns() -> Result<Vec<String>, Fail> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(u64::try_from(STDIN_BYTES).map_err(|_| Fail::Request)? + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Fail::Request)?;
    if bytes.is_empty() || bytes.len() > STDIN_BYTES {
        return Err(Fail::Request);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Fail::Request)?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() || lines.len() > MAX_TURNS || lines.iter().any(|line| line.is_empty()) {
        return Err(Fail::Request);
    }
    let mut statements = Vec::with_capacity(lines.len());
    for line in lines {
        let value = parse_json_value(line, TURN_VERSION).map_err(|_| Fail::Request)?;
        reject_null(&value)?;
        let turn: TurnDoc = serde_json::from_value(value).map_err(|_| Fail::Request)?;
        if turn.version != TURN_VERSION {
            return Err(Fail::Request);
        }
        validate_sql(&turn.sql)?;
        statements.push(turn.sql);
    }
    Ok(statements)
}

fn host_ok(host: &str) -> bool {
    let bytes = host.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 253
        && !bytes.starts_with(b"-")
        && !bytes.ends_with(b"-")
        && !bytes.starts_with(b".")
        && !bytes.ends_with(b".")
        && !host.contains("..")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'-')
}

fn plain_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn audit_available(path: &Path) -> Result<(), Fail> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(Fail::Audit);
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let parent_meta = fs::metadata(parent).map_err(|_| Fail::Audit)?;
    if !parent_meta.is_dir() {
        return Err(Fail::Audit);
    }
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Fail::Audit),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Fail::Audit),
    }
}

fn read_password(path: &Path) -> Result<String, Fail> {
    let bytes = read_regular(path, PASSWORD_FILE_BYTES)?;
    let trimmed = if bytes.ends_with(b"\r\n") {
        &bytes[..bytes.len() - 2]
    } else if bytes.ends_with(b"\n") {
        &bytes[..bytes.len() - 1]
    } else {
        bytes.as_slice()
    };
    if trimmed.is_empty() || trimmed.len() > PASSWORD_BYTES || trimmed.contains(&0) {
        return Err(Fail::Request);
    }
    String::from_utf8(trimmed.to_vec()).map_err(|_| Fail::Request)
}

fn read_ca(path: &Path) -> Result<Vec<u8>, Fail> {
    let bytes = read_regular(path, CA_BYTES)?;
    if bytes.is_empty() || bytes.windows(11).any(|window| window == b"PRIVATE KEY") {
        return Err(Fail::Tls);
    }
    Ok(bytes)
}

fn read_regular(path: &Path, max: usize) -> Result<Vec<u8>, Fail> {
    if path.as_os_str().is_empty() {
        return Err(Fail::Request);
    }
    let meta = fs::symlink_metadata(path).map_err(|_| Fail::Request)?;
    if !meta.file_type().is_file() || meta.len() > u64::try_from(max).unwrap_or(u64::MAX) {
        return Err(Fail::Request);
    }
    let mut file = File::open(path).map_err(|_| Fail::Request)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(u64::try_from(max).unwrap_or(0) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Fail::Request)?;
    if bytes.len() > max {
        return Err(Fail::Request);
    }
    Ok(bytes)
}

fn reject_null(value: &Value) -> Result<(), Fail> {
    match value {
        Value::Null => Err(Fail::Request),
        Value::Array(items) => items.iter().try_for_each(reject_null),
        Value::Object(map) => map.values().try_for_each(reject_null),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

#[derive(Debug)]
enum Fail {
    Usage,
    Request,
    Tls,
    Policy,
    Guard,
    Audit,
    Query,
    Io,
}

impl Fail {
    fn exit(self) -> ExitCode {
        let (code, token) = match self {
            Self::Usage => (EXIT_USAGE, "usage"),
            Self::Request => (EXIT_REQUEST, "request"),
            Self::Tls => (EXIT_TLS, "tls"),
            Self::Policy => (EXIT_POLICY, "policy"),
            Self::Guard => (EXIT_GUARD, "guard"),
            Self::Audit => (EXIT_AUDIT, "audit"),
            Self::Query => (EXIT_QUERY, "query"),
            Self::Io => (EXIT_IO, "io"),
        };
        eprintln!("query-pg: {token}");
        ExitCode::from(code)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDoc {
    version: String,
    endpoint: EndpointDoc,
    password_file: String,
    ca_file: String,
    audit_file: String,
    identity: IdentityDoc,
    policies: Vec<PolicyDoc>,
    catalog: CatalogDoc,
    statement: StatementDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointDoc {
    host: String,
    port: u16,
    database: String,
    user: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityDoc {
    project: String,
    role: String,
    policy_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDoc {
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    table: TableDoc,
    #[serde(default)]
    row_filter: Option<RowFilterDoc>,
}

impl PolicyDoc {
    fn columns_len(&self) -> usize {
        self.table.columns.len()
    }

    fn to_acl(&self) -> Result<ColumnAclPolicy, Fail> {
        let row_filter = match &self.row_filter {
            None => None,
            Some(filter) => Some(filter.to_filter()?),
        };
        Ok(ColumnAclPolicy {
            project: self.project.clone(),
            config_digest: self.config_digest.clone(),
            generation: self.generation,
            role: self.role.clone(),
            login: self.login.clone(),
            table: LogicalTable {
                namespace: self.table.namespace.clone(),
                name: self.table.name.clone(),
                columns: self
                    .table
                    .columns
                    .iter()
                    .map(ColumnDoc::to_spec)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            row_filter,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowFilterDoc {
    kind: String,
    column: String,
    claim: String,
}

impl RowFilterDoc {
    fn to_filter(&self) -> Result<RowFilter, Fail> {
        if self.kind != "claims" {
            return Err(Fail::Request);
        }
        Ok(RowFilter::Claims {
            column: self.column.clone(),
            claim: self.claim.clone(),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDoc {
    namespace: String,
    name: String,
    columns: Vec<ColumnDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnDoc {
    name: String,
    status: String,
    semantic_type: String,
    pii: PiiDoc,
}

impl ColumnDoc {
    fn to_spec(&self) -> Result<ColumnSpec, Fail> {
        let status = match self.status.as_str() {
            "confirmed" => ClassStatus::Confirmed,
            "auto_accepted" => ClassStatus::AutoAccepted,
            "unclassified" => ClassStatus::Unclassified,
            _ => return Err(Fail::Request),
        };
        Ok(ColumnSpec {
            name: self.name.clone(),
            status,
            semantic_type: self.semantic_type.clone(),
            pii: self.pii.to_class()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PiiDoc {
    class: String,
    #[serde(default)]
    explicit_allow: Option<bool>,
}

impl PiiDoc {
    fn to_class(&self) -> Result<PiiClass, Fail> {
        match self.class.as_str() {
            "none" if self.explicit_allow.is_none() => Ok(PiiClass::None),
            "unknown" if self.explicit_allow.is_none() => Ok(PiiClass::Unknown),
            "declared" => Ok(PiiClass::Declared {
                explicit_allow: self.explicit_allow.ok_or(Fail::Request)?,
            }),
            _ => Err(Fail::Request),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDoc {
    max_tables: u32,
    max_columns: u32,
    max_functions: u32,
    max_operators: u32,
    large_table_row_threshold: u64,
    unknown_rows: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatementDoc {
    source: String,
    #[serde(default)]
    sql: Option<String>,
    max_rows: u32,
    max_result_bytes: u64,
    wall_clock_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnDoc {
    version: String,
    sql: String,
}

#[derive(Serialize)]
struct RowLine {
    record: &'static str,
    turn: u32,
    cells: Vec<OutCell>,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum OutCell {
    #[serde(rename = "null")]
    Null,
    #[serde(rename = "bool")]
    Bool { value: bool },
    #[serde(rename = "int")]
    Int { value: String },
    #[serde(rename = "text")]
    Text { value: String },
    #[serde(rename = "decimal")]
    Decimal {
        negative: bool,
        coefficient: String,
        scale: i32,
    },
}

#[derive(Serialize)]
struct SummaryLine {
    record: &'static str,
    turn: u32,
    status: &'static str,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
    rollback_confirmed: bool,
    socket_dropped: bool,
    end_audit_ok: bool,
    original_sha256: String,
    executed_sha256: String,
    stop: Option<&'static str>,
    sqlstate: Option<String>,
}

#[derive(Serialize)]
struct AdmitLine {
    protocol: &'static str,
    record: &'static str,
    original_sha256: String,
    executed_sha256: String,
    fingerprint_hex: String,
    role: String,
    project: String,
    policy_id: String,
    backend_pid: i32,
    backend_start: String,
}

#[derive(Serialize)]
struct CompleteLine {
    protocol: &'static str,
    record: &'static str,
    status: &'static str,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
    rollback_confirmed: bool,
    socket_dropped: bool,
    stop: Option<&'static str>,
    sqlstate: Option<String>,
    original_sha256: String,
    executed_sha256: String,
    fingerprint_hex: String,
    role: String,
    project: String,
    policy_id: String,
    backend_pid: i32,
    backend_start: String,
}

const HELP: &str = r#"query-pg — run one trusted local PostgreSQL SELECT

Usage:
  query-pg --request <path>
  query-pg --help

--request reads one oo.gateway.query-pg/v1 JSON document from a regular file.
The database password is read only from the password_file named in that
document. The command does not accept the password as an argument or from
the environment. Diagnostics do not include the password, the SQL text, or
a connection string.

The document names a TLS endpoint, a CA file, the current column policies,
catalog budgets, the statement, and an audit file. statement.source is
"request" or "stdin". Stdin carries up to 8 oo.gateway.query-pg-turn/v1
lines for the same role. identity.role is oo_r_ plus the policy role slug.
The audit file is created only for a statement the guard admits. Admission
is synced before the query starts. Completion is synced after the outcome.
An existing audit path is refused before a connection is opened.

Exit 0 is an accepted executor summary with status succeeded.
Exit 1 is a local stdout failure.
Exit 2 is usage: no arguments, an unknown flag, or a missing request path.
Exit 3 is a request, path, or stdin failure.
Exit 4 is TLS or CA verification.
Exit 5 is policy compilation or catalog load.
Exit 6 is a guard refusal. The audit file is not written.
Exit 7 is an audit file or admission failure. User SQL is not started.
Exit 8 is a query, role, limit, or session failure.

This command does not install permissions, does not read a source database,
and does not accept an HTTP request. File-permission deployment is outside
this command.
"#;

#[cfg(test)]
mod tests {
    use oo_core::ExactDecimal;
    use oo_gateway::query_executor::Cell;

    use super::out_cell;

    #[test]
    fn decimal_json_is_exact_and_existing_kinds_stay() {
        let positive = Cell::Decimal(ExactDecimal::new(false, "123456789012345678", 4).unwrap());
        assert_eq!(
            serde_json::to_string(&out_cell(&positive)).unwrap(),
            r#"{"kind":"decimal","negative":false,"coefficient":"123456789012345678","scale":4}"#
        );
        let negative = Cell::Decimal(ExactDecimal::new(true, "123456789012345678", 4).unwrap());
        assert_eq!(
            serde_json::to_string(&out_cell(&negative)).unwrap(),
            r#"{"kind":"decimal","negative":true,"coefficient":"123456789012345678","scale":4}"#
        );
        let zero = Cell::Decimal(ExactDecimal::new(true, "0", 4).unwrap());
        assert_eq!(
            serde_json::to_string(&out_cell(&zero)).unwrap(),
            r#"{"kind":"decimal","negative":false,"coefficient":"0","scale":4}"#
        );
        let small = Cell::Decimal(ExactDecimal::new(false, "1", 4).unwrap());
        assert_eq!(
            serde_json::to_string(&out_cell(&small)).unwrap(),
            r#"{"kind":"decimal","negative":false,"coefficient":"1","scale":4}"#
        );
        assert_eq!(
            serde_json::to_string(&out_cell(&Cell::Null)).unwrap(),
            r#"{"kind":"null"}"#
        );
        assert_eq!(
            serde_json::to_string(&out_cell(&Cell::Bool(true))).unwrap(),
            r#"{"kind":"bool","value":true}"#
        );
        assert_eq!(
            serde_json::to_string(&out_cell(&Cell::Bool(false))).unwrap(),
            r#"{"kind":"bool","value":false}"#
        );
        assert_eq!(
            serde_json::to_string(&out_cell(&Cell::Int("-9223372036854775808".to_string()))).unwrap(),
            r#"{"kind":"int","value":"-9223372036854775808"}"#
        );
        assert_eq!(
            serde_json::to_string(&out_cell(&Cell::Text("alpha".to_string()))).unwrap(),
            r#"{"kind":"text","value":"alpha"}"#
        );
    }
}
