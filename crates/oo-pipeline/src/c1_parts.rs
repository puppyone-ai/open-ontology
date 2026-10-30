//! Shared C1 chunked-baseline code: part files, part manifests, canonical
//! cells and rows, and the composed state digest.
//!
//! The producer (`tools/mysql-preview`), the loader, admission and the oracle
//! all use this module, so they compute the same bytes. Protocol
//! `oo.pipeline.c1-parts/v2`. Design: `specs/chunked-baseline/DESIGN.md` §3.6
//! and §3.8.
//!
//! Part files are Arrow IPC streams. The caller encodes the batch with its own
//! Arrow writer and writes through [`PartSink`], which enforces
//! [`PART_MAX_BYTES`] while the caller writes. This crate has no Arrow writer
//! dependency. [`read_part`] and [`for_each_part`] read parts back through
//! `native_arrow::decode_arrow_stream`, one part at a time.
//!
//! The composed digest adds each canonical row to a key bucket. A bucket keeps
//! a row count and the sum of the row SHA-256 values modulo 2^256. The sum does
//! not depend on order, so both sides may add rows in any order. This replaces
//! the key-ordered hash in DESIGN §3.8(a). The bucket form is what the source
//! oracle also needs.
//!
//! Canonical cell: `0x00` for SQL NULL and for an unavailable cell (Arrow stores
//! that as a null). Otherwise `0x01`, a big-endian `u32` payload length, and the
//! payload. Payloads: bool `0`/`1`; int and micros as decimal ASCII; decimal as
//! plain text with exactly the column scale; float as big-endian IEEE-754 bits
//! (NaN canonicalized); text and bytes as stored; date `YYYY-MM-DD`; JSON as
//! sorted-key text with normalized numbers and no whitespace.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use oo_connectors::types::{CellPresence, DecodedCell};
use oo_core::canonical::{JsonNode, LogicalType, LogicalValue, parse_json, validate_date};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::native_arrow::{self, DecodedField, DecodedStream, NativeCell};

/// Manifest protocol.
pub const PARTS_PROTOCOL: &str = "oo.pipeline.c1-parts/v2";
/// Largest part file. This is `native_arrow::MAX_ARROW` (32 MiB).
pub const PART_MAX_BYTES: u64 = 32 * 1024 * 1024;
/// Position column of a CDC part. Arrow `decimal128(38,0)`, non-negative.
pub const POS_COLUMN: &str = "_oo_pos";
/// Operation column of a CDC part. Arrow `utf8`, value `upsert` or `delete`.
pub const OP_COLUMN: &str = "_oo_op";
/// CDC operation for an upsert.
pub const OP_UPSERT: &str = "upsert";
/// CDC operation for a delete.
pub const OP_DELETE: &str = "delete";
/// Largest bucket exponent for [`StateDigest`]. This gives at most 65,536 buckets.
pub const BUCKET_BITS_MAX: u8 = 16;
/// Key-bucket exponent of the composed digest (DESIGN D2). A protocol constant: the
/// producer, the loader and the expected-state model must all use this value.
pub const C1_BUCKET_BITS: u8 = 12;

const PARTS_DIGEST_PREFIX: &[u8] = b"oo.c1.parts/v2\n";
const STATE_PREFIX: &[u8] = b"oo.c1.state/v2\n";
const CELL_NULL: u8 = 0;
const CELL_VALUE: u8 = 1;
const F64_CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;
const MICROS_PER_DAY: i128 = 86_400_000_000;
const KEY_OFFSET: i128 = 1 << 63;
const KEY_SPAN: i128 = 3 << 63;
const KEY_MIN: i128 = -(1 << 63);
const KEY_MAX: i128 = (1 << 64) - 1;

/// Closed reason token. The message never carries a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C1PartsError {
    /// Stable reason token.
    pub reason: &'static str,
}

/// Result alias for this module.
pub type C1PartsResult<T> = Result<T, C1PartsError>;

fn fail<T>(reason: &'static str) -> C1PartsResult<T> {
    Err(C1PartsError { reason })
}

impl fmt::Display for C1PartsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c1 parts: {}", self.reason)
    }
}

impl std::error::Error for C1PartsError {}

/// Part kind. A fill part is a snapshot chunk. A CDC part holds one table's
/// transaction-batch rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartKind {
    /// Snapshot chunk. It has no `_oo_pos` column. Its position is `_oo_pos = 0`.
    Fill,
    /// CDC batch rows. They carry `_oo_pos` and `_oo_op`.
    Cdc,
}

impl PartKind {
    /// Manifest token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Cdc => "cdc",
        }
    }

    fn parse(text: &str) -> C1PartsResult<Self> {
        match text {
            "fill" => Ok(Self::Fill),
            "cdc" => Ok(Self::Cdc),
            _ => fail("part_kind"),
        }
    }
}

/// One part in the manifest. `key_lo` and `key_hi` are the inclusive bounds of
/// the integer primary key. Both are `None` when `rows == 0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartEntry {
    /// Part number, 1-based and contiguous within its kind.
    pub part_no: u32,
    /// Part kind.
    pub kind: PartKind,
    /// Smallest key in the part.
    pub key_lo: Option<i128>,
    /// Largest key in the part.
    pub key_hi: Option<i128>,
    /// Row count.
    pub rows: u64,
    /// File size in bytes. At most [`PART_MAX_BYTES`].
    pub bytes: u64,
    /// SHA-256 of the file, lowercase hex.
    pub sha256: String,
}

/// File name of a part inside a directory.
pub fn part_file_name(kind: PartKind, part_no: u32) -> C1PartsResult<String> {
    if part_no == 0 {
        return fail("part_no");
    }
    Ok(format!("{}-{:08}.arrow", kind.as_str(), part_no))
}

/// Writer that enforces [`PART_MAX_BYTES`]. A write that would make the total
/// larger fails. The caller must then discard the partial file.
#[derive(Debug)]
pub struct PartSink<W: Write> {
    inner: W,
    written: u64,
}

impl<W: Write> PartSink<W> {
    /// Wrap a writer. Bytes written so far start at zero.
    pub fn new(inner: W) -> Self {
        Self { inner, written: 0 }
    }

    /// Bytes accepted so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Return the inner writer and the total byte count.
    pub fn into_parts(self) -> (W, u64) {
        (self.inner, self.written)
    }
}

impl<W: Write> Write for PartSink<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let room = PART_MAX_BYTES - self.written;
        if buf.len() as u64 > room {
            return Err(io::Error::other("c1 parts: part_too_large"));
        }
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn validate_entry(entry: &PartEntry) -> C1PartsResult<()> {
    if entry.part_no == 0 {
        return fail("part_no");
    }
    if entry.bytes == 0 || entry.bytes > PART_MAX_BYTES {
        return fail("part_bytes");
    }
    if !is_hex64(&entry.sha256) {
        return fail("part_sha256");
    }
    match (entry.rows, entry.key_lo, entry.key_hi) {
        (0, None, None) => Ok(()),
        (0, _, _) => fail("key_range"),
        (_, Some(lo), Some(hi)) if lo <= hi && in_key_domain(lo) && in_key_domain(hi) => Ok(()),
        _ => fail("key_range"),
    }
}

fn in_key_domain(key: i128) -> bool {
    (KEY_MIN..=KEY_MAX).contains(&key)
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Read one part file, check its size and SHA-256 against the entry, and decode
/// it. The row count and schema are checked before the part is trusted.
///
/// A fill part has no `_oo_pos` or `_oo_op` column. A CDC part ends with a
/// `decimal128(38,0)` `_oo_pos` column and a `utf8` `_oo_op` column whose values
/// are `upsert` or `delete`.
pub fn read_part(dir: &Path, entry: &PartEntry) -> C1PartsResult<DecodedStream> {
    let bytes = read_bounded(&dir.join(part_file_name(entry.kind, entry.part_no)?))?;
    verify_part_bytes(&bytes, entry)
}

fn verify_part_bytes(bytes: &[u8], entry: &PartEntry) -> C1PartsResult<DecodedStream> {
    if bytes.len() as u64 != entry.bytes {
        return fail("part_bytes");
    }
    if sha256_hex(bytes) != entry.sha256 {
        return fail("part_sha256");
    }
    check_stream(bytes, entry.kind, entry.rows)
}

fn check_stream(bytes: &[u8], kind: PartKind, rows: u64) -> C1PartsResult<DecodedStream> {
    let stream = native_arrow::decode_arrow_stream(bytes).map_err(|fault| C1PartsError {
        reason: fault.reason(),
    })?;
    if stream.rows as u64 != rows {
        return fail("part_rows");
    }
    validate_schema(kind, &stream)?;
    Ok(stream)
}

fn validate_schema(kind: PartKind, stream: &DecodedStream) -> C1PartsResult<()> {
    let fields = &stream.fields;
    match kind {
        PartKind::Fill => {
            if fields
                .iter()
                .any(|f| f.name == POS_COLUMN || f.name == OP_COLUMN)
            {
                return fail("part_schema");
            }
            Ok(())
        }
        PartKind::Cdc => {
            let n = fields.len();
            if n < 3 {
                return fail("part_schema");
            }
            if fields[..n - 2]
                .iter()
                .any(|f| f.name == POS_COLUMN || f.name == OP_COLUMN)
            {
                return fail("part_schema");
            }
            if !pos_field_ok(&fields[n - 2]) || !op_field_ok(&fields[n - 1]) {
                return fail("part_schema");
            }
            for cell in &stream.columns[n - 2] {
                match cell {
                    NativeCell::Decimal {
                        coefficient,
                        scale: 0,
                        ..
                    } if *coefficient >= 0 => {}
                    _ => return fail("cdc_pos"),
                }
            }
            for cell in &stream.columns[n - 1] {
                match cell {
                    NativeCell::Utf8(text) if text == OP_UPSERT || text == OP_DELETE => {}
                    _ => return fail("cdc_op"),
                }
            }
            Ok(())
        }
    }
}

fn pos_field_ok(field: &DecodedField) -> bool {
    field.name == POS_COLUMN
        && field.arrow_type == "decimal128"
        && field.precision == Some(38)
        && field.scale == Some(0)
}

fn op_field_ok(field: &DecodedField) -> bool {
    field.name == OP_COLUMN && field.arrow_type == "utf8"
}

/// Visit parts in manifest order. Each part is read, verified and decoded before
/// the next one. No part is kept after its visit returns, so memory is bounded by
/// one part, not by the table.
pub fn for_each_part<F>(dir: &Path, manifest: &PartManifest, mut visit: F) -> C1PartsResult<()>
where
    F: FnMut(&PartEntry, DecodedStream) -> C1PartsResult<()>,
{
    for entry in manifest.parts() {
        let stream = read_part(dir, entry)?;
        visit(entry, stream)?;
    }
    Ok(())
}

/// Read a part file that the caller just wrote and return its manifest entry.
/// This checks the bound, the row count and the schema before the entry is made.
pub fn part_entry_for_file(
    path: &Path,
    part_no: u32,
    kind: PartKind,
    key_lo: Option<i128>,
    key_hi: Option<i128>,
    rows: u64,
) -> C1PartsResult<PartEntry> {
    let bytes = read_bounded(path)?;
    check_stream(&bytes, kind, rows)?;
    let entry = PartEntry {
        part_no,
        kind,
        key_lo,
        key_hi,
        rows,
        bytes: bytes.len() as u64,
        sha256: sha256_hex(&bytes),
    };
    validate_entry(&entry)?;
    Ok(entry)
}

fn read_bounded(path: &Path) -> C1PartsResult<Vec<u8>> {
    let file = File::open(path).map_err(|_| C1PartsError {
        reason: "part_open",
    })?;
    let mut bytes = Vec::new();
    file.take(PART_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| C1PartsError {
            reason: "part_read",
        })?;
    if bytes.len() as u64 > PART_MAX_BYTES {
        return fail("part_too_large");
    }
    Ok(bytes)
}

/// Parts of one table in manifest order. Built only by [`PartManifest::new`] or
/// [`PartManifest::from_json`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartManifest {
    parts: Vec<PartEntry>,
    digest: String,
}

impl PartManifest {
    /// Validate and order the parts. Parts are sorted by `(kind, part_no)`.
    /// Within each kind, `part_no` must run `1..=n` with no duplicate and no gap.
    /// Fill chunks with rows must have ascending, non-overlapping key ranges.
    pub fn new(mut parts: Vec<PartEntry>) -> C1PartsResult<Self> {
        if parts.is_empty() {
            return fail("manifest_empty");
        }
        for part in &parts {
            validate_entry(part)?;
        }
        parts.sort_by_key(|part| (part.kind, part.part_no));
        let mut expect: BTreeMap<PartKind, u32> = BTreeMap::new();
        let mut fill_high: Option<i128> = None;
        for part in &parts {
            let next = expect.entry(part.kind).or_insert(1);
            if part.part_no < *next {
                return fail("part_no_duplicate");
            }
            if part.part_no > *next {
                return fail("part_no_gap");
            }
            *next += 1;
            if part.kind == PartKind::Fill && part.rows > 0 {
                if let (Some(high), Some(low)) = (fill_high, part.key_lo)
                    && low <= high
                {
                    return fail("chunk_order");
                }
                fill_high = part.key_hi;
            }
        }
        let digest = parts_digest_of(&parts)?;
        Ok(Self { parts, digest })
    }

    /// Parts in manifest order.
    pub fn parts(&self) -> &[PartEntry] {
        &self.parts
    }

    /// Aggregate digest. See [`parts_digest_of`].
    pub fn parts_digest(&self) -> &str {
        &self.digest
    }

    /// Total row count across all parts.
    pub fn total_rows(&self) -> u64 {
        self.parts.iter().map(|part| part.rows).sum()
    }

    /// Manifest JSON. Keys are decimal strings.
    pub fn to_json(&self) -> Value {
        json!({
            "protocol": PARTS_PROTOCOL,
            "parts": self.parts.iter().map(entry_json).collect::<Vec<_>>(),
            "parts_digest": self.digest,
        })
    }

    /// Parse and validate manifest JSON. The stored `parts_digest` must match the
    /// recomputed digest.
    pub fn from_json(value: &Value) -> C1PartsResult<Self> {
        if value["protocol"].as_str() != Some(PARTS_PROTOCOL) {
            return fail("manifest_protocol");
        }
        let Some(items) = value["parts"].as_array() else {
            return fail("manifest_parts");
        };
        let parts = items
            .iter()
            .map(entry_from_json)
            .collect::<C1PartsResult<Vec<_>>>()?;
        let manifest = Self::new(parts)?;
        if value["parts_digest"].as_str() != Some(manifest.digest.as_str()) {
            return fail("parts_digest");
        }
        Ok(manifest)
    }
}

fn entry_json(part: &PartEntry) -> Value {
    json!({
        "part_no": part.part_no,
        "kind": part.kind.as_str(),
        "key_lo": part.key_lo.map(|key| key.to_string()),
        "key_hi": part.key_hi.map(|key| key.to_string()),
        "rows": part.rows,
        "bytes": part.bytes,
        "sha256": part.sha256,
    })
}

fn entry_from_json(value: &Value) -> C1PartsResult<PartEntry> {
    let number = |name: &str| -> C1PartsResult<u64> {
        value[name].as_u64().ok_or(C1PartsError {
            reason: "manifest_field",
        })
    };
    let key = |name: &str| -> C1PartsResult<Option<i128>> {
        match &value[name] {
            Value::Null => Ok(None),
            Value::String(text) => text.parse::<i128>().map(Some).map_err(|_| C1PartsError {
                reason: "manifest_key",
            }),
            _ => fail("manifest_key"),
        }
    };
    let part_no =
        u32::try_from(number("part_no")?).map_err(|_| C1PartsError { reason: "part_no" })?;
    let kind = PartKind::parse(value["kind"].as_str().unwrap_or(""))?;
    Ok(PartEntry {
        part_no,
        kind,
        key_lo: key("key_lo")?,
        key_hi: key("key_hi")?,
        rows: number("rows")?,
        bytes: number("bytes")?,
        sha256: value["sha256"].as_str().unwrap_or("").to_owned(),
    })
}

/// Aggregate digest of a table's parts.
///
/// One part: the digest is that file's SHA-256, so a small table keeps the v1
/// receipt shape. More parts: SHA-256 over `oo.c1.parts/v2` and one line per part,
/// sorted by `(kind, part_no)`, so the input order does not matter.
pub fn parts_digest_of(parts: &[PartEntry]) -> C1PartsResult<String> {
    match parts {
        [] => fail("manifest_empty"),
        [single] => Ok(single.sha256.clone()),
        _ => {
            let mut lines: Vec<&PartEntry> = parts.iter().collect();
            lines.sort_by_key(|part| (part.kind, part.part_no));
            let mut hasher = Sha256::new();
            hasher.update(PARTS_DIGEST_PREFIX);
            for part in lines {
                let line = format!(
                    "{}|{}|{}|{}|{}|{}|{}\n",
                    part.kind.as_str(),
                    part.part_no,
                    part.key_lo.map(|key| key.to_string()).unwrap_or_default(),
                    part.key_hi.map(|key| key.to_string()).unwrap_or_default(),
                    part.rows,
                    part.bytes,
                    part.sha256,
                );
                hasher.update(line.as_bytes());
            }
            Ok(hex_lower(&hasher.finalize()))
        }
    }
}

/// Typed cell as both sides see it. The source builds it from the MySQL decoder
/// output. The target builds it from PostgreSQL text readback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// SQL NULL. An unavailable cell is also `Null`.
    Null,
    /// Boolean.
    Bool(bool),
    /// Integer, signed or unsigned up to 64 bits.
    Int(i128),
    /// Decimal as plain text. Canonicalized to the column scale.
    Decimal(String),
    /// IEEE-754 bit pattern of the `f64` value. A float4 is widened first.
    Float64(u64),
    /// UTF-8 text. JSON text also uses this variant.
    Text(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
    /// Calendar date.
    Date {
        /// Year, 1 to 9999.
        year: i32,
        /// Month, 1 to 12.
        month: u8,
        /// Day of month.
        day: u8,
    },
    /// Signed microseconds. A duration is relative to zero. A timestamp is
    /// relative to the Unix epoch in UTC.
    Micros(i64),
}

/// Logical type of a PostgreSQL column as the target stores it. This matches the
/// MySQL mapping in `oo-connectors::mysql::map`.
pub fn logical_type_of_pg(oid: i64, typmod: i32) -> C1PartsResult<LogicalType> {
    Ok(match oid {
        16 => LogicalType::Bool,
        20 | 21 | 23 => LogicalType::Int,
        1700 => {
            // Numeric typmod is ((precision << 16) | scale) + 4. Unconstrained has no scale.
            if typmod < 4 {
                return fail("numeric_typmod");
            }
            let packed = typmod - 4;
            LogicalType::Decimal {
                precision: ((packed >> 16) & 0xffff) as u16,
                scale: (packed & 0xffff) as i16,
            }
        }
        700 | 701 => LogicalType::Float64,
        25 | 1043 | 1042 => LogicalType::Text,
        114 => LogicalType::Json,
        17 => LogicalType::Bytes,
        1082 => LogicalType::Date,
        1186 => LogicalType::DurationMicros,
        1114 => LogicalType::TimestampNaive,
        1184 => LogicalType::TimestampTz,
        _ => return fail("pg_oid"),
    })
}

/// Name of a canonical logical type in a final-load request (DESIGN D7). The name
/// is part of the request, so the producer and the loader agree on the canonical
/// encoding of every column without either one deriving it from a PostgreSQL oid.
pub fn logical_type_name(ty: &LogicalType) -> C1PartsResult<String> {
    Ok(match ty {
        LogicalType::Bool => "bool".to_owned(),
        LogicalType::Int => "int".to_owned(),
        LogicalType::Decimal { precision, scale } => format!("decimal({precision},{scale})"),
        LogicalType::Float64 => "float64".to_owned(),
        LogicalType::Text => "text".to_owned(),
        LogicalType::Bytes => "bytes".to_owned(),
        LogicalType::Date => "date".to_owned(),
        LogicalType::Time => "time".to_owned(),
        LogicalType::DurationMicros => "duration_micros".to_owned(),
        LogicalType::TimestampNaive => "timestamp_naive".to_owned(),
        LogicalType::TimestampTz => "timestamp_tz".to_owned(),
        LogicalType::Json => "json".to_owned(),
        LogicalType::Array(_) | LogicalType::Struct(_) => return fail("type_name"),
    })
}

/// Inverse of [`logical_type_name`]. Any other name is refused.
pub fn logical_type_from_name(name: &str) -> C1PartsResult<LogicalType> {
    Ok(match name {
        "bool" => LogicalType::Bool,
        "int" => LogicalType::Int,
        "float64" => LogicalType::Float64,
        "text" => LogicalType::Text,
        "bytes" => LogicalType::Bytes,
        "date" => LogicalType::Date,
        "time" => LogicalType::Time,
        "duration_micros" => LogicalType::DurationMicros,
        "timestamp_naive" => LogicalType::TimestampNaive,
        "timestamp_tz" => LogicalType::TimestampTz,
        "json" => LogicalType::Json,
        other => {
            let inner = other
                .strip_prefix("decimal(")
                .and_then(|rest| rest.strip_suffix(')'))
                .ok_or(C1PartsError {
                    reason: "type_name",
                })?;
            let (precision, scale) = inner.split_once(',').ok_or(C1PartsError {
                reason: "type_name",
            })?;
            LogicalType::Decimal {
                precision: precision.parse().map_err(|_| C1PartsError {
                    reason: "type_name",
                })?,
                scale: scale.parse().map_err(|_| C1PartsError {
                    reason: "type_name",
                })?,
            }
        }
    })
}

/// Build a [`Cell`] from one decoded MySQL cell.
pub fn source_cell(cell: &DecodedCell) -> C1PartsResult<Cell> {
    if cell.presence != CellPresence::Value {
        return Ok(Cell::Null);
    }
    let Some(value) = &cell.value else {
        return fail("value_missing");
    };
    Ok(match value {
        LogicalValue::Bool(item) => Cell::Bool(*item),
        LogicalValue::Int(exact) => {
            Cell::Int(
                exact
                    .to_string()
                    .parse::<i128>()
                    .map_err(|_| C1PartsError {
                        reason: "int_range",
                    })?,
            )
        }
        LogicalValue::Decimal(exact) => Cell::Decimal(exact.to_string()),
        LogicalValue::Float(_) => Cell::Float64(cell.float_bits.ok_or(C1PartsError {
            reason: "float_bits",
        })?),
        LogicalValue::Text(text) => Cell::Text(text.clone()),
        LogicalValue::Bytes(bytes) => Cell::Bytes(bytes.clone()),
        LogicalValue::Date { year, month, day } => Cell::Date {
            year: *year,
            month: *month,
            day: *day,
        },
        LogicalValue::TimeMicros(micros) | LogicalValue::DurationMicros(micros) => {
            Cell::Micros(*micros)
        }
        LogicalValue::TimestampTzMicros(micros) => Cell::Micros(*micros),
        LogicalValue::TimestampNaive {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        } => Cell::Micros(civil_micros(
            *year, *month, *day, *hour, *minute, *second, *micros,
        )?),
        LogicalValue::Json(node) => Cell::Text(json_text(node)),
        LogicalValue::Array(_) | LogicalValue::Struct(_) => return fail("unsupported_type"),
    })
}

/// Build a [`Cell`] from PostgreSQL text output for an OID. The text form is
/// what `::text` returns with the default `IntervalStyle = postgres`, and with
/// the session `TimeZone` (`timestamptz` carries its own offset). `None` is SQL
/// NULL.
pub fn pg_text_cell(oid: i64, text: Option<&str>) -> C1PartsResult<Cell> {
    let Some(text) = text else {
        return Ok(Cell::Null);
    };
    Ok(match oid {
        16 => match text {
            "t" => Cell::Bool(true),
            "f" => Cell::Bool(false),
            _ => return fail("pg_bool"),
        },
        20 | 21 | 23 => Cell::Int(
            text.parse::<i128>()
                .map_err(|_| C1PartsError { reason: "pg_int" })?,
        ),
        1700 => Cell::Decimal(text.to_owned()),
        700 => {
            let value: f32 = text
                .parse()
                .map_err(|_| C1PartsError { reason: "pg_float" })?;
            Cell::Float64(f64::from(value).to_bits())
        }
        701 => {
            let value: f64 = text
                .parse()
                .map_err(|_| C1PartsError { reason: "pg_float" })?;
            Cell::Float64(value.to_bits())
        }
        25 | 1043 | 114 => Cell::Text(text.to_owned()),
        // bpchar output is padded to its declared length. MySQL CHAR has no trailing spaces.
        1042 => Cell::Text(text.trim_end_matches(' ').to_owned()),
        17 => Cell::Bytes(parse_bytea_hex(text)?),
        1082 => {
            let (year, month, day) = parse_pg_date(text)?;
            Cell::Date { year, month, day }
        }
        1186 => Cell::Micros(parse_pg_interval(text)?),
        1114 => Cell::Micros(parse_pg_timestamp(text, false)?),
        1184 => Cell::Micros(parse_pg_timestamp(text, true)?),
        _ => return fail("pg_oid"),
    })
}

/// Canonical bytes of one cell under its logical type. See the module doc.
pub fn canonical_cell(ty: &LogicalType, cell: &Cell) -> C1PartsResult<Vec<u8>> {
    if *cell == Cell::Null {
        return Ok(vec![CELL_NULL]);
    }
    let payload = payload(ty, cell)?;
    let length = u32::try_from(payload.len()).map_err(|_| C1PartsError {
        reason: "cell_budget",
    })?;
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(CELL_VALUE);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Concatenated canonical cells of one row, in column order.
pub fn canonical_row(types: &[LogicalType], cells: &[Cell]) -> C1PartsResult<Vec<u8>> {
    if types.len() != cells.len() {
        return fail("row_width");
    }
    let mut out = Vec::new();
    for (ty, cell) in types.iter().zip(cells) {
        out.extend_from_slice(&canonical_cell(ty, cell)?);
    }
    Ok(out)
}

/// Integer key of a single-integer primary-key cell.
pub fn bucket_key(cell: &Cell) -> C1PartsResult<i128> {
    let key = match cell {
        Cell::Int(value) => *value,
        Cell::Decimal(text) => text
            .parse::<i128>()
            .map_err(|_| C1PartsError { reason: "key_type" })?,
        _ => return fail("key_type"),
    };
    if in_key_domain(key) {
        Ok(key)
    } else {
        fail("key_range")
    }
}

/// Bucket index of a key. The key domain is `[-2^63, 2^64)`. Buckets split that
/// domain into equal shares, so the index is monotone in the key.
pub fn bucket_of(key: i128, bucket_bits: u8) -> C1PartsResult<u32> {
    if bucket_bits > BUCKET_BITS_MAX {
        return fail("bucket_bits");
    }
    if !in_key_domain(key) {
        return fail("key_range");
    }
    let offset = (key + KEY_OFFSET) as u128;
    let buckets = 1u128 << bucket_bits;
    let index = offset * buckets / (KEY_SPAN as u128);
    u32::try_from(index).map_err(|_| C1PartsError {
        reason: "bucket_index",
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Bucket {
    count: u64,
    sum: [u8; 32],
}

/// Order-independent state digest of one table, split into key buckets.
///
/// Each bucket keeps a row count and the sum of its row SHA-256 values modulo
/// 2^256. Rows can be added and merged in any order and give the same result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDigest {
    bucket_bits: u8,
    buckets: BTreeMap<u32, Bucket>,
    rows: u64,
}

impl StateDigest {
    /// Empty digest with `2^bucket_bits` buckets.
    pub fn new(bucket_bits: u8) -> C1PartsResult<Self> {
        if bucket_bits > BUCKET_BITS_MAX {
            return fail("bucket_bits");
        }
        Ok(Self {
            bucket_bits,
            buckets: BTreeMap::new(),
            rows: 0,
        })
    }

    /// Bucket exponent.
    pub fn bucket_bits(&self) -> u8 {
        self.bucket_bits
    }

    /// Rows added, including merged rows.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Add one canonical row under its integer key.
    pub fn add_row(&mut self, key: i128, canonical_row: &[u8]) -> C1PartsResult<()> {
        let index = bucket_of(key, self.bucket_bits)?;
        let digest: [u8; 32] = Sha256::digest(canonical_row).into();
        let bucket = self.buckets.entry(index).or_default();
        bucket.count = bucket.count.checked_add(1).ok_or(C1PartsError {
            reason: "bucket_count",
        })?;
        bucket.sum = add_mod_256(&bucket.sum, &digest);
        self.rows = self.rows.checked_add(1).ok_or(C1PartsError {
            reason: "bucket_count",
        })?;
        Ok(())
    }

    /// Merge another digest with the same bucket exponent. The result does not
    /// depend on which side is merged first.
    pub fn merge(&mut self, other: &StateDigest) -> C1PartsResult<()> {
        if other.bucket_bits != self.bucket_bits {
            return fail("bucket_bits");
        }
        for (index, bucket) in &other.buckets {
            let mine = self.buckets.entry(*index).or_default();
            mine.count = mine.count.checked_add(bucket.count).ok_or(C1PartsError {
                reason: "bucket_count",
            })?;
            mine.sum = add_mod_256(&mine.sum, &bucket.sum);
        }
        self.rows = self.rows.checked_add(other.rows).ok_or(C1PartsError {
            reason: "bucket_count",
        })?;
        Ok(())
    }

    /// Row count and sum of one bucket. A bucket with no rows returns `None`.
    pub fn bucket(&self, index: u32) -> Option<(u64, [u8; 32])> {
        self.buckets.get(&index).map(|b| (b.count, b.sum))
    }

    /// SHA-256 over the bucket exponent and each non-empty bucket in index order.
    pub fn table_digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.bucket_bits]);
        for (index, bucket) in &self.buckets {
            hasher.update(index.to_be_bytes());
            hasher.update(bucket.count.to_be_bytes());
            hasher.update(bucket.sum);
        }
        hasher.finalize().into()
    }
}

/// Composed digest over several tables. The preimage is `oo.c1.state/v2`, then
/// for each table in name order: `T:`, the name, a newline, and the table digest.
pub fn composed_digest(tables: &[(&str, &StateDigest)]) -> C1PartsResult<String> {
    let mut sorted: Vec<(&str, &StateDigest)> = tables.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(right.0));
    let mut hasher = Sha256::new();
    hasher.update(STATE_PREFIX);
    let mut previous: Option<&str> = None;
    for (name, digest) in sorted {
        if name.contains('\n') || previous == Some(name) {
            return fail("table_name");
        }
        previous = Some(name);
        hasher.update(b"T:");
        hasher.update(name.as_bytes());
        hasher.update(b"\n");
        hasher.update(digest.table_digest());
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn payload(ty: &LogicalType, cell: &Cell) -> C1PartsResult<Vec<u8>> {
    match (ty, cell) {
        (LogicalType::Bool, Cell::Bool(value)) => Ok(if *value { b"1" } else { b"0" }.to_vec()),
        (LogicalType::Int, Cell::Int(value)) => Ok(value.to_string().into_bytes()),
        (LogicalType::Decimal { scale, .. }, Cell::Decimal(text)) => {
            canonical_decimal(text, i32::from(*scale))
        }
        (LogicalType::Float64, Cell::Float64(bits)) => {
            let bits = if f64::from_bits(*bits).is_nan() {
                F64_CANONICAL_NAN
            } else {
                *bits
            };
            Ok(bits.to_be_bytes().to_vec())
        }
        (LogicalType::Text, Cell::Text(text)) => Ok(text.as_bytes().to_vec()),
        (LogicalType::Json, Cell::Text(text)) => Ok(canonical_json(text)?.into_bytes()),
        (LogicalType::Bytes, Cell::Bytes(bytes)) => Ok(bytes.clone()),
        (LogicalType::Date, Cell::Date { year, month, day }) => {
            if !(1..=9999).contains(year) {
                return fail("date_range");
            }
            validate_date(*year, *month, *day).map_err(|_| C1PartsError {
                reason: "date_range",
            })?;
            Ok(format!("{year:04}-{month:02}-{day:02}").into_bytes())
        }
        (
            LogicalType::DurationMicros | LogicalType::TimestampNaive | LogicalType::TimestampTz,
            Cell::Micros(micros),
        ) => Ok(micros.to_string().into_bytes()),
        _ => fail("type_mismatch"),
    }
}

fn canonical_decimal(text: &str, scale: i32) -> C1PartsResult<Vec<u8>> {
    if scale < 0 {
        return fail("decimal_scale");
    }
    let scale = scale as usize;
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return fail("decimal_text");
    }
    let (kept, dropped) = if fraction.len() > scale {
        fraction.split_at(scale)
    } else {
        (fraction, "")
    };
    if dropped.bytes().any(|b| b != b'0') {
        return fail("decimal_scale");
    }
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let nonzero = whole != "0" || kept.bytes().any(|b| b != b'0');
    let mut out = String::new();
    if negative && nonzero {
        out.push('-');
    }
    out.push_str(whole);
    if scale > 0 {
        out.push('.');
        out.push_str(kept);
        for _ in kept.len()..scale {
            out.push('0');
        }
    }
    Ok(out.into_bytes())
}

fn canonical_json(text: &str) -> C1PartsResult<String> {
    let node = parse_json(text).map_err(|_| C1PartsError { reason: "json" })?;
    Ok(json_text(&node))
}

fn json_text(node: &JsonNode) -> String {
    let mut out = String::new();
    render_json(node, &mut out);
    out
}

fn render_json(node: &JsonNode, out: &mut String) {
    match node {
        JsonNode::Null => out.push_str("null"),
        JsonNode::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        JsonNode::Number(value) => out.push_str(&value.clone().normalize_json().to_string()),
        JsonNode::String(value) => push_json_string(value, out),
        JsonNode::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                render_json(item, out);
            }
            out.push(']');
        }
        JsonNode::Object(items) => {
            out.push('{');
            for (index, (key, value)) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_json_string(key, out);
                out.push(':');
                render_json(value, out);
            }
            out.push('}');
        }
    }
}

fn push_json_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn add_mod_256(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut carry = 0u16;
    for index in (0..32).rev() {
        let sum = u16::from(left[index]) + u16::from(right[index]) + carry;
        out[index] = sum as u8;
        carry = sum >> 8;
    }
    out
}

fn civil_micros(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> C1PartsResult<i64> {
    validate_date(year, month, day).map_err(|_| C1PartsError {
        reason: "timestamp_range",
    })?;
    if hour > 23 || minute > 59 || second > 59 || micros > 999_999 {
        return fail("timestamp_range");
    }
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    let seconds = i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    let total =
        i128::from(days) * MICROS_PER_DAY + i128::from(seconds) * 1_000_000 + i128::from(micros);
    i64::try_from(total).map_err(|_| C1PartsError {
        reason: "timestamp_range",
    })
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn parse_bytea_hex(text: &str) -> C1PartsResult<Vec<u8>> {
    let Some(hex) = text.strip_prefix("\\x") else {
        return fail("pg_bytea");
    };
    if hex.len() % 2 != 0 || !hex.is_ascii() {
        return fail("pg_bytea");
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(&hex[at..at + 2], 16)
                .map_err(|_| C1PartsError { reason: "pg_bytea" })
        })
        .collect()
}

fn parse_pg_date(text: &str) -> C1PartsResult<(i32, u8, u8)> {
    let mut parts = text.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return fail("pg_date");
    };
    if year.len() < 4 || month.len() != 2 || day.len() != 2 {
        return fail("pg_date");
    }
    let year: i32 = digits(year)?;
    let month: u8 = digits(month)?;
    let day: u8 = digits(day)?;
    if !(1..=9999).contains(&year) {
        return fail("pg_date");
    }
    validate_date(year, month, day).map_err(|_| C1PartsError { reason: "pg_date" })?;
    Ok((year, month, day))
}

fn digits<T: std::str::FromStr>(text: &str) -> C1PartsResult<T> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return fail("pg_digits");
    }
    text.parse::<T>().map_err(|_| C1PartsError {
        reason: "pg_digits",
    })
}

/// Fraction digits up to six, right-padded to micros.
fn parse_fraction(text: &str) -> C1PartsResult<i64> {
    if text.is_empty() || text.len() > 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return fail("pg_fraction");
    }
    let mut value: i64 = digits(text)?;
    for _ in text.len()..6 {
        value *= 10;
    }
    Ok(value)
}

/// `HH:MM:SS[.ffffff]` as a clock inside one day.
fn parse_clock(text: &str) -> C1PartsResult<(u8, u8, u8, u32)> {
    let mut parts = text.split(':');
    let (Some(hour), Some(minute), Some(second), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return fail("pg_clock");
    };
    let (seconds, fraction) = match second.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (second, None),
    };
    let hour: u8 = digits(hour)?;
    let minute: u8 = digits(minute)?;
    let second: u8 = digits(seconds)?;
    if hour > 23 || minute > 59 || second > 59 {
        return fail("pg_clock");
    }
    let micros = match fraction {
        Some(fraction) => parse_fraction(fraction)? as u32,
        None => 0,
    };
    Ok((hour, minute, second, micros))
}

/// Parse `YYYY-MM-DD HH:MM:SS[.f]`, with a trailing `+HH[:MM[:SS]]` offset for
/// `timestamptz`. The result is UTC micros.
fn parse_pg_timestamp(text: &str, zoned: bool) -> C1PartsResult<i64> {
    let (date, rest) = text.split_once(' ').ok_or(C1PartsError {
        reason: "pg_timestamp",
    })?;
    let (year, month, day) = parse_pg_date(date)?;
    let (clock, offset_seconds) = if zoned {
        let at = rest.find(['+', '-']).ok_or(C1PartsError {
            reason: "pg_timestamp",
        })?;
        (&rest[..at], parse_offset(&rest[at..])?)
    } else {
        if rest.contains(['+', '-']) {
            return fail("pg_timestamp");
        }
        (rest, 0)
    };
    let (hour, minute, second, micros) = parse_clock(clock)?;
    let local = civil_micros(year, month, day, hour, minute, second, micros)?;
    local
        .checked_sub(offset_seconds * 1_000_000)
        .ok_or(C1PartsError {
            reason: "timestamp_range",
        })
}

fn parse_offset(text: &str) -> C1PartsResult<i64> {
    let (sign, body) = match text.as_bytes().first() {
        Some(b'+') => (1i64, &text[1..]),
        Some(b'-') => (-1i64, &text[1..]),
        _ => return fail("pg_offset"),
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return fail("pg_offset");
    }
    let weights = [3_600i64, 60, 1];
    let mut total = 0i64;
    for (part, weight) in parts.iter().zip(weights) {
        if part.len() != 2 {
            return fail("pg_offset");
        }
        total += digits::<i64>(part)? * weight;
    }
    Ok(sign * total)
}

/// Parse a PostgreSQL interval in `postgres` style: `[N day(s)] [-]HH:MM:SS[.f]`.
/// Months and years are refused, because MySQL TIME never produces them.
fn parse_pg_interval(text: &str) -> C1PartsResult<i64> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return fail("pg_interval");
    }
    let mut total: i64 = 0;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let part = if token.contains(':') {
            index += 1;
            parse_hms(token)?
        } else {
            let days: i64 = token.parse().map_err(|_| C1PartsError {
                reason: "pg_interval",
            })?;
            match tokens.get(index + 1) {
                Some(&"day") | Some(&"days") => {}
                _ => return fail("pg_interval"),
            }
            index += 2;
            days.checked_mul(86_400_000_000).ok_or(C1PartsError {
                reason: "pg_interval",
            })?
        };
        total = total.checked_add(part).ok_or(C1PartsError {
            reason: "pg_interval",
        })?;
    }
    Ok(total)
}

fn parse_hms(token: &str) -> C1PartsResult<i64> {
    let (negative, body) = match token.as_bytes().first() {
        Some(b'-') => (true, &token[1..]),
        Some(b'+') => (false, &token[1..]),
        _ => (false, token),
    };
    let parts: Vec<&str> = body.split(':').collect();
    let [hours, minutes, seconds] = parts.as_slice() else {
        return fail("pg_interval");
    };
    let hours: i64 = digits(hours)?;
    if minutes.len() != 2 {
        return fail("pg_interval");
    }
    let minutes: i64 = digits(minutes)?;
    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, parse_fraction(fraction)?),
        None => (*seconds, 0),
    };
    if whole.len() != 2 {
        return fail("pg_interval");
    }
    let whole: i64 = digits(whole)?;
    if minutes > 59 || whole > 59 {
        return fail("pg_interval");
    }
    let micros = hours * 3_600_000_000 + minutes * 60_000_000 + whole * 1_000_000 + fraction;
    Ok(if negative { -micros } else { micros })
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_core::canonical::FloatClass;
    use serde_json::Value;
    use std::path::PathBuf;

    const GOLDEN: &str = include_str!("../tests/fixtures/c1_parts/golden-v2.json");
    const PG_READBACK: &str = include_str!("../tests/fixtures/c1_parts/pg17-readback-v2.json");
    const MANIFEST: &str = include_str!("../tests/fixtures/c1_parts/manifest-v2.json");

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/c1_parts")
    }

    fn golden() -> Value {
        serde_json::from_str(GOLDEN).expect("golden json")
    }

    fn type_of(value: &Value) -> LogicalType {
        match value["kind"].as_str().expect("kind") {
            "bool" => LogicalType::Bool,
            "int" => LogicalType::Int,
            "decimal" => LogicalType::Decimal {
                precision: 38,
                scale: value["scale"].as_i64().expect("scale") as i16,
            },
            "float64" => LogicalType::Float64,
            "text" => LogicalType::Text,
            "json" => LogicalType::Json,
            "bytes" => LogicalType::Bytes,
            "date" => LogicalType::Date,
            "micros" => LogicalType::DurationMicros,
            other => panic!("unknown golden kind {other}"),
        }
    }

    fn cell_of(value: &Value) -> Cell {
        if value.get("null").is_some() {
            return Cell::Null;
        }
        if let Some(item) = value.get("bool") {
            return Cell::Bool(item.as_bool().expect("bool"));
        }
        if let Some(item) = value.get("int") {
            return Cell::Int(item.as_str().expect("int").parse().expect("i128"));
        }
        if let Some(item) = value.get("decimal") {
            return Cell::Decimal(item.as_str().expect("decimal").to_owned());
        }
        if let Some(item) = value.get("float_bits") {
            let text = item.as_str().expect("bits");
            return Cell::Float64(u64::from_str_radix(&text[2..], 16).expect("hex bits"));
        }
        if let Some(item) = value.get("text") {
            return Cell::Text(item.as_str().expect("text").to_owned());
        }
        if let Some(item) = value.get("bytes_hex") {
            return Cell::Bytes(unhex(item.as_str().expect("hex")));
        }
        if let Some(item) = value.get("date") {
            let parts: Vec<i64> = item
                .as_str()
                .expect("date")
                .split('-')
                .map(|p| p.parse().expect("date part"))
                .collect();
            return Cell::Date {
                year: parts[0] as i32,
                month: parts[1] as u8,
                day: parts[2] as u8,
            };
        }
        if let Some(item) = value.get("micros") {
            return Cell::Micros(item.as_str().expect("micros").parse().expect("i64"));
        }
        panic!("unknown golden cell {value}");
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    fn entry(
        kind: PartKind,
        part_no: u32,
        lo: Option<i128>,
        hi: Option<i128>,
        rows: u64,
    ) -> PartEntry {
        PartEntry {
            part_no,
            kind,
            key_lo: lo,
            key_hi: hi,
            rows,
            bytes: 1024,
            sha256: "a".repeat(64),
        }
    }

    fn fixture_entry(
        name: &str,
        kind: PartKind,
        part_no: u32,
        lo: Option<i128>,
        hi: Option<i128>,
        rows: u64,
    ) -> PartEntry {
        let bytes = std::fs::read(fixtures().join(name)).expect("fixture");
        PartEntry {
            part_no,
            kind,
            key_lo: lo,
            key_hi: hi,
            rows,
            bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        }
    }

    fn shuffled<T: Clone>(items: &[T], seed: u64) -> Vec<T> {
        let mut out = items.to_vec();
        let mut state = seed;
        for index in (1..out.len()).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let other = (state >> 33) as usize % (index + 1);
            out.swap(index, other);
        }
        out
    }

    fn err_of<T: std::fmt::Debug>(result: C1PartsResult<T>) -> &'static str {
        result.expect_err("expected an error").reason
    }

    fn decoded(value: LogicalValue) -> DecodedCell {
        DecodedCell {
            presence: CellPresence::Value,
            value: Some(value),
            arrow: None,
            reject: None,
            actual_precision: None,
            actual_scale: None,
            float_bits: None,
        }
    }

    #[test]
    fn part_size_bound_matches_the_loader_limit() {
        assert_eq!(PART_MAX_BYTES, 32 * 1024 * 1024);
    }

    #[test]
    fn part_sink_enforces_the_32_mib_bound() {
        let chunk = vec![7u8; 1024 * 1024];
        let mut sink = PartSink::new(Vec::new());
        for _ in 0..32 {
            sink.write_all(&chunk).expect("within bound");
        }
        assert_eq!(sink.written(), PART_MAX_BYTES);
        let error = sink.write_all(&[1u8]).expect_err("one byte over the bound");
        assert!(error.to_string().contains("part_too_large"));
        assert_eq!(
            sink.written(),
            PART_MAX_BYTES,
            "rejected write is not counted"
        );
        let (inner, total) = sink.into_parts();
        assert_eq!(total, PART_MAX_BYTES);
        assert_eq!(inner.len() as u64, PART_MAX_BYTES);
    }

    #[test]
    fn part_sink_rejects_one_oversized_write_without_writing_it() {
        let mut sink = PartSink::new(Vec::new());
        let big = vec![0u8; PART_MAX_BYTES as usize + 1];
        assert!(sink.write_all(&big).is_err());
        assert_eq!(sink.written(), 0);
        let (inner, _) = sink.into_parts();
        assert!(inner.is_empty());
    }

    #[test]
    fn part_entry_rejects_inconsistent_fields() {
        let good = entry(PartKind::Fill, 1, Some(1), Some(9), 3);
        assert!(validate_entry(&good).is_ok());
        let mut zero_part = good.clone();
        zero_part.part_no = 0;
        assert_eq!(err_of(validate_entry(&zero_part).map(|_| ())), "part_no");
        let mut empty = good.clone();
        empty.bytes = 0;
        assert_eq!(err_of(validate_entry(&empty).map(|_| ())), "part_bytes");
        let mut huge = good.clone();
        huge.bytes = PART_MAX_BYTES + 1;
        assert_eq!(err_of(validate_entry(&huge).map(|_| ())), "part_bytes");
        let mut upper = good.clone();
        upper.sha256 = "A".repeat(64);
        assert_eq!(err_of(validate_entry(&upper).map(|_| ())), "part_sha256");
        let reversed = entry(PartKind::Fill, 1, Some(9), Some(1), 3);
        assert_eq!(err_of(validate_entry(&reversed).map(|_| ())), "key_range");
        let missing_keys = entry(PartKind::Fill, 1, None, None, 3);
        assert_eq!(
            err_of(validate_entry(&missing_keys).map(|_| ())),
            "key_range"
        );
        let keys_on_empty = entry(PartKind::Cdc, 1, Some(1), Some(1), 0);
        assert_eq!(
            err_of(validate_entry(&keys_on_empty).map(|_| ())),
            "key_range"
        );
        let out_of_domain = entry(PartKind::Fill, 1, Some(0), Some(1 << 64), 3);
        assert_eq!(
            err_of(validate_entry(&out_of_domain).map(|_| ())),
            "key_range"
        );
        let empty_ok = entry(PartKind::Fill, 3, None, None, 0);
        assert!(validate_entry(&empty_ok).is_ok());
    }

    #[test]
    fn part_file_names_are_fixed_and_reject_zero() {
        assert_eq!(
            part_file_name(PartKind::Fill, 1).expect("name"),
            "fill-00000001.arrow"
        );
        assert_eq!(
            part_file_name(PartKind::Cdc, 42).expect("name"),
            "cdc-00000042.arrow"
        );
        assert_eq!(err_of(part_file_name(PartKind::Cdc, 0)), "part_no");
    }

    #[test]
    fn single_part_digest_is_that_files_sha256() {
        let part = fixture_entry("fill-00000003.arrow", PartKind::Fill, 1, None, None, 0);
        assert_eq!(
            parts_digest_of(std::slice::from_ref(&part)).expect("digest"),
            part.sha256
        );
        let manifest = PartManifest::new(vec![part.clone()]).expect("single part manifest");
        assert_eq!(manifest.parts_digest(), part.sha256);
    }

    #[test]
    fn parts_digest_does_not_depend_on_input_order() {
        let parts = vec![
            entry(PartKind::Fill, 1, Some(1), Some(50_000), 50_000),
            entry(PartKind::Fill, 2, Some(50_001), Some(99_999), 49_999),
            entry(PartKind::Cdc, 1, Some(7), Some(9), 2),
        ];
        let forward = parts_digest_of(&parts).expect("forward");
        let mut reversed = parts.clone();
        reversed.reverse();
        assert_eq!(parts_digest_of(&reversed).expect("reversed"), forward);
        for seed in 1..8 {
            assert_eq!(
                parts_digest_of(&shuffled(&parts, seed)).expect("shuffled"),
                forward
            );
        }
        assert_eq!(forward.len(), 64);
        let mut changed = parts.clone();
        changed[1].rows += 1;
        assert_ne!(parts_digest_of(&changed).expect("changed"), forward);
    }

    #[test]
    fn manifest_order_is_canonical_and_rejects_structural_faults() {
        let parts = vec![
            entry(PartKind::Cdc, 1, Some(7), Some(9), 2),
            entry(PartKind::Fill, 2, Some(50_001), Some(99_999), 49_999),
            entry(PartKind::Fill, 1, Some(1), Some(50_000), 50_000),
        ];
        let manifest = PartManifest::new(parts.clone()).expect("ordered");
        let kinds: Vec<(PartKind, u32)> = manifest
            .parts()
            .iter()
            .map(|p| (p.kind, p.part_no))
            .collect();
        assert_eq!(
            kinds,
            vec![(PartKind::Fill, 1), (PartKind::Fill, 2), (PartKind::Cdc, 1)]
        );
        assert_eq!(manifest.total_rows(), 50_000 + 49_999 + 2);
        assert_eq!(
            PartManifest::new(shuffled(&parts, 3))
                .expect("shuffled")
                .parts_digest(),
            manifest.parts_digest()
        );

        let gap = vec![
            entry(PartKind::Fill, 1, Some(1), Some(2), 2),
            entry(PartKind::Fill, 3, Some(3), Some(4), 2),
        ];
        assert_eq!(err_of(PartManifest::new(gap)), "part_no_gap");
        let duplicate = vec![
            entry(PartKind::Fill, 1, Some(1), Some(2), 2),
            entry(PartKind::Fill, 1, Some(3), Some(4), 2),
        ];
        assert_eq!(err_of(PartManifest::new(duplicate)), "part_no_duplicate");
        let overlap = vec![
            entry(PartKind::Fill, 1, Some(1), Some(10), 2),
            entry(PartKind::Fill, 2, Some(10), Some(20), 2),
        ];
        assert_eq!(err_of(PartManifest::new(overlap)), "chunk_order");
        assert_eq!(err_of(PartManifest::new(Vec::new())), "manifest_empty");
    }

    #[test]
    fn manifest_json_round_trips_and_detects_tampering() {
        let parts = vec![
            entry(
                PartKind::Fill,
                1,
                Some(-9_223_372_036_854_775_808),
                Some(0),
                2,
            ),
            entry(
                PartKind::Fill,
                2,
                Some(1),
                Some(18_446_744_073_709_551_615),
                3,
            ),
        ];
        let manifest = PartManifest::new(parts).expect("manifest");
        let value = manifest.to_json();
        assert_eq!(value["protocol"], PARTS_PROTOCOL);
        assert_eq!(value["parts"][1]["key_hi"], "18446744073709551615");
        let back = PartManifest::from_json(&value).expect("round trip");
        assert_eq!(back, manifest);

        let mut tampered = value.clone();
        tampered["parts_digest"] = json!("0".repeat(64));
        assert_eq!(err_of(PartManifest::from_json(&tampered)), "parts_digest");
        let mut wrong_protocol = value.clone();
        wrong_protocol["protocol"] = json!("oo.pipeline.c1-parts/v1");
        assert_eq!(
            err_of(PartManifest::from_json(&wrong_protocol)),
            "manifest_protocol"
        );
        let mut bad_key = value.clone();
        bad_key["parts"][0]["key_lo"] = json!("x");
        assert_eq!(err_of(PartManifest::from_json(&bad_key)), "manifest_key");
        let mut bad_kind = value.clone();
        bad_kind["parts"][0]["kind"] = json!("snapshot");
        assert_eq!(err_of(PartManifest::from_json(&bad_kind)), "part_kind");
    }

    #[test]
    fn golden_manifest_fixture_matches_its_digest_and_files() {
        let value: Value = serde_json::from_str(MANIFEST).expect("manifest fixture");
        let manifest = PartManifest::from_json(&value).expect("fixture manifest");
        assert_eq!(manifest.parts().len(), 4);
        assert_eq!(manifest.total_rows(), 8);
        let mut seen = Vec::new();
        for_each_part(&fixtures(), &manifest, |part, stream| {
            seen.push((part.kind, part.part_no, stream.rows));
            Ok(())
        })
        .expect("stream every part");
        assert_eq!(
            seen,
            vec![
                (PartKind::Fill, 1, 3),
                (PartKind::Fill, 2, 2),
                (PartKind::Fill, 3, 0),
                (PartKind::Cdc, 1, 3)
            ]
        );
    }

    #[test]
    fn fill_part_decodes_typed_columns() {
        let value: Value = serde_json::from_str(MANIFEST).expect("manifest fixture");
        let manifest = PartManifest::from_json(&value).expect("fixture manifest");
        let part = &manifest.parts()[0];
        let stream = read_part(&fixtures(), part).expect("fill part");
        let names: Vec<&str> = stream.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["id", "qty", "name", "blob", "created"]);
        assert_eq!(
            stream.columns[0][2],
            NativeCell::Int {
                bit_width: 64,
                value: 3
            }
        );
        assert_eq!(stream.columns[2][1], NativeCell::Utf8(String::new()));
        assert_eq!(stream.columns[3][0], NativeCell::Binary(vec![0, 255, 16]));
    }

    #[test]
    fn cdc_part_carries_typed_position_and_operation() {
        let value: Value = serde_json::from_str(MANIFEST).expect("manifest fixture");
        let manifest = PartManifest::from_json(&value).expect("fixture manifest");
        let part = manifest
            .parts()
            .iter()
            .find(|p| p.kind == PartKind::Cdc)
            .expect("cdc");
        let stream = read_part(&fixtures(), part).expect("cdc part");
        let n = stream.fields.len();
        assert_eq!(stream.fields[n - 2].name, POS_COLUMN);
        assert_eq!(stream.fields[n - 1].name, OP_COLUMN);
        let positions: Vec<i128> = stream.columns[n - 2]
            .iter()
            .map(|cell| match cell {
                NativeCell::Decimal { coefficient, .. } => *coefficient,
                other => panic!("pos {other:?}"),
            })
            .collect();
        assert_eq!(positions, vec![5231, 5231, 5260]);
        let ops: Vec<String> = stream.columns[n - 1]
            .iter()
            .map(|cell| match cell {
                NativeCell::Utf8(text) => text.clone(),
                other => panic!("op {other:?}"),
            })
            .collect();
        assert_eq!(ops, vec!["upsert", "upsert", "delete"]);
    }

    #[test]
    fn part_bytes_are_checked_before_they_are_trusted() {
        let bytes = std::fs::read(fixtures().join("fill-00000001.arrow")).expect("fixture");
        let good = fixture_entry(
            "fill-00000001.arrow",
            PartKind::Fill,
            1,
            Some(1),
            Some(3),
            3,
        );
        assert!(verify_part_bytes(&bytes, &good).is_ok());

        let mut flipped = bytes.clone();
        let last = flipped.len() - 9;
        flipped[last] ^= 0x01;
        assert_eq!(err_of(verify_part_bytes(&flipped, &good)), "part_sha256");

        assert_eq!(
            err_of(verify_part_bytes(&bytes[..bytes.len() - 1], &good)),
            "part_bytes"
        );

        let mut wrong_rows = good.clone();
        wrong_rows.rows = 4;
        assert_eq!(err_of(verify_part_bytes(&bytes, &wrong_rows)), "part_rows");
    }

    #[test]
    fn fill_part_must_not_carry_cdc_columns() {
        let bytes =
            std::fs::read(fixtures().join("fill-with-pos-00000001.arrow")).expect("fixture");
        let mut entry = fixture_entry(
            "fill-with-pos-00000001.arrow",
            PartKind::Fill,
            1,
            Some(5),
            Some(5),
            1,
        );
        entry.bytes = bytes.len() as u64;
        assert_eq!(err_of(verify_part_bytes(&bytes, &entry)), "part_schema");
    }

    #[test]
    fn cdc_part_rejects_an_operation_other_than_upsert_or_delete() {
        let bytes = std::fs::read(fixtures().join("cdc-bad-op-00000001.arrow")).expect("fixture");
        let entry = fixture_entry(
            "cdc-bad-op-00000001.arrow",
            PartKind::Cdc,
            1,
            Some(4),
            Some(4),
            1,
        );
        assert_eq!(err_of(verify_part_bytes(&bytes, &entry)), "cdc_op");
    }

    #[test]
    fn part_entry_for_file_checks_the_written_part() {
        let path = fixtures().join("fill-00000001.arrow");
        let entry =
            part_entry_for_file(&path, 1, PartKind::Fill, Some(1), Some(3), 3).expect("entry");
        assert_eq!(
            entry.sha256,
            fixture_entry("fill-00000001.arrow", PartKind::Fill, 1, None, None, 0).sha256
        );
        assert_eq!(
            err_of(part_entry_for_file(
                &path,
                1,
                PartKind::Fill,
                Some(1),
                Some(3),
                2
            )),
            "part_rows"
        );
        let missing = fixtures().join("no-such-part.arrow");
        assert_eq!(
            err_of(part_entry_for_file(
                &missing,
                1,
                PartKind::Fill,
                None,
                None,
                0
            )),
            "part_open"
        );
    }

    #[test]
    fn bounded_read_refuses_a_file_over_the_bound() {
        let dir = std::env::temp_dir();
        let name = format!("c1-parts-bound-{}.bin", std::process::id());
        let path = dir.join(&name);
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(PART_MAX_BYTES + 1).expect("sparse file");
        drop(file);
        let result = read_bounded(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(err_of(result), "part_too_large");
    }

    #[test]
    fn golden_canonical_vectors_match() {
        let golden = golden();
        let vectors = golden["canonical"].as_array().expect("canonical");
        assert!(vectors.len() >= 30, "golden corpus is too small");
        for vector in vectors {
            let ty = type_of(&vector["type"]);
            let cell = cell_of(&vector["cell"]);
            let got = canonical_cell(&ty, &cell).expect("canonical");
            assert_eq!(
                hex_lower(&got),
                vector["expect_hex"].as_str().expect("hex"),
                "vector {}",
                vector["name"]
            );
        }
    }

    #[test]
    fn golden_parity_expectations_match_the_postgres_readback() {
        let golden = golden();
        let readback: Value = serde_json::from_str(PG_READBACK).expect("readback");
        let parity = golden["parity"].as_array().expect("parity");
        let columns = readback["columns"].as_array().expect("columns");
        assert_eq!(parity.len(), columns.len());
        for column in columns {
            let name = column["name"].as_str().expect("name");
            let oid = column["oid"].as_i64().expect("oid");
            let typmod = column["typmod"].as_i64().expect("typmod") as i32;
            let text = column["text"].as_str();
            let expect = parity
                .iter()
                .find(|p| p["column"] == name)
                .expect("parity entry")["expect_hex"]
                .as_str()
                .expect("hex");
            let ty = logical_type_of_pg(oid, typmod).expect("type");
            let cell = pg_text_cell(oid, text).expect("readback cell");
            assert_eq!(
                hex_lower(&canonical_cell(&ty, &cell).expect("canonical")),
                expect,
                "column {name}"
            );
        }
    }

    #[test]
    fn signed_zero_and_time_sign_are_not_lost_in_readback() {
        let ty = LogicalType::Float64;
        let positive = pg_text_cell(701, Some("0")).expect("zero");
        let negative = pg_text_cell(701, Some("-0")).expect("negative zero");
        assert_ne!(
            canonical_cell(&ty, &positive).expect("c"),
            canonical_cell(&ty, &negative).expect("c")
        );
        let time = pg_text_cell(1186, Some("-838:59:59")).expect("time");
        assert_eq!(time, Cell::Micros(-3_020_399_000_000));
    }

    #[test]
    fn canonical_rejects_values_outside_the_type() {
        assert_eq!(
            err_of(canonical_cell(&LogicalType::Int, &Cell::Text("1".into()))),
            "type_mismatch"
        );
        assert_eq!(
            err_of(canonical_cell(
                &LogicalType::Decimal {
                    precision: 10,
                    scale: 2
                },
                &Cell::Decimal("1.001".into())
            )),
            "decimal_scale"
        );
        assert_eq!(
            err_of(canonical_cell(
                &LogicalType::Decimal {
                    precision: 10,
                    scale: 2
                },
                &Cell::Decimal("1,5".into())
            )),
            "decimal_text"
        );
        assert_eq!(
            err_of(canonical_cell(
                &LogicalType::Date,
                &Cell::Date {
                    year: 2024,
                    month: 2,
                    day: 30
                }
            )),
            "date_range"
        );
        assert_eq!(
            err_of(canonical_cell(
                &LogicalType::Date,
                &Cell::Date {
                    year: 0,
                    month: 1,
                    day: 1
                }
            )),
            "date_range"
        );
        assert_eq!(
            err_of(canonical_cell(&LogicalType::Json, &Cell::Text("{".into()))),
            "json"
        );
        assert_eq!(err_of(canonical_row(&[LogicalType::Int], &[])), "row_width");
        assert_eq!(
            canonical_cell(&LogicalType::Int, &Cell::Null).expect("null"),
            vec![0]
        );
        assert!(canonical_decimal("1.5", -1).is_err());
    }

    #[test]
    fn canonical_row_is_the_concatenation_of_cells() {
        let types = [LogicalType::Int, LogicalType::Text];
        let cells = [Cell::Int(7), Cell::Null];
        let row = canonical_row(&types, &cells).expect("row");
        let mut expected = canonical_cell(&types[0], &cells[0]).expect("a");
        expected.extend(canonical_cell(&types[1], &cells[1]).expect("b"));
        assert_eq!(row, expected);
    }

    #[test]
    fn source_cells_match_their_readback_for_the_same_value() {
        let naive = decoded(LogicalValue::TimestampNaive {
            year: 1969,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            micros: 500_000,
        });
        assert_eq!(
            source_cell(&naive).expect("naive"),
            pg_text_cell(1114, Some("1969-12-31 23:59:59.5")).expect("pg")
        );
        let zoned = decoded(LogicalValue::TimestampTzMicros(1_704_133_800_000_000));
        assert_eq!(
            source_cell(&zoned).expect("zoned"),
            pg_text_cell(1184, Some("2024-01-02 00:00:00+05:30")).expect("pg tz")
        );
        let json = decoded(LogicalValue::Json(
            parse_json("{\"b\":1,\"a\":[]}").expect("json"),
        ));
        let json_cell = source_cell(&json).expect("json cell");
        let ty = LogicalType::Json;
        assert_eq!(
            canonical_cell(&ty, &json_cell).expect("canonical"),
            canonical_cell(
                &ty,
                &pg_text_cell(114, Some("{\"a\": [], \"b\": 1}")).expect("pg")
            )
            .expect("canonical")
        );
        let mut null = decoded(LogicalValue::Text("x".into()));
        null.presence = CellPresence::Null;
        assert_eq!(source_cell(&null).expect("null"), Cell::Null);
        let mut unavailable = decoded(LogicalValue::Text("x".into()));
        unavailable.presence = CellPresence::Unavailable;
        assert_eq!(source_cell(&unavailable).expect("unavailable"), Cell::Null);
        let float = decoded(LogicalValue::Float(FloatClass::Zero));
        assert_eq!(err_of(source_cell(&float)), "float_bits");
    }

    #[test]
    fn pg_readback_parses_each_supported_oid_and_refuses_the_rest() {
        assert_eq!(pg_text_cell(16, Some("t")).expect("bool"), Cell::Bool(true));
        assert_eq!(err_of(pg_text_cell(16, Some("yes"))), "pg_bool");
        assert_eq!(pg_text_cell(23, Some("-7")).expect("int"), Cell::Int(-7));
        assert_eq!(
            pg_text_cell(1042, Some("a  ")).expect("bpchar"),
            Cell::Text("a".into())
        );
        assert_eq!(
            pg_text_cell(1043, Some("a  ")).expect("varchar"),
            Cell::Text("a  ".into())
        );
        assert_eq!(
            pg_text_cell(17, Some("\\x00ff")).expect("bytea"),
            Cell::Bytes(vec![0, 255])
        );
        assert_eq!(err_of(pg_text_cell(17, Some("\\x0"))), "pg_bytea");
        assert_eq!(err_of(pg_text_cell(17, Some("00ff"))), "pg_bytea");
        assert_eq!(
            pg_text_cell(1082, Some("0001-01-01")).expect("date"),
            Cell::Date {
                year: 1,
                month: 1,
                day: 1
            }
        );
        assert_eq!(err_of(pg_text_cell(1082, Some("0000-01-01"))), "pg_date");
        assert_eq!(err_of(pg_text_cell(1082, Some("2024-02-30"))), "pg_date");
        assert_eq!(
            pg_text_cell(1186, Some("1 day -02:00:00")).expect("interval"),
            Cell::Micros(79_200_000_000)
        );
        assert_eq!(err_of(pg_text_cell(1186, Some("1 mon"))), "pg_interval");
        assert_eq!(err_of(pg_text_cell(1186, Some("00:61:00"))), "pg_interval");
        assert_eq!(
            pg_text_cell(1184, Some("2024-01-02 00:00:00+05:30")).expect("tz"),
            Cell::Micros(1_704_133_800_000_000)
        );
        assert_eq!(
            err_of(pg_text_cell(1114, Some("2024-01-02 00:00:00+05:30"))),
            "pg_timestamp"
        );
        assert_eq!(pg_text_cell(1114, None).expect("null"), Cell::Null);
        assert_eq!(err_of(pg_text_cell(99999, Some("1"))), "pg_oid");
        assert_eq!(err_of(logical_type_of_pg(1700, 0)), "numeric_typmod");
        assert_eq!(
            logical_type_of_pg(1700, 1310724).expect("numeric"),
            LogicalType::Decimal {
                precision: 20,
                scale: 0
            }
        );
        assert_eq!(err_of(logical_type_of_pg(9999, -1)), "pg_oid");
    }

    #[test]
    fn bucket_index_is_monotone_over_the_whole_key_domain() {
        assert_eq!(bucket_of(-(1i128 << 63), 4).expect("min"), 0);
        assert_eq!(bucket_of((1i128 << 64) - 1, 4).expect("max"), 15);
        let mut last = 0;
        for key in [
            -(1i128 << 63),
            -1,
            0,
            1,
            1 << 40,
            (1 << 63) - 1,
            1 << 63,
            (1 << 64) - 1,
        ] {
            let index = bucket_of(key, 4).expect("index");
            assert!(index >= last);
            assert!(index < 16);
            last = index;
        }
        assert_eq!(err_of(bucket_of(1i128 << 64, 4)), "key_range");
        assert_eq!(err_of(bucket_of(0, 17)), "bucket_bits");
        assert_eq!(
            bucket_key(&Cell::Decimal("18446744073709551615".into())).expect("u64"),
            (1i128 << 64) - 1
        );
        assert_eq!(err_of(bucket_key(&Cell::Text("1".into()))), "key_type");
    }

    fn rows_for_digest() -> Vec<(i128, Vec<u8>)> {
        let types = [
            LogicalType::Int,
            LogicalType::Decimal {
                precision: 10,
                scale: 2,
            },
            LogicalType::Text,
        ];
        let raw = [
            (
                -(1i128 << 63),
                vec![
                    Cell::Int(-(1i128 << 63)),
                    Cell::Decimal("-12.3".into()),
                    Cell::Text("x".into()),
                ],
            ),
            (
                -1,
                vec![Cell::Int(-1), Cell::Decimal("0".into()), Cell::Null],
            ),
            (
                0,
                vec![
                    Cell::Int(0),
                    Cell::Decimal("1.50".into()),
                    Cell::Text(String::new()),
                ],
            ),
            (1, vec![Cell::Int(1), Cell::Null, Cell::Text("é".into())]),
            (
                (1i128 << 64) - 1,
                vec![
                    Cell::Int((1i128 << 64) - 1),
                    Cell::Decimal("99999999.99".into()),
                    Cell::Text("last".into()),
                ],
            ),
        ];
        raw.iter()
            .map(|(key, cells)| (*key, canonical_row(&types, cells).expect("row")))
            .collect()
    }

    #[test]
    fn state_digest_does_not_depend_on_row_or_merge_order() {
        let rows = rows_for_digest();
        let mut forward = StateDigest::new(4).expect("digest");
        for (key, row) in &rows {
            forward.add_row(*key, row).expect("add");
        }
        let mut reverse = StateDigest::new(4).expect("digest");
        for (key, row) in rows.iter().rev() {
            reverse.add_row(*key, row).expect("add");
        }
        assert_eq!(forward, reverse);
        assert_eq!(forward.rows(), 5);

        let mut left = StateDigest::new(4).expect("left");
        let mut right = StateDigest::new(4).expect("right");
        for (index, (key, row)) in rows.iter().enumerate() {
            if index % 2 == 0 {
                left.add_row(*key, row).expect("left add");
            } else {
                right.add_row(*key, row).expect("right add");
            }
        }
        let mut ab = left.clone();
        ab.merge(&right).expect("ab");
        let mut ba = right.clone();
        ba.merge(&left).expect("ba");
        assert_eq!(ab, forward);
        assert_eq!(ba, forward);
        assert_eq!(
            err_of(
                StateDigest::new(4)
                    .expect("d")
                    .merge(&StateDigest::new(5).expect("e"))
            ),
            "bucket_bits"
        );
    }

    #[test]
    fn state_digest_counts_duplicate_rows_and_detects_a_changed_value() {
        let rows = rows_for_digest();
        let mut once = StateDigest::new(2).expect("digest");
        once.add_row(rows[0].0, &rows[0].1).expect("add");
        let mut twice = once.clone();
        twice.add_row(rows[0].0, &rows[0].1).expect("add twice");
        assert_eq!(twice.rows(), 2);
        assert_ne!(
            once.table_digest(),
            twice.table_digest(),
            "a duplicate row is counted"
        );
        let (count, _) = twice
            .bucket(bucket_of(rows[0].0, 2).expect("bucket"))
            .expect("bucket");
        assert_eq!(count, 2);

        let mut changed = StateDigest::new(2).expect("digest");
        let mut altered = rows[0].1.clone();
        let last = altered.len() - 1;
        altered[last] ^= 0x01;
        changed.add_row(rows[0].0, &altered).expect("add");
        assert_ne!(changed.table_digest(), once.table_digest());
    }

    #[test]
    fn golden_composed_digest_matches_the_independent_generator() {
        let golden = golden();
        let composed = &golden["composed"];
        let bits = composed["bucket_bits"].as_u64().expect("bits") as u8;
        let tables = composed["tables"].as_array().expect("tables");
        let types: Vec<LogicalType> = composed["column_types"]
            .as_array()
            .expect("types")
            .iter()
            .map(type_of)
            .collect();
        let mut digests: Vec<(String, StateDigest)> = Vec::new();
        for table in tables {
            let name = table["name"].as_str().expect("name").to_owned();
            let mut digest = StateDigest::new(bits).expect("digest");
            let rows: Vec<&Value> = table["rows"].as_array().expect("rows").iter().collect();
            for row in shuffled(&rows, 11) {
                let cells: Vec<Cell> = row["cells"]
                    .as_array()
                    .expect("cells")
                    .iter()
                    .map(cell_of)
                    .collect();
                let key = bucket_key(&cells[0]).expect("key");
                assert_eq!(row["key"].as_str().expect("key"), key.to_string());
                digest
                    .add_row(key, &canonical_row(&types, &cells).expect("row"))
                    .expect("add");
            }
            let expect = composed["expect_table_digests"][name.as_str()]
                .as_str()
                .expect("table digest");
            assert_eq!(hex_lower(&digest.table_digest()), expect, "table {name}");
            digests.push((name, digest));
        }
        let refs: Vec<(&str, &StateDigest)> =
            digests.iter().rev().map(|(n, d)| (n.as_str(), d)).collect();
        assert_eq!(
            composed_digest(&refs).expect("composed"),
            composed["expect_composed_digest"]
                .as_str()
                .expect("composed digest")
        );
    }

    #[test]
    fn composed_digest_rejects_duplicate_or_multiline_names() {
        let digest = StateDigest::new(1).expect("digest");
        assert_eq!(
            err_of(composed_digest(&[("a", &digest), ("a", &digest)])),
            "table_name"
        );
        assert_eq!(err_of(composed_digest(&[("a\nb", &digest)])), "table_name");
        assert_eq!(err_of(StateDigest::new(17)), "bucket_bits");
    }

    #[test]
    fn golden_parts_digest_matches_the_independent_generator() {
        let golden = golden();
        let parts: Vec<PartEntry> = golden["parts"]["parts"]
            .as_array()
            .expect("parts")
            .iter()
            .map(|p| {
                let key = |name: &str| {
                    p[name]
                        .as_str()
                        .map(|text| text.parse::<i128>().expect("key"))
                };
                PartEntry {
                    part_no: p["part_no"].as_u64().expect("no") as u32,
                    kind: PartKind::parse(p["kind"].as_str().expect("kind")).expect("kind"),
                    key_lo: key("key_lo"),
                    key_hi: key("key_hi"),
                    rows: p["rows"].as_u64().expect("rows"),
                    bytes: p["bytes"].as_u64().expect("bytes"),
                    sha256: p["sha256"].as_str().expect("sha").to_owned(),
                }
            })
            .collect();
        assert_eq!(
            parts_digest_of(&parts).expect("digest"),
            golden["parts"]["expect_parts_digest"]
                .as_str()
                .expect("digest")
        );
    }

    #[test]
    fn json_text_is_sorted_normalized_and_compact() {
        assert_eq!(
            canonical_json("{\"b\": 1.0, \"a\": [true, null]}").expect("json"),
            "{\"a\":[true,null],\"b\":1}"
        );
        assert_eq!(
            canonical_json("[1e3, -0.0, 2.50]").expect("json"),
            "[1000,0,2.5]"
        );
        assert_eq!(
            canonical_json("\"\\u0001\\n\"").expect("json"),
            "\"\\u0001\\n\""
        );
        assert_eq!(err_of(canonical_json("{\"a\":1,\"a\":2}")), "json");
    }

    #[test]
    fn decimal_canonical_form_pads_and_drops_only_zero_digits() {
        assert_eq!(
            canonical_decimal("-12.3", 2).expect("pad"),
            b"-12.30".to_vec()
        );
        assert_eq!(
            canonical_decimal("7.000", 2).expect("drop zeros"),
            b"7.00".to_vec()
        );
        assert_eq!(
            canonical_decimal("-0.00", 2).expect("zero"),
            b"0.00".to_vec()
        );
        assert_eq!(
            canonical_decimal("0003", 0).expect("leading zeros"),
            b"3".to_vec()
        );
        assert_eq!(
            err_of(canonical_decimal("1.005", 2).map(|_| ())),
            "decimal_scale"
        );
        assert_eq!(
            err_of(canonical_decimal(".5", 2).map(|_| ())),
            "decimal_text"
        );
    }

    #[test]
    fn micros_from_civil_date_match_the_unix_epoch() {
        assert_eq!(civil_micros(1970, 1, 1, 0, 0, 0, 0).expect("epoch"), 0);
        assert_eq!(
            civil_micros(1969, 12, 31, 23, 59, 59, 500_000).expect("pre"),
            -500_000
        );
        assert_eq!(
            civil_micros(2024, 1, 2, 3, 4, 5, 1).expect("post"),
            1_704_164_645_000_001
        );
        assert_eq!(
            err_of(civil_micros(2024, 1, 2, 24, 0, 0, 0)),
            "timestamp_range"
        );
    }

    #[test]
    fn decimal_and_pg_clock_ranges_are_checked() {
        assert_eq!(err_of(parse_clock("24:00:00").map(|_| ())), "pg_clock");
        assert_eq!(
            parse_clock("12:34:56.000007").expect("clock"),
            (12, 34, 56, 7)
        );
        assert_eq!(parse_fraction("5").expect("fraction"), 500_000);
        assert_eq!(err_of(parse_fraction("1234567").map(|_| ())), "pg_fraction");
        assert_eq!(parse_offset("-05:30").expect("offset"), -19_800);
        assert_eq!(err_of(parse_offset("+5").map(|_| ())), "pg_offset");
    }

    #[test]
    fn logical_type_names_round_trip_and_unknown_names_are_refused() {
        for ty in [
            LogicalType::Bool,
            LogicalType::Int,
            LogicalType::Decimal {
                precision: 20,
                scale: 0,
            },
            LogicalType::Decimal {
                precision: 10,
                scale: 2,
            },
            LogicalType::Float64,
            LogicalType::Text,
            LogicalType::Bytes,
            LogicalType::Date,
            LogicalType::Time,
            LogicalType::DurationMicros,
            LogicalType::TimestampNaive,
            LogicalType::TimestampTz,
            LogicalType::Json,
        ] {
            let name = logical_type_name(&ty).expect("name");
            assert_eq!(logical_type_from_name(&name).expect("parse"), ty, "{name}");
        }
        assert_eq!(err_of(logical_type_from_name("varchar")), "type_name");
        assert_eq!(err_of(logical_type_from_name("decimal(10)")), "type_name");
        assert_eq!(err_of(logical_type_from_name("decimal(x,2)")), "type_name");
        assert_eq!(
            err_of(logical_type_name(&LogicalType::Array(Box::new(
                LogicalType::Int
            )))),
            "type_name"
        );
    }

    /// D7. A MySQL JSON column is canonical `Text`, not `Json`. The producer's
    /// Arrow text and the PostgreSQL `json` readback then hash to the same bytes,
    /// spaces included. A canonical `Json` would reparse and rerender the text, so
    /// the two sides could disagree on a value that is not in normal form.
    #[test]
    fn json_with_spaces_digests_the_same_on_the_source_part_and_the_target_readback() {
        let text = r#"{"a": 1, "b": [1, 2], "c": {"x": "y z"}}"#;
        let source = vec![Cell::Int(1), Cell::Text(text.to_owned())];
        let target = vec![
            Cell::Int(1),
            pg_text_cell(114, Some(text)).expect("readback"),
        ];
        let types = [LogicalType::Int, LogicalType::Text];
        let mut source_digest = StateDigest::new(C1_BUCKET_BITS).expect("bits");
        let mut target_digest = StateDigest::new(C1_BUCKET_BITS).expect("bits");
        source_digest
            .add_row(1, &canonical_row(&types, &source).expect("source row"))
            .expect("add");
        target_digest
            .add_row(1, &canonical_row(&types, &target).expect("target row"))
            .expect("add");
        assert_eq!(source_digest.table_digest(), target_digest.table_digest());
        // The same text as `Json` is re-rendered, so it is a different encoding.
        let as_json =
            canonical_cell(&LogicalType::Json, &Cell::Text(text.to_owned())).expect("json");
        let as_text =
            canonical_cell(&LogicalType::Text, &Cell::Text(text.to_owned())).expect("text");
        assert_ne!(as_json, as_text);
    }
}
