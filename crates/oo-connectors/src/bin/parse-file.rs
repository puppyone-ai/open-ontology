//! `parse-file` admits one local CSV, TSV, or JSONL file to the file-worker parent.
//!
//! The request document arrives on stdin. This process hashes the named worker
//! executable, then calls [`ParentConfig::start`]. It pulls verified batches,
//! drops each record batch after counting lineage, and writes a summary only
//! after [`ParentPull::finish`] returns [`ParentParseComplete`]. It does not
//! call the in-process delimited or JSONL readers, and it does not mint an
//! import, a publication, or a kernel admission.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use oo_connectors::{
    BomPolicy, ChildRequest, DelimitedColumn, DelimitedDialect, DelimitedError, DelimitedProfile,
    DelimitedType, Failure, FailureKind, GroupSignal, HeaderPolicy, JsonlColumn, JsonlError,
    JsonlProfile, JsonlType, KernelRestriction, NullPolicy, ParentConfig, ParentError,
    ParentParseComplete, PullBudget, SourceProfile, VerifiedBatch, FILE_WORKER_PARENT_PROTOCOL,
    JSONL_PROTOCOL, KERNEL_RESTRICTION, DELIMITED_PROTOCOL,
};
use oo_core::parse_json_value;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const REQUEST_PROTOCOL: &str = "oo.connectors.parse-file/v1";
const RESULT_PROTOCOL: &str = "oo.connectors.parse-file-result/v1";
const ERROR_PROTOCOL: &str = "oo.connectors.parse-file-error/v1";
const LINEAGE_PREFIX: &str = "oo.connectors.parse-file-lineage/v1\n";
const REQUEST_MAX: usize = 1_048_576;
const EXECUTABLE_MAX_BYTES: u64 = 2_147_483_648;
const EXECUTABLE_HASH_MS: u64 = 120_000;
const PARENT_DEADLINE_MAX: u64 = 120_000;
const WORKER_DEADLINE_MAX: u64 = 3_600_000;
const READ_CHUNK: usize = 8192;

const CREDENTIAL_KEYS: &[&str] = &[
    "access_key",
    "api_key",
    "authorization",
    "connection_string",
    "credential",
    "credentials",
    "dsn",
    "passwd",
    "password",
    "private_key",
    "secret",
    "secret_key",
    "token",
];

const EXIT_USAGE: u8 = 2;
const EXIT_CONFIG: u8 = 3;
const EXIT_READ: u8 = 4;
const EXIT_PARENT: u8 = 5;
const EXIT_HASH: u8 = 6;
const EXIT_DEADLINE: u8 = 7;

struct Fail {
    outcome: &'static str,
    fact: &'static str,
    code: u8,
    worker_token: Option<&'static str>,
    reaped: bool,
    exit_code: Option<i32>,
    term_signal: Option<i32>,
    group_signal: &'static str,
}

impl Fail {
    const fn bare(outcome: &'static str, fact: &'static str, code: u8) -> Self {
        Self {
            outcome,
            fact,
            code,
            worker_token: None,
            reaped: false,
            exit_code: None,
            term_signal: None,
            group_signal: "not_needed",
        }
    }

    const fn usage(fact: &'static str) -> Self {
        Self::bare("usage", fact, EXIT_USAGE)
    }

    const fn config(fact: &'static str) -> Self {
        Self::bare("config", fact, EXIT_CONFIG)
    }

    const fn read(fact: &'static str) -> Self {
        Self::bare("read", fact, EXIT_READ)
    }

    const fn parent(fact: &'static str) -> Self {
        Self::bare("parent", fact, EXIT_PARENT)
    }

    const fn hash(fact: &'static str) -> Self {
        Self::bare("hash", fact, EXIT_HASH)
    }

    const fn deadline(fact: &'static str) -> Self {
        Self::bare("deadline", fact, EXIT_DEADLINE)
    }

    fn supervised(failure: Failure) -> Self {
        let (outcome, code) = match failure.kind {
            FailureKind::HashMismatch => ("hash", EXIT_HASH),
            FailureKind::TimedOut => ("deadline", EXIT_DEADLINE),
            _ => ("parent", EXIT_PARENT),
        };
        Self {
            outcome,
            fact: kind_fact(failure.kind),
            code,
            worker_token: failure.token,
            reaped: failure.teardown.reaped,
            exit_code: failure.teardown.exit_code,
            term_signal: failure.teardown.term_signal,
            group_signal: group_name(failure.teardown.group_signal),
        }
    }
}

struct Request {
    executable: String,
    executable_sha256: [u8; 32],
    input: String,
    input_sha256: [u8; 32],
    parent_deadline_ms: u64,
    profile: SourceProfile,
    budget: PullBudget,
    max_rows: u64,
}

struct Fold {
    rows: u64,
    fields: u64,
    value: u64,
    null: u64,
    empty: u64,
    missing: u64,
    first_line: u64,
    last_line: u64,
    first_offset: u64,
    last_offset: u64,
    raw_bytes: u64,
    arrow_bytes: u64,
    batches: u64,
    schema_identity: Option<[u8; 32]>,
    profile_digest: Option<[u8; 32]>,
    config_identity: Option<[u8; 32]>,
}

impl Fold {
    const fn new() -> Self {
        Self {
            rows: 0,
            fields: 0,
            value: 0,
            null: 0,
            empty: 0,
            missing: 0,
            first_line: 0,
            last_line: 0,
            first_offset: 0,
            last_offset: 0,
            raw_bytes: 0,
            arrow_bytes: 0,
            batches: 0,
            schema_identity: None,
            profile_digest: None,
            config_identity: None,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(document) => match io::stdout().write_all(document.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => {
                emit(&Fail::read("stdout"));
                ExitCode::from(EXIT_READ)
            }
        },
        Err(fail) => {
            emit(&fail);
            ExitCode::from(fail.code)
        }
    }
}

fn run() -> Result<String, Fail> {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    match args.next() {
        None => execute(),
        Some(flag) if flag == "--help" => {
            if args.next().is_some() {
                return Err(Fail::usage("unknown_flag"));
            }
            Ok(help_text())
        }
        Some(_) => Err(Fail::usage("unknown_flag")),
    }
}

fn help_text() -> String {
    format!(
        "\
parse-file reads one {REQUEST_PROTOCOL} document from stdin and writes one {RESULT_PROTOCOL} summary.

Usage:
  parse-file
  parse-file --help

The request is at most {REQUEST_MAX} bytes. It names CSV, TSV, or JSONL, the input path, the input SHA-256, the profile, the budget, the absolute worker executable, and that executable's SHA-256.
parent_deadline_ms is the file_worker_parent clock, from 1 to 120000 milliseconds. budget.deadline_ms is the worker's between-read clock. The two clocks are not added.
The command does not infer a dialect from the file bytes.
It hashes the executable, then calls ParentConfig::start. It pulls batches until a clean EOF and calls finish. ParentParseComplete is the only success source.
Stdout on success is one summary of counts and digests. It has no raw rows and no cell text.
admits_import, published, and pg_run_complete come from that receipt and are false. The command does not import, does not publish, and does not open a database.
kernel restriction is unsupported. That is not a containment certificate.
Exit 0 is success. Exit 2 is usage. Exit 3 is config. Exit 4 is read. Exit 5 is parent. Exit 6 is hash. Exit 7 is deadline.
Failures write one {ERROR_PROTOCOL} object to stderr and write no result to stdout.
"
    )
}

fn execute() -> Result<String, Fail> {
    let text = read_stdin()?;
    let value = parse_json_value(&text, REQUEST_PROTOCOL).map_err(|_| Fail::config("request_json"))?;
    reject_credentials(&value)?;
    let request = parse_request(&value)?;
    let worker = Path::new(&request.executable);
    if !(1..=PARENT_DEADLINE_MAX).contains(&request.parent_deadline_ms) {
        return Err(Fail::deadline("deadline"));
    }
    let config = ParentConfig::try_new(worker, request.parent_deadline_ms).map_err(from_parent)?;
    let executable_sha = hash_executable(worker)?;
    if executable_sha != request.executable_sha256 {
        return Err(Fail::hash("executable_sha256"));
    }
    let child = ChildRequest::try_new(
        Path::new(&request.input),
        request.input_sha256,
        request.profile.clone(),
        request.budget.clone(),
    )
    .map_err(from_parent)?;
    let mut pull = config.start(&child).map_err(from_parent)?;
    let mut fold = Fold::new();
    let jsonl = matches!(&request.profile, SourceProfile::Jsonl(_));
    while let Some(batch) = pull.next_batch().map_err(from_parent)? {
        absorb(&mut fold, batch, request.max_rows, jsonl)?;
    }
    let done = pull.finish().map_err(from_parent)?;
    summary(&request, &fold, &done)
}

fn read_stdin() -> Result<String, Fail> {
    let mut stdin = io::stdin();
    let mut buf = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK];
    loop {
        let count = loop {
            match stdin.read(&mut chunk) {
                Ok(count) => break count,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Fail::read("stdin")),
            }
        };
        if count == 0 {
            break;
        }
        let room = REQUEST_MAX.saturating_add(1).saturating_sub(buf.len());
        if count > room {
            return Err(Fail::config("request_bytes"));
        }
        buf.extend_from_slice(&chunk[..count]);
        if buf.len() > REQUEST_MAX {
            return Err(Fail::config("request_bytes"));
        }
    }
    String::from_utf8(buf).map_err(|_| Fail::config("request_json"))
}

fn hash_executable(path: &Path) -> Result<[u8; 32], Fail> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fail::read("executable_io"))?;
    if !meta.file_type().is_file() {
        return Err(Fail::config("worker_path"));
    }
    if meta.len() > EXECUTABLE_MAX_BYTES {
        return Err(Fail::config("executable_bytes"));
    }
    let mut file = File::open(path).map_err(|_| Fail::read("executable_io"))?;
    let deadline = Instant::now() + Duration::from_millis(EXECUTABLE_HASH_MS);
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; READ_CHUNK];
    let mut seen = 0_u64;
    loop {
        if Instant::now() >= deadline {
            return Err(Fail::deadline("deadline"));
        }
        let count = loop {
            match file.read(&mut buf) {
                Ok(count) => break count,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Fail::read("executable_io")),
            }
        };
        if count == 0 {
            break;
        }
        let width = u64::try_from(count).map_err(|_| Fail::read("executable_io"))?;
        seen = seen.saturating_add(width);
        if seen > EXECUTABLE_MAX_BYTES {
            return Err(Fail::config("executable_bytes"));
        }
        hasher.update(&buf[..count]);
    }
    if seen != meta.len() {
        return Err(Fail::read("executable_io"));
    }
    Ok(hasher.finalize().into())
}

fn absorb(fold: &mut Fold, batch: VerifiedBatch, max_rows: u64, jsonl: bool) -> Result<(), Fail> {
    if fold.batches >= max_rows {
        return Err(Fail::parent("protocol"));
    }
    if batch.batch_index != fold.batches {
        return Err(Fail::parent("protocol"));
    }
    let record_rows = batch.record.num_rows();
    if record_rows != batch.rows.len() {
        return Err(Fail::parent("protocol"));
    }
    if !batch.rows.is_empty() && (batch.raw_bytes == 0 || batch.arrow_bytes == 0) {
        return Err(Fail::parent("protocol"));
    }
    match fold.schema_identity {
        None => {
            fold.schema_identity = Some(batch.schema_identity);
            fold.profile_digest = Some(batch.profile_digest);
            fold.config_identity = Some(batch.config_identity);
        }
        Some(schema) => {
            if schema != batch.schema_identity
                || fold.profile_digest != Some(batch.profile_digest)
                || fold.config_identity != Some(batch.config_identity)
            {
                return Err(Fail::parent("receipt"));
            }
        }
    }
    fold.raw_bytes = fold
        .raw_bytes
        .checked_add(batch.raw_bytes)
        .ok_or(Fail::parent("protocol"))?;
    fold.arrow_bytes = fold
        .arrow_bytes
        .checked_add(batch.arrow_bytes)
        .ok_or(Fail::parent("protocol"))?;
    for row in &batch.rows {
        if fold.rows == 0 {
            fold.first_line = row.line;
            fold.first_offset = row.byte_offset;
        }
        bump(&mut fold.rows)?;
        fold.last_line = row.line;
        fold.last_offset = row.byte_offset;
        for field in &row.fields {
            bump(&mut fold.fields)?;
            match field.presence {
                "value" => bump(&mut fold.value)?,
                "null" => bump(&mut fold.null)?,
                "empty" if !jsonl => bump(&mut fold.empty)?,
                "missing" if jsonl => bump(&mut fold.missing)?,
                _ => return Err(Fail::parent("protocol")),
            }
        }
    }
    fold.batches += 1;
    Ok(())
}

fn summary(request: &Request, fold: &Fold, done: &ParentParseComplete) -> Result<String, Fail> {
    let kernel = match KERNEL_RESTRICTION {
        KernelRestriction::Unsupported => "unsupported",
    };
    if done.admits_import() || done.published() || done.pg_run_complete() {
        return Err(Fail::parent("admission"));
    }
    if done.arrow_ipc() != (done.batches() > 0) {
        return Err(Fail::parent("receipt"));
    }
    let view = profile_view(&request.profile, request.input_sha256)?;
    if done.reader_protocol() != view.reader {
        return Err(Fail::parent("protocol"));
    }
    if done.profile_id() != view.id
        || done.profile_digest() != view.digest
        || done.config_identity() != view.config
        || done.object_sha256() != request.input_sha256
        || done.columns() != view.columns
        || done.rows() != fold.rows
        || done.batches() != fold.batches
    {
        return Err(Fail::parent("receipt"));
    }
    let expect_fields = done
        .rows()
        .checked_mul(u64::from(done.columns()))
        .ok_or(Fail::parent("protocol"))?;
    if fold.fields != expect_fields {
        return Err(Fail::parent("receipt"));
    }
    if (view.jsonl && fold.empty != 0) || (!view.jsonl && fold.missing != 0) {
        return Err(Fail::parent("protocol"));
    }
    if done.rows() == 0
        && (fold.fields != 0
            || fold.value != 0
            || fold.null != 0
            || fold.empty != 0
            || fold.missing != 0
            || fold.first_line != 0
            || fold.last_line != 0
            || fold.first_offset != 0
            || fold.last_offset != 0
            || fold.raw_bytes != 0
            || fold.arrow_bytes != 0)
    {
        return Err(Fail::parent("receipt"));
    }
    if done.batches() > 0
        && (fold.schema_identity != Some(done.schema_identity())
            || fold.profile_digest != Some(done.profile_digest())
            || fold.config_identity != Some(done.config_identity()))
    {
        return Err(Fail::parent("receipt"));
    }
    let lineage_sha = sha256(lineage_preimage(fold).as_bytes());
    let body = serde_json::json!({
        "admits_import": done.admits_import(),
        "arrow_bytes": fold.arrow_bytes,
        "arrow_ipc": done.arrow_ipc(),
        "batches": done.batches(),
        "columns": done.columns(),
        "config_identity": hex_encode(&done.config_identity()),
        "kernel_restriction": kernel,
        "lineage": {
            "empty": fold.empty,
            "fields": fold.fields,
            "first_byte_offset": fold.first_offset,
            "first_line": fold.first_line,
            "last_byte_offset": fold.last_offset,
            "last_line": fold.last_line,
            "missing": fold.missing,
            "null": fold.null,
            "rows": fold.rows,
            "value": fold.value,
        },
        "lineage_sha256": hex_encode(&lineage_sha),
        "object_sha256": hex_encode(&done.object_sha256()),
        "parent_protocol": FILE_WORKER_PARENT_PROTOCOL,
        "pg_run_complete": done.pg_run_complete(),
        "profile_digest": hex_encode(&done.profile_digest()),
        "profile_id": done.profile_id(),
        "protocol": RESULT_PROTOCOL,
        "published": done.published(),
        "raw_bytes": fold.raw_bytes,
        "reader_protocol": done.reader_protocol(),
        "rows": done.rows(),
        "schema_identity": hex_encode(&done.schema_identity()),
        "types": view.types,
    });
    let mut text = serde_json::to_string(&body).map_err(|_| Fail::read("stdout"))?;
    text.push('\n');
    Ok(text)
}

struct ProfileView {
    id: String,
    digest: [u8; 32],
    config: [u8; 32],
    columns: u32,
    reader: &'static str,
    types: Vec<Value>,
    jsonl: bool,
}

fn profile_view(profile: &SourceProfile, object_sha256: [u8; 32]) -> Result<ProfileView, Fail> {
    match profile {
        SourceProfile::Jsonl(profile) => {
            let columns = u32::try_from(profile.columns().len()).map_err(|_| Fail::config("column_count"))?;
            Ok(ProfileView {
                id: profile.id().to_owned(),
                digest: profile.profile_digest(),
                config: profile.config_identity(object_sha256),
                columns,
                reader: JSONL_PROTOCOL,
                types: profile.columns().iter().map(jsonl_type).collect(),
                jsonl: true,
            })
        }
        SourceProfile::Delimited(profile) => {
            let columns = u32::try_from(profile.columns().len()).map_err(|_| Fail::config("column_count"))?;
            Ok(ProfileView {
                id: profile.id().to_owned(),
                digest: profile.profile_digest(),
                config: profile.config_identity(object_sha256),
                columns,
                reader: DELIMITED_PROTOCOL,
                types: profile.columns().iter().map(delimited_type).collect(),
                jsonl: false,
            })
        }
    }
}

fn delimited_type(column: &DelimitedColumn) -> Value {
    match column.ty() {
        DelimitedType::Decimal { precision, scale } => serde_json::json!({
            "name": column.name(),
            "nullable": column.nullable(),
            "precision": precision,
            "scale": scale,
            "type": "decimal",
        }),
        other => serde_json::json!({
            "name": column.name(),
            "nullable": column.nullable(),
            "type": other.as_str(),
        }),
    }
}

fn jsonl_type(column: &JsonlColumn) -> Value {
    serde_json::json!({
        "name": column.name(),
        "nullable": column.nullable(),
        "type": column.ty().as_str(),
    })
}

fn lineage_preimage(fold: &Fold) -> String {
    format!(
        "\
{LINEAGE_PREFIX}\
rows={}
fields={}
value={}
null={}
empty={}
missing={}
first_line={}
last_line={}
first_byte_offset={}
last_byte_offset={}
raw_bytes={}
arrow_bytes={}
",
        fold.rows,
        fold.fields,
        fold.value,
        fold.null,
        fold.empty,
        fold.missing,
        fold.first_line,
        fold.last_line,
        fold.first_offset,
        fold.last_offset,
        fold.raw_bytes,
        fold.arrow_bytes,
    )
}

fn parse_request(value: &Value) -> Result<Request, Fail> {
    let root = object(value)?;
    exact_keys(
        root,
        &[
            "budget",
            "executable",
            "format",
            "input",
            "parent_deadline_ms",
            "profile",
            "protocol",
        ],
    )?;
    if require_str(root, "protocol")? != REQUEST_PROTOCOL {
        return Err(Fail::config("version"));
    }
    let parent_deadline_ms = parent_deadline(root)?;
    let executable = object(require(root, "executable")?)?;
    exact_keys(executable, &["path", "sha256"])?;
    let input = object(require(root, "input")?)?;
    exact_keys(input, &["path", "sha256"])?;
    let format = require_str(root, "format")?;
    let profile_value = require(root, "profile")?;
    let budget_value = require(root, "budget")?;
    let (profile, budget, max_rows) = match format {
        "csv" | "tsv" => delimited_body(format, profile_value, budget_value)?,
        "jsonl" => jsonl_body(profile_value, budget_value)?,
        _ => return Err(Fail::config("format")),
    };
    Ok(Request {
        executable: require_str(executable, "path")?.to_owned(),
        executable_sha256: decode_sha256(require_str(executable, "sha256")?, "executable_sha256")?,
        input: require_str(input, "path")?.to_owned(),
        input_sha256: decode_sha256(require_str(input, "sha256")?, "input_sha256")?,
        parent_deadline_ms,
        profile,
        budget,
        max_rows,
    })
}

fn delimited_body(
    format: &str,
    profile_value: &Value,
    budget_value: &Value,
) -> Result<(SourceProfile, PullBudget, u64), Fail> {
    let map = object(profile_value)?;
    exact_keys(map, &["bom", "columns", "dialect", "header", "id", "null"])?;
    let dialect_text = require_str(map, "dialect")?;
    let separator = match (format, dialect_text) {
        ("csv", "csv") => b',',
        ("tsv", "tsv") => b'\t',
        _ => return Err(Fail::config("dialect")),
    };
    let dialect = DelimitedDialect::try_from_separator(separator).map_err(from_delimited)?;
    let bom = bom_policy(require_str(map, "bom")?)?;
    let header = header_policy(require_str(map, "header")?)?;
    let nulls = null_policy(require(map, "null")?)?;
    let columns = delimited_columns(require(map, "columns")?)?;
    let profile = DelimitedProfile::try_new(require_str(map, "id")?, dialect, bom, header, nulls, columns)
        .map_err(from_delimited)?;
    let (budget, max_rows) = delimited_budget(budget_value)?;
    Ok((SourceProfile::Delimited(profile), budget, max_rows))
}

fn jsonl_body(profile_value: &Value, budget_value: &Value) -> Result<(SourceProfile, PullBudget, u64), Fail> {
    let map = object(profile_value)?;
    exact_keys(map, &["columns", "id"])?;
    let columns = jsonl_columns(require(map, "columns")?)?;
    let profile = JsonlProfile::try_new(require_str(map, "id")?, columns).map_err(from_jsonl)?;
    let (budget, max_rows) = jsonl_budget(budget_value)?;
    Ok((SourceProfile::Jsonl(profile), budget, max_rows))
}

fn delimited_columns(value: &Value) -> Result<Vec<DelimitedColumn>, Fail> {
    let items = value.as_array().ok_or(Fail::config("columns"))?;
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let map = object(item)?;
        let ty_text = require_str(map, "type")?;
        let nullable = require_bool(map, "nullable")?;
        let ty = match ty_text {
            "utf8" => {
                exact_keys(map, &["name", "nullable", "type"])?;
                DelimitedType::Utf8
            }
            "bool" => {
                exact_keys(map, &["name", "nullable", "type"])?;
                DelimitedType::Bool
            }
            "int64" => {
                exact_keys(map, &["name", "nullable", "type"])?;
                DelimitedType::Int64
            }
            "decimal" => {
                exact_keys(map, &["name", "nullable", "precision", "scale", "type"])?;
                DelimitedType::Decimal {
                    precision: require_u8(map, "precision", "decimal_precision")?,
                    scale: require_i8(map, "scale", "decimal_scale")?,
                }
            }
            _ => return Err(Fail::config("type")),
        };
        columns.push(DelimitedColumn::try_new(require_str(map, "name")?, ty, nullable).map_err(from_delimited)?);
    }
    Ok(columns)
}

fn jsonl_columns(value: &Value) -> Result<Vec<JsonlColumn>, Fail> {
    let items = value.as_array().ok_or(Fail::config("columns"))?;
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let map = object(item)?;
        exact_keys(map, &["name", "nullable", "type"])?;
        let ty = match require_str(map, "type")? {
            "utf8" => JsonlType::Utf8,
            "bool" => JsonlType::Bool,
            "int64" => JsonlType::Int64,
            "json_text" => JsonlType::JsonText,
            _ => return Err(Fail::config("type")),
        };
        columns.push(
            JsonlColumn::try_new(require_str(map, "name")?, ty, require_bool(map, "nullable")?)
                .map_err(from_jsonl)?,
        );
    }
    Ok(columns)
}

fn delimited_budget(value: &Value) -> Result<(PullBudget, u64), Fail> {
    let map = object(value)?;
    exact_keys(
        map,
        &[
            "batch_bytes",
            "batch_rows",
            "deadline_ms",
            "max_columns",
            "max_field_bytes",
            "max_fields",
            "max_record_bytes",
            "max_rows",
            "max_total_bytes",
        ],
    )?;
    let worker_deadline = worker_deadline(map)?;
    let max_rows = require_u64(map, "max_rows", "budget")?;
    let budget = PullBudget::delimited(
        require_u32(map, "batch_rows", "budget")?,
        require_u64(map, "batch_bytes", "budget")?,
        require_u32(map, "max_record_bytes", "budget")?,
        require_u32(map, "max_field_bytes", "budget")?,
        require_u64(map, "max_total_bytes", "budget")?,
        max_rows,
        require_u32(map, "max_columns", "budget")?,
        require_u32(map, "max_fields", "budget")?,
        worker_deadline,
    )
    .map_err(from_parent)?;
    Ok((budget, max_rows))
}

fn jsonl_budget(value: &Value) -> Result<(PullBudget, u64), Fail> {
    let map = object(value)?;
    exact_keys(
        map,
        &[
            "batch_bytes",
            "batch_rows",
            "deadline_ms",
            "max_columns",
            "max_fields",
            "max_record_bytes",
            "max_rows",
            "max_total_bytes",
        ],
    )?;
    let worker_deadline = worker_deadline(map)?;
    let max_rows = require_u64(map, "max_rows", "budget")?;
    let budget = PullBudget::jsonl(
        require_u32(map, "batch_rows", "budget")?,
        require_u64(map, "batch_bytes", "budget")?,
        require_u32(map, "max_record_bytes", "budget")?,
        require_u64(map, "max_total_bytes", "budget")?,
        max_rows,
        require_u32(map, "max_columns", "budget")?,
        require_u32(map, "max_fields", "budget")?,
        worker_deadline,
    )
    .map_err(from_parent)?;
    Ok((budget, max_rows))
}

fn worker_deadline(map: &Map<String, Value>) -> Result<u64, Fail> {
    let millis = require_u64(map, "deadline_ms", "worker_deadline")?;
    if millis == 0 || millis > WORKER_DEADLINE_MAX {
        return Err(Fail::config("worker_deadline"));
    }
    Ok(millis)
}

fn parent_deadline(map: &Map<String, Value>) -> Result<u64, Fail> {
    let value = require(map, "parent_deadline_ms")?;
    if !integer_number(value) {
        return Err(Fail::config("deadline"));
    }
    match value.as_u64() {
        Some(millis) if (1..=PARENT_DEADLINE_MAX).contains(&millis) => Ok(millis),
        Some(_) | None => Err(Fail::deadline("deadline")),
    }
}

fn bump(slot: &mut u64) -> Result<(), Fail> {
    *slot = slot.checked_add(1).ok_or(Fail::parent("protocol"))?;
    Ok(())
}

fn bom_policy(text: &str) -> Result<BomPolicy, Fail> {
    match text {
        "refuse" => Ok(BomPolicy::Refuse),
        "strip" => Ok(BomPolicy::Strip),
        "require" => Ok(BomPolicy::Require),
        _ => Err(Fail::config("bom")),
    }
}

fn header_policy(text: &str) -> Result<HeaderPolicy, Fail> {
    match text {
        "absent" => Ok(HeaderPolicy::Absent),
        "exact" => Ok(HeaderPolicy::Exact),
        _ => Err(Fail::config("header")),
    }
}

fn null_policy(value: &Value) -> Result<NullPolicy, Fail> {
    let map = object(value)?;
    match require_str(map, "policy")? {
        "empty" => {
            exact_keys(map, &["policy"])?;
            Ok(NullPolicy::Empty)
        }
        "token" => {
            exact_keys(map, &["policy", "text"])?;
            Ok(NullPolicy::Token(require_str(map, "text")?.to_owned()))
        }
        _ => Err(Fail::config("null")),
    }
}

fn reject_credentials(value: &Value) -> Result<(), Fail> {
    match value {
        Value::Object(map) => {
            for key in map.keys() {
                if CREDENTIAL_KEYS.contains(&key.as_str()) {
                    return Err(Fail::config("credentials"));
                }
            }
            for child in map.values() {
                reject_credentials(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_credentials(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn from_parent(err: ParentError) -> Fail {
    match err {
        ParentError::Config(token) => match token {
            "deadline" => Fail::deadline("deadline"),
            "input_io" | "not_regular" | "input_changed" => Fail::read(token),
            "spawn" => Fail::parent("spawn"),
            "worker_path" | "budget" | "worker_deadline" | "path_form" | "profile_budget"
            | "input_too_large" => Fail::config(token),
            _ => Fail::config("parent_config"),
        },
        ParentError::Failed(failure) => Fail::supervised(failure),
    }
}

fn from_delimited(err: DelimitedError) -> Fail {
    Fail::config(err.fact())
}

fn from_jsonl(err: JsonlError) -> Fail {
    Fail::config(err.fact())
}

fn kind_fact(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::Spawn => "spawn",
        FailureKind::TimedOut => "timed_out",
        FailureKind::Malformed => "malformed",
        FailureKind::Trailer => "trailer",
        FailureKind::HashMismatch => "hash_mismatch",
        FailureKind::Protocol => "protocol",
        FailureKind::Ended => "ended",
        FailureKind::Worker => "worker",
        FailureKind::Io => "io",
    }
}

fn group_name(signal: GroupSignal) -> &'static str {
    match signal {
        GroupSignal::NotNeeded => "not_needed",
        GroupSignal::Sent => "sent",
        GroupSignal::SkippedIdentity => "skipped_identity",
        GroupSignal::Rejected => "rejected",
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, Fail> {
    value.as_object().ok_or(Fail::config("request_json"))
}

fn exact_keys(map: &Map<String, Value>, keys: &[&str]) -> Result<(), Fail> {
    for key in map.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(Fail::config("unknown_field"));
        }
    }
    for key in keys {
        if !map.contains_key(*key) {
            return Err(Fail::config("request_field"));
        }
    }
    Ok(())
}

fn require<'a>(map: &'a Map<String, Value>, key: &'static str) -> Result<&'a Value, Fail> {
    map.get(key).ok_or(Fail::config("request_field"))
}

fn require_str<'a>(map: &'a Map<String, Value>, key: &'static str) -> Result<&'a str, Fail> {
    require(map, key)?.as_str().ok_or(Fail::config("request_field"))
}

fn require_bool(map: &Map<String, Value>, key: &'static str) -> Result<bool, Fail> {
    require(map, key)?.as_bool().ok_or(Fail::config("request_field"))
}

fn require_u64(map: &Map<String, Value>, key: &'static str, fact: &'static str) -> Result<u64, Fail> {
    let value = require(map, key)?;
    if !integer_number(value) {
        return Err(Fail::config(fact));
    }
    value.as_u64().ok_or(Fail::config(fact))
}

fn require_u32(map: &Map<String, Value>, key: &'static str, fact: &'static str) -> Result<u32, Fail> {
    u32::try_from(require_u64(map, key, fact)?).map_err(|_| Fail::config(fact))
}

fn require_u8(map: &Map<String, Value>, key: &'static str, fact: &'static str) -> Result<u8, Fail> {
    u8::try_from(require_u64(map, key, fact)?).map_err(|_| Fail::config(fact))
}

fn require_i8(map: &Map<String, Value>, key: &'static str, fact: &'static str) -> Result<i8, Fail> {
    let value = require(map, key)?;
    if !integer_number(value) {
        return Err(Fail::config(fact));
    }
    let number = value.as_i64().ok_or(Fail::config(fact))?;
    i8::try_from(number).map_err(|_| Fail::config(fact))
}

fn integer_number(value: &Value) -> bool {
    let Some(text) = value.as_number().map(ToString::to_string) else {
        return false;
    };
    let bytes = text.as_bytes();
    let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);
    !digits.is_empty()
        && digits.iter().all(|byte| byte.is_ascii_digit())
        && !bytes.iter().any(|byte| matches!(byte, b'.' | b'e' | b'E' | b'+'))
}

fn decode_sha256(text: &str, fact: &'static str) -> Result<[u8; 32], Fail> {
    if text.len() != 64 || !text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(Fail::config(fact));
    }
    let mut out = [0_u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 2;
        *slot = u8::from_str_radix(&text[start..start + 2], 16).map_err(|_| Fail::config(fact))?;
    }
    Ok(out)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn emit(fail: &Fail) {
    let body = serde_json::json!({
        "exit_code": fail.exit_code,
        "fact": fail.fact,
        "group_signal": fail.group_signal,
        "outcome": fail.outcome,
        "protocol": ERROR_PROTOCOL,
        "reaped": fail.reaped,
        "term_signal": fail.term_signal,
        "worker_token": fail.worker_token,
    });
    let text = serde_json::to_string(&body).unwrap_or_else(|_| {
        "{\"exit_code\":null,\"fact\":\"stderr\",\"group_signal\":\"not_needed\",\"outcome\":\"read\",\"protocol\":\"oo.connectors.parse-file-error/v1\",\"reaped\":false,\"term_signal\":null,\"worker_token\":null}".to_owned()
    });
    let mut line = text;
    line.push('\n');
    let _ = io::stderr().write_all(line.as_bytes());
}
