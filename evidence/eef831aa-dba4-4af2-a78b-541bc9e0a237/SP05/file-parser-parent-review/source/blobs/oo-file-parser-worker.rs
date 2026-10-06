//! File parser child: explicit CSV, TSV, or JSONL file to bounded Arrow IPC.
//!
//! One versioned JSON request arrives on stdin. The process opens one
//! caller-admitted regular file and pulls the public JSONL or delimited
//! reader. Each record batch is one length-prefixed Arrow IPC stream, followed
//! by that batch's lineage frame. The completion frame is written only after
//! the reader mints its token. That token is not an import, a publication, or
//! a PostgreSQL `RunComplete`.
//!
//! This process does not apply a kernel network, filesystem, or memory
//! restriction. The parent owns the watchdog, the kill, and the reap.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use arrow_array::{Array, RecordBatch};
use arrow_ipc::MetadataVersion;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_schema::{DataType, Field, Schema};
use oo_connectors::{
    BomPolicy, DELIMITED_LINEAGE_VERSION, DELIMITED_PROFILE_VERSION, DELIMITED_PROTOCOL,
    DELIMITED_RECEIPT_VERSION, DelimitedBudget, DelimitedColumn, DelimitedComplete,
    DelimitedDialect, DelimitedError, DelimitedErrorKind, DelimitedFieldPresence, DelimitedItem,
    DelimitedProfile, DelimitedReader, DelimitedRowLineage, DelimitedType, FieldPresence,
    HeaderPolicy, JSONL_LINEAGE_VERSION, JSONL_PROFILE_VERSION, JSONL_PROTOCOL,
    JSONL_RECEIPT_VERSION, JsonlBudget, JsonlColumn, JsonlError, JsonlErrorKind, JsonlItem,
    JsonlProfile, JsonlReader, JsonlType, NullPolicy, RowLineage,
};
use oo_core::parse_json_value;
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

const PROTOCOL: &str = "oo.source.file-parser-worker/v1";
const REQUEST_MAX_BYTES: usize = 1_048_576;
const PATH_MAX_BYTES: usize = 4096;
const DEADLINE_MS_MAX: u64 = 3_600_000;
const FRAME_OVERHEAD_BYTES: u64 = 65_536;
const COMPLETE_MAX_BYTES: usize = 65_536;
const IPC_ALIGNMENT: usize = 8;
const TAG_IPC: u32 = 1;
const TAG_LINEAGE: u32 = 2;
const TAG_COMPLETE: u32 = 3;

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

#[derive(Clone, Copy)]
struct Fail {
    token: &'static str,
    exit: u8,
}

impl Fail {
    const fn new(token: &'static str, exit: u8) -> Self {
        Self { token, exit }
    }

    const fn config(token: &'static str) -> Self {
        Self::new(token, 2)
    }

    const fn protocol(token: &'static str) -> Self {
        Self::new(token, 4)
    }

    const fn io() -> Self {
        Self::new("io", 7)
    }

    const fn ceiling(token: &'static str) -> Self {
        Self::new(token, 8)
    }
}

enum Work {
    Jsonl {
        profile: JsonlProfile,
        budget: JsonlBudget,
    },
    Delimited {
        profile: DelimitedProfile,
        budget: DelimitedBudget,
    },
}

struct Request {
    path: String,
    expected: [u8; 32],
    work: Work,
}

struct ReaderIdentity {
    protocol: &'static str,
    profile_version: &'static str,
    lineage_version: &'static str,
    receipt_version: &'static str,
    profile_id: String,
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    object_sha256: [u8; 32],
    rows: u64,
    columns: u32,
    batches: u64,
}

fn main() {
    std::panic::set_hook(Box::new(|_| {
        let _ = writeln!(io::stderr(), "oo-file-parser-worker: internal");
    }));
    let exit = match run() {
        Ok(()) => 0,
        Err(fail) => {
            let _ = writeln!(io::stderr(), "oo-file-parser-worker: {}", fail.token);
            fail.exit
        }
    };
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    std::process::exit(i32::from(exit));
}

fn run() -> Result<(), Fail> {
    let bytes = read_request()?;
    let value = parse_request(&bytes)?;
    reject_credentials(&value)?;
    require_protocol(&value)?;
    let request = decode_request(&value)?;
    let file = open_input(&request.path)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    match request.work {
        Work::Jsonl { profile, budget } => {
            drive_jsonl(&mut stdout, file, profile, request.expected, budget)?;
        }
        Work::Delimited { profile, budget } => {
            drive_delimited(&mut stdout, file, profile, request.expected, budget)?;
        }
    }
    stdout.flush().map_err(|_| Fail::io())?;
    Ok(())
}

fn read_request() -> Result<Vec<u8>, Fail> {
    let mut stdin = io::stdin().lock();
    let mut body = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| Fail::io())?;
        if read == 0 {
            break;
        }
        if body.len().saturating_add(read) > REQUEST_MAX_BYTES {
            return Err(Fail::config("request_too_large"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    if body.is_empty() {
        return Err(Fail::config("request_syntax"));
    }
    Ok(body)
}

fn parse_request(bytes: &[u8]) -> Result<Value, Fail> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fail::config("request_syntax"))?;
    match parse_json_value(text, PROTOCOL) {
        Ok(value) => Ok(value),
        Err(list) => {
            let duplicate = list
                .items
                .iter()
                .any(|item| item.code.ends_with("/json.duplicate_key"));
            if duplicate {
                Err(Fail::config("duplicate_key"))
            } else {
                Err(Fail::config("request_syntax"))
            }
        }
    }
}

fn reject_credentials(value: &Value) -> Result<(), Fail> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if CREDENTIAL_KEYS.contains(&key.as_str()) {
                    return Err(Fail::config("credentials"));
                }
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

fn require_protocol(value: &Value) -> Result<(), Fail> {
    let Some(map) = value.as_object() else {
        return Err(Fail::config("request_syntax"));
    };
    match map.get("protocol") {
        Some(Value::String(text)) if text == PROTOCOL => Ok(()),
        Some(Value::String(_)) => Err(Fail::config("unknown_protocol")),
        _ => Err(Fail::config("config")),
    }
}

fn decode_request(value: &Value) -> Result<Request, Fail> {
    let map = exact_object(
        value,
        &["protocol", "input", "object_sha256", "profile", "budget"],
    )?;
    let input = exact_object(required(map, "input")?, &["class", "admission", "path"])?;
    if require_str(input, "class")? != "local_file" {
        return Err(Fail::config("config"));
    }
    if require_str(input, "admission")? != "caller_admitted_immutable" {
        return Err(Fail::config("config"));
    }
    let path = require_str(input, "path")?;
    if !valid_path(path) {
        return Err(Fail::config("path_form"));
    }
    let expected = parse_sha256(require_str(map, "object_sha256")?)?;
    let profile = required(map, "profile")?;
    let budget = required(map, "budget")?;
    let work = match peek_format(profile)? {
        "jsonl" => Work::Jsonl {
            profile: jsonl_profile(profile)?,
            budget: jsonl_budget(budget)?,
        },
        "delimited" => Work::Delimited {
            profile: delimited_profile(profile)?,
            budget: delimited_budget(budget)?,
        },
        _ => return Err(Fail::config("config")),
    };
    Ok(Request {
        path: path.to_owned(),
        expected,
        work,
    })
}

fn peek_format(value: &Value) -> Result<&str, Fail> {
    let Some(map) = value.as_object() else {
        return Err(Fail::config("config"));
    };
    match map.get("format") {
        Some(Value::String(text)) => Ok(text.as_str()),
        _ => Err(Fail::config("config")),
    }
}

fn jsonl_profile(value: &Value) -> Result<JsonlProfile, Fail> {
    let map = exact_object(value, &["format", "id", "columns"])?;
    if require_str(map, "format")? != "jsonl" {
        return Err(Fail::config("config"));
    }
    let columns = jsonl_columns(required(map, "columns")?)?;
    JsonlProfile::try_new(require_str(map, "id")?, columns).map_err(from_jsonl)
}

fn jsonl_columns(value: &Value) -> Result<Vec<JsonlColumn>, Fail> {
    let Some(items) = value.as_array() else {
        return Err(Fail::config("config"));
    };
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let map = exact_object(item, &["name", "type", "nullable"])?;
        let ty = match require_str(map, "type")? {
            "utf8" => JsonlType::Utf8,
            "bool" => JsonlType::Bool,
            "int64" => JsonlType::Int64,
            "json_text" => JsonlType::JsonText,
            _ => return Err(Fail::config("config")),
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

fn jsonl_budget(value: &Value) -> Result<JsonlBudget, Fail> {
    let map = exact_object(
        value,
        &[
            "batch_rows",
            "batch_bytes",
            "max_record_bytes",
            "max_total_bytes",
            "max_rows",
            "max_columns",
            "max_fields",
            "deadline_ms",
        ],
    )?;
    JsonlBudget::try_new(
        require_u32(map, "batch_rows")?,
        require_u64(map, "batch_bytes")?,
        require_u32(map, "max_record_bytes")?,
        require_u64(map, "max_total_bytes")?,
        require_u64(map, "max_rows")?,
        require_u32(map, "max_columns")?,
        require_u32(map, "max_fields")?,
        deadline(require_u64(map, "deadline_ms")?)?,
    )
    .map_err(from_jsonl)
}

fn delimited_profile(value: &Value) -> Result<DelimitedProfile, Fail> {
    let map = exact_object(
        value,
        &[
            "format", "id", "dialect", "bom", "header", "null", "columns",
        ],
    )?;
    if require_str(map, "format")? != "delimited" {
        return Err(Fail::config("config"));
    }
    let dialect = match require_str(map, "dialect")? {
        "csv" => DelimitedDialect::Csv,
        "tsv" => DelimitedDialect::Tsv,
        _ => return Err(Fail::config("config")),
    };
    let bom = match require_str(map, "bom")? {
        "refuse" => BomPolicy::Refuse,
        "strip" => BomPolicy::Strip,
        "require" => BomPolicy::Require,
        _ => return Err(Fail::config("config")),
    };
    let header = match require_str(map, "header")? {
        "absent" => HeaderPolicy::Absent,
        "exact" => HeaderPolicy::Exact,
        _ => return Err(Fail::config("config")),
    };
    let nulls = null_policy(required(map, "null")?)?;
    let columns = delimited_columns(required(map, "columns")?)?;
    DelimitedProfile::try_new(
        require_str(map, "id")?,
        dialect,
        bom,
        header,
        nulls,
        columns,
    )
    .map_err(from_delimited)
}

fn null_policy(value: &Value) -> Result<NullPolicy, Fail> {
    let Some(map) = value.as_object() else {
        return Err(Fail::config("config"));
    };
    let policy = match map.get("policy") {
        Some(Value::String(text)) => text.as_str(),
        _ => return Err(Fail::config("config")),
    };
    match policy {
        "empty" => {
            exact_object(value, &["policy"])?;
            Ok(NullPolicy::Empty)
        }
        "token" => {
            let map = exact_object(value, &["policy", "text"])?;
            Ok(NullPolicy::Token(require_str(map, "text")?.to_owned()))
        }
        _ => Err(Fail::config("config")),
    }
}

fn delimited_columns(value: &Value) -> Result<Vec<DelimitedColumn>, Fail> {
    let Some(items) = value.as_array() else {
        return Err(Fail::config("config"));
    };
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        columns.push(delimited_column(item)?);
    }
    Ok(columns)
}

fn delimited_column(value: &Value) -> Result<DelimitedColumn, Fail> {
    let Some(map) = value.as_object() else {
        return Err(Fail::config("config"));
    };
    let type_name = match map.get("type") {
        Some(Value::String(text)) => text.as_str(),
        _ => return Err(Fail::config("config")),
    };
    let keys: &[&str] = if type_name == "decimal" {
        &["name", "type", "nullable", "precision", "scale"]
    } else {
        &["name", "type", "nullable"]
    };
    let map = exact_object(value, keys)?;
    let ty = match type_name {
        "utf8" => DelimitedType::Utf8,
        "bool" => DelimitedType::Bool,
        "int64" => DelimitedType::Int64,
        "decimal" => DelimitedType::Decimal {
            precision: require_u8(map, "precision")?,
            scale: require_i8(map, "scale")?,
        },
        _ => return Err(Fail::config("config")),
    };
    DelimitedColumn::try_new(
        require_str(map, "name")?,
        ty,
        require_bool(map, "nullable")?,
    )
    .map_err(from_delimited)
}

fn delimited_budget(value: &Value) -> Result<DelimitedBudget, Fail> {
    let map = exact_object(
        value,
        &[
            "batch_rows",
            "batch_bytes",
            "max_record_bytes",
            "max_field_bytes",
            "max_total_bytes",
            "max_rows",
            "max_columns",
            "max_fields",
            "deadline_ms",
        ],
    )?;
    DelimitedBudget::try_new(
        require_u32(map, "batch_rows")?,
        require_u64(map, "batch_bytes")?,
        require_u32(map, "max_record_bytes")?,
        require_u32(map, "max_field_bytes")?,
        require_u64(map, "max_total_bytes")?,
        require_u64(map, "max_rows")?,
        require_u32(map, "max_columns")?,
        require_u32(map, "max_fields")?,
        deadline(require_u64(map, "deadline_ms")?)?,
    )
    .map_err(from_delimited)
}

fn deadline(millis: u64) -> Result<Instant, Fail> {
    if millis == 0 || millis > DEADLINE_MS_MAX {
        return Err(Fail::config("config"));
    }
    Instant::now()
        .checked_add(Duration::from_millis(millis))
        .ok_or(Fail::config("config"))
}

fn valid_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() > PATH_MAX_BYTES || bytes.contains(&0) {
        return false;
    }
    if !path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    let mut parts = path.split('/');
    if parts.next() != Some("") {
        return false;
    }
    let mut saw_name = false;
    for part in parts {
        if part.is_empty() || part == "." || part == ".." {
            return false;
        }
        saw_name = true;
    }
    saw_name
}

fn open_input(path: &str) -> Result<File, Fail> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fail::io())?;
    if !meta.file_type().is_file() {
        return Err(Fail::config("not_regular"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nofollow_flag());
    }
    let file = options.open(path).map_err(|_| Fail::io())?;
    let meta = file.metadata().map_err(|_| Fail::io())?;
    if !meta.is_file() {
        return Err(Fail::config("not_regular"));
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
const fn nofollow_flag() -> i32 {
    0x20000
}

#[cfg(target_os = "macos")]
const fn nofollow_flag() -> i32 {
    0x0100
}

fn drive_jsonl(
    stdout: &mut impl Write,
    file: File,
    profile: JsonlProfile,
    expected: [u8; 32],
    budget: JsonlBudget,
) -> Result<(), Fail> {
    let expected_schema = jsonl_schema(&profile);
    let cap = frame_cap(budget.batch_bytes())?;
    let mut reader = JsonlReader::open(file, profile, expected, budget).map_err(from_jsonl)?;
    let mut ipc_frames = 0u64;
    while let JsonlItem::Batch(batch) = reader.next_batch().map_err(from_jsonl)? {
        refuse_if_ipc_exceeds(&batch.record_batch, cap)?;
        let identity = require_schema(&batch.record_batch, &expected_schema)?;
        let lineage = jsonl_lineage_frame(
            cap,
            batch.batch_index,
            &batch.header,
            identity,
            &batch.lineage,
            batch.raw_bytes,
            batch.arrow_bytes,
        )?;
        let ipc = encode_ipc(&batch.record_batch, cap)?;
        write_frame(stdout, TAG_IPC, &ipc)?;
        write_frame(stdout, TAG_LINEAGE, &lineage)?;
        ipc_frames += 1;
    }
    let complete = reader.finish().map_err(from_jsonl)?;
    if !complete.admits_import && !complete.published {
        write_complete(
            stdout,
            &ReaderIdentity {
                protocol: complete.protocol,
                profile_version: complete.profile_version,
                lineage_version: complete.lineage_version,
                receipt_version: complete.receipt_version,
                profile_id: complete.profile_id,
                profile_digest: complete.profile_digest,
                config_identity: complete.config_identity,
                object_sha256: complete.object_sha256,
                rows: complete.rows,
                columns: complete.columns,
                batches: complete.batches,
            },
            &expected_schema,
            ipc_frames,
        )
    } else {
        Err(Fail::protocol("protocol"))
    }
}

fn drive_delimited(
    stdout: &mut impl Write,
    file: File,
    profile: DelimitedProfile,
    expected: [u8; 32],
    budget: DelimitedBudget,
) -> Result<(), Fail> {
    let expected_schema = delimited_schema(&profile);
    let cap = frame_cap(budget.batch_bytes())?;
    let mut reader =
        DelimitedReader::open(file, profile, expected, budget).map_err(from_delimited)?;
    let mut ipc_frames = 0u64;
    while let DelimitedItem::Batch(batch) = reader.next_batch().map_err(from_delimited)? {
        refuse_if_ipc_exceeds(&batch.record_batch, cap)?;
        let identity = require_schema(&batch.record_batch, &expected_schema)?;
        let lineage = delimited_lineage_frame(
            cap,
            batch.batch_index,
            &batch.header,
            identity,
            &batch.lineage,
            batch.raw_bytes,
            batch.arrow_bytes,
        )?;
        let ipc = encode_ipc(&batch.record_batch, cap)?;
        write_frame(stdout, TAG_IPC, &ipc)?;
        write_frame(stdout, TAG_LINEAGE, &lineage)?;
        ipc_frames += 1;
    }
    let complete = reader.finish().map_err(from_delimited)?;
    if !complete.admits_import && !complete.published {
        write_complete(
            stdout,
            &identity_from_delimited(&complete),
            &expected_schema,
            ipc_frames,
        )
    } else {
        Err(Fail::protocol("protocol"))
    }
}

fn identity_from_delimited(complete: &DelimitedComplete) -> ReaderIdentity {
    ReaderIdentity {
        protocol: complete.protocol,
        profile_version: complete.profile_version,
        lineage_version: complete.lineage_version,
        receipt_version: complete.receipt_version,
        profile_id: complete.profile_id.clone(),
        profile_digest: complete.profile_digest,
        config_identity: complete.config_identity,
        object_sha256: complete.object_sha256,
        rows: complete.rows,
        columns: complete.columns,
        batches: complete.batches,
    }
}

fn refuse_if_ipc_exceeds(record: &RecordBatch, cap: usize) -> Result<(), Fail> {
    let memory: usize = record
        .columns()
        .iter()
        .map(|column| column.get_array_memory_size())
        .sum();
    if memory > cap {
        Err(Fail::ceiling("ipc_ceiling"))
    } else {
        Ok(())
    }
}

fn require_schema(record: &RecordBatch, expected: &Schema) -> Result<[u8; 32], Fail> {
    let actual = schema_identity(record.schema().as_ref())?;
    let predicted = schema_identity(expected)?;
    if actual != predicted {
        return Err(Fail::protocol("schema_drift"));
    }
    Ok(actual)
}

fn encode_ipc(record: &RecordBatch, cap: usize) -> Result<Vec<u8>, Fail> {
    let options = IpcWriteOptions::try_new(IPC_ALIGNMENT, false, MetadataVersion::V5)
        .map_err(|_| Fail::protocol("arrow_type"))?;
    let sink = BoundedWriter::new(cap);
    let mut writer = StreamWriter::try_new_with_options(sink, record.schema().as_ref(), options)
        .map_err(map_arrow)?;
    writer.write(record).map_err(map_arrow)?;
    let sink = writer.into_inner().map_err(map_arrow)?;
    let bytes = sink.into_bytes();
    if bytes.is_empty() || bytes.len() > cap {
        return Err(Fail::ceiling("ipc_ceiling"));
    }
    Ok(bytes)
}

fn map_arrow(err: arrow_schema::ArrowError) -> Fail {
    if err.to_string().contains("ipc_ceiling") {
        Fail::ceiling("ipc_ceiling")
    } else {
        Fail::protocol("arrow_type")
    }
}

fn jsonl_schema(profile: &JsonlProfile) -> Schema {
    let fields = profile
        .columns()
        .iter()
        .map(|column| {
            let (data_type, logical) = match column.ty() {
                JsonlType::Utf8 => (DataType::Utf8, "utf8"),
                JsonlType::JsonText => (DataType::Utf8, "json_text"),
                JsonlType::Bool => (DataType::Boolean, "bool"),
                JsonlType::Int64 => (DataType::Int64, "int64"),
            };
            let mut metadata = HashMap::new();
            metadata.insert("oo.jsonl.protocol".to_owned(), JSONL_PROTOCOL.to_owned());
            metadata.insert("oo.jsonl.logical".to_owned(), logical.to_owned());
            if logical == "json_text" {
                metadata.insert("oo.jsonl.encoding".to_owned(), "raw-json-utf8".to_owned());
            }
            Field::new(column.name(), data_type, column.nullable()).with_metadata(metadata)
        })
        .collect::<Vec<_>>();
    Schema::new(fields)
}

fn delimited_schema(profile: &DelimitedProfile) -> Schema {
    let fields = profile
        .columns()
        .iter()
        .map(|column| {
            let (data_type, logical) = match column.ty() {
                DelimitedType::Utf8 => (DataType::Utf8, "utf8"),
                DelimitedType::Bool => (DataType::Boolean, "bool"),
                DelimitedType::Int64 => (DataType::Int64, "int64"),
                DelimitedType::Decimal { precision, scale } => {
                    (DataType::Decimal128(precision, scale), "decimal")
                }
            };
            let mut metadata = HashMap::new();
            metadata.insert(
                "oo.delimited.protocol".to_owned(),
                DELIMITED_PROTOCOL.to_owned(),
            );
            metadata.insert("oo.delimited.logical".to_owned(), logical.to_owned());
            if let DelimitedType::Decimal { precision, scale } = column.ty() {
                metadata.insert("oo.delimited.precision".to_owned(), precision.to_string());
                metadata.insert("oo.delimited.scale".to_owned(), scale.to_string());
            }
            Field::new(column.name(), data_type, column.nullable()).with_metadata(metadata)
        })
        .collect::<Vec<_>>();
    Schema::new(fields)
}

fn schema_identity(schema: &Schema) -> Result<[u8; 32], Fail> {
    Ok(sha256(schema_preimage(schema)?.as_bytes()))
}

fn schema_preimage(schema: &Schema) -> Result<String, Fail> {
    let mut body = String::from("oo-file-parser-schema/v1\n");
    for field in schema.fields() {
        plain(field.name())?;
        body.push_str("field=");
        body.push_str(field.name());
        body.push('\t');
        body.push_str(&type_token(field.data_type())?);
        body.push('\t');
        body.push(if field.is_nullable() { '1' } else { '0' });
        body.push('\n');
        let mut keys: Vec<&str> = field.metadata().keys().map(String::as_str).collect();
        keys.sort_unstable();
        for key in keys {
            let value = &field.metadata()[key];
            plain(key)?;
            plain(value)?;
            body.push_str("meta=");
            body.push_str(field.name());
            body.push('\t');
            body.push_str(key);
            body.push('\t');
            body.push_str(value);
            body.push('\n');
        }
    }
    Ok(body)
}

fn type_token(ty: &DataType) -> Result<String, Fail> {
    match ty {
        DataType::Utf8 => Ok("utf8".to_owned()),
        DataType::Boolean => Ok("bool".to_owned()),
        DataType::Int64 => Ok("int64".to_owned()),
        DataType::Decimal128(precision, scale) => Ok(format!("decimal128:{precision}:{scale}")),
        _ => Err(Fail::protocol("arrow_type")),
    }
}

fn plain(text: &str) -> Result<(), Fail> {
    if text.contains(['\n', '\t']) {
        Err(Fail::protocol("schema_meta"))
    } else {
        Ok(())
    }
}

fn jsonl_lineage_frame(
    cap: usize,
    batch_index: u64,
    header: &oo_connectors::JsonlHeader,
    schema_identity: [u8; 32],
    rows: &[RowLineage],
    raw_bytes: u64,
    arrow_bytes: u64,
) -> Result<Vec<u8>, Fail> {
    let mut sink = BoundedWriter::new(cap);
    lineage_preamble(
        &mut sink,
        JSONL_PROTOCOL,
        batch_index,
        header.profile_digest,
        header.config_identity,
        schema_identity,
        raw_bytes,
        arrow_bytes,
    )?;
    for row in rows {
        write_text(
            &mut sink,
            &format!(
                "row={row_index}\t{line}\t{byte_offset}\n",
                row_index = row.row_index,
                line = row.line,
                byte_offset = row.byte_offset,
            ),
        )?;
        for field in &row.fields {
            plain(&field.name)?;
            let presence = match field.presence {
                FieldPresence::Value => "value",
                FieldPresence::Null => "null",
                FieldPresence::Missing => "missing",
            };
            write_text(
                &mut sink,
                &format!(
                    "field={name}\t{line}\t{offset}\t{presence}\n",
                    name = field.name,
                    line = field.line,
                    offset = field.byte_offset,
                ),
            )?;
        }
    }
    finish_bounded(sink, "lineage_ceiling")
}

fn delimited_lineage_frame(
    cap: usize,
    batch_index: u64,
    header: &oo_connectors::DelimitedHeader,
    schema_identity: [u8; 32],
    rows: &[DelimitedRowLineage],
    raw_bytes: u64,
    arrow_bytes: u64,
) -> Result<Vec<u8>, Fail> {
    let mut sink = BoundedWriter::new(cap);
    lineage_preamble(
        &mut sink,
        DELIMITED_PROTOCOL,
        batch_index,
        header.profile_digest,
        header.config_identity,
        schema_identity,
        raw_bytes,
        arrow_bytes,
    )?;
    for row in rows {
        write_text(
            &mut sink,
            &format!(
                "row={row_index}\t{line}\t{byte_offset}\n",
                row_index = row.row_index,
                line = row.line,
                byte_offset = row.byte_offset,
            ),
        )?;
        for field in &row.fields {
            plain(&field.name)?;
            let presence = match field.presence {
                DelimitedFieldPresence::Value => "value",
                DelimitedFieldPresence::Null => "null",
                DelimitedFieldPresence::Empty => "empty",
            };
            write_text(
                &mut sink,
                &format!(
                    "field={name}\t{line}\t{offset}\t{presence}\n",
                    name = field.name,
                    line = field.line,
                    offset = field.byte_offset,
                ),
            )?;
        }
    }
    finish_bounded(sink, "lineage_ceiling")
}

#[allow(clippy::too_many_arguments)]
fn lineage_preamble(
    sink: &mut BoundedWriter,
    reader_protocol: &str,
    batch_index: u64,
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    schema_identity: [u8; 32],
    raw_bytes: u64,
    arrow_bytes: u64,
) -> Result<(), Fail> {
    write_text(sink, "oo-file-parser-lineage/v1\n")?;
    write_text(sink, &format!("batch_index={batch_index}\n"))?;
    write_text(sink, &format!("reader_protocol={reader_protocol}\n"))?;
    write_text(sink, &format!("profile_digest={}\n", hex(&profile_digest)))?;
    write_text(
        sink,
        &format!("config_identity={}\n", hex(&config_identity)),
    )?;
    write_text(
        sink,
        &format!("schema_identity={}\n", hex(&schema_identity)),
    )?;
    write_text(sink, &format!("raw_bytes={raw_bytes}\n"))?;
    write_text(sink, &format!("arrow_bytes={arrow_bytes}\n"))?;
    Ok(())
}

fn expect_reader_versions(identity: &ReaderIdentity) -> Result<(), Fail> {
    let (profile_version, lineage_version, receipt_version) = if identity.protocol == JSONL_PROTOCOL
    {
        (
            JSONL_PROFILE_VERSION,
            JSONL_LINEAGE_VERSION,
            JSONL_RECEIPT_VERSION,
        )
    } else if identity.protocol == DELIMITED_PROTOCOL {
        (
            DELIMITED_PROFILE_VERSION,
            DELIMITED_LINEAGE_VERSION,
            DELIMITED_RECEIPT_VERSION,
        )
    } else {
        return Err(Fail::protocol("protocol"));
    };
    if identity.profile_version != profile_version
        || identity.lineage_version != lineage_version
        || identity.receipt_version != receipt_version
    {
        return Err(Fail::protocol("protocol"));
    }
    Ok(())
}

fn write_complete(
    stdout: &mut impl Write,
    identity: &ReaderIdentity,
    expected_schema: &Schema,
    ipc_frames: u64,
) -> Result<(), Fail> {
    expect_reader_versions(identity)?;
    if identity.batches != ipc_frames {
        return Err(Fail::protocol("protocol"));
    }
    plain(&identity.profile_id)?;
    let schema_identity = schema_identity(expected_schema)?;
    let arrow_ipc = if ipc_frames == 0 { "false" } else { "true" };
    let mut sink = BoundedWriter::new(COMPLETE_MAX_BYTES);
    write_receipt(&mut sink, "oo-file-parser-complete/v1\n")?;
    write_receipt(&mut sink, &format!("protocol={PROTOCOL}\n"))?;
    write_receipt(
        &mut sink,
        &format!("reader_protocol={protocol}\n", protocol = identity.protocol),
    )?;
    write_receipt(
        &mut sink,
        &format!(
            "reader_profile_version={version}\n",
            version = identity.profile_version
        ),
    )?;
    write_receipt(
        &mut sink,
        &format!(
            "reader_lineage_version={version}\n",
            version = identity.lineage_version
        ),
    )?;
    write_receipt(
        &mut sink,
        &format!(
            "reader_receipt_version={version}\n",
            version = identity.receipt_version
        ),
    )?;
    write_receipt(
        &mut sink,
        &format!(
            "profile_id={profile_id}\n",
            profile_id = identity.profile_id
        ),
    )?;
    write_receipt(
        &mut sink,
        &format!("profile_digest={}\n", hex(&identity.profile_digest)),
    )?;
    write_receipt(
        &mut sink,
        &format!("config_identity={}\n", hex(&identity.config_identity)),
    )?;
    write_receipt(
        &mut sink,
        &format!("schema_identity={}\n", hex(&schema_identity)),
    )?;
    write_receipt(
        &mut sink,
        &format!("object_sha256={}\n", hex(&identity.object_sha256)),
    )?;
    write_receipt(&mut sink, &format!("rows={rows}\n", rows = identity.rows))?;
    write_receipt(
        &mut sink,
        &format!("columns={columns}\n", columns = identity.columns),
    )?;
    write_receipt(
        &mut sink,
        &format!("batches={batches}\n", batches = identity.batches),
    )?;
    write_receipt(&mut sink, &format!("ipc_frames={ipc_frames}\n"))?;
    write_receipt(&mut sink, &format!("lineage_frames={ipc_frames}\n"))?;
    write_receipt(&mut sink, "admits_import=false\n")?;
    write_receipt(&mut sink, "published=false\n")?;
    write_receipt(&mut sink, &format!("arrow_ipc={arrow_ipc}\n"))?;
    write_receipt(&mut sink, "executes_native=false\n")?;
    write_receipt(&mut sink, "kernel_network=unsupported\n")?;
    write_receipt(&mut sink, "kernel_filesystem=unsupported\n")?;
    write_receipt(&mut sink, "kernel_memory=unsupported\n")?;
    write_receipt(&mut sink, "parent_kill=caller\n")?;
    write_receipt(&mut sink, "parent_reap=caller\n")?;
    write_receipt(&mut sink, "launcher=not_this_process\n")?;
    write_receipt(&mut sink, "pg_run_complete=false\n")?;
    write_receipt(&mut sink, "resumable=false\n")?;
    if sink.failed() {
        return Err(Fail::protocol("protocol"));
    }
    let payload = sink.into_bytes();
    if payload.is_empty() {
        return Err(Fail::protocol("protocol"));
    }
    write_frame(stdout, TAG_COMPLETE, &payload)
}

fn write_frame(stdout: &mut impl Write, tag: u32, payload: &[u8]) -> Result<(), Fail> {
    if payload.is_empty() {
        return Err(Fail::protocol("protocol"));
    }
    let len = u32::try_from(payload.len()).map_err(|_| Fail::ceiling("ipc_ceiling"))?;
    stdout
        .write_all(&tag.to_le_bytes())
        .map_err(|_| Fail::io())?;
    stdout
        .write_all(&len.to_le_bytes())
        .map_err(|_| Fail::io())?;
    stdout.write_all(payload).map_err(|_| Fail::io())?;
    Ok(())
}

fn write_text(sink: &mut BoundedWriter, text: &str) -> Result<(), Fail> {
    sink.write_all(text.as_bytes())
        .map_err(|_| Fail::ceiling("lineage_ceiling"))
}

fn write_receipt(sink: &mut BoundedWriter, text: &str) -> Result<(), Fail> {
    sink.write_all(text.as_bytes())
        .map_err(|_| Fail::protocol("protocol"))
}

fn finish_bounded(sink: BoundedWriter, token: &'static str) -> Result<Vec<u8>, Fail> {
    if sink.failed() {
        return Err(Fail::ceiling(token));
    }
    let bytes = sink.into_bytes();
    if bytes.is_empty() {
        return Err(Fail::protocol("protocol"));
    }
    Ok(bytes)
}

struct BoundedWriter {
    buf: Vec<u8>,
    cap: usize,
    failed: bool,
}

impl BoundedWriter {
    fn new(cap: usize) -> Self {
        let start = 1024.min(cap);
        Self {
            buf: Vec::with_capacity(start),
            cap,
            failed: false,
        }
    }

    fn failed(&self) -> bool {
        self.failed
    }

    fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(quota());
        }
        let Some(next) = self.buf.len().checked_add(data.len()) else {
            self.failed = true;
            return Err(quota());
        };
        if next > self.cap {
            self.failed = true;
            return Err(quota());
        }
        if self.buf.capacity() < next {
            self.buf.reserve_exact(next - self.buf.len());
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn quota() -> io::Error {
    io::Error::new(io::ErrorKind::QuotaExceeded, "ipc_ceiling")
}

fn frame_cap(batch_bytes: u64) -> Result<usize, Fail> {
    let sum = batch_bytes
        .checked_add(FRAME_OVERHEAD_BYTES)
        .ok_or(Fail::config("config"))?;
    usize::try_from(sum).map_err(|_| Fail::config("config"))
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, Fail> {
    let Some(map) = value.as_object() else {
        return Err(Fail::config("config"));
    };
    for key in map.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(Fail::config("unknown_key"));
        }
    }
    for key in keys {
        if !map.contains_key(*key) {
            return Err(Fail::config("config"));
        }
    }
    Ok(map)
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Fail> {
    map.get(key).ok_or(Fail::config("config"))
}

fn require_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, Fail> {
    match map.get(key) {
        Some(Value::String(text)) => Ok(text.as_str()),
        _ => Err(Fail::config("config")),
    }
}

fn require_bool(map: &Map<String, Value>, key: &str) -> Result<bool, Fail> {
    match map.get(key) {
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(Fail::config("config")),
    }
}

fn require_u64(map: &Map<String, Value>, key: &str) -> Result<u64, Fail> {
    match map.get(key) {
        Some(Value::Number(number)) => parse_u64(number),
        _ => Err(Fail::config("config")),
    }
}

fn require_u32(map: &Map<String, Value>, key: &str) -> Result<u32, Fail> {
    u32::try_from(require_u64(map, key)?).map_err(|_| Fail::config("config"))
}

fn require_u8(map: &Map<String, Value>, key: &str) -> Result<u8, Fail> {
    u8::try_from(require_u64(map, key)?).map_err(|_| Fail::config("config"))
}

fn require_i8(map: &Map<String, Value>, key: &str) -> Result<i8, Fail> {
    match map.get(key) {
        Some(Value::Number(number)) => {
            let value = parse_i64(number)?;
            i8::try_from(value).map_err(|_| Fail::config("config"))
        }
        _ => Err(Fail::config("config")),
    }
}

fn parse_u64(number: &Number) -> Result<u64, Fail> {
    let text = number.as_str();
    if !unsigned_digits(text) {
        return Err(Fail::config("config"));
    }
    text.parse::<u64>().map_err(|_| Fail::config("config"))
}

fn parse_i64(number: &Number) -> Result<i64, Fail> {
    let text = number.as_str();
    let digits = text.strip_prefix('-').unwrap_or(text);
    if text.starts_with('+') || !unsigned_digits(digits) {
        return Err(Fail::config("config"));
    }
    text.parse::<i64>().map_err(|_| Fail::config("config"))
}

fn unsigned_digits(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text.len() == 1 || !text.starts_with('0'))
}

fn parse_sha256(text: &str) -> Result<[u8; 32], Fail> {
    let lowercase_hex = text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    if !lowercase_hex {
        return Err(Fail::config("config"));
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let hi = hex_value(text.as_bytes()[index * 2])?;
        let lo = hex_value(text.as_bytes()[index * 2 + 1])?;
        *byte = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Result<u8, Fail> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Fail::config("config")),
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn from_jsonl(err: JsonlError) -> Fail {
    map_reader(jsonl_kind(err.kind()), err.fact())
}

fn from_delimited(err: DelimitedError) -> Fail {
    map_reader(delimited_kind(err.kind()), err.fact())
}

fn jsonl_kind(kind: JsonlErrorKind) -> &'static str {
    match kind {
        JsonlErrorKind::Config => "config",
        JsonlErrorKind::Budget => "budget",
        JsonlErrorKind::Protocol => "protocol",
        JsonlErrorKind::Encoding => "encoding",
        JsonlErrorKind::Hash => "hash",
        JsonlErrorKind::Cancelled => "cancelled",
        JsonlErrorKind::Io => "io",
    }
}

fn delimited_kind(kind: DelimitedErrorKind) -> &'static str {
    match kind {
        DelimitedErrorKind::Config => "config",
        DelimitedErrorKind::Budget => "budget",
        DelimitedErrorKind::Protocol => "protocol",
        DelimitedErrorKind::Encoding => "encoding",
        DelimitedErrorKind::Hash => "hash",
        DelimitedErrorKind::Cancelled => "cancelled",
        DelimitedErrorKind::Io => "io",
    }
}

fn map_reader(kind: &str, fact: &str) -> Fail {
    match (kind, fact) {
        ("hash", _) => Fail::new("hash_mismatch", 6),
        ("budget", "deadline") => Fail::new("deadline", 3),
        ("budget", _) => Fail::new("budget", 3),
        ("encoding", _) => Fail::new("encoding", 5),
        ("io", _) => Fail::io(),
        ("cancelled", _) => Fail::new("cancelled", 9),
        ("config", _) => Fail::config("config"),
        ("protocol", "header_required") => Fail::protocol("header_required"),
        ("protocol", "truncated") => Fail::protocol("truncated"),
        ("protocol", _) => Fail::protocol("protocol"),
        _ => Fail::protocol("protocol"),
    }
}
