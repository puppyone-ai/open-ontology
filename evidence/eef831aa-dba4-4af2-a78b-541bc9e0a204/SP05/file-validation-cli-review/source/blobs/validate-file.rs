//! `validate-file` checks one local CSV, TSV, or JSONL file against an explicit profile.
//!
//! The request names the format, dialect, types, BOM policy, header policy, null
//! policy, budgets, expected SHA-256, and input path. This command does not infer
//! a dialect from the bytes. It builds the reader with the public constructors,
//! pulls a real [`std::fs::File`] until a clean EOF, and prints a summary only
//! after `finish` returns a receipt.
//!
//! A finished parse is not an import and not a publication. The command does not
//! open a database, write Arrow IPC, or claim a worker or an isolation boundary.
//! Cancel and the deadline are checked before each blocking `read`, not during it.

#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use oo_connectors::{
    BomPolicy, DelimitedBudget, DelimitedColumn, DelimitedComplete, DelimitedDialect,
    DelimitedError, DelimitedErrorKind, DelimitedFieldPresence, DelimitedHeader, DelimitedItem,
    DelimitedProfile, DelimitedReader, DelimitedRowLineage, DelimitedType, FieldPresence,
    HeaderPolicy, JsonlBudget, JsonlColumn, JsonlComplete, JsonlError, JsonlErrorKind, JsonlHeader,
    JsonlItem, JsonlProfile, JsonlReader, JsonlType, NullPolicy, RowLineage,
};
use oo_core::parse_json_value;
use serde_json::{Map, Value};

const REQUEST_PROTOCOL: &str = "oo.pipeline.validate-file/v1";
const RESULT_PROTOCOL: &str = "oo.pipeline.validate-file-result/v1";
const ERROR_PROTOCOL: &str = "oo.pipeline.validate-file-error/v1";
const REQUEST_MAX_BYTES: u64 = 1_048_576;
const MAX_DEADLINE_MS: u64 = 120_000;

const EXIT_USAGE: u8 = 2;
const EXIT_CONFIG: u8 = 3;
const EXIT_READ: u8 = 4;
const EXIT_PARSE: u8 = 5;
const EXIT_BUDGET: u8 = 6;
const EXIT_HASH: u8 = 7;
const EXIT_CANCEL: u8 = 8;

struct Fail {
    outcome: &'static str,
    fact: &'static str,
    code: u8,
}

impl Fail {
    const fn usage(fact: &'static str) -> Self {
        Self {
            outcome: "usage",
            fact,
            code: EXIT_USAGE,
        }
    }

    const fn config(fact: &'static str) -> Self {
        Self {
            outcome: "config",
            fact,
            code: EXIT_CONFIG,
        }
    }

    const fn read(fact: &'static str) -> Self {
        Self {
            outcome: "read",
            fact,
            code: EXIT_READ,
        }
    }

    const fn parse(fact: &'static str) -> Self {
        Self {
            outcome: "parse",
            fact,
            code: EXIT_PARSE,
        }
    }

    const fn budget(fact: &'static str) -> Self {
        Self {
            outcome: "budget",
            fact,
            code: EXIT_BUDGET,
        }
    }

    const fn hash(fact: &'static str) -> Self {
        Self {
            outcome: "hash",
            fact,
            code: EXIT_HASH,
        }
    }

    const fn cancel(fact: &'static str) -> Self {
        Self {
            outcome: "cancel",
            fact,
            code: EXIT_CANCEL,
        }
    }
}

enum Body {
    Delimited {
        profile: DelimitedProfile,
        budget: DelimitedBudget,
    },
    Jsonl {
        profile: JsonlProfile,
        budget: JsonlBudget,
    },
}

struct Request {
    input_path: String,
    expected: [u8; 32],
    cancel_before_read: bool,
    body: Body,
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
        }
    }

    fn row(&mut self, line: u64, offset: u64) {
        if self.rows == 0 {
            self.first_line = line;
            self.first_offset = offset;
        }
        self.rows += 1;
        self.last_line = line;
        self.last_offset = offset;
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
    let mut args = env::args_os();
    let _argv0 = args.next();
    let Some(first) = args.next() else {
        return Err(Fail::usage("usage"));
    };
    if first == OsStr::new("--help") {
        if args.next().is_some() {
            return Err(Fail::usage("unknown_flag"));
        }
        return Ok(help_text());
    }
    if first != OsStr::new("--request") {
        return Err(Fail::usage("unknown_flag"));
    }
    let Some(path) = args.next() else {
        return Err(Fail::usage("usage"));
    };
    if args.next().is_some() {
        return Err(Fail::usage("unknown_flag"));
    }
    let Some(path) = path.to_str() else {
        return Err(Fail::config("request_path"));
    };
    execute(path)
}

fn help_text() -> String {
    format!(
        "\
validate-file checks one local CSV, TSV, or JSONL file against an explicit profile.

Usage:
  validate-file --request <path>
  validate-file --help

The request file is one JSON document, protocol {REQUEST_PROTOCOL}, at most {REQUEST_MAX_BYTES} bytes.
It names the format, the input path, the expected SHA-256, the profile, and the budgets.
CSV and TSV also name the dialect, the BOM policy, the header policy, and the null policy.
The command does not infer a dialect from the file bytes.
It opens the named local file, pulls bounded reads until a clean EOF, and calls finish.
Stdout on success is one {RESULT_PROTOCOL} summary with no row payloads.
admits_import and published are false. The command does not import rows, publish a table, open a database, or write Arrow IPC.
A blocking read is not interrupted. Cancel is checked before the next read, not during it.
The command does not claim a worker or an isolation boundary, and it does not authenticate a public caller.
Exit 0 is success. Exit 2 is usage. Exit 3 is config. Exit 4 is read. Exit 5 is parse. Exit 6 is budget. Exit 7 is hash. Exit 8 is cancel.
Failures write one {ERROR_PROTOCOL} object to stderr and write no result to stdout.
"
    )
}

fn execute(request_path: &str) -> Result<String, Fail> {
    let text = read_request(request_path)?;
    let value =
        parse_json_value(&text, REQUEST_PROTOCOL).map_err(|_| Fail::config("request_json"))?;
    let request = parse_request(&value)?;
    let file = open_input(&request.input_path)?;
    match request.body {
        Body::Delimited { profile, budget } => run_delimited(
            file,
            profile,
            request.expected,
            budget,
            request.cancel_before_read,
        ),
        Body::Jsonl { profile, budget } => run_jsonl(
            file,
            profile,
            request.expected,
            budget,
            request.cancel_before_read,
        ),
    }
}

/// Size is checked before the body buffer is filled. A longer file is refused
/// without allocating a buffer of that length.
fn read_request(path: &str) -> Result<String, Fail> {
    if path.is_empty() || path.contains('\0') {
        return Err(Fail::config("request_path"));
    }
    let file = File::open(path).map_err(|_| Fail::config("request_open"))?;
    let meta = file.metadata().map_err(|_| Fail::config("request_open"))?;
    if !meta.is_file() {
        return Err(Fail::config("request_file"));
    }
    if meta.len() > REQUEST_MAX_BYTES {
        return Err(Fail::config("request_bytes"));
    }
    let mut buf = Vec::new();
    file.take(REQUEST_MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| Fail::config("request_open"))?;
    if buf.len() as u64 > REQUEST_MAX_BYTES {
        return Err(Fail::config("request_bytes"));
    }
    String::from_utf8(buf).map_err(|_| Fail::config("request_json"))
}

fn open_input(path: &str) -> Result<File, Fail> {
    if path.is_empty() || path.contains('\0') {
        return Err(Fail::config("input_path"));
    }
    let file = File::open(path).map_err(|_| Fail::read("open"))?;
    let meta = file.metadata().map_err(|_| Fail::read("open"))?;
    if !meta.is_file() {
        return Err(Fail::read("not_file"));
    }
    Ok(file)
}

fn parse_request(value: &Value) -> Result<Request, Fail> {
    let map = object(value)?;
    exact_keys(
        map,
        &[
            "budget",
            "cancel_before_read",
            "expected_sha256",
            "format",
            "input_path",
            "profile",
            "protocol",
        ],
    )?;
    if require_str(map, "protocol")? != REQUEST_PROTOCOL {
        return Err(Fail::config("version"));
    }
    let format = require_str(map, "format")?;
    let input_path = require_str(map, "input_path")?.to_owned();
    let expected = decode_sha256(require_str(map, "expected_sha256")?)?;
    let cancel_before_read = require_bool(map, "cancel_before_read")?;
    let profile = object(require(map, "profile")?)?;
    let budget = object(require(map, "budget")?)?;
    let body = match format {
        "csv" => Body::Delimited {
            profile: delimited_profile(profile, b',')?,
            budget: delimited_budget(budget)?,
        },
        "tsv" => Body::Delimited {
            profile: delimited_profile(profile, b'\t')?,
            budget: delimited_budget(budget)?,
        },
        "jsonl" => Body::Jsonl {
            profile: jsonl_profile(profile)?,
            budget: jsonl_budget(budget)?,
        },
        _ => return Err(Fail::config("format")),
    };
    Ok(Request {
        input_path,
        expected,
        cancel_before_read,
        body,
    })
}

fn delimited_profile(map: &Map<String, Value>, separator: u8) -> Result<DelimitedProfile, Fail> {
    exact_keys(map, &["bom", "columns", "dialect", "header", "id", "null"])?;
    let dialect = DelimitedDialect::try_from_separator(separator).map_err(from_delimited)?;
    if require_str(map, "dialect")? != dialect.as_str() {
        return Err(Fail::config("dialect"));
    }
    let columns = delimited_columns(require(map, "columns")?)?;
    DelimitedProfile::try_new(
        require_str(map, "id")?,
        dialect,
        bom_policy(require_str(map, "bom")?)?,
        header_policy(require_str(map, "header")?)?,
        null_policy(object(require(map, "null")?)?)?,
        columns,
    )
    .map_err(from_delimited)
}

fn delimited_columns(value: &Value) -> Result<Vec<DelimitedColumn>, Fail> {
    let items = value.as_array().ok_or(Fail::config("columns"))?;
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let map = object(item)?;
        let ty_name = require_str(map, "type")?;
        let column = if ty_name == "decimal" {
            exact_keys(map, &["name", "nullable", "precision", "scale", "type"])?;
            let precision = require_u8(map, "precision", "decimal_precision")?;
            let scale = require_i8(map, "scale", "decimal_scale")?;
            DelimitedColumn::try_new(
                require_str(map, "name")?,
                DelimitedType::Decimal { precision, scale },
                require_bool(map, "nullable")?,
            )
        } else {
            exact_keys(map, &["name", "nullable", "type"])?;
            let ty = match ty_name {
                "utf8" => DelimitedType::Utf8,
                "bool" => DelimitedType::Bool,
                "int64" => DelimitedType::Int64,
                _ => return Err(Fail::config("type")),
            };
            DelimitedColumn::try_new(
                require_str(map, "name")?,
                ty,
                require_bool(map, "nullable")?,
            )
        };
        columns.push(column.map_err(from_delimited)?);
    }
    Ok(columns)
}

fn null_policy(map: &Map<String, Value>) -> Result<NullPolicy, Fail> {
    let policy = require_str(map, "policy")?;
    match policy {
        "empty" => {
            exact_keys(map, &["policy"])?;
            Ok(NullPolicy::Empty)
        }
        "token" => {
            exact_keys(map, &["policy", "token"])?;
            Ok(NullPolicy::Token(require_str(map, "token")?.to_owned()))
        }
        _ => Err(Fail::config("null")),
    }
}

fn delimited_budget(map: &Map<String, Value>) -> Result<DelimitedBudget, Fail> {
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
    DelimitedBudget::try_new(
        require_u32(map, "batch_rows", "batch_rows")?,
        require_u64(map, "batch_bytes", "batch_bytes")?,
        require_u32(map, "max_record_bytes", "max_record")?,
        require_u32(map, "max_field_bytes", "max_field")?,
        require_u64(map, "max_total_bytes", "max_total")?,
        require_u64(map, "max_rows", "max_rows")?,
        require_u32(map, "max_columns", "max_columns")?,
        require_u32(map, "max_fields", "max_fields")?,
        deadline(map)?,
    )
    .map_err(from_delimited)
}

fn jsonl_profile(map: &Map<String, Value>) -> Result<JsonlProfile, Fail> {
    exact_keys(map, &["columns", "id"])?;
    JsonlProfile::try_new(
        require_str(map, "id")?,
        jsonl_columns(require(map, "columns")?)?,
    )
    .map_err(from_jsonl)
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
            JsonlColumn::try_new(
                require_str(map, "name")?,
                ty,
                require_bool(map, "nullable")?,
            )
            .map_err(from_jsonl)?,
        );
    }
    Ok(columns)
}

fn jsonl_budget(map: &Map<String, Value>) -> Result<JsonlBudget, Fail> {
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
    JsonlBudget::try_new(
        require_u32(map, "batch_rows", "batch_rows")?,
        require_u64(map, "batch_bytes", "batch_bytes")?,
        require_u32(map, "max_record_bytes", "max_record")?,
        require_u64(map, "max_total_bytes", "max_total")?,
        require_u64(map, "max_rows", "max_rows")?,
        require_u32(map, "max_columns", "max_columns")?,
        require_u32(map, "max_fields", "max_fields")?,
        deadline(map)?,
    )
    .map_err(from_jsonl)
}

fn deadline(map: &Map<String, Value>) -> Result<Instant, Fail> {
    let millis = require_u64(map, "deadline_ms", "deadline")?;
    if !(1..=MAX_DEADLINE_MS).contains(&millis) {
        return Err(Fail::config("deadline"));
    }
    Ok(Instant::now() + Duration::from_millis(millis))
}

fn run_delimited(
    file: File,
    profile: DelimitedProfile,
    expected: [u8; 32],
    budget: DelimitedBudget,
    cancel_before_read: bool,
) -> Result<String, Fail> {
    let mut reader =
        DelimitedReader::open(file, profile, expected, budget).map_err(from_delimited)?;
    let header = reader.header();
    closed_delimited(&header)?;
    if cancel_before_read {
        reader.cancel_handle().request();
    }
    let mut fold = Fold::new();
    let mut batches = 0_u64;
    loop {
        match reader.next_batch().map_err(from_delimited)? {
            DelimitedItem::Batch(batch) => {
                if batch.resumable {
                    return Err(Fail::parse("resumable"));
                }
                closed_delimited(&batch.header)?;
                batches += 1;
                for row in &batch.lineage {
                    fold_delimited(&mut fold, row);
                }
            }
            DelimitedItem::Exhausted(done) => {
                let complete = reader.finish().map_err(from_delimited)?;
                if complete.admits_import || complete.published {
                    return Err(Fail::parse("closed_flag"));
                }
                if complete.object_sha256 != expected {
                    return Err(Fail::hash("hash_mismatch"));
                }
                if complete.rows != done.rows
                    || complete.batches != done.batches
                    || complete.batches != batches
                    || complete.rows != fold.rows
                    || complete.object_sha256 != done.object_sha256
                    || complete.profile_id != header.profile_id
                    || complete.profile_digest != header.profile_digest
                    || complete.config_identity != header.config_identity
                    || complete.protocol != header.protocol
                {
                    return Err(Fail::parse("receipt"));
                }
                return render_delimited(&header, &complete, &fold);
            }
        }
    }
}

fn fold_delimited(fold: &mut Fold, row: &DelimitedRowLineage) {
    fold.row(row.line, row.byte_offset);
    for field in &row.fields {
        fold.fields += 1;
        match field.presence {
            DelimitedFieldPresence::Value => fold.value += 1,
            DelimitedFieldPresence::Null => fold.null += 1,
            DelimitedFieldPresence::Empty => fold.empty += 1,
        }
    }
}

fn closed_delimited(header: &DelimitedHeader) -> Result<(), Fail> {
    if header.admits_import || header.published || header.arrow_ipc || header.executes_native {
        return Err(Fail::parse("closed_flag"));
    }
    Ok(())
}

fn run_jsonl(
    file: File,
    profile: JsonlProfile,
    expected: [u8; 32],
    budget: JsonlBudget,
    cancel_before_read: bool,
) -> Result<String, Fail> {
    let mut reader = JsonlReader::open(file, profile, expected, budget).map_err(from_jsonl)?;
    let header = reader.header();
    closed_jsonl(&header)?;
    if cancel_before_read {
        reader.cancel_handle().request();
    }
    let mut fold = Fold::new();
    let mut batches = 0_u64;
    loop {
        match reader.next_batch().map_err(from_jsonl)? {
            JsonlItem::Batch(batch) => {
                if batch.resumable {
                    return Err(Fail::parse("resumable"));
                }
                closed_jsonl(&batch.header)?;
                batches += 1;
                for row in &batch.lineage {
                    fold_jsonl(&mut fold, row);
                }
            }
            JsonlItem::Exhausted(done) => {
                let complete = reader.finish().map_err(from_jsonl)?;
                if complete.admits_import || complete.published {
                    return Err(Fail::parse("closed_flag"));
                }
                if complete.object_sha256 != expected {
                    return Err(Fail::hash("hash_mismatch"));
                }
                if complete.rows != done.rows
                    || complete.batches != done.batches
                    || complete.batches != batches
                    || complete.rows != fold.rows
                    || complete.object_sha256 != done.object_sha256
                    || complete.profile_id != header.profile_id
                    || complete.profile_digest != header.profile_digest
                    || complete.config_identity != header.config_identity
                    || complete.protocol != header.protocol
                {
                    return Err(Fail::parse("receipt"));
                }
                return render_jsonl(&header, &complete, &fold);
            }
        }
    }
}

fn fold_jsonl(fold: &mut Fold, row: &RowLineage) {
    fold.row(row.line, row.byte_offset);
    for field in &row.fields {
        fold.fields += 1;
        match field.presence {
            FieldPresence::Value => fold.value += 1,
            FieldPresence::Null => fold.null += 1,
            FieldPresence::Missing => fold.missing += 1,
        }
    }
}

fn closed_jsonl(header: &JsonlHeader) -> Result<(), Fail> {
    if header.admits_import || header.published || header.arrow_ipc || header.executes_native {
        return Err(Fail::parse("closed_flag"));
    }
    Ok(())
}

fn render_delimited(
    header: &DelimitedHeader,
    complete: &DelimitedComplete,
    fold: &Fold,
) -> Result<String, Fail> {
    if header.columns.len() != header.types.len()
        || header.columns.len() != header.nullability.len()
    {
        return Err(Fail::parse("receipt"));
    }
    let mut types = Vec::with_capacity(header.columns.len());
    for ((name, ty), nullable) in header
        .columns
        .iter()
        .zip(header.types.iter())
        .zip(&header.nullability)
    {
        types.push(delimited_type(name, *ty, *nullable));
    }
    finish_document(summary_value(&Drawn {
        source_protocol: complete.protocol,
        profile_version: complete.profile_version,
        lineage_version: complete.lineage_version,
        receipt_version: complete.receipt_version,
        profile_id: &complete.profile_id,
        profile_digest: &complete.profile_digest,
        config_identity: &complete.config_identity,
        object_sha256: &complete.object_sha256,
        rows: complete.rows,
        columns: complete.columns,
        batches: complete.batches,
        admits_import: complete.admits_import,
        published: complete.published,
        arrow_ipc: header.arrow_ipc,
        executes_native: header.executes_native,
        types,
        fold,
    }))
}

fn render_jsonl(
    header: &JsonlHeader,
    complete: &JsonlComplete,
    fold: &Fold,
) -> Result<String, Fail> {
    if header.columns.len() != header.types.len()
        || header.columns.len() != header.nullability.len()
    {
        return Err(Fail::parse("receipt"));
    }
    let mut types = Vec::with_capacity(header.columns.len());
    for ((name, ty), nullable) in header
        .columns
        .iter()
        .zip(header.types.iter())
        .zip(&header.nullability)
    {
        types.push(jsonl_type(name, *ty, *nullable));
    }
    finish_document(summary_value(&Drawn {
        source_protocol: complete.protocol,
        profile_version: complete.profile_version,
        lineage_version: complete.lineage_version,
        receipt_version: complete.receipt_version,
        profile_id: &complete.profile_id,
        profile_digest: &complete.profile_digest,
        config_identity: &complete.config_identity,
        object_sha256: &complete.object_sha256,
        rows: complete.rows,
        columns: complete.columns,
        batches: complete.batches,
        admits_import: complete.admits_import,
        published: complete.published,
        arrow_ipc: header.arrow_ipc,
        executes_native: header.executes_native,
        types,
        fold,
    }))
}

fn delimited_type(name: &str, ty: DelimitedType, nullable: bool) -> Value {
    match ty {
        DelimitedType::Decimal { precision, scale } => serde_json::json!({
            "name": name,
            "nullable": nullable,
            "precision": precision,
            "scale": scale,
            "type": "decimal",
        }),
        other => serde_json::json!({
            "name": name,
            "nullable": nullable,
            "type": other.as_str(),
        }),
    }
}

fn jsonl_type(name: &str, ty: JsonlType, nullable: bool) -> Value {
    serde_json::json!({
        "name": name,
        "nullable": nullable,
        "type": ty.as_str(),
    })
}

/// Closed flags copied from the reader header and the finish receipt.
#[allow(clippy::struct_excessive_bools)]
struct Drawn<'a> {
    source_protocol: &'a str,
    profile_version: &'a str,
    lineage_version: &'a str,
    receipt_version: &'a str,
    profile_id: &'a str,
    profile_digest: &'a [u8; 32],
    config_identity: &'a [u8; 32],
    object_sha256: &'a [u8; 32],
    rows: u64,
    columns: u32,
    batches: u64,
    admits_import: bool,
    published: bool,
    arrow_ipc: bool,
    executes_native: bool,
    types: Vec<Value>,
    fold: &'a Fold,
}

fn summary_value(drawn: &Drawn<'_>) -> Value {
    let fold = drawn.fold;
    serde_json::json!({
        "admits_import": drawn.admits_import,
        "arrow_ipc": drawn.arrow_ipc,
        "batches": drawn.batches,
        "columns": drawn.columns,
        "config_identity": hex_encode(drawn.config_identity),
        "executes_native": drawn.executes_native,
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
        "lineage_version": drawn.lineage_version,
        "object_sha256": hex_encode(drawn.object_sha256),
        "profile_digest": hex_encode(drawn.profile_digest),
        "profile_id": drawn.profile_id,
        "profile_version": drawn.profile_version,
        "protocol": RESULT_PROTOCOL,
        "published": drawn.published,
        "receipt_version": drawn.receipt_version,
        "rows": drawn.rows,
        "source_protocol": drawn.source_protocol,
        "types": &drawn.types,
    })
}

fn finish_document(value: Value) -> Result<String, Fail> {
    let mut text = serde_json::to_string(&value).map_err(|_| Fail::read("stdout"))?;
    text.push('\n');
    Ok(text)
}

fn from_delimited(err: DelimitedError) -> Fail {
    let fact = err.fact();
    match err.kind() {
        DelimitedErrorKind::Config => Fail::config(fact),
        DelimitedErrorKind::Budget => Fail::budget(fact),
        DelimitedErrorKind::Protocol | DelimitedErrorKind::Encoding => Fail::parse(fact),
        DelimitedErrorKind::Hash => Fail::hash(fact),
        DelimitedErrorKind::Cancelled => Fail::cancel(fact),
        DelimitedErrorKind::Io => Fail::read(fact),
    }
}

fn from_jsonl(err: JsonlError) -> Fail {
    let fact = err.fact();
    match err.kind() {
        JsonlErrorKind::Config => Fail::config(fact),
        JsonlErrorKind::Budget => Fail::budget(fact),
        JsonlErrorKind::Protocol | JsonlErrorKind::Encoding => Fail::parse(fact),
        JsonlErrorKind::Hash => Fail::hash(fact),
        JsonlErrorKind::Cancelled => Fail::cancel(fact),
        JsonlErrorKind::Io => Fail::read(fact),
    }
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
    require(map, key)?
        .as_str()
        .ok_or(Fail::config("request_field"))
}

fn require_bool(map: &Map<String, Value>, key: &'static str) -> Result<bool, Fail> {
    require(map, key)?
        .as_bool()
        .ok_or(Fail::config("request_field"))
}

fn require_u64(
    map: &Map<String, Value>,
    key: &'static str,
    fact: &'static str,
) -> Result<u64, Fail> {
    require(map, key)?.as_u64().ok_or(Fail::config(fact))
}

fn require_u32(
    map: &Map<String, Value>,
    key: &'static str,
    fact: &'static str,
) -> Result<u32, Fail> {
    u32::try_from(require_u64(map, key, fact)?).map_err(|_| Fail::config(fact))
}

fn require_u8(map: &Map<String, Value>, key: &'static str, fact: &'static str) -> Result<u8, Fail> {
    u8::try_from(require_u64(map, key, fact)?).map_err(|_| Fail::config(fact))
}

fn require_i8(map: &Map<String, Value>, key: &'static str, fact: &'static str) -> Result<i8, Fail> {
    let number = require(map, key)?.as_i64().ok_or(Fail::config(fact))?;
    i8::try_from(number).map_err(|_| Fail::config(fact))
}

fn decode_sha256(text: &str) -> Result<[u8; 32], Fail> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Fail::config("expected_sha256"));
    }
    let mut out = [0_u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 2;
        *slot = u8::from_str_radix(&text[start..start + 2], 16)
            .map_err(|_| Fail::config("expected_sha256"))?;
    }
    Ok(out)
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
        "fact": fail.fact,
        "outcome": fail.outcome,
        "protocol": ERROR_PROTOCOL,
    });
    let text = serde_json::to_string(&body).unwrap_or_else(|_| {
        "{\"fact\":\"stderr\",\"outcome\":\"read\",\"protocol\":\"oo.pipeline.validate-file-error/v1\"}".to_owned()
    });
    let mut line = text;
    line.push('\n');
    let _ = io::stderr().write_all(line.as_bytes());
}
