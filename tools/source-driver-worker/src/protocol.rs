//! Frozen `oo.source.driver-worker/v1` messages, frames, and Arrow IPC 60.
//!
//! The child JSON has no target, Agent DB, model, or Gateway credential.
//! Duplicate keys are rejected before a typed read. This module does not load
//! a native library.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::ParentError;

pub const PROTOCOL: &str = "oo.source.driver-worker/v1";
pub const SCHEMA_ID: &str = "oo.source.driver-schema/v1";
pub const ARTIFACT_NAME: &str = "driver-artifact.bin";
pub const DRIVER_NAME: &str = "customer-provided";
pub const READ_KIND: &str = "batch_full";

pub const PARENT_DEADLINE_MIN_MS: u64 = 1;
pub const PARENT_DEADLINE_MAX_MS: u64 = 120_000;
pub const CHILD_DEADLINE_MAX_MS: u64 = 3_600_000;
pub const REQUEST_MAX_BYTES: usize = 1_048_576;
pub const PATH_MAX_BYTES: usize = 4096;
pub const FRAME_OVERHEAD: u64 = 65_536;
pub const COMPLETE_MAX_BYTES: usize = 65_536;
pub const IPC_ALIGNMENT: usize = 8;
pub const DRIVER_MAX_BYTES: u64 = 512 * 1024 * 1024;
pub const BATCH_ROWS_MAX: u64 = 10_000;
pub const BATCH_BYTES_MAX: u64 = 16_777_216;
pub const MAX_ROWS_MAX: u64 = 1_000_000;
pub const MAX_BYTES_MAX: u64 = 1_073_741_824;

pub const TAG_IPC: u32 = 1;
pub const TAG_LINEAGE: u32 = 2;
pub const TAG_COMPLETE: u32 = 3;

const FORBIDDEN: &[&str] = &[
    "agent_db",
    "agent_db_password",
    "agent_db_url",
    "connection_string",
    "gateway",
    "gateway_credential",
    "gateway_token",
    "model",
    "model_api_key",
    "model_credential",
    "passwd",
    "password",
    "raw_credential",
    "secret",
    "token",
];

/// Caller-held inputs. `target_credential_ref` is never encoded.
#[derive(Clone)]
pub struct ChildBody {
    pub source_credential_ref: String,
    pub target_credential_ref: String,
    pub image_digest: String,
    pub volume: String,
    pub driver_digest: String,
    pub batch_rows: u64,
    pub batch_bytes: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub deadline_ms: u64,
    pub statement_ref: String,
    pub forwarded: Vec<u8>,
}

pub fn parse_caller(bytes: &[u8], driver_volume: &Path) -> Result<ChildBody, ParentError> {
    let value = parse_value(bytes)?;
    reject_forbidden(&value)?;
    let map = exact_object(&value, &[
        "protocol",
        "source_credential_ref",
        "target_credential_ref",
        "image_digest",
        "driver",
        "budget",
        "read",
    ])?;
    let body = body_from_map(map, true)?;
    let volume = path_text(driver_volume).ok_or(ParentError::Config("driver_volume"))?;
    if volume != body.volume {
        return Err(ParentError::Config("driver_volume"));
    }
    Ok(body)
}

pub fn parse_child(bytes: &[u8]) -> Result<ChildBody, ParentError> {
    let value = parse_value(bytes)?;
    reject_forbidden(&value)?;
    let map = exact_object(&value, &[
        "protocol",
        "source_credential_ref",
        "image_digest",
        "driver",
        "budget",
        "read",
    ])?;
    body_from_map(map, false)
}

fn body_from_map(map: &Map<String, Value>, caller: bool) -> Result<ChildBody, ParentError> {
    if map.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(ParentError::Config("protocol"));
    }
    let source = required_str(map, "source_credential_ref")?;
    if !approved_ref(source) {
        return Err(ParentError::Config("credential_ref"));
    }
    let target = if caller {
        let text = required_str(map, "target_credential_ref")?;
        if !approved_ref(text) {
            return Err(ParentError::Config("credential_ref"));
        }
        text.to_owned()
    } else if map.contains_key("target_credential_ref") {
        return Err(ParentError::Config("unknown_key"));
    } else {
        String::new()
    };
    let image = required_str(map, "image_digest")?;
    if !approved_digest(image) {
        return Err(ParentError::Config("image_digest"));
    }
    let driver = required_object(map, "driver", &["name", "volume", "artifact", "digest"])?;
    if driver.get("name").and_then(Value::as_str) != Some(DRIVER_NAME) {
        return Err(ParentError::Config("read_unsupported"));
    }
    if driver.get("artifact").and_then(Value::as_str) != Some(ARTIFACT_NAME) {
        return Err(ParentError::Config("DRIVER_MISSING"));
    }
    let volume = required_str(driver, "volume")?;
    if !valid_absolute(volume) {
        return Err(ParentError::Config("driver_volume"));
    }
    let driver_digest = required_str(driver, "digest")?;
    if !approved_digest(driver_digest) {
        return Err(ParentError::Config("driver_digest"));
    }
    let budget = required_object(map, "budget", &[
        "batch_rows",
        "batch_bytes",
        "max_rows",
        "max_bytes",
        "deadline_ms",
    ])?;
    let batch_rows = required_u64(budget, "batch_rows")?;
    let batch_bytes = required_u64(budget, "batch_bytes")?;
    let max_rows = required_u64(budget, "max_rows")?;
    let max_bytes = required_u64(budget, "max_bytes")?;
    let deadline_ms = required_u64(budget, "deadline_ms")?;
    if !(1..=BATCH_ROWS_MAX).contains(&batch_rows)
        || !(1..=BATCH_BYTES_MAX).contains(&batch_bytes)
        || !(1..=MAX_ROWS_MAX).contains(&max_rows)
        || !(1..=MAX_BYTES_MAX).contains(&max_bytes)
        || !(1..=CHILD_DEADLINE_MAX_MS).contains(&deadline_ms)
        || batch_rows > max_rows
        || batch_bytes > max_bytes
    {
        return Err(ParentError::Config("budget"));
    }
    let read = required_object(map, "read", &["kind", "statement_ref"])?;
    if read.get("kind").and_then(Value::as_str) != Some(READ_KIND) {
        return Err(ParentError::Config("read_unsupported"));
    }
    let statement = required_str(read, "statement_ref")?;
    if !approved_statement(statement) {
        return Err(ParentError::Config("statement_ref"));
    }
    let mut body = ChildBody {
        source_credential_ref: source.to_owned(),
        target_credential_ref: target,
        image_digest: image.to_owned(),
        volume: volume.to_owned(),
        driver_digest: driver_digest.to_owned(),
        batch_rows,
        batch_bytes,
        max_rows,
        max_bytes,
        deadline_ms,
        statement_ref: statement.to_owned(),
        forwarded: Vec::new(),
    };
    body.forwarded = encode_child(&body)?;
    Ok(body)
}

pub fn encode_child(body: &ChildBody) -> Result<Vec<u8>, ParentError> {
    let text = format!(
        "{{\"protocol\":{protocol},\"source_credential_ref\":{source},\"image_digest\":{image},\"driver\":{{\"name\":{name},\"volume\":{volume},\"artifact\":{artifact},\"digest\":{digest}}},\"budget\":{{\"batch_rows\":{batch_rows},\"batch_bytes\":{batch_bytes},\"max_rows\":{max_rows},\"max_bytes\":{max_bytes},\"deadline_ms\":{deadline}}},\"read\":{{\"kind\":{kind},\"statement_ref\":{statement}}}}}",
        protocol = json_str(PROTOCOL)?,
        source = json_str(&body.source_credential_ref)?,
        image = json_str(&body.image_digest)?,
        name = json_str(DRIVER_NAME)?,
        volume = json_str(&body.volume)?,
        artifact = json_str(ARTIFACT_NAME)?,
        digest = json_str(&body.driver_digest)?,
        batch_rows = body.batch_rows,
        batch_bytes = body.batch_bytes,
        max_rows = body.max_rows,
        max_bytes = body.max_bytes,
        deadline = body.deadline_ms,
        kind = json_str(READ_KIND)?,
        statement = json_str(&body.statement_ref)?,
    );
    if text.len() > REQUEST_MAX_BYTES {
        return Err(ParentError::Config("request_too_large"));
    }
    if text.contains(&body.target_credential_ref) && !body.target_credential_ref.is_empty() {
        return Err(ParentError::Config("forbidden_credential"));
    }
    Ok(text.into_bytes())
}

pub fn driver_schema() -> Schema {
    Schema::new(vec![
        Field::new("k", DataType::Int64, false),
        Field::new("v", DataType::Utf8, true),
    ])
}

pub fn schema_fingerprint(schema: &Schema) -> Result<[u8; 32], ParentError> {
    if schema.fields().len() != 2 || !schema.metadata().is_empty() {
        return Err(ParentError::Config("schema"));
    }
    let mut text = String::from(SCHEMA_ID);
    text.push('\n');
    for field in schema.fields() {
        if !field.metadata().is_empty() {
            return Err(ParentError::Config("schema"));
        }
        let kind = match field.data_type() {
            DataType::Int64 => "int64",
            DataType::Utf8 => "utf8",
            _ => return Err(ParentError::Config("schema")),
        };
        text.push_str(field.name());
        text.push(':');
        text.push_str(kind);
        text.push(':');
        text.push_str(if field.is_nullable() { "true" } else { "false" });
        text.push('\n');
    }
    let expected = "oo.source.driver-schema/v1\nk:int64:false\nv:utf8:true\n";
    if text != expected {
        return Err(ParentError::Config("schema"));
    }
    Ok(sha256(text.as_bytes()))
}

pub fn fingerprint_hex() -> Result<String, ParentError> {
    Ok(hex(&schema_fingerprint(&driver_schema())?))
}

pub fn rows_batch(rows: &[(i64, Option<&str>)]) -> Result<RecordBatch, ParentError> {
    if rows.is_empty() {
        return Err(ParentError::Config("protocol"));
    }
    let keys = Int64Array::from(rows.iter().map(|row| row.0).collect::<Vec<_>>());
    let values = StringArray::from(
        rows.iter()
            .map(|row| row.1)
            .collect::<Vec<_>>(),
    );
    RecordBatch::try_new(
        Arc::new(driver_schema()),
        vec![Arc::new(keys), Arc::new(values)],
    )
    .map_err(|_| ParentError::Config("schema"))
}

pub fn encode_ipc(batch: &RecordBatch, cap: usize) -> Result<(Vec<u8>, usize), ParentError> {
    let memory: usize = batch
        .columns()
        .iter()
        .map(|column| column.get_array_memory_size())
        .sum();
    if memory == 0 || memory > cap {
        return Err(ParentError::Config("budget"));
    }
    let options = IpcWriteOptions::try_new(IPC_ALIGNMENT, false, MetadataVersion::V5)
        .map_err(|_| ParentError::Config("schema"))?;
    let sink = BoundedWriter::new(cap);
    let mut writer = StreamWriter::try_new_with_options(sink, batch.schema().as_ref(), options)
        .map_err(|_| ParentError::Config("schema"))?;
    writer.write(batch).map_err(|_| ParentError::Config("schema"))?;
    writer.finish().map_err(|_| ParentError::Config("schema"))?;
    let sink = writer.into_inner().map_err(|_| ParentError::Config("schema"))?;
    if sink.failed() {
        return Err(ParentError::Config("budget"));
    }
    let bytes = sink.into_bytes();
    if bytes.is_empty() || bytes.len() > cap {
        return Err(ParentError::Config("budget"));
    }
    Ok((bytes, memory))
}

pub fn decode_one_batch(payload: &[u8]) -> Result<RecordBatch, ()> {
    let cursor = io::Cursor::new(payload);
    let mut reader = StreamReader::try_new(cursor, None).map_err(|_| ())?;
    let Some(first) = reader.next() else {
        return Err(());
    };
    let batch = first.map_err(|_| ())?;
    if reader.next().is_some() {
        return Err(());
    }
    schema_fingerprint(batch.schema().as_ref()).map_err(|_| ())?;
    Ok(batch)
}

pub fn lineage_text(index: u64, rows: u64, arrow_bytes: usize, fingerprint: &str) -> String {
    format!(
        "batch_index={index}\nrows={rows}\narrow_bytes={arrow_bytes}\nschema_fingerprint={fingerprint}\n"
    )
}

pub fn completion_text(
    statement_ref: &str,
    rows: u64,
    batches: u64,
    fingerprint: &str,
    driver_digest: &str,
    image_digest: &str,
) -> String {
    let arrow_ipc = if batches == 0 { "false" } else { "true" };
    format!(
        "\
protocol={PROTOCOL}
statement_ref={statement_ref}
rows={rows}
batches={batches}
ipc_frames={batches}
lineage_frames={batches}
arrow_ipc={arrow_ipc}
schema_fingerprint={fingerprint}
driver_digest={driver_digest}
image_digest={image_digest}
source_credential_forwarded=true
target_credential_forwarded=false
agent_db_credential_forwarded=false
model_credential_forwarded=false
gateway_credential_forwarded=false
fallback=false
admits_import=false
published=false
pg_run_complete=false
kernel_network=pending
vendor_library_loaded=false
"
    )
}

pub fn write_frame(mut stdout: impl Write, tag: u32, payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() || payload.len() > u32::MAX as usize {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame"));
    }
    let len = payload.len() as u32;
    stdout.write_all(&tag.to_le_bytes())?;
    stdout.write_all(&len.to_le_bytes())?;
    stdout.write_all(payload)?;
    Ok(())
}

pub fn file_digest(path: &Path, max_bytes: u64) -> Result<String, ParentError> {
    let hash = hash_file(path, max_bytes)?;
    Ok(format!("sha256:{}", hex(&hash)))
}

pub fn hash_file(path: &Path, max_bytes: u64) -> Result<[u8; 32], ParentError> {
    let mut file = File::open(path).map_err(|_| ParentError::Config("io"))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 8192];
    loop {
        let read = file.read(&mut buf).map_err(|_| ParentError::Config("io"))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(ParentError::Config("driver_too_large"));
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.finalize().into())
}

pub fn approved_digest(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("sha256:") else {
        return false;
    };
    rest.len() == 64 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn approved_ref(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("secret://") else {
        return false;
    };
    (1..=200).contains(&rest.len())
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'.' | b'-'))
}

pub fn approved_statement(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
}

pub fn valid_absolute(path: &str) -> bool {
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

pub fn path_text(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn parse_value(bytes: &[u8]) -> Result<Value, ParentError> {
    if bytes.is_empty() || bytes.len() > REQUEST_MAX_BYTES {
        return Err(ParentError::Config("request_syntax"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ParentError::Config("request_syntax"))?;
    match reject_duplicate_keys(text) {
        Ok(()) => {}
        Err(ScanErr::Duplicate) => return Err(ParentError::Config("duplicate_key")),
        Err(ScanErr::Syntax) => return Err(ParentError::Config("request_syntax")),
    }
    serde_json::from_str(text).map_err(|_| ParentError::Config("request_syntax"))
}

fn reject_forbidden(value: &Value) -> Result<(), ParentError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN.contains(&key.as_str()) {
                    return Err(ParentError::Config("forbidden_credential"));
                }
                reject_forbidden(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_forbidden(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ParentError> {
    let Some(map) = value.as_object() else {
        return Err(ParentError::Config("request_syntax"));
    };
    for key in map.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(ParentError::Config("unknown_key"));
        }
    }
    for key in keys {
        if !map.contains_key(*key) {
            return Err(ParentError::Config("protocol"));
        }
    }
    Ok(map)
}

fn required_object<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    keys: &[&str],
) -> Result<&'a Map<String, Value>, ParentError> {
    let Some(value) = map.get(key) else {
        return Err(ParentError::Config("protocol"));
    };
    exact_object(value, keys)
}

fn required_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, ParentError> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or(ParentError::Config("protocol"))
}

fn required_u64(map: &Map<String, Value>, key: &str) -> Result<u64, ParentError> {
    let Some(value) = map.get(key) else {
        return Err(ParentError::Config("budget"));
    };
    value.as_u64().ok_or(ParentError::Config("budget"))
}

fn json_str(text: &str) -> Result<String, ParentError> {
    serde_json::to_string(text).map_err(|_| ParentError::Config("protocol"))
}

enum ScanErr {
    Duplicate,
    Syntax,
}

fn reject_duplicate_keys(text: &str) -> Result<(), ScanErr> {
    let mut scan = Scan { bytes: text.as_bytes(), index: 0 };
    scan.skip();
    scan.value()?;
    scan.skip();
    if scan.index != scan.bytes.len() {
        return Err(ScanErr::Syntax);
    }
    Ok(())
}

struct Scan<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl Scan<'_> {
    fn skip(&mut self) {
        while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_whitespace() {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn value(&mut self) -> Result<(), ScanErr> {
        self.skip();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err(ScanErr::Syntax),
        }
    }

    fn object(&mut self) -> Result<(), ScanErr> {
        self.index += 1;
        let mut keys = HashSet::new();
        loop {
            self.skip();
            if self.peek() == Some(b'}') {
                self.index += 1;
                return Ok(());
            }
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(ScanErr::Duplicate);
            }
            self.skip();
            if self.peek() != Some(b':') {
                return Err(ScanErr::Syntax);
            }
            self.index += 1;
            self.value()?;
            self.skip();
            match self.peek() {
                Some(b',') => {
                    self.index += 1;
                }
                Some(b'}') => {
                    self.index += 1;
                    return Ok(());
                }
                _ => return Err(ScanErr::Syntax),
            }
        }
    }

    fn array(&mut self) -> Result<(), ScanErr> {
        self.index += 1;
        loop {
            self.skip();
            if self.peek() == Some(b']') {
                self.index += 1;
                return Ok(());
            }
            self.value()?;
            self.skip();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    return Ok(());
                }
                _ => return Err(ScanErr::Syntax),
            }
        }
    }

    fn string(&mut self) -> Result<String, ScanErr> {
        if self.peek() != Some(b'"') {
            return Err(ScanErr::Syntax);
        }
        self.index += 1;
        let mut out = String::new();
        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];
            self.index += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let next = self.bytes.get(self.index).copied().ok_or(ScanErr::Syntax)?;
                    self.index += 1;
                    match next {
                        b'"' | b'\\' | b'/' => out.push(next as char),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.bytes.get(self.index..self.index + 4).ok_or(ScanErr::Syntax)?;
                            self.index += 4;
                            let text = std::str::from_utf8(hex).map_err(|_| ScanErr::Syntax)?;
                            let code = u32::from_str_radix(text, 16).map_err(|_| ScanErr::Syntax)?;
                            out.push(char::from_u32(code).ok_or(ScanErr::Syntax)?);
                        }
                        _ => return Err(ScanErr::Syntax),
                    }
                }
                byte if byte < 0x20 => return Err(ScanErr::Syntax),
                byte if byte >= 0x80 => return Err(ScanErr::Syntax),
                byte => out.push(byte as char),
            }
        }
        Err(ScanErr::Syntax)
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), ScanErr> {
        if self.bytes.get(self.index..self.index + word.len()) != Some(word) {
            return Err(ScanErr::Syntax);
        }
        self.index += word.len();
        Ok(())
    }

    fn number(&mut self) -> Result<(), ScanErr> {
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        let start = self.index;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.index += 1;
        }
        if self.index == start {
            return Err(ScanErr::Syntax);
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let frac = self.index;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == frac {
                return Err(ScanErr::Syntax);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exp = self.index;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == exp {
                return Err(ScanErr::Syntax);
            }
        }
        Ok(())
    }
}

struct BoundedWriter {
    buf: Vec<u8>,
    cap: usize,
    failed: bool,
}

impl BoundedWriter {
    fn new(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(1024.min(cap)),
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
            return Err(io::Error::new(io::ErrorKind::QuotaExceeded, "ipc_ceiling"));
        }
        let Some(next) = self.buf.len().checked_add(data.len()) else {
            self.failed = true;
            return Err(io::Error::new(io::ErrorKind::QuotaExceeded, "ipc_ceiling"));
        };
        if next > self.cap {
            self.failed = true;
            return Err(io::Error::new(io::ErrorKind::QuotaExceeded, "ipc_ceiling"));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
