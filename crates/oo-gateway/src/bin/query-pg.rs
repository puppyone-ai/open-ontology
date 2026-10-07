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

/// Private TLS proof for numeric quantity cells.
///
/// The harness runs only when `OO_A327_LIVE` is set. It compiles the live
/// catalog through the existing binaries and compares `query-pg` cells with
/// the hand literals. It does not print SQL.
#[cfg(test)]
mod a327_live {
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use oo_gateway::{connect_require_tls, TlsEndpoint};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use tokio_postgres::Client;

    const PROJECT: &str = "a327num";
    const SOURCE_DB: &str = "oo_a327_src";
    const TARGET_DB: &str = "oo_a327_dst";
    const READER: &str = "oo_a327_reader";
    const RUNTIME: &str = "oo_a327_runtime";
    const LOGIN: &str = "oo_a327_login";
    const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a327";
    const SHARD: &str = "a327main";
    const GENERATION: i64 = 27;
    const AMOUNT_COEFF: &str = "123456789012345678";
    const WIDE_COEFF: &str = "12345678901234567890123456789012345678";
    const WIDE_TEXT: &str = "1234567890123456789012345678901234.5678";
    const SUCCESS_SQL: &str = "SELECT entry_id, amount, wide FROM shop.entry ORDER BY entry_id";
    const HIDDEN_SQL: &str = "SELECT hidden FROM shop.entry";
    const DECISIONS: &[(&str, &str, &str, &str)] = &[
        ("shop.entry", "entry_id", "none", "identifier"),
        ("shop.entry", "amount", "none", "quantity"),
        ("shop.entry", "wide", "none", "quantity"),
    ];

    struct Stop {
        step: String,
        detail: String,
    }

    impl Stop {
        fn new(step: &str, detail: impl std::fmt::Display) -> Self {
            Self { step: step.to_string(), detail: detail.to_string() }
        }
    }

    struct Env {
        evidence: PathBuf,
        host: String,
        port: u16,
        admin_user: String,
        admin_password: String,
        reader_password: String,
        runtime_password: String,
        login_password: String,
        ca: Vec<u8>,
        pipeline: PathBuf,
        builder: PathBuf,
        reviewed: PathBuf,
        bundle: PathBuf,
        policy: PathBuf,
        install: PathBuf,
        mapping: PathBuf,
        analyze: PathBuf,
        init_pg: PathBuf,
        query: PathBuf,
        secrets: [&'static str; 4],
    }

    impl Env {
        fn load() -> Result<Self, Stop> {
            let port = env_var("OO_A327_PORT")?.parse::<u16>().map_err(|_| Stop::new("env", "port"))?;
            let admin_password = read_secret(&env_var("OO_A327_ADMIN_PASSWORD_FILE")?)?;
            let reader_password = read_secret(&env_var("OO_A327_READER_PASSWORD_FILE")?)?;
            let runtime_password = read_secret(&env_var("OO_A327_RUNTIME_PASSWORD_FILE")?)?;
            let login_password = read_secret(&env_var("OO_A327_LOGIN_PASSWORD_FILE")?)?;
            let ca = fs::read(env_var("OO_A327_CA_FILE")?).map_err(|_| Stop::new("env", "ca"))?;
            Ok(Self {
                evidence: PathBuf::from(env_var("OO_A327_EVIDENCE")?),
                host: env_var("OO_A327_HOST")?,
                port,
                admin_user: env_var("OO_A327_ADMIN_USER")?,
                admin_password,
                reader_password,
                runtime_password,
                login_password,
                ca,
                pipeline: PathBuf::from(env_var("OO_A327_PIPELINE")?),
                builder: PathBuf::from(env_var("OO_A327_BUILDER")?),
                reviewed: PathBuf::from(env_var("OO_A327_REVIEWED_EFFECTIVE")?),
                bundle: PathBuf::from(env_var("OO_A327_ROLE_PACK_BUNDLE")?),
                policy: PathBuf::from(env_var("OO_A327_REVIEWED_POLICY")?),
                install: PathBuf::from(env_var("OO_A327_INSTALL")?),
                mapping: PathBuf::from(env_var("OO_A327_MAPPING")?),
                analyze: PathBuf::from(env_var("OO_A327_ANALYZE")?),
                init_pg: PathBuf::from(env_var("OO_A327_INIT")?),
                query: PathBuf::from(env_var("OO_A327_QUERY")?),
                secrets: [
                    "/secrets/admin.pass",
                    "/secrets/reader.pass",
                    "/secrets/runtime.pass",
                    "/secrets/login.pass",
                ],
            })
        }

        fn passwords(&self) -> [&str; 4] {
            [
                self.admin_password.as_str(),
                self.reader_password.as_str(),
                self.runtime_password.as_str(),
                self.login_password.as_str(),
            ]
        }
    }

    struct Session {
        client: Client,
        driver: tokio::task::JoinHandle<()>,
    }

    impl Session {
        async fn open(env: &Env, database: &str, user: &str, password: &str) -> Result<Self, Stop> {
            let endpoint = TlsEndpoint {
                host: env.host.clone(),
                port: env.port,
                database: database.to_string(),
                user: user.to_string(),
            };
            let (client, connection) = connect_require_tls(&endpoint, password, &env.ca)
                .await
                .map_err(|_| Stop::new("dial", format!("user={user} db={database}")))?;
            let driver = tokio::spawn(async move {
                let _ = connection.await;
            });
            Ok(Self { client, driver })
        }

        async fn close(self) {
            drop(self.client);
            let _ = self.driver.await;
        }
    }

    struct CmdOut {
        code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    enum CellExpect {
        Int(&'static str),
        Null,
        Dec { negative: bool, coefficient: &'static str },
    }

    #[test]
    fn live_numeric_proof() {
        if std::env::var_os("OO_A327_LIVE").is_none() {
            return;
        }
        assert_eq!(AMOUNT_COEFF.len(), 18);
        assert_eq!(WIDE_COEFF.len(), 38);
        assert!(AMOUNT_COEFF.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(WIDE_COEFF.bytes().all(|byte| byte.is_ascii_digit()));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let evidence = std::env::var("OO_A327_EVIDENCE").unwrap_or_default();
        match runtime.block_on(run()) {
            Ok(outcome) => {
                let dir = PathBuf::from(&evidence);
                fs::write(dir.join("outcome.json"), serde_json::to_vec_pretty(&outcome).expect("outcome")).expect("write outcome");
                fs::write(dir.join("outcome.txt"), "numeric_query_recorded\n").expect("write outcome text");
            }
            Err(stop) => {
                let body = json!({"step": stop.step, "detail": stop.detail});
                let _ = fs::write(Path::new(&evidence).join("stop.json"), serde_json::to_vec_pretty(&body).unwrap_or_default());
                panic!("stop step={} detail={}", stop.step, stop.detail);
            }
        }
    }

    async fn run() -> Result<Value, Stop> {
        let env = Env::load()?;
        fs::create_dir_all(env.evidence.join("steps")).map_err(|_| Stop::new("evidence", "steps"))?;
        fs::create_dir_all(env.evidence.join("requests")).map_err(|_| Stop::new("evidence", "requests"))?;
        fs::create_dir_all(env.evidence.join("audits")).map_err(|_| Stop::new("evidence", "audits"))?;
        let admin = Session::open(&env, "postgres", &env.admin_user, &env.admin_password).await?;
        let ssl = scalar(&admin.client, "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await?;
        let version = scalar(&admin.client, "SHOW server_version").await?;
        if ssl != "true" && ssl != "t" {
            return Err(Stop::new("tls", "ssl is not true"));
        }
        if !version.starts_with("17.10") {
            return Err(Stop::new("version", version));
        }
        prepare_cluster(&admin.client, &env).await?;
        admin.close().await;
        let source = Session::open(&env, SOURCE_DB, &env.admin_user, &env.admin_password).await?;
        exec(&source.client, &fixture_sql()).await?;
        source.close().await;

        let catalog = run_bin(&env, &env.pipeline, &["catalog-pg"], &to_vec(&source_request(&env)?)?, "catalog-pg")?;
        require_exit(&env, "catalog-pg", &catalog, 0)?;
        let captured = run_bin(&env, &env.builder, &["captured-pg-draft"], &catalog.stdout, "captured-pg-draft")?;
        require_exit(&env, "captured-pg-draft", &captured, 0)?;
        let captured_value = parse_json(&captured.stdout, "captured-pg-draft")?;
        let reviews = structural_reviews(&captured_value)?;
        let review_in = to_vec(&json!({"catalog": parse_json(&catalog.stdout, "catalog-pg")?, "reviews": reviews}))?;
        let reviewed = run_bin(&env, &env.builder, &["review-pg-draft"], &review_in, "review-pg-draft")?;
        require_exit(&env, "review-pg-draft", &reviewed, 0)?;
        let artifact_text = exact_text(&reviewed.stdout, "review-pg-draft")?;
        let artifact = parse_json(artifact_text.as_bytes(), "review-pg-draft")?;
        if artifact["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
            return Err(Stop::new("review-pg-draft", "protocol"));
        }
        let schema_hash = text_at(&artifact, "/draft/provenance/schema_hash", "schema_hash")?;
        let decisions = decision_list(&artifact, &schema_hash)?;
        let decisions_text = serde_json::to_string(&decisions).map_err(|_| Stop::new("decisions", "encode"))?;
        let effective_in = to_vec(&json!({
            "protocol": "oo.builder.reviewed-effective-oos-command/v1",
            "artifact": artifact,
            "settings": settings(),
            "decisions": decisions
        }))?;
        let effective_out = run_bin(&env, &env.reviewed, &[], &effective_in, "reviewed-effective-oos")?;
        require_exit(&env, "reviewed-effective-oos", &effective_out, 0)?;
        let command_text = exact_text(&effective_out.stdout, "reviewed-effective-oos")?;
        let command = parse_json(command_text.as_bytes(), "reviewed-effective-oos")?;
        if command["protocol"] != "oo.builder.reviewed-effective-oos-command/v1" {
            return Err(Stop::new("reviewed-effective-oos", "protocol"));
        }
        let effective = command.get("effective_oos").cloned().ok_or_else(|| Stop::new("reviewed-effective-oos", "effective_oos"))?;
        check_applied(&effective)?;
        let visibility = visibility_from(&effective)?;
        let pack_in = to_vec(&json!({
            "protocol": "oo.builder.role-pack-command/v1",
            "effective_oos": effective,
            "visibility": visibility
        }))?;
        let pack = run_bin(&env, &env.builder, &["role-pack"], &pack_in, "role-pack")?;
        require_exit(&env, "role-pack", &pack, 0)?;
        let pack_value = parse_json(&pack.stdout, "role-pack")?;
        let pack_digest = pack_value["pack_digest"].as_str().unwrap_or("").to_string();
        if pack_digest.len() != 64 {
            return Err(Stop::new("role-pack", "pack_digest"));
        }
        let request_path = env.evidence.join("requests/role-pack-command.json");
        fs::write(&request_path, &pack_in).map_err(|_| Stop::new("role-pack-bundle", "request"))?;
        // The bundle compiler publishes with a same-directory hard link.
        // The evidence bind may not support that, so the output stays on the target volume.
        let bundle_path = PathBuf::from("/target/a327/a327-role-pack-bundle.json");
        if bundle_path.exists() {
            return Err(Stop::new("role-pack-bundle", "output exists"));
        }
        let bundle = run_bin(
            &env,
            &env.bundle,
            &["--request", request_path.to_str().ok_or_else(|| Stop::new("role-pack-bundle", "path"))?, "--output", bundle_path.to_str().ok_or_else(|| Stop::new("role-pack-bundle", "path"))?],
            b"",
            "role-pack-bundle",
        )?;
        require_exit(&env, "role-pack-bundle", &bundle, 0)?;

        let early = run_bin(&env, &env.pipeline, &["import-pg"], &to_vec(&import_request(&env)?)?, "import-pg-not-ready")?;
        if early.code == 0 {
            return Err(Stop::new("import-pg-not-ready", "target was already ready"));
        }
        let init = run_bin(&env, &env.init_pg, &[], &to_vec(&init_request(&env)?)?, "init-pg")?;
        require_exit(&env, "init-pg", &init, 0)?;
        let admin = Session::open(&env, "postgres", &env.admin_user, &env.admin_password).await?;
        exec(&admin.client, &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}")).await?;
        exec(&admin.client, &format!("GRANT oo_pipeline_runtime TO {RUNTIME}")).await?;
        admin.close().await;
        let imported = run_bin(&env, &env.pipeline, &["import-pg"], &to_vec(&import_request(&env)?)?, "import-pg")?;
        require_exit(&env, "import-pg", &imported, 0)?;
        let target = Session::open(&env, TARGET_DB, &env.admin_user, &env.admin_password).await?;
        let (digest, generation) = mapping_identity(&target.client).await?;
        target.close().await;
        if generation != GENERATION {
            return Err(Stop::new("mapping", "generation"));
        }

        let binding = binding_json(&command, &schema_hash, &digest, generation);
        let policy_in = to_vec(&json!({
            "protocol": "oo.gateway.reviewed-policy-cli/v1",
            "command": command_text,
            "artifact": artifact_text,
            "decisions": decisions_text,
            "binding": binding
        }))?;
        let policy_out = run_bin(&env, &env.policy, &[], &policy_in, "reviewed-policy")?;
        require_exit(&env, "reviewed-policy", &policy_out, 0)?;
        let policy_doc = parse_json(&policy_out.stdout, "reviewed-policy")?;
        if policy_doc["protocol"] != "oo.gateway.reviewed-policy-cli/v1" {
            return Err(Stop::new("reviewed-policy", "protocol"));
        }
        let policies = policy_doc.get("policies").cloned().ok_or_else(|| Stop::new("reviewed-policy", "policies"))?;
        if policies.as_array().map(Vec::len).unwrap_or(0) != 1 {
            return Err(Stop::new("reviewed-policy", "table count"));
        }
        let install_in = to_vec(&json!({
            "protocol": "oo.gateway.install-column-acl-cli/v1",
            "policies": policies.clone(),
            "endpoint": admin_endpoint(&env, TARGET_DB),
            "password_file": env.secrets[0],
            "ca_file": "/secrets/ca.crt"
        }))?;
        let install = run_bin(&env, &env.install, &[], &install_in, "install-column-acl")?;
        require_exit(&env, "install-column-acl", &install, 0)?;
        let mapping_in = to_vec(&json!({
            "protocol": "oo.gateway.provision-mapping-reader-cli/v1",
            "policies": policies.clone(),
            "login": LOGIN,
            "endpoint": admin_endpoint(&env, TARGET_DB),
            "password_file": env.secrets[0],
            "ca_file": "/secrets/ca.crt"
        }))?;
        let mapping = run_bin(&env, &env.mapping, &[], &mapping_in, "provision-mapping-reader")?;
        require_exit(&env, "provision-mapping-reader", &mapping, 0)?;

        let before = query_once(&env, "before-analyze", &policies, &digest, SUCCESS_SQL)?;
        require_exit(&env, "before-analyze", &before, 5)?;
        expect_stderr(&before, "query-pg: policy\n")?;
        expect_absent(&env, "before-analyze")?;
        if exposes_sql(&before.stdout) || exposes_sql(&before.stderr) {
            return Err(Stop::new("before-analyze", "sql exposure"));
        }

        let analyze_in = to_vec(&analyze_request(&env, &digest, generation))?;
        let analyze = run_bin(&env, &env.analyze, &[], &analyze_in, "analyze-pg")?;
        require_exit(&env, "analyze-pg", &analyze, 0)?;

        let success = query_once(&env, "amounts", &policies, &digest, SUCCESS_SQL)?;
        require_exit(&env, "amounts", &success, 0)?;
        expect_rows(&success.stdout, &success_rows())?;
        scan_query(&env, "amounts", &success)?;
        let hidden = query_once(&env, "hidden", &policies, &digest, HIDDEN_SQL)?;
        require_exit(&env, "hidden", &hidden, 6)?;
        expect_stderr(&hidden, "query-pg: guard\n")?;
        expect_absent(&env, "hidden")?;
        if exposes_sql(&hidden.stdout) || exposes_sql(&hidden.stderr) {
            return Err(Stop::new("hidden", "sql exposure"));
        }

        let bundle_bytes = fs::read(&bundle_path).map_err(|_| Stop::new("bundle", "read"))?;
        fs::write(env.evidence.join("role-pack-bundle.json"), &bundle_bytes).map_err(|_| Stop::new("bundle", "copy"))?;
        let bundle_sha256 = sha256_hex(&bundle_bytes);
        Ok(json!({
            "fixed_ancestor": "e6ebf93b5ac261c097a24d145611e0300e4f88b5",
            "server_version": version,
            "ssl": ssl,
            "schema_hash": schema_hash,
            "config_digest": digest,
            "generation": generation,
            "pack_digest": pack_digest,
            "bundle_sha256": bundle_sha256,
            "policy_tables": 1,
            "infinity_storage": "numeric(18,4) rejects Infinity at insert with sqlstate 22003",
            "nan_import": "refused numeric_nan",
            "before_analyze_exit": 5,
            "amounts_exit": 0,
            "amounts_rows": 6,
            "hidden_exit": 6,
            "sql_in_query_output": false,
            "cells": "hand_literals",
            "parent_acceptance": false,
            "m1_i01": "not_claimed",
            "m1_i02": "not_claimed",
            "release": "not_claimed"
        }))
    }

    fn success_rows() -> Vec<Vec<CellExpect>> {
        let dec = |negative: bool, coefficient: &'static str| CellExpect::Dec { negative, coefficient };
        vec![
            vec![CellExpect::Int("1"), dec(false, AMOUNT_COEFF), dec(false, WIDE_COEFF)],
            vec![CellExpect::Int("2"), dec(true, AMOUNT_COEFF), dec(true, WIDE_COEFF)],
            vec![CellExpect::Int("3"), dec(false, "1"), dec(false, "1")],
            vec![CellExpect::Int("4"), dec(true, "1"), dec(true, "1")],
            vec![CellExpect::Int("5"), dec(false, "0"), dec(false, "0")],
            vec![CellExpect::Int("6"), CellExpect::Null, CellExpect::Null],
        ]
    }

    fn fixture_sql() -> String {
        format!(
            "CREATE SCHEMA shop;\
             CREATE TABLE shop.entry (entry_id int8 PRIMARY KEY, amount numeric(18,4), wide numeric(38,4), hidden int8 NOT NULL);\
             INSERT INTO shop.entry (entry_id, amount, wide, hidden) VALUES\
               (1, 12345678901234.5678, {WIDE_TEXT}, 1),\
               (2, -12345678901234.5678, -{WIDE_TEXT}, 2),\
               (3, 0.0001, 0.0001, 3),\
               (4, -0.0001, -0.0001, 4),\
               (5, 0.0000, 0.0000, 5),\
               (6, NULL, NULL, 6);\
             REVOKE ALL ON SCHEMA shop FROM PUBLIC;\
             REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;\
             GRANT USAGE ON SCHEMA shop TO {READER};\
             GRANT SELECT ON ALL TABLES IN SCHEMA shop TO {READER};"
        )
    }

    async fn prepare_cluster(client: &Client, env: &Env) -> Result<(), Stop> {
        let _ = client.batch_execute(&format!("DROP DATABASE IF EXISTS {SOURCE_DB} WITH (FORCE)")).await;
        let _ = client.batch_execute(&format!("DROP DATABASE IF EXISTS {TARGET_DB} WITH (FORCE)")).await;
        for role in [READER, RUNTIME, LOGIN, "oo_r_clerk", "oo_pipeline_runtime", "oo_target_owner"] {
            let _ = client.batch_execute(&format!("DROP ROLE IF EXISTS {role}")).await;
        }
        create_role(client, READER, &env.reader_password, "INHERIT").await?;
        create_role(client, RUNTIME, &env.runtime_password, "INHERIT").await?;
        create_role(client, LOGIN, &env.login_password, "NOINHERIT").await?;
        exec(client, &format!("CREATE DATABASE {SOURCE_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'")).await?;
        exec(client, &format!("CREATE DATABASE {TARGET_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'")).await?;
        exec(client, &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}")).await?;
        exec(client, &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}")).await?;
        Ok(())
    }

    async fn create_role(client: &Client, name: &str, password: &str, inherit: &str) -> Result<(), Stop> {
        exec(client, &format!("CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION {inherit}")).await
    }

    async fn exec(client: &Client, sql: &str) -> Result<(), Stop> {
        client.batch_execute(sql).await.map_err(|err| {
            let state = err.code().map(|code| code.code().to_string()).unwrap_or_else(|| "none".to_string());
            Stop::new("sql", format!("sqlstate={state}"))
        })
    }

    async fn scalar(client: &Client, sql: &str) -> Result<String, Stop> {
        let row = client.query_one(sql, &[]).await.map_err(|_| Stop::new("sql", "scalar"))?;
        row.try_get(0).map_err(|_| Stop::new("sql", "scalar"))
    }

    async fn mapping_identity(client: &Client) -> Result<(String, i64), Stop> {
        let rows = client
            .query("SELECT DISTINCT source, config_digest, run_generation FROM _oo.physical_mappings", &[])
            .await
            .map_err(|_| Stop::new("mapping", "read"))?;
        if rows.len() != 1 {
            return Err(Stop::new("mapping", format!("rows={}", rows.len())));
        }
        let source: String = rows[0].try_get(0).map_err(|_| Stop::new("mapping", "source"))?;
        let digest: String = rows[0].try_get(1).map_err(|_| Stop::new("mapping", "digest"))?;
        let generation: i64 = rows[0].try_get(2).map_err(|_| Stop::new("mapping", "generation"))?;
        if source != PROJECT || digest.len() != 64 {
            return Err(Stop::new("mapping", "identity"));
        }
        Ok((digest, generation))
    }

    fn source_request(env: &Env) -> Result<Value, Stop> {
        Ok(json!({
            "protocol": "oo.pipeline.catalog-pg/v1",
            "source": source_object(env)
        }))
    }

    fn import_request(env: &Env) -> Result<Value, Stop> {
        Ok(json!({
            "protocol": "oo.pipeline.import-pg/v1",
            "source": source_object(env),
            "target": {
                "host": env.host,
                "port": env.port,
                "database": TARGET_DB,
                "runtime_principal": RUNTIME,
                "password_file": env.secrets[2],
                "ca_file": "/secrets/ca.crt"
            },
            "shard": SHARD,
            "owner": OWNER,
            "source_generation": GENERATION
        }))
    }

    fn init_request(env: &Env) -> Result<Value, Stop> {
        Ok(json!({
            "protocol": "oo.pipeline.init-pg/v1",
            "connect_ms": 30000,
            "target": {
                "host": env.host,
                "port": env.port,
                "database": TARGET_DB,
                "administrator": env.admin_user,
                "password_file": env.secrets[0],
                "ca_file": "/secrets/ca.crt"
            }
        }))
    }

    fn analyze_request(env: &Env, digest: &str, generation: i64) -> Value {
        json!({
            "protocol": "oo.pipeline.analyze-pg/v1",
            "project": PROJECT,
            "source": PROJECT,
            "config_digest": digest,
            "source_generation": generation,
            "tables": [
                {"namespace": "shop", "name": "entry"}
            ],
            "statement_timeout_ms": 5000,
            "lock_timeout_ms": 5000,
            "target": {
                "host": env.host,
                "port": env.port,
                "database": TARGET_DB,
                "operator": env.admin_user,
                "password_file": env.secrets[0],
                "ca_file": "/secrets/ca.crt"
            }
        })
    }

    fn source_object(env: &Env) -> Value {
        json!({
            "source_id": PROJECT,
            "host": env.host,
            "port": env.port,
            "database": SOURCE_DB,
            "username": READER,
            "secret_ref": "secret://pg/a327/reader",
            "password_file": env.secrets[1],
            "ca_file": "/secrets/ca.crt",
            "tables": [
                {"schema": "shop", "table": "entry"}
            ]
        })
    }

    fn settings() -> Value {
        json!({
            "project": PROJECT,
            "ontology_version": "ont-a327",
            "catalog_version": "cat-a327",
            "policy": {"id": "pol-a327", "version": "1"},
            "knowledge": {"id": "know-a327", "version": "1"},
            "ontology": {
                "spec_version": "oos/v1",
                "name": "shop",
                "locale": "en-US",
                "include_schemas": ["shop"],
                "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []},
                "classification": {"auto_accept": "rule_high_confidence"},
                "lifecycle": {"deprecation_min_releases": 2},
                "compile": {
                    "per_role": true,
                    "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                    "sample_values": {"default": "masked", "max_per_column": 0}
                }
            }
        })
    }

    fn structural_reviews(captured: &Value) -> Result<Value, Stop> {
        let schema_hash = captured["wrapper"]["schema_hash"].as_str().ok_or_else(|| Stop::new("captured-pg-draft", "schema_hash"))?;
        let queue = captured["draft"]["review_queue"].as_array().ok_or_else(|| Stop::new("captured-pg-draft", "review_queue"))?;
        let mut reviews = Vec::new();
        for item in queue {
            let kind = item["kind"].as_str().unwrap_or("");
            if kind != "structural_column" && kind != "structural_primary_key" && kind != "candidate_relationship" {
                continue;
            }
            let item_id = item["item_id"].as_str().unwrap_or("");
            if item_id.starts_with("pii/") || item_id.starts_with("semantic/") || item_id.starts_with("type/") {
                continue;
            }
            let fingerprint = item["fact_fingerprint"].as_str().ok_or_else(|| Stop::new("captured-pg-draft", "fact_fingerprint"))?;
            reviews.push(json!({
                "item_id": item_id,
                "action": "accept",
                "schema_hash": schema_hash,
                "fact_fingerprint": fingerprint
            }));
        }
        if reviews.is_empty() {
            return Err(Stop::new("captured-pg-draft", "no structural review items"));
        }
        Ok(Value::Array(reviews))
    }

    fn decision_list(artifact: &Value, schema_hash: &str) -> Result<Vec<Value>, Stop> {
        let mut out = Vec::new();
        for (dataset, column, pii, semantic) in DECISIONS {
            let item_id = format!("column/{dataset}/{column}");
            let queue = artifact["draft"]["review_queue"].as_array().ok_or_else(|| Stop::new("decisions", "review_queue"))?;
            let fingerprint = queue
                .iter()
                .find(|item| item["kind"] == "structural_column" && item["item_id"] == item_id)
                .and_then(|item| item["fact_fingerprint"].as_str())
                .ok_or_else(|| Stop::new("decisions", item_id))?;
            out.push(json!({
                "dataset": dataset,
                "column": column,
                "schema_hash": schema_hash,
                "fact_fingerprint": fingerprint,
                "pii": pii,
                "semantic_type": semantic,
                "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
            }));
        }
        Ok(out)
    }

    fn check_applied(effective: &Value) -> Result<(), Stop> {
        for (dataset, column, pii, semantic) in DECISIONS {
            let found = find_column(effective, dataset, column)?;
            if found["pii"] != *pii || found["semantic_type"] != *semantic {
                return Err(Stop::new("reviewed-effective-oos", format!("applied {dataset}.{column}")));
            }
            if (*column == "amount" || *column == "wide") && found["data_type"] != "numeric" {
                return Err(Stop::new("reviewed-effective-oos", format!("data_type {dataset}.{column}")));
            }
        }
        Ok(())
    }

    fn find_column<'a>(effective: &'a Value, dataset: &str, column: &str) -> Result<&'a Value, Stop> {
        let datasets = effective["datasets"].as_array().ok_or_else(|| Stop::new("effective", "datasets"))?;
        for item in datasets {
            if item["dataset"].as_str() != Some(dataset) {
                continue;
            }
            let columns = item["columns"].as_array().ok_or_else(|| Stop::new("effective", "columns"))?;
            for found in columns {
                if found["name"].as_str() == Some(column) {
                    return Ok(found);
                }
            }
        }
        Err(Stop::new("effective", format!("missing {dataset}.{column}")))
    }

    fn visibility_from(effective: &Value) -> Result<Value, Stop> {
        let datasets = effective["datasets"].as_array().ok_or_else(|| Stop::new("visibility", "datasets"))?;
        let mut listed = Vec::new();
        for item in datasets {
            let name = item["dataset"].as_str().ok_or_else(|| Stop::new("visibility", "dataset"))?;
            let columns = item["columns"].as_array().ok_or_else(|| Stop::new("visibility", "columns"))?;
            let mut names = Vec::new();
            for column in columns {
                let column_name = column["name"].as_str().ok_or_else(|| Stop::new("visibility", "column"))?;
                if DECISIONS.iter().any(|(dataset, decided, _, _)| *dataset == name && *decided == column_name) {
                    names.push(column_name);
                }
            }
            if names.is_empty() {
                continue;
            }
            listed.push(json!({
                "dataset": name,
                "row_security": "none",
                "global_stats": "denied",
                "columns": names
            }));
        }
        if listed.is_empty() {
            return Err(Stop::new("visibility", "empty"));
        }
        Ok(json!({
            "protocol": "oo.builder.role-visibility/v1",
            "project": PROJECT,
            "principal": "fixture_operator",
            "role": "oo_r_clerk",
            "ontology_version": "ont-a327",
            "catalog_version": "cat-a327",
            "policy": {"id": "pol-a327", "version": "1"},
            "knowledge": {"id": "know-a327", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": listed,
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        }))
    }

    fn binding_json(command: &Value, schema_hash: &str, digest: &str, generation: i64) -> Value {
        let mut allow = Vec::new();
        for (dataset, column, _, _) in DECISIONS {
            allow.push(json!({"dataset": dataset, "column": column, "explicit_allow": false}));
        }
        json!({
            "identity": {
                "project": PROJECT,
                "ontology_version": "ont-a327",
                "catalog_version": "cat-a327",
                "policy_id": "pol-a327",
                "policy_version": "1",
                "knowledge_id": "know-a327",
                "knowledge_version": "1"
            },
            "seal": {
                "source_id": PROJECT,
                "schema": "shop",
                "schema_hash": schema_hash,
                "artifact_sha256": command["provenance"]["sealed_source_sha256"].as_str().unwrap_or("")
            },
            "config_digest": digest,
            "generation": generation,
            "role": "clerk",
            "login": LOGIN,
            "allow": allow
        })
    }

    fn admin_endpoint(env: &Env, database: &str) -> Value {
        json!({"host": env.host, "port": env.port, "database": database, "user": env.admin_user})
    }

    fn query_once(env: &Env, name: &str, policies: &Value, digest: &str, sql: &str) -> Result<CmdOut, Stop> {
        let audit_path = env.evidence.join(format!("audits/{name}.jsonl"));
        let audit = audit_path.to_str().ok_or_else(|| Stop::new(name, "audit path"))?;
        let request = json!({
            "version": "oo.gateway.query-pg/v1",
            "endpoint": {"host": env.host, "port": env.port, "database": TARGET_DB, "user": LOGIN},
            "password_file": env.secrets[3],
            "ca_file": "/secrets/ca.crt",
            "audit_file": audit,
            "identity": {"project": PROJECT, "role": "oo_r_clerk", "policy_id": digest},
            "policies": policies,
            "catalog": {
                "max_tables": 8,
                "max_columns": 64,
                "max_functions": 1,
                "max_operators": 64,
                "large_table_row_threshold": 1000000,
                "unknown_rows": "refuse"
            },
            "statement": {"source": "request", "sql": sql, "max_rows": 20, "max_result_bytes": 8192, "wall_clock_ms": 30000}
        });
        let bytes = to_vec(&request)?;
        if exposes_secret(&bytes, &env.passwords()) {
            return Err(Stop::new(name, "request contains a password"));
        }
        let path = env.evidence.join(format!("requests/{name}.json"));
        fs::write(&path, &bytes).map_err(|_| Stop::new(name, "request"))?;
        let path_text = path.to_str().ok_or_else(|| Stop::new(name, "path"))?;
        run_bin(env, &env.query, &["--request", path_text], b"", name)
    }

    fn expect_rows(stdout: &[u8], rows: &[Vec<CellExpect>]) -> Result<(), Stop> {
        let lines = json_lines(stdout, "amounts")?;
        if lines.len() != rows.len() + 1 {
            return Err(Stop::new("amounts", format!("lines={}", lines.len())));
        }
        for (index, expect) in rows.iter().enumerate() {
            if lines[index]["record"].as_str() != Some("row") || lines[index]["turn"].as_i64() != Some(0) {
                return Err(Stop::new("amounts", "row header"));
            }
            let cells = lines[index]["cells"].as_array().ok_or_else(|| Stop::new("amounts", "cells"))?;
            if cells.len() != expect.len() {
                return Err(Stop::new("amounts", "width"));
            }
            for (cell, expected) in cells.iter().zip(expect.iter()) {
                match expected {
                    CellExpect::Int(value) => {
                        if cell["kind"].as_str() != Some("int") || cell["value"].as_str() != Some(*value) {
                            return Err(Stop::new("amounts", "int"));
                        }
                    }
                    CellExpect::Null => {
                        if cell["kind"].as_str() != Some("null") {
                            return Err(Stop::new("amounts", "null"));
                        }
                    }
                    CellExpect::Dec { negative, coefficient } => {
                        if cell["kind"].as_str() != Some("decimal")
                            || cell["negative"].as_bool() != Some(*negative)
                            || cell["coefficient"].as_str() != Some(*coefficient)
                            || cell["scale"].as_i64() != Some(4)
                        {
                            return Err(Stop::new("amounts", "decimal"));
                        }
                    }
                }
            }
        }
        let summary = lines.last().ok_or_else(|| Stop::new("amounts", "summary"))?;
        if summary["record"].as_str() != Some("summary")
            || summary["status"].as_str() != Some("succeeded")
            || summary["returned_rows"].as_u64() != Some(rows.len() as u64)
            || !summary["stop"].is_null()
        {
            return Err(Stop::new("amounts", "summary"));
        }
        Ok(())
    }

    fn scan_query(env: &Env, name: &str, output: &CmdOut) -> Result<(), Stop> {
        if exposes_sql(&output.stdout) || exposes_sql(&output.stderr) {
            return Err(Stop::new(name, "sql exposure"));
        }
        let audit = env.evidence.join(format!("audits/{name}.jsonl"));
        if audit.exists() {
            let bytes = fs::read(&audit).map_err(|_| Stop::new(name, "audit"))?;
            if exposes_sql(&bytes) || exposes_secret(&bytes, &env.passwords()) {
                return Err(Stop::new(name, "audit exposure"));
            }
        }
        Ok(())
    }

    fn exposes_sql(bytes: &[u8]) -> bool {
        let texts = [SUCCESS_SQL, HIDDEN_SQL];
        let text = String::from_utf8_lossy(bytes);
        texts.iter().any(|sql| text.contains(sql)) || text.contains("SELECT ")
    }

    fn expect_stderr(output: &CmdOut, line: &str) -> Result<(), Stop> {
        if output.stderr == line.as_bytes() {
            Ok(())
        } else {
            Err(Stop::new("stderr", format!("len={}", output.stderr.len())))
        }
    }

    fn expect_absent(env: &Env, name: &str) -> Result<(), Stop> {
        let audit = env.evidence.join(format!("audits/{name}.jsonl"));
        if audit.exists() {
            Err(Stop::new(name, "audit was written"))
        } else {
            Ok(())
        }
    }

    fn json_lines(bytes: &[u8], step: &str) -> Result<Vec<Value>, Stop> {
        let text = std::str::from_utf8(bytes).map_err(|_| Stop::new(step, "utf8"))?;
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let mut out = Vec::new();
        for line in lines {
            out.push(serde_json::from_str(line).map_err(|_| Stop::new(step, "json"))?);
        }
        Ok(out)
    }

    fn run_bin(env: &Env, bin: &Path, args: &[&str], stdin: &[u8], name: &str) -> Result<CmdOut, Stop> {
        if exposes_secret(stdin, &env.passwords()) && !stdin_is_request(name) {
            return Err(Stop::new(name, "stdin contains a password"));
        }
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| Stop::new(name, "spawn"))?;
        child.stdin.take().ok_or_else(|| Stop::new(name, "stdin"))?.write_all(stdin).map_err(|_| Stop::new(name, "write"))?;
        let output = child.wait_with_output().map_err(|_| Stop::new(name, "wait"))?;
        let code = output.status.code().unwrap_or(127);
        let stdout = redact(&output.stdout, &env.passwords());
        let stderr = redact(&output.stderr, &env.passwords());
        let dir = env.evidence.join("steps");
        fs::write(dir.join(format!("{name}.stdout")), &stdout).map_err(|_| Stop::new(name, "evidence"))?;
        fs::write(dir.join(format!("{name}.stderr")), &stderr).map_err(|_| Stop::new(name, "evidence"))?;
        fs::write(dir.join(format!("{name}.exit")), format!("{code}\n")).map_err(|_| Stop::new(name, "evidence"))?;
        Ok(CmdOut { code, stdout, stderr })
    }

    fn stdin_is_request(name: &str) -> bool {
        matches!(name, "catalog-pg" | "import-pg" | "import-pg-not-ready" | "init-pg" | "analyze-pg" | "install-column-acl" | "provision-mapping-reader" | "amounts" | "nan" | "pos-inf" | "neg-inf" | "hidden" | "before-analyze")
    }

    fn require_exit(env: &Env, name: &str, output: &CmdOut, code: i32) -> Result<(), Stop> {
        if output.code == code {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut detail = format!("exit {} stderr={}", output.code, stderr.chars().take(300).collect::<String>());
        for secret in env.passwords() {
            if !secret.is_empty() {
                detail = detail.replace(secret, "[redacted]");
            }
        }
        Err(Stop::new(name, detail))
    }

    fn redact(bytes: &[u8], secrets: &[&str]) -> Vec<u8> {
        if exposes_secret(bytes, secrets) {
            b"redacted\n".to_vec()
        } else {
            bytes.to_vec()
        }
    }

    fn exposes_secret(bytes: &[u8], secrets: &[&str]) -> bool {
        let text = String::from_utf8_lossy(bytes);
        secrets.iter().any(|secret| !secret.is_empty() && text.contains(secret))
    }

    fn parse_json(bytes: &[u8], step: &str) -> Result<Value, Stop> {
        let text = std::str::from_utf8(bytes).map_err(|_| Stop::new(step, "utf8"))?;
        serde_json::from_str(text.trim_end()).map_err(|_| Stop::new(step, "json"))
    }

    fn exact_text(bytes: &[u8], step: &str) -> Result<String, Stop> {
        if bytes.ends_with(b"\n") {
            return Err(Stop::new(step, "trailing newline"));
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| Stop::new(step, "utf8"))
    }

    fn text_at(value: &Value, pointer: &str, step: &str) -> Result<String, Stop> {
        value.pointer(pointer).and_then(Value::as_str).map(str::to_string).ok_or_else(|| Stop::new(step, "missing"))
    }

    fn to_vec(value: &Value) -> Result<Vec<u8>, Stop> {
        serde_json::to_vec(value).map_err(|_| Stop::new("json", "encode"))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn env_var(name: &str) -> Result<String, Stop> {
        std::env::var(name).map_err(|_| Stop::new("env", name))
    }

    fn read_secret(path: &str) -> Result<String, Stop> {
        let bytes = fs::read(path).map_err(|_| Stop::new("env", "secret"))?;
        let text = String::from_utf8(bytes).map_err(|_| Stop::new("env", "secret"))?;
        let trimmed = text.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() || trimmed.len() > 1024 {
            return Err(Stop::new("env", "secret"));
        }
        Ok(trimmed.to_string())
    }
}
