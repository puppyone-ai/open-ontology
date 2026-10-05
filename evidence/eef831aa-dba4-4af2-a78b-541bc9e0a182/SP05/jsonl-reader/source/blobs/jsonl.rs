//! Bounded JSONL pull. Protocol `oo.source.jsonl/v1`.
//!
//! The caller supplies an immutable profile and the SHA-256 of the whole
//! object. This reader does not infer a format, a column type, or a semantic
//! class. It projects flat JSON objects into one series-60 `RecordBatch` per
//! pull. `JsonlComplete` is minted only after a clean EOF, every selected
//! column was covered, and the streamed byte hash matches. A batch is not
//! that token. This parser does not import, publish, or open a database.

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use arrow_array::{Array, ArrayRef, BooleanArray, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use oo_core::{DecimalError, ExactDecimal};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// JSONL source protocol. This is not `oo.source.pgsql/v1`.
pub const JSONL_PROTOCOL: &str = "oo.source.jsonl/v1";

/// Profile identity version. The preimage is documented in the contract.
pub const JSONL_PROFILE_VERSION: &str = "oo.source.jsonl.profile/v1";

/// Row and field lineage carried on each batch.
pub const JSONL_LINEAGE_VERSION: &str = "oo.source.jsonl.lineage/v1";

/// Header and completion receipt version.
pub const JSONL_RECEIPT_VERSION: &str = "oo.source.jsonl.receipt/v1";

/// Hard cap for one pull's row count. The caller budget is at most this.
pub const JSONL_MAX_BATCH_ROWS: u32 = 65_536;

/// Hard cap for the raw record bytes accumulated in one batch.
pub const JSONL_MAX_BATCH_BYTES: u64 = 16 * 1024 * 1024;

/// Hard cap for one JSON value, including its surrounding whitespace inside the value.
pub const JSONL_MAX_RECORD_BYTES: u32 = 1_048_576;

/// Hard cap for hashed input bytes. This is not an allocation of that size.
pub const JSONL_MAX_TOTAL_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// Hard cap for rows emitted by one reader.
pub const JSONL_MAX_ROWS: u64 = 16_777_216;

/// Hard cap for profile columns and for `max_columns`.
pub const JSONL_MAX_COLUMNS: u32 = 256;

/// Hard cap for keys in one JSON object.
pub const JSONL_MAX_FIELDS: u32 = 256;

/// Nesting depth for a declared raw JSON value. The top-level record is flat.
pub const JSONL_MAX_DEPTH: usize = 32;

/// Elements in one array inside a raw JSON value.
pub const JSONL_MAX_ARRAY_ELEMENTS: u32 = 100_000;

/// Maximum byte length of a profile id or a column name.
pub const JSONL_MAX_NAME_BYTES: usize = 256;

const PROFILE_PREFIX: &str = "oo-jsonl-profile/v1\n";
const CONFIG_PREFIX: &str = "oo-jsonl-config-identity/v1\n";

/// Closed JSONL failure. Display is a kind and a static fact. It has no row bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonlError {
    kind: JsonlErrorKind,
    fact: &'static str,
}

/// Class of a JSONL failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonlErrorKind {
    /// The profile or budget is rejected before a read.
    Config,
    /// A row, byte, field, depth, or deadline limit failed.
    Budget,
    /// The bytes are not the profile's JSONL contract.
    Protocol,
    /// The input is not strict UTF-8.
    Encoding,
    /// The full-object SHA-256 did not match. Compared only at EOF.
    Hash,
    /// The caller cancelled the pull.
    Cancelled,
    /// The underlying reader failed. The OS message is dropped.
    Io,
}

impl JsonlError {
    const fn new(kind: JsonlErrorKind, fact: &'static str) -> Self {
        Self { kind, fact }
    }

    pub const fn config(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Config, fact)
    }

    pub const fn budget(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Budget, fact)
    }

    pub const fn protocol(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Protocol, fact)
    }

    pub const fn encoding(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Encoding, fact)
    }

    pub const fn hash(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Hash, fact)
    }

    pub const fn cancelled(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Cancelled, fact)
    }

    pub const fn io(fact: &'static str) -> Self {
        Self::new(JsonlErrorKind::Io, fact)
    }

    pub const fn kind(self) -> JsonlErrorKind {
        self.kind
    }

    pub const fn fact(self) -> &'static str {
        self.fact
    }
}

impl std::fmt::Display for JsonlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "jsonl error: {} fact={}",
            kind_name(self.kind),
            self.fact
        )
    }
}

impl std::error::Error for JsonlError {}

fn kind_name(kind: JsonlErrorKind) -> &'static str {
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

/// Column types this slice will project. There is no float and no decimal variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonlType {
    /// A JSON string. Objects and arrays are refused, not coerced.
    Utf8,
    /// JSON `true` or `false` only.
    Bool,
    /// A JSON number whose lexical form is an exact `i64`. Fractions and exponents are refused.
    Int64,
    /// The raw JSON text of one value. Nested JSON stays text. This is not a typed coercion.
    JsonText,
}

impl JsonlType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Bool => "bool",
            Self::Int64 => "int64",
            Self::JsonText => "json_text",
        }
    }
}

/// One selected column. The name is the JSON object key after string decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonlColumn {
    name: String,
    ty: JsonlType,
    nullable: bool,
}

impl JsonlColumn {
    /// Reject an empty name, surrounding whitespace, ASCII controls, and names past 256 bytes.
    pub fn try_new(name: &str, ty: JsonlType, nullable: bool) -> Result<Self, JsonlError> {
        if !valid_label(name) {
            return Err(JsonlError::config("column_name"));
        }
        Ok(Self {
            name: name.to_owned(),
            ty,
            nullable,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ty(&self) -> JsonlType {
        self.ty
    }

    pub fn nullable(&self) -> bool {
        self.nullable
    }
}

/// Immutable projection. Column order is the Arrow order. Names are exact and unique.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonlProfile {
    id: String,
    columns: Vec<JsonlColumn>,
}

impl JsonlProfile {
    /// Reject an empty selection, a duplicate name, and an ambiguous label.
    /// The profile has no PII or semantic-class field and does not infer one.
    pub fn try_new(id: &str, columns: Vec<JsonlColumn>) -> Result<Self, JsonlError> {
        if !valid_label(id) {
            return Err(JsonlError::config("profile_id"));
        }
        if columns.is_empty() {
            return Err(JsonlError::config("empty_selection"));
        }
        if columns.len() > JSONL_MAX_COLUMNS as usize {
            return Err(JsonlError::config("column_count"));
        }
        for (index, column) in columns.iter().enumerate() {
            if columns[..index].iter().any(|other| other.name == column.name) {
                return Err(JsonlError::config("duplicate_column"));
            }
        }
        Ok(Self {
            id: id.to_owned(),
            columns,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn columns(&self) -> &[JsonlColumn] {
        &self.columns
    }

    /// SHA-256 of the profile preimage. Budgets and the object hash are not included.
    pub fn profile_digest(&self) -> [u8; 32] {
        sha256(profile_preimage(self).as_bytes())
    }

    /// SHA-256 of the profile preimage plus the expected object hash.
    /// Pull budgets are not part of this identity.
    pub fn config_identity(&self, expected_sha256: [u8; 32]) -> [u8; 32] {
        sha256(config_preimage(self, expected_sha256).as_bytes())
    }
}

fn valid_label(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= JSONL_MAX_NAME_BYTES
        && text.trim() == text
        && bytes.iter().all(|byte| (0x20..0x7f).contains(byte) || *byte >= 0x80)
        && !text.chars().any(|ch| ch.is_whitespace() && ch != ' ')
}

fn profile_preimage(profile: &JsonlProfile) -> String {
    let mut body = String::from(PROFILE_PREFIX);
    body.push_str("id=");
    body.push_str(&profile.id);
    body.push('\n');
    for column in &profile.columns {
        body.push_str("column=");
        body.push_str(&column.name);
        body.push('\t');
        body.push_str(column.ty.as_str());
        body.push('\t');
        body.push(if column.nullable { '1' } else { '0' });
        body.push('\n');
    }
    body
}

fn config_preimage(profile: &JsonlProfile, expected_sha256: [u8; 32]) -> String {
    let mut body = String::from(CONFIG_PREFIX);
    body.push_str(&profile_preimage(profile));
    body.push_str("expected_sha256=");
    body.push_str(&hex_encode(&expected_sha256));
    body.push('\n');
    body
}

fn read_into(inner: &mut impl Read, buf: &mut [u8]) -> Result<usize, JsonlError> {
    loop {
        match inner.read(buf) {
            Ok(read) => return Ok(read),
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(JsonlError::io("read")),
        }
    }
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

/// Caps for one pull. `queue_batches` is 1 and is not a parameter.
/// These fields are not part of [`JsonlProfile::config_identity`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonlBudget {
    batch_rows: u32,
    batch_bytes: u64,
    max_record_bytes: u32,
    max_total_bytes: u64,
    max_rows: u64,
    max_columns: u32,
    max_fields: u32,
    deadline: Instant,
}

impl JsonlBudget {
    /// Reject a zero cap and any cap above the hard ceiling.
    /// `max_total_bytes` must be at least one maximum record.
    /// The eight arguments are the budget fields. They stay explicit so a caller
    /// cannot omit a cap.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        batch_rows: u32,
        batch_bytes: u64,
        max_record_bytes: u32,
        max_total_bytes: u64,
        max_rows: u64,
        max_columns: u32,
        max_fields: u32,
        deadline: Instant,
    ) -> Result<Self, JsonlError> {
        if batch_rows == 0 || batch_rows > JSONL_MAX_BATCH_ROWS {
            return Err(JsonlError::config("batch_rows"));
        }
        if batch_bytes == 0 || batch_bytes > JSONL_MAX_BATCH_BYTES {
            return Err(JsonlError::config("batch_bytes"));
        }
        if max_record_bytes == 0 || max_record_bytes > JSONL_MAX_RECORD_BYTES {
            return Err(JsonlError::config("max_record"));
        }
        if max_total_bytes < u64::from(max_record_bytes) || max_total_bytes > JSONL_MAX_TOTAL_BYTES
        {
            return Err(JsonlError::config("max_total"));
        }
        if max_rows == 0 || max_rows > JSONL_MAX_ROWS {
            return Err(JsonlError::config("max_rows"));
        }
        if max_columns == 0 || max_columns > JSONL_MAX_COLUMNS {
            return Err(JsonlError::config("max_columns"));
        }
        if max_fields == 0 || max_fields > JSONL_MAX_FIELDS {
            return Err(JsonlError::config("max_fields"));
        }
        Ok(Self {
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            deadline,
        })
    }

    pub fn batch_rows(self) -> u32 {
        self.batch_rows
    }

    pub fn batch_bytes(self) -> u64 {
        self.batch_bytes
    }

    pub fn max_record_bytes(self) -> u32 {
        self.max_record_bytes
    }

    pub fn max_total_bytes(self) -> u64 {
        self.max_total_bytes
    }

    pub fn max_rows(self) -> u64 {
        self.max_rows
    }

    pub fn max_columns(self) -> u32 {
        self.max_columns
    }

    pub fn max_fields(self) -> u32 {
        self.max_fields
    }

    pub fn deadline(self) -> Instant {
        self.deadline
    }

    /// The pull retains one batch. This is not configurable.
    pub fn queue_batches(self) -> u32 {
        1
    }
}

/// Versioned description of the profile. Import and publication are false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonlHeader {
    pub protocol: &'static str,
    pub profile_version: &'static str,
    pub lineage_version: &'static str,
    pub receipt_version: &'static str,
    pub profile_id: String,
    pub profile_digest: [u8; 32],
    pub config_identity: [u8; 32],
    pub columns: Vec<String>,
    pub types: Vec<JsonlType>,
    pub nullability: Vec<bool>,
    /// Always false. A pure parse does not admit a database load.
    pub admits_import: bool,
    /// Always false. A pure parse does not publish a table.
    pub published: bool,
    /// Always false. This slice builds a `RecordBatch` and does not write Arrow IPC.
    pub arrow_ipc: bool,
    /// Always false. The parser does not execute a native payload.
    pub executes_native: bool,
}

/// How a profile column was present on one record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPresence {
    /// A JSON value other than `null`.
    Value,
    /// JSON `null` on a nullable column.
    Null,
    /// The key was absent on a nullable column.
    Missing,
}

/// One selected field on one record. `name` is the profile name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLineage {
    pub name: String,
    pub line: u64,
    pub byte_offset: u64,
    pub presence: FieldPresence,
}

/// Lineage for one emitted row. `line` is the 1-based line of the record's `{`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowLineage {
    pub row_index: u64,
    pub line: u64,
    pub byte_offset: u64,
    pub fields: Vec<FieldLineage>,
}

/// One demanded batch. `resumable` is false. This is not [`JsonlComplete`].
#[allow(exported_private_dependencies)]
#[derive(Debug)]
pub struct JsonlBatch {
    pub batch_index: u64,
    pub record_batch: RecordBatch,
    pub lineage: Vec<RowLineage>,
    pub raw_bytes: u64,
    pub arrow_bytes: u64,
    pub header: JsonlHeader,
    pub resumable: bool,
}

/// The pull has no further record. This is not [`JsonlComplete`].
/// There is no boolean EOF admission on this value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct JsonlExhausted {
    pub rows: u64,
    pub batches: u64,
    pub object_sha256: [u8; 32],
    seal: ExhaustSeal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExhaustSeal;

/// One answer from [`JsonlReader::next_batch`].
#[allow(exported_private_dependencies)]
#[derive(Debug)]
pub enum JsonlItem {
    /// Boxed so the exhaustion variant is not the size of a record batch.
    Batch(Box<JsonlBatch>),
    Exhausted(JsonlExhausted),
}

/// Proof that the object was read to EOF, the hash matched, and every emitted
/// row covered the profile. Callers cannot construct it. `admits_import` and
/// `published` stay false: the proof is about the parse, not a database load.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct JsonlComplete {
    pub protocol: &'static str,
    pub profile_version: &'static str,
    pub lineage_version: &'static str,
    pub receipt_version: &'static str,
    pub profile_id: String,
    pub profile_digest: [u8; 32],
    pub config_identity: [u8; 32],
    pub object_sha256: [u8; 32],
    pub rows: u64,
    pub columns: u32,
    pub batches: u64,
    pub admits_import: bool,
    pub published: bool,
    seal: CompleteSeal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompleteSeal;

/// The owned reader was released without a completion token.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct JsonlCancelled {
    pub profile_id: String,
    pub rows_emitted: u64,
    pub released: bool,
    seal: CancelSeal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CancelSeal;

/// Caller handle. [`JsonlCancel::request`] only sets the flag.
#[derive(Clone, Debug)]
pub struct JsonlCancel {
    flag: Arc<AtomicBool>,
}

impl JsonlCancel {
    pub fn request(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }
}

/// Pull-driven JSONL reader over any [`Read`].
///
/// The reader owns `inner` and drops it on [`JsonlReader::finish`],
/// [`JsonlReader::cancel`], and `Drop`. It does not call `read_to_end` or
/// `read_line`. One batch is allocated at a time. The object hash is updated
/// from raw bytes before those bytes are parsed, and it is compared to
/// `expected_sha256` only after a real EOF at a record boundary.
pub struct JsonlReader<R: Read> {
    inner: R,
    profile: JsonlProfile,
    expected: [u8; 32],
    budget: JsonlBudget,
    window: Vec<u8>,
    base: u64,
    pos: usize,
    len: usize,
    eof: bool,
    hasher: Sha256,
    total: u64,
    line: u64,
    col: u64,
    rows: u64,
    batches: u64,
    hash_ok: bool,
    object_sha256: [u8; 32],
    exhausted_delivered: bool,
    failed: Option<JsonlError>,
    cancel: Arc<AtomicBool>,
}

impl<R: Read> std::fmt::Debug for JsonlReader<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonlReader")
            .field("profile_id", &self.profile.id)
            .field("rows", &self.rows)
            .field("batches", &self.batches)
            .field("exhausted", &self.exhausted_delivered)
            .finish()
    }
}

impl<R: Read> Drop for JsonlReader<R> {
    fn drop(&mut self) {
        self.window.zeroize();
    }
}

impl<R: Read> JsonlReader<R> {
    /// Check the budget, the profile width, and the deadline, then allocate
    /// the single record window. No bytes are read here.
    pub fn open(
        inner: R,
        profile: JsonlProfile,
        expected_sha256: [u8; 32],
        budget: JsonlBudget,
    ) -> Result<Self, JsonlError> {
        if profile.columns.len() > budget.max_columns as usize {
            return Err(JsonlError::config("max_columns"));
        }
        if u32::try_from(profile.columns.len()).unwrap_or(u32::MAX) > budget.max_fields {
            return Err(JsonlError::config("max_fields"));
        }
        if Instant::now() >= budget.deadline {
            return Err(JsonlError::budget("deadline"));
        }
        let window = vec![0u8; budget.max_record_bytes as usize];
        Ok(Self {
            inner,
            profile,
            expected: expected_sha256,
            budget,
            window,
            base: 0,
            pos: 0,
            len: 0,
            eof: false,
            hasher: Sha256::new(),
            total: 0,
            line: 1,
            col: 1,
            rows: 0,
            batches: 0,
            hash_ok: false,
            object_sha256: [0; 32],
            exhausted_delivered: false,
            failed: None,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn header(&self) -> JsonlHeader {
        JsonlHeader {
            protocol: JSONL_PROTOCOL,
            profile_version: JSONL_PROFILE_VERSION,
            lineage_version: JSONL_LINEAGE_VERSION,
            receipt_version: JSONL_RECEIPT_VERSION,
            profile_id: self.profile.id.clone(),
            profile_digest: self.profile.profile_digest(),
            config_identity: self.profile.config_identity(self.expected),
            columns: self.profile.columns.iter().map(|column| column.name.clone()).collect(),
            types: self.profile.columns.iter().map(|column| column.ty).collect(),
            nullability: self
                .profile
                .columns
                .iter()
                .map(|column| column.nullable)
                .collect(),
            admits_import: false,
            published: false,
            arrow_ipc: false,
            executes_native: false,
        }
    }

    pub fn cancel_handle(&self) -> JsonlCancel {
        JsonlCancel {
            flag: Arc::clone(&self.cancel),
        }
    }

    /// Demand the next batch, or [`JsonlItem::Exhausted`] after a clean EOF.
    /// Exhaustion is not a completion token. A hash mismatch is returned only
    /// once EOF has been observed; an earlier batch from this pull can still
    /// have been delivered, and it does not admit the object.
    pub fn next_batch(&mut self) -> Result<JsonlItem, JsonlError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        if self.exhausted_delivered {
            return Ok(self.exhausted_item());
        }
        self.guard()?;
        let mut pending: Option<Pending> = None;
        loop {
            self.guard()?;
            self.skip_ws()?;
            if self.peek_between()?.is_none() {
                return self.finish_stream(pending);
            }
            if pending.is_none() && self.rows >= self.budget.max_rows {
                return Err(self.fail(JsonlError::budget("max_rows")));
            }
            let mark = self.mark();
            let row = self.parse_record()?;
            let pending_rows = pending.as_ref().map(|batch| batch.rows).unwrap_or(0);
            let pending_bytes = pending.as_ref().map(|batch| batch.bytes).unwrap_or(0);
            let accepted = self.rows + pending_rows;
            let batch_row_cap = u64::from(self.budget.batch_rows);
            let overflow_rows = pending_rows + 1 > batch_row_cap;
            let overflow_bytes = pending_bytes.saturating_add(row.raw_len) > self.budget.batch_bytes;
            let overflow_total = accepted + 1 > self.budget.max_rows;
            if pending_rows == 0 && overflow_bytes {
                return Err(self.fail(JsonlError::budget("batch_bytes")));
            }
            if pending_rows == 0 && overflow_total {
                return Err(self.fail(JsonlError::budget("max_rows")));
            }
            if pending_rows > 0 && (overflow_rows || overflow_bytes || overflow_total) {
                self.rewind(mark);
                let Some(batch) = pending.take() else {
                    return Err(self.fail(JsonlError::protocol("row_width")));
                };
                return self.emit(batch);
            }
            if pending.is_none() {
                pending = Some(self.begin_pending());
            }
            let Some(slot) = pending.as_mut() else {
                return Err(self.fail(JsonlError::protocol("row_width")));
            };
            self.push_row(slot, row)?;
            if slot.rows == u64::from(self.budget.batch_rows) {
                let Some(batch) = pending.take() else {
                    return Err(self.fail(JsonlError::protocol("row_width")));
                };
                return self.emit(batch);
            }
        }
    }

    /// Mint [`JsonlComplete`] only after [`JsonlItem::Exhausted`] was delivered
    /// and the hash matched. A partial pull, a cancel, and a hash mismatch
    /// return an error and no token. The reader is released either way.
    pub fn finish(self) -> Result<JsonlComplete, JsonlError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(JsonlError::cancelled("cancelled"));
        }
        if !self.exhausted_delivered || !self.hash_ok || self.object_sha256 != self.expected {
            return Err(JsonlError::protocol("not_exhausted"));
        }
        let columns = u32::try_from(self.profile.columns.len())
            .map_err(|_| JsonlError::protocol("column_count"))?;
        Ok(JsonlComplete {
            protocol: JSONL_PROTOCOL,
            profile_version: JSONL_PROFILE_VERSION,
            lineage_version: JSONL_LINEAGE_VERSION,
            receipt_version: JSONL_RECEIPT_VERSION,
            profile_id: self.profile.id.clone(),
            profile_digest: self.profile.profile_digest(),
            config_identity: self.profile.config_identity(self.expected),
            object_sha256: self.object_sha256,
            rows: self.rows,
            columns,
            batches: self.batches,
            admits_import: false,
            published: false,
            seal: CompleteSeal,
        })
    }

    /// Release the reader. This does not mint [`JsonlComplete`].
    pub fn cancel(self) -> JsonlCancelled {
        self.cancel.store(true, Ordering::Relaxed);
        JsonlCancelled {
            profile_id: self.profile.id.clone(),
            rows_emitted: self.rows,
            released: true,
            seal: CancelSeal,
        }
    }

    fn finish_stream(&mut self, pending: Option<Pending>) -> Result<JsonlItem, JsonlError> {
        let hash_err = self.note_hash().err();
        if let Some(batch) = pending
            && batch.rows > 0
        {
            if let Some(err) = hash_err {
                self.failed = Some(err);
            }
            return self.emit(batch);
        }
        if let Some(err) = self.failed {
            return Err(err);
        }
        if let Some(err) = hash_err {
            return Err(err);
        }
        self.exhausted_delivered = true;
        Ok(self.exhausted_item())
    }

    fn exhausted_item(&self) -> JsonlItem {
        JsonlItem::Exhausted(JsonlExhausted {
            rows: self.rows,
            batches: self.batches,
            object_sha256: self.object_sha256,
            seal: ExhaustSeal,
        })
    }

    fn note_hash(&mut self) -> Result<(), JsonlError> {
        if !self.eof {
            return Err(self.fail(JsonlError::protocol("truncated")));
        }
        if self.hash_ok {
            return Ok(());
        }
        if let Some(err) = self.failed {
            return Err(err);
        }
        let digest: [u8; 32] = self.hasher.clone().finalize().into();
        self.object_sha256 = digest;
        if digest != self.expected {
            return Err(self.fail(JsonlError::hash("hash_mismatch")));
        }
        self.hash_ok = true;
        Ok(())
    }

    fn begin_pending(&self) -> Pending {
        let capacity = self.budget.batch_rows as usize;
        Pending {
            cols: self
                .profile
                .columns
                .iter()
                .map(|column| ColBuf::empty(column.ty, capacity))
                .collect(),
            lineage: Vec::with_capacity(capacity),
            bytes: 0,
            rows: 0,
        }
    }

    fn push_row(&mut self, pending: &mut Pending, row: ParsedRow) -> Result<(), JsonlError> {
        if row.vals.len() != pending.cols.len() || row.fields.len() != pending.cols.len() {
            return Err(self.fail(JsonlError::protocol("row_width")));
        }
        for (buf, value) in pending.cols.iter_mut().zip(row.vals) {
            push_cell(buf, value).map_err(|err| self.fail(err))?;
        }
        let row_index = self.rows + pending.rows;
        pending.lineage.push(RowLineage {
            row_index,
            line: row.line,
            byte_offset: row.offset,
            fields: row.fields,
        });
        pending.bytes = pending.bytes.saturating_add(row.raw_len);
        pending.rows += 1;
        Ok(())
    }

    fn emit(&mut self, pending: Pending) -> Result<JsonlItem, JsonlError> {
        let record_batch = self.build_batch(pending.cols)?;
        let arrow_bytes = record_batch
            .columns()
            .iter()
            .map(|column| column.get_array_memory_size() as u64)
            .sum();
        let batch = JsonlBatch {
            batch_index: self.batches,
            record_batch,
            lineage: pending.lineage,
            raw_bytes: pending.bytes,
            arrow_bytes,
            header: self.header(),
            resumable: false,
        };
        self.rows += pending.rows;
        self.batches += 1;
        Ok(JsonlItem::Batch(Box::new(batch)))
    }

    fn build_batch(&mut self, cols: Vec<ColBuf>) -> Result<RecordBatch, JsonlError> {
        let mut fields = Vec::with_capacity(cols.len());
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(cols.len());
        for (column, buf) in self.profile.columns.iter().zip(cols) {
            let (data_type, array, logical) = match buf {
                ColBuf::Utf8(values) => (
                    DataType::Utf8,
                    Arc::new(StringArray::from(values)) as ArrayRef,
                    "utf8",
                ),
                ColBuf::Json(values) => (
                    DataType::Utf8,
                    Arc::new(StringArray::from(values)) as ArrayRef,
                    "json_text",
                ),
                ColBuf::Bool(values) => (
                    DataType::Boolean,
                    Arc::new(BooleanArray::from(values)) as ArrayRef,
                    "bool",
                ),
                ColBuf::Int(values) => (
                    DataType::Int64,
                    Arc::new(Int64Array::from(values)) as ArrayRef,
                    "int64",
                ),
            };
            let mut metadata = HashMap::new();
            metadata.insert("oo.jsonl.protocol".to_owned(), JSONL_PROTOCOL.to_owned());
            metadata.insert("oo.jsonl.logical".to_owned(), logical.to_owned());
            if logical == "json_text" {
                metadata.insert(
                    "oo.jsonl.encoding".to_owned(),
                    "raw-json-utf8".to_owned(),
                );
            }
            fields.push(
                Field::new(&column.name, data_type, column.nullable).with_metadata(metadata),
            );
            arrays.push(array);
        }
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
            .map_err(|_| self.fail(JsonlError::protocol("arrow_batch")))
    }

    fn guard(&mut self) -> Result<(), JsonlError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(self.fail(JsonlError::cancelled("cancelled")));
        }
        if Instant::now() >= self.budget.deadline {
            return Err(self.fail(JsonlError::budget("deadline")));
        }
        Ok(())
    }

    fn fail(&mut self, err: JsonlError) -> JsonlError {
        if self.failed.is_none() {
            self.failed = Some(err);
        }
        self.failed.unwrap_or(err)
    }

    fn mark(&self) -> Mark {
        Mark {
            abs: self.current_abs(),
            line: self.line,
            col: self.col,
        }
    }

    fn current_abs(&self) -> u64 {
        self.base + self.pos as u64
    }

    fn rewind(&mut self, mark: Mark) {
        let index = (mark.abs - self.base) as usize;
        self.pos = index;
        self.line = mark.line;
        self.col = mark.col;
    }

    fn skip_ws(&mut self) -> Result<(), JsonlError> {
        loop {
            let keep = self.current_abs();
            match self.peek_keep(keep)? {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.bump(),
                _ => return Ok(()),
            }
        }
    }

    fn skip_ws_keep(&mut self, keep: u64) -> Result<(), JsonlError> {
        loop {
            match self.peek_keep(keep)? {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.bump(),
                _ => return Ok(()),
            }
        }
    }

    fn peek_between(&mut self) -> Result<Option<u8>, JsonlError> {
        let keep = self.current_abs();
        self.peek_keep(keep)
    }

    fn peek_keep(&mut self, keep_abs: u64) -> Result<Option<u8>, JsonlError> {
        if self.pos >= self.len && !self.eof {
            self.pull_bytes(keep_abs)?;
        }
        if self.pos >= self.len {
            if self.eof {
                return Ok(None);
            }
            return Err(self.fail(JsonlError::protocol("read_stall")));
        }
        Ok(Some(self.window[self.pos]))
    }

    fn bump(&mut self) {
        let byte = self.window[self.pos];
        self.pos += 1;
        if byte == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
    }

    fn pull_bytes(&mut self, keep_abs: u64) -> Result<(), JsonlError> {
        self.guard()?;
        if keep_abs < self.base {
            return Err(self.fail(JsonlError::protocol("offset")));
        }
        let keep = match usize::try_from(keep_abs - self.base) {
            Ok(keep) => keep,
            Err(_) => return Err(self.fail(JsonlError::protocol("offset"))),
        };
        if keep > self.pos || keep > self.len {
            return Err(self.fail(JsonlError::protocol("offset")));
        }
        self.slide(keep);
        if self.eof {
            return Ok(());
        }
        if self.total >= self.budget.max_total_bytes {
            let mut probe = [0u8; 1];
            let read = self.read_some(&mut probe)?;
            if read == 0 {
                self.eof = true;
                return Ok(());
            }
            return Err(self.fail(JsonlError::budget("max_total")));
        }
        if self.len == self.window.len() {
            return Err(self.fail(JsonlError::budget("max_record")));
        }
        let room = self.window.len() - self.len;
        let remain = usize::try_from(self.budget.max_total_bytes - self.total).unwrap_or(room);
        let allow = room.min(remain);
        let start = self.len;
        let read = match read_into(&mut self.inner, &mut self.window[start..start + allow]) {
            Ok(read) => read,
            Err(err) => return Err(self.fail(err)),
        };
        if read == 0 {
            self.eof = true;
            return Ok(());
        }
        self.hasher.update(&self.window[start..start + read]);
        self.len += read;
        self.total += read as u64;
        Ok(())
    }

    fn slide(&mut self, keep_from: usize) {
        if keep_from == 0 {
            return;
        }
        self.window.copy_within(keep_from..self.len, 0);
        self.len -= keep_from;
        self.pos -= keep_from;
        self.base += keep_from as u64;
    }

    fn read_some(&mut self, buf: &mut [u8]) -> Result<usize, JsonlError> {
        read_into(&mut self.inner, buf).map_err(|err| self.fail(err))
    }

    fn parse_record(&mut self) -> Result<ParsedRow, JsonlError> {
        let mark = self.mark();
        let keep = mark.abs;
        match self.peek_keep(keep)? {
            Some(b'{') => self.bump(),
            Some(_) => return Err(self.fail(JsonlError::protocol("not_object"))),
            None => return Err(self.fail(JsonlError::protocol("truncated"))),
        }
        let width = self.profile.columns.len();
        let mut vals = Vec::<Option<CellVal>>::with_capacity(width);
        vals.resize_with(width, || None);
        let mut pos = vec![None; width];
        let mut seen_keys = 0u32;
        self.skip_ws_keep(keep)?;
        if self.eat(keep, b'}')? {
            return self.finish_row(mark, vals, pos);
        }
        loop {
            self.guard()?;
            if seen_keys >= self.budget.max_fields {
                return Err(self.fail(JsonlError::budget("max_fields")));
            }
            seen_keys += 1;
            self.skip_ws_keep(keep)?;
            let key_line = self.line;
            let key_offset = self.current_abs();
            if self.peek_keep(keep)? != Some(b'"') {
                return Err(self.fail(JsonlError::protocol("syntax")));
            }
            let key = self.parse_string(keep)?;
            let slot = self
                .profile
                .columns
                .iter()
                .position(|column| column.name == key);
            self.skip_ws_keep(keep)?;
            if !self.eat(keep, b':')? {
                return Err(self.fail(JsonlError::protocol("syntax")));
            }
            let Some(slot) = slot else {
                return Err(self.fail(JsonlError::protocol("unknown_key")));
            };
            if vals[slot].is_some() {
                return Err(self.fail(JsonlError::protocol("duplicate_key")));
            }
            vals[slot] = Some(self.parse_typed(keep, slot)?);
            pos[slot] = Some((key_line, key_offset));
            self.skip_ws_keep(keep)?;
            match self.peek_keep(keep)? {
                Some(b',') => {
                    self.bump();
                    self.skip_ws_keep(keep)?;
                    if self.peek_keep(keep)? == Some(b'}') {
                        return Err(self.fail(JsonlError::protocol("syntax")));
                    }
                }
                Some(b'}') => {
                    self.bump();
                    break;
                }
                Some(_) => return Err(self.fail(JsonlError::protocol("syntax"))),
                None => return Err(self.fail(JsonlError::protocol("truncated"))),
            }
        }
        self.finish_row(mark, vals, pos)
    }

    fn finish_row(
        &mut self,
        mark: Mark,
        vals: Vec<Option<CellVal>>,
        pos: Vec<Option<(u64, u64)>>,
    ) -> Result<ParsedRow, JsonlError> {
        let mut fields = Vec::with_capacity(vals.len());
        for (index, column) in self.profile.columns.iter().enumerate() {
            match &vals[index] {
                None => {
                    if !column.nullable {
                        return Err(self.fail(JsonlError::protocol("missing_key")));
                    }
                    fields.push(FieldLineage {
                        name: column.name.clone(),
                        line: mark.line,
                        byte_offset: mark.abs,
                        presence: FieldPresence::Missing,
                    });
                }
                Some(CellVal::Null) => {
                    let (line, offset) = pos[index].unwrap_or((mark.line, mark.abs));
                    fields.push(FieldLineage {
                        name: column.name.clone(),
                        line,
                        byte_offset: offset,
                        presence: FieldPresence::Null,
                    });
                }
                Some(_) => {
                    let (line, offset) = pos[index].unwrap_or((mark.line, mark.abs));
                    fields.push(FieldLineage {
                        name: column.name.clone(),
                        line,
                        byte_offset: offset,
                        presence: FieldPresence::Value,
                    });
                }
            }
        }
        Ok(ParsedRow {
            vals,
            fields,
            line: mark.line,
            offset: mark.abs,
            raw_len: self.current_abs() - mark.abs,
        })
    }

    fn parse_typed(&mut self, keep: u64, slot: usize) -> Result<CellVal, JsonlError> {
        self.skip_ws_keep(keep)?;
        let kind = self.profile.columns[slot].ty;
        let nullable = self.profile.columns[slot].nullable;
        let next = match self.peek_keep(keep)? {
            Some(byte) => byte,
            None => return Err(self.fail(JsonlError::protocol("truncated"))),
        };
        if next == b'n' {
            self.consume_lit(keep, b"null")?;
            if !nullable {
                return Err(self.fail(JsonlError::protocol("null_not_allowed")));
            }
            return Ok(CellVal::Null);
        }
        if next == b'{' || next == b'[' {
            if kind == JsonlType::JsonText {
                return Ok(CellVal::Json(self.capture_raw(keep)?));
            }
            return Err(self.fail(JsonlError::protocol("nested_refused")));
        }
        match kind {
            JsonlType::Utf8 => {
                if next == b'"' {
                    Ok(CellVal::Utf8(self.parse_string(keep)?))
                } else {
                    Err(self.fail(JsonlError::protocol("type_mismatch")))
                }
            }
            JsonlType::Bool => {
                if next == b't' {
                    self.consume_lit(keep, b"true")?;
                    Ok(CellVal::Bool(true))
                } else if next == b'f' {
                    self.consume_lit(keep, b"false")?;
                    Ok(CellVal::Bool(false))
                } else {
                    Err(self.fail(JsonlError::protocol("type_mismatch")))
                }
            }
            JsonlType::Int64 => {
                if next == b'-' || next.is_ascii_digit() {
                    self.parse_int64(keep)
                } else {
                    Err(self.fail(JsonlError::protocol("type_mismatch")))
                }
            }
            JsonlType::JsonText => Ok(CellVal::Json(self.capture_raw(keep)?)),
        }
    }

    fn parse_int64(&mut self, keep: u64) -> Result<CellVal, JsonlError> {
        let raw = self.scan_number_bytes(keep)?;
        let text = std::str::from_utf8(&raw)
            .map_err(|_| self.fail(JsonlError::protocol("invalid_number")))?;
        let fractional = raw
            .iter()
            .any(|byte| *byte == b'.' || *byte == b'e' || *byte == b'E');
        match ExactDecimal::parse_json_number(text) {
            Ok(_) if fractional => Err(self.fail(JsonlError::protocol("not_int64"))),
            Ok(_) => match text.parse::<i64>() {
                Ok(value) => Ok(CellVal::Int(value)),
                Err(_) => Err(self.fail(JsonlError::protocol("int64_overflow"))),
            },
            Err(DecimalError::Budget) => Err(self.fail(JsonlError::budget("number_budget"))),
            Err(_) => Err(self.fail(JsonlError::protocol("invalid_number"))),
        }
    }

    fn capture_raw(&mut self, keep: u64) -> Result<String, JsonlError> {
        self.skip_ws_keep(keep)?;
        let start = self.current_abs();
        self.scan_value(keep, 1)?;
        let end = self.current_abs();
        if end < start || start < self.base {
            return Err(self.fail(JsonlError::protocol("offset")));
        }
        let from = (start - self.base) as usize;
        let to = (end - self.base) as usize;
        let bytes = self.window[from..to].to_vec();
        String::from_utf8(bytes).map_err(|_| self.fail(JsonlError::encoding("utf8")))
    }

    fn scan_value(&mut self, keep: u64, depth: usize) -> Result<(), JsonlError> {
        if depth > JSONL_MAX_DEPTH {
            return Err(self.fail(JsonlError::budget("depth")));
        }
        match self.peek_keep(keep)? {
            Some(b'{') => self.scan_object(keep, depth),
            Some(b'[') => self.scan_array(keep, depth),
            Some(b'"') => self.parse_string(keep).map(|_| ()),
            Some(b't') => self.consume_lit(keep, b"true"),
            Some(b'f') => self.consume_lit(keep, b"false"),
            Some(b'n') => self.consume_lit(keep, b"null"),
            Some(b'-' | b'0'..=b'9') => self.scan_number_bytes(keep).map(|_| ()),
            Some(_) => Err(self.fail(JsonlError::protocol("syntax"))),
            None => Err(self.fail(JsonlError::protocol("truncated"))),
        }
    }

    fn scan_object(&mut self, keep: u64, depth: usize) -> Result<(), JsonlError> {
        self.consume_lit(keep, b"{")?;
        self.skip_ws_keep(keep)?;
        if self.eat(keep, b'}')? {
            return Ok(());
        }
        let mut keys = Vec::new();
        loop {
            if keys.len() >= self.budget.max_fields as usize {
                return Err(self.fail(JsonlError::budget("max_fields")));
            }
            self.skip_ws_keep(keep)?;
            if self.peek_keep(keep)? != Some(b'"') {
                return Err(self.fail(JsonlError::protocol("syntax")));
            }
            let key = self.parse_string(keep)?;
            if keys.iter().any(|seen| seen == &key) {
                return Err(self.fail(JsonlError::protocol("duplicate_key")));
            }
            keys.push(key);
            self.skip_ws_keep(keep)?;
            if !self.eat(keep, b':')? {
                return Err(self.fail(JsonlError::protocol("syntax")));
            }
            self.skip_ws_keep(keep)?;
            self.scan_value(keep, depth + 1)?;
            self.skip_ws_keep(keep)?;
            match self.peek_keep(keep)? {
                Some(b',') => {
                    self.bump();
                    self.skip_ws_keep(keep)?;
                    if self.peek_keep(keep)? == Some(b'}') {
                        return Err(self.fail(JsonlError::protocol("syntax")));
                    }
                }
                Some(b'}') => {
                    self.bump();
                    return Ok(());
                }
                Some(_) => return Err(self.fail(JsonlError::protocol("syntax"))),
                None => return Err(self.fail(JsonlError::protocol("truncated"))),
            }
        }
    }

    fn scan_array(&mut self, keep: u64, depth: usize) -> Result<(), JsonlError> {
        self.consume_lit(keep, b"[")?;
        self.skip_ws_keep(keep)?;
        if self.eat(keep, b']')? {
            return Ok(());
        }
        let mut count = 0u32;
        loop {
            if count >= JSONL_MAX_ARRAY_ELEMENTS {
                return Err(self.fail(JsonlError::budget("array_elements")));
            }
            count += 1;
            self.skip_ws_keep(keep)?;
            self.scan_value(keep, depth + 1)?;
            self.skip_ws_keep(keep)?;
            match self.peek_keep(keep)? {
                Some(b',') => {
                    self.bump();
                    self.skip_ws_keep(keep)?;
                    if self.peek_keep(keep)? == Some(b']') {
                        return Err(self.fail(JsonlError::protocol("syntax")));
                    }
                }
                Some(b']') => {
                    self.bump();
                    return Ok(());
                }
                Some(_) => return Err(self.fail(JsonlError::protocol("syntax"))),
                None => return Err(self.fail(JsonlError::protocol("truncated"))),
            }
        }
    }

    fn parse_string(&mut self, keep: u64) -> Result<String, JsonlError> {
        self.consume_lit(keep, b"\"")?;
        let mut out = String::new();
        let mut pending: Option<u16> = None;
        loop {
            let byte = match self.peek_keep(keep)? {
                Some(byte) => byte,
                None => return Err(self.fail(JsonlError::protocol("truncated"))),
            };
            if byte == b'"' {
                self.bump();
                if pending.is_some() {
                    return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
                }
                return Ok(out);
            }
            if byte == b'\\' {
                self.bump();
                let escaped = match self.peek_keep(keep)? {
                    Some(byte) => byte,
                    None => return Err(self.fail(JsonlError::protocol("truncated"))),
                };
                self.bump();
                match escaped {
                    b'"' | b'\\' | b'/' => self.push_char(&mut out, escaped as char)?,
                    b'b' => self.push_char(&mut out, '\u{0008}')?,
                    b'f' => self.push_char(&mut out, '\u{000c}')?,
                    b'n' => self.push_char(&mut out, '\n')?,
                    b'r' => self.push_char(&mut out, '\r')?,
                    b't' => self.push_char(&mut out, '\t')?,
                    b'u' => self.take_hex_unit(keep, &mut pending, &mut out)?,
                    _ => return Err(self.fail(JsonlError::protocol("invalid_escape"))),
                }
                continue;
            }
            if byte < 0x20 {
                return Err(self.fail(JsonlError::protocol("control_char")));
            }
            if pending.is_some() {
                return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
            }
            let ch = self.decode_char(keep)?;
            self.push_char(&mut out, ch)?;
        }
    }

    fn push_char(&mut self, out: &mut String, ch: char) -> Result<(), JsonlError> {
        if out.len().saturating_add(ch.len_utf8()) > self.budget.max_record_bytes as usize {
            return Err(self.fail(JsonlError::budget("max_record")));
        }
        out.push(ch);
        Ok(())
    }

    fn take_hex_unit(
        &mut self,
        keep: u64,
        pending: &mut Option<u16>,
        out: &mut String,
    ) -> Result<(), JsonlError> {
        let mut hex = [0u8; 4];
        for slot in &mut hex {
            let byte = match self.peek_keep(keep)? {
                Some(byte) if byte.is_ascii_hexdigit() => byte,
                Some(_) => return Err(self.fail(JsonlError::protocol("invalid_escape"))),
                None => return Err(self.fail(JsonlError::protocol("truncated"))),
            };
            self.bump();
            *slot = byte;
        }
        let text = std::str::from_utf8(&hex)
            .map_err(|_| self.fail(JsonlError::protocol("invalid_escape")))?;
        let unit = u32::from_str_radix(text, 16)
            .map_err(|_| self.fail(JsonlError::protocol("invalid_escape")))?;
        self.push_utf16(unit, pending, out)
    }

    fn push_utf16(
        &mut self,
        unit: u32,
        pending: &mut Option<u16>,
        out: &mut String,
    ) -> Result<(), JsonlError> {
        if (0xD800..=0xDBFF).contains(&unit) {
            if pending.is_some() {
                return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
            }
            *pending = Some(
                u16::try_from(unit)
                    .map_err(|_| self.fail(JsonlError::protocol("utf16_surrogate")))?,
            );
            return Ok(());
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            let Some(high) = pending.take() else {
                return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
            };
            let scalar = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (unit - 0xDC00);
            let Some(ch) = char::from_u32(scalar) else {
                return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
            };
            return self.push_char(out, ch);
        }
        if pending.is_some() {
            return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
        }
        let Some(ch) = char::from_u32(unit) else {
            return Err(self.fail(JsonlError::protocol("utf16_surrogate")));
        };
        self.push_char(out, ch)
    }

    fn decode_char(&mut self, keep: u64) -> Result<char, JsonlError> {
        let first = match self.peek_keep(keep)? {
            Some(byte) => byte,
            None => return Err(self.fail(JsonlError::encoding("utf8"))),
        };
        let width = match first {
            0x00..=0x7F => 1,
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Err(self.fail(JsonlError::encoding("utf8"))),
        };
        let mut buf = [0u8; 4];
        for slot in &mut buf[..width] {
            *slot = match self.peek_keep(keep)? {
                Some(byte) => byte,
                None => return Err(self.fail(JsonlError::encoding("utf8"))),
            };
            self.bump();
        }
        let text = std::str::from_utf8(&buf[..width])
            .map_err(|_| self.fail(JsonlError::encoding("utf8")))?;
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Ok(ch),
            _ => Err(self.fail(JsonlError::encoding("utf8"))),
        }
    }

    fn scan_number_bytes(&mut self, keep: u64) -> Result<Vec<u8>, JsonlError> {
        let mut raw = Vec::new();
        let first = self.take_byte(keep, &mut raw)?;
        if first == b'-' {
            let digit = self.take_byte(keep, &mut raw)?;
            if !digit.is_ascii_digit() {
                return Err(self.fail(JsonlError::protocol("invalid_number")));
            }
            self.scan_int_rest(keep, &mut raw, digit)?;
        } else if first.is_ascii_digit() {
            self.scan_int_rest(keep, &mut raw, first)?;
        } else {
            return Err(self.fail(JsonlError::protocol("invalid_number")));
        }
        if self.peek_keep(keep)? == Some(b'.') {
            self.take_byte(keep, &mut raw)?;
            let mut digits = 0usize;
            loop {
                match self.peek_keep(keep)? {
                    Some(byte) if byte.is_ascii_digit() => {
                        digits += 1;
                        self.take_byte(keep, &mut raw)?;
                    }
                    _ => break,
                }
            }
            if digits == 0 {
                return Err(self.fail(JsonlError::protocol("invalid_number")));
            }
        }
        if matches!(self.peek_keep(keep)?, Some(b'e' | b'E')) {
            self.take_byte(keep, &mut raw)?;
            if matches!(self.peek_keep(keep)?, Some(b'+' | b'-')) {
                self.take_byte(keep, &mut raw)?;
            }
            let mut digits = 0usize;
            loop {
                match self.peek_keep(keep)? {
                    Some(byte) if byte.is_ascii_digit() => {
                        digits += 1;
                        self.take_byte(keep, &mut raw)?;
                    }
                    _ => break,
                }
            }
            if digits == 0 {
                return Err(self.fail(JsonlError::protocol("invalid_number")));
            }
        }
        Ok(raw)
    }

    fn scan_int_rest(&mut self, keep: u64, raw: &mut Vec<u8>, first_digit: u8) -> Result<(), JsonlError> {
        if first_digit == b'0' {
            if matches!(self.peek_keep(keep)?, Some(byte) if byte.is_ascii_digit()) {
                return Err(self.fail(JsonlError::protocol("invalid_number")));
            }
            return Ok(());
        }
        if !first_digit.is_ascii_digit() {
            return Err(self.fail(JsonlError::protocol("invalid_number")));
        }
        loop {
            match self.peek_keep(keep)? {
                Some(byte) if byte.is_ascii_digit() => {
                    self.take_byte(keep, raw)?;
                }
                _ => return Ok(()),
            }
        }
    }

    fn take_byte(&mut self, keep: u64, raw: &mut Vec<u8>) -> Result<u8, JsonlError> {
        let byte = match self.peek_keep(keep)? {
            Some(byte) => byte,
            None => return Err(self.fail(JsonlError::protocol("truncated"))),
        };
        if raw.len() >= self.window.len() {
            return Err(self.fail(JsonlError::budget("max_record")));
        }
        raw.push(byte);
        self.bump();
        Ok(byte)
    }

    fn consume_lit(&mut self, keep: u64, literal: &[u8]) -> Result<(), JsonlError> {
        for expected in literal {
            match self.peek_keep(keep)? {
                Some(byte) if byte == *expected => self.bump(),
                Some(_) => return Err(self.fail(JsonlError::protocol("syntax"))),
                None => return Err(self.fail(JsonlError::protocol("truncated"))),
            }
        }
        Ok(())
    }

    fn eat(&mut self, keep: u64, byte: u8) -> Result<bool, JsonlError> {
        if self.peek_keep(keep)? == Some(byte) {
            self.bump();
            return Ok(true);
        }
        Ok(false)
    }
}

struct Mark {
    abs: u64,
    line: u64,
    col: u64,
}

enum CellVal {
    Utf8(String),
    Bool(bool),
    Int(i64),
    Json(String),
    Null,
}

struct ParsedRow {
    vals: Vec<Option<CellVal>>,
    fields: Vec<FieldLineage>,
    line: u64,
    offset: u64,
    raw_len: u64,
}

enum ColBuf {
    Utf8(Vec<Option<String>>),
    Bool(Vec<Option<bool>>),
    Int(Vec<Option<i64>>),
    Json(Vec<Option<String>>),
}

impl ColBuf {
    fn empty(ty: JsonlType, capacity: usize) -> Self {
        match ty {
            JsonlType::Utf8 => Self::Utf8(Vec::with_capacity(capacity)),
            JsonlType::Bool => Self::Bool(Vec::with_capacity(capacity)),
            JsonlType::Int64 => Self::Int(Vec::with_capacity(capacity)),
            JsonlType::JsonText => Self::Json(Vec::with_capacity(capacity)),
        }
    }
}

struct Pending {
    cols: Vec<ColBuf>,
    lineage: Vec<RowLineage>,
    bytes: u64,
    rows: u64,
}

fn push_cell(buf: &mut ColBuf, value: Option<CellVal>) -> Result<(), JsonlError> {
    match (buf, value) {
        (ColBuf::Utf8(values), Some(CellVal::Utf8(text))) => values.push(Some(text)),
        (ColBuf::Utf8(values), Some(CellVal::Null) | None) => values.push(None),
        (ColBuf::Bool(values), Some(CellVal::Bool(flag))) => values.push(Some(flag)),
        (ColBuf::Bool(values), Some(CellVal::Null) | None) => values.push(None),
        (ColBuf::Int(values), Some(CellVal::Int(number))) => values.push(Some(number)),
        (ColBuf::Int(values), Some(CellVal::Null) | None) => values.push(None),
        (ColBuf::Json(values), Some(CellVal::Json(text))) => values.push(Some(text)),
        (ColBuf::Json(values), Some(CellVal::Null) | None) => values.push(None),
        _ => return Err(JsonlError::protocol("column_type")),
    }
    Ok(())
}
