//! Wire values, bounds, and the canonical data preimage.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

use crate::sha256::{hex, sha256};

pub const CONTRACT: &str = "cdc-spool-v1";
pub const INPUT_CONTRACT: &str = "cdc-transaction-v1";
pub const PROVENANCE: &str = "caller_declared";
pub const MAX_ACTIVE: usize = 1;
pub const MAX_EVENTS: u64 = 500_000;
pub const MAX_CHUNK_EVENTS: u64 = 8_192;
pub const MAX_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_FILES: u64 = 96;
pub const MAX_DISK_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_COLLECTIONS: usize = 256;
pub const MAX_CELLS: usize = 4096;
pub const MAX_ORDINAL: u16 = 32_767;
pub const INDEX_ALLOWANCE: u64 = 128;
pub const PROGRESS_ALLOWANCE: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultCode {
    Malformed,
    BindRequired,
    BindDuplicate,
    BindInvalid,
    TxnInvalid,
    TxnIdReuse,
    UnknownTxn,
    SequenceInvalid,
    SequenceGap,
    SequenceOutOfRange,
    DuplicateConflict,
    EndTooEarly,
    EndConflict,
    CountMismatch,
    CountOverflow,
    CollectionInvalid,
    CollectionUnknown,
    CollectionCountGap,
    KeyInvalid,
    PayloadInvalid,
    OrdinalMismatch,
    OrdinalOrder,
    OrdinalRange,
    TimestampInvalid,
    CommitLsnInvalid,
    CommitPositionOverflow,
    RollbackAfterComplete,
    LineTooLong,
    SingleStream,
    ActiveTxnExceeded,
    EventCap,
    DiskQuota,
    FileCap,
    ChunkBound,
    LimitRaised,
    PathExists,
    Io,
    ChecksumFault,
    Truncated,
    MissingChunk,
    SymlinkRefused,
    OutputMismatch,
    PayloadConflict,
    StreamClosed,
    NotCommitted,
    AttestationRefused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub code: FaultCode,
    pub txn_id: Option<String>,
}

impl Fault {
    pub fn new(code: FaultCode, txn_id: Option<String>) -> Self {
        Self { code, txn_id }
    }
}

impl fmt::Display for FaultCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        match value.as_str() {
            Some(text) => formatter.write_str(text),
            None => Err(fmt::Error),
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.code.to_string())
    }
}

impl std::error::Error for Fault {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ordinal(pub u16);

impl<'de> Deserialize<'de> for Ordinal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrdinalVisitor;

        impl Visitor<'_> for OrdinalVisitor {
            type Value = Ordinal;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an integer ordinal")
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Ordinal, E> {
                if value > u64::from(MAX_ORDINAL) {
                    return Err(E::custom("ordinal_range"));
                }
                Ok(Ordinal(u16::try_from(value).map_err(|_| E::custom("ordinal_range"))?))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Ordinal, E> {
                if value < 0 {
                    return Err(E::custom("ordinal_range"));
                }
                self.visit_u64(u64::try_from(value).map_err(|_| E::custom("ordinal_range"))?)
            }
        }

        deserializer.deserialize_any(OrdinalVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Null { ordinal: u16 },
    Unavailable { ordinal: u16 },
    Bool { ordinal: u16, value: bool },
    Int { ordinal: u16, value: String },
    Decimal { ordinal: u16, value: String },
    Text { ordinal: u16, value: String },
    Bytes { ordinal: u16, value: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum RawCell {
    Null { ordinal: Ordinal },
    Unavailable { ordinal: Ordinal },
    Bool { ordinal: Ordinal, value: bool },
    Int { ordinal: Ordinal, value: String },
    Decimal { ordinal: Ordinal, value: String },
    Text { ordinal: Ordinal, value: String },
    Bytes { ordinal: Ordinal, value: String },
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match RawCell::deserialize(deserializer)? {
            RawCell::Null { ordinal } => Self::Null { ordinal: ordinal.0 },
            RawCell::Unavailable { ordinal } => Self::Unavailable { ordinal: ordinal.0 },
            RawCell::Bool { ordinal, value } => Self::Bool {
                ordinal: ordinal.0,
                value,
            },
            RawCell::Int { ordinal, value } => Self::Int {
                ordinal: ordinal.0,
                value,
            },
            RawCell::Decimal { ordinal, value } => Self::Decimal {
                ordinal: ordinal.0,
                value,
            },
            RawCell::Text { ordinal, value } => Self::Text {
                ordinal: ordinal.0,
                value,
            },
            RawCell::Bytes { ordinal, value } => Self::Bytes {
                ordinal: ordinal.0,
                value,
            },
        })
    }
}

impl Cell {
    pub fn ordinal(&self) -> u16 {
        match self {
            Self::Null { ordinal }
            | Self::Unavailable { ordinal }
            | Self::Bool { ordinal, .. }
            | Self::Int { ordinal, .. }
            | Self::Decimal { ordinal, .. }
            | Self::Text { ordinal, .. }
            | Self::Bytes { ordinal, .. } => *ordinal,
        }
    }

    fn value_bytes(&self) -> usize {
        match self {
            Self::Null { .. } | Self::Unavailable { .. } | Self::Bool { .. } => 0,
            Self::Int { value, .. }
            | Self::Decimal { value, .. }
            | Self::Text { value, .. }
            | Self::Bytes { value, .. } => value.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Wire {
    Bind {
        source: String,
        lineage: String,
        epoch: u32,
        selected_tables: Vec<String>,
        schema_hash: String,
        config_hash: String,
        type_hash: String,
        hook_hashes: Vec<String>,
        commit_position_provenance: String,
    },
    Barrier {
        reason: String,
        #[serde(default)]
        lineage: Option<String>,
        #[serde(default)]
        epoch: Option<u32>,
    },
    Begin {
        txn_id: String,
    },
    Data {
        txn_id: String,
        global_txn_sequence: u64,
        data_collection_sequence: u64,
        table: String,
        key: Vec<Cell>,
        op: Op,
        payload: Vec<Cell>,
        unavailable_ordinals: Vec<Ordinal>,
    },
    End {
        txn_id: String,
        count: u64,
        per_collection_counts: BTreeMap<String, u64>,
        commit_lsn: String,
        source_commit_timestamp: String,
    },
    Rollback {
        txn_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub source: String,
    pub lineage: String,
    pub epoch: u32,
    pub selected_tables: Vec<String>,
    pub schema_hash: String,
    pub config_hash: String,
    pub type_hash: String,
    pub hook_hashes: Vec<String>,
}

pub struct CanonicalData<'a> {
    pub txn_id: &'a str,
    pub global_txn_sequence: u64,
    pub data_collection_sequence: u64,
    pub table: &'a str,
    pub key: &'a [Cell],
    pub op: Op,
    pub payload: &'a [Cell],
    pub unavailable_ordinals: &'a [u16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_events: u64,
    pub max_chunk_events: u64,
    pub max_chunk_bytes: u64,
    pub max_disk_bytes: u64,
    pub max_files: u64,
    pub max_line_bytes: usize,
    pub max_collections: usize,
}

impl Limits {
    pub fn product() -> Self {
        Self {
            max_events: MAX_EVENTS,
            max_chunk_events: MAX_CHUNK_EVENTS,
            max_chunk_bytes: MAX_CHUNK_BYTES,
            max_disk_bytes: MAX_DISK_BYTES,
            max_files: MAX_FILES,
            max_line_bytes: MAX_LINE_BYTES,
            max_collections: MAX_COLLECTIONS,
        }
    }

    pub fn checked(self) -> Result<Self, FaultCode> {
        let product = Self::product();
        if self.max_events == 0
            || self.max_events > product.max_events
            || self.max_chunk_events == 0
            || self.max_chunk_events > product.max_chunk_events
            || self.max_chunk_bytes == 0
            || self.max_chunk_bytes > product.max_chunk_bytes
            || self.max_disk_bytes == 0
            || self.max_disk_bytes > product.max_disk_bytes
            || self.max_files == 0
            || self.max_files > product.max_files
            || self.max_line_bytes == 0
            || self.max_line_bytes > product.max_line_bytes
            || self.max_collections == 0
            || self.max_collections > product.max_collections
        {
            return Err(FaultCode::LimitRaised);
        }
        Ok(self)
    }
}

pub fn raw_value_bytes(key: &[Cell], payload: &[Cell]) -> u64 {
    key.iter()
        .chain(payload)
        .map(Cell::value_bytes)
        .fold(0u64, |total, len| total.saturating_add(u64::try_from(len).unwrap_or(u64::MAX)))
}

pub fn validate_bind(bind: &Bind, provenance: &str) -> Result<(), FaultCode> {
    if !is_source(&bind.source) || !is_hex64(&bind.lineage) || provenance != PROVENANCE {
        return Err(FaultCode::BindInvalid);
    }
    if !is_hex64(&bind.schema_hash) || !is_hex64(&bind.config_hash) || !is_hex64(&bind.type_hash) {
        return Err(FaultCode::BindInvalid);
    }
    if bind.selected_tables.len() > 256 || !sorted_unique(&bind.selected_tables) {
        return Err(FaultCode::BindInvalid);
    }
    if bind.selected_tables.iter().any(|table| !is_qualified(table)) {
        return Err(FaultCode::BindInvalid);
    }
    if bind.hook_hashes.len() > 32 || !sorted_unique(&bind.hook_hashes) {
        return Err(FaultCode::BindInvalid);
    }
    if bind.hook_hashes.iter().any(|hash| !is_hex64(hash)) {
        return Err(FaultCode::BindInvalid);
    }
    Ok(())
}

pub fn validate_txn_id(text: &str) -> Result<(), FaultCode> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_alphanumeric() {
        return Err(FaultCode::TxnInvalid);
    }
    if bytes.iter().skip(1).all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
    }) {
        Ok(())
    } else {
        Err(FaultCode::TxnInvalid)
    }
}

pub fn validate_table(text: &str) -> Result<(), FaultCode> {
    if is_qualified(text) {
        Ok(())
    } else {
        Err(FaultCode::CollectionInvalid)
    }
}

pub fn validate_timestamp(text: &str) -> Result<(), FaultCode> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes.len() > 27 || bytes[10] != b'T' {
        return Err(FaultCode::TimestampInvalid);
    }
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return Err(FaultCode::TimestampInvalid);
    }
    let year = parse_digits(&bytes[0..4])?;
    let month = parse_digits(&bytes[5..7])?;
    let day = parse_digits(&bytes[8..10])?;
    let hour = parse_digits(&bytes[11..13])?;
    let minute = parse_digits(&bytes[14..16])?;
    let second = parse_digits(&bytes[17..19])?;
    if year == 0 || year > 9999 || hour > 23 || minute > 59 || second > 59 {
        return Err(FaultCode::TimestampInvalid);
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return Err(FaultCode::TimestampInvalid),
    };
    if day == 0 || day > max_day {
        return Err(FaultCode::TimestampInvalid);
    }
    if bytes.len() == 20 {
        return if bytes[19] == b'Z' {
            Ok(())
        } else {
            Err(FaultCode::TimestampInvalid)
        };
    }
    if bytes[19] != b'.' || bytes[bytes.len() - 1] != b'Z' {
        return Err(FaultCode::TimestampInvalid);
    }
    let frac = &bytes[20..bytes.len() - 1];
    if frac.is_empty() || frac.len() > 6 || !frac.iter().all(|byte| byte.is_ascii_digit()) {
        return Err(FaultCode::TimestampInvalid);
    }
    if frac.last() == Some(&b'0') {
        return Err(FaultCode::TimestampInvalid);
    }
    Ok(())
}

pub fn validate_key(cells: &[Cell]) -> Result<(), FaultCode> {
    if cells.is_empty() {
        return Err(FaultCode::KeyInvalid);
    }
    validate_cell_run(cells, true)
}

pub fn validate_payload(cells: &[Cell], unavailable: &[u16]) -> Result<(), FaultCode> {
    validate_cell_run(cells, false)?;
    if !strictly_increasing(unavailable) {
        return Err(FaultCode::OrdinalMismatch);
    }
    let mut found = Vec::new();
    for cell in cells {
        if matches!(cell, Cell::Unavailable { .. }) {
            found.push(cell.ordinal());
        }
    }
    if found == unavailable {
        Ok(())
    } else {
        Err(FaultCode::OrdinalMismatch)
    }
}

pub fn canonical_preimage(data: &CanonicalData<'_>) -> Result<String, FaultCode> {
    let mut out = String::new();
    out.push('{');
    push_u64(&mut out, "data_collection_sequence", data.data_collection_sequence);
    out.push(',');
    push_u64(&mut out, "global_txn_sequence", data.global_txn_sequence);
    out.push_str(",\"key\":");
    push_cells(&mut out, data.key)?;
    out.push_str(",\"op\":");
    out.push_str(&json_string(op_name(data.op))?);
    out.push_str(",\"payload\":");
    push_cells(&mut out, data.payload)?;
    out.push_str(",\"table\":");
    out.push_str(&json_string(data.table)?);
    out.push_str(",\"txn_id\":");
    out.push_str(&json_string(data.txn_id)?);
    out.push_str(",\"unavailable_ordinals\":");
    push_u16s(&mut out, data.unavailable_ordinals);
    out.push('}');
    Ok(out)
}

pub fn event_hash(preimage: &str) -> Result<[u8; 32], FaultCode> {
    sha256(preimage.as_bytes()).ok_or(FaultCode::PayloadInvalid)
}

pub fn event_hash_hex(preimage: &str) -> Result<String, FaultCode> {
    Ok(hex(&event_hash(preimage)?))
}

fn validate_cell_run(cells: &[Cell], key: bool) -> Result<(), FaultCode> {
    if cells.len() > MAX_CELLS {
        return Err(if key {
            FaultCode::KeyInvalid
        } else {
            FaultCode::PayloadInvalid
        });
    }
    let mut previous: Option<u16> = None;
    for cell in cells {
        let ordinal = cell.ordinal();
        if previous.is_some_and(|seen| ordinal <= seen) {
            return Err(FaultCode::OrdinalOrder);
        }
        previous = Some(ordinal);
        match cell {
            Cell::Null { .. } | Cell::Unavailable { .. } if key => {
                return Err(FaultCode::KeyInvalid);
            }
            Cell::Null { .. } | Cell::Unavailable { .. } | Cell::Bool { .. } => {}
            Cell::Int { value, .. } => valid_int(value, key)?,
            Cell::Decimal { value, .. } => valid_decimal(value, key)?,
            Cell::Text { .. } => {}
            Cell::Bytes { value, .. } => valid_bytes(value, key)?,
        }
    }
    Ok(())
}

fn valid_int(text: &str, key: bool) -> Result<(), FaultCode> {
    let code = if key {
        FaultCode::KeyInvalid
    } else {
        FaultCode::PayloadInvalid
    };
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || digits.len() > 76 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(code);
    }
    if text.starts_with('-') && digits.bytes().all(|byte| byte == b'0') {
        return Err(code);
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return Err(code);
    }
    Ok(())
}

fn valid_decimal(text: &str, key: bool) -> Result<(), FaultCode> {
    let code = if key {
        FaultCode::KeyInvalid
    } else {
        FaultCode::PayloadInvalid
    };
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int_part, frac) = match rest.split_once('.') {
        Some((int_part, frac)) => (int_part, Some(frac)),
        None => (rest, None),
    };
    if int_part.is_empty()
        || int_part.len() > 76
        || !int_part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(code);
    }
    if int_part.len() > 1 && int_part.starts_with('0') {
        return Err(code);
    }
    if let Some(frac) = frac
        && (frac.is_empty()
            || !frac.bytes().all(|byte| byte.is_ascii_digit())
            || int_part.len() + frac.len() > 76)
    {
        return Err(code);
    }
    let frac_zero = frac.unwrap_or("").bytes().all(|byte| byte == b'0');
    if negative && int_part.bytes().all(|byte| byte == b'0') && frac_zero {
        return Err(code);
    }
    Ok(())
}

fn valid_bytes(text: &str, key: bool) -> Result<(), FaultCode> {
    let code = if key {
        FaultCode::KeyInvalid
    } else {
        FaultCode::PayloadInvalid
    };
    if text.len().is_multiple_of(2)
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(code)
    }
}

fn push_cells(out: &mut String, cells: &[Cell]) -> Result<(), FaultCode> {
    out.push('[');
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"ordinal\":");
        out.push_str(&cell.ordinal().to_string());
        out.push_str(",\"type\":");
        out.push_str(&json_string(cell_type(cell))?);
        match cell {
            Cell::Null { .. } | Cell::Unavailable { .. } => {}
            Cell::Bool { value, .. } => {
                out.push_str(",\"value\":");
                out.push_str(if *value { "true" } else { "false" });
            }
            Cell::Int { value, .. }
            | Cell::Decimal { value, .. }
            | Cell::Text { value, .. }
            | Cell::Bytes { value, .. } => {
                out.push_str(",\"value\":");
                out.push_str(&json_string(value)?);
            }
        }
        out.push('}');
    }
    out.push(']');
    Ok(())
}

fn push_u64(out: &mut String, name: &str, value: u64) {
    out.push('"');
    out.push_str(name);
    out.push_str("\":");
    out.push_str(&value.to_string());
}

fn push_u16s(out: &mut String, values: &[u16]) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
}

fn cell_type(cell: &Cell) -> &'static str {
    match cell {
        Cell::Null { .. } => "null",
        Cell::Unavailable { .. } => "unavailable",
        Cell::Bool { .. } => "bool",
        Cell::Int { .. } => "int",
        Cell::Decimal { .. } => "decimal",
        Cell::Text { .. } => "text",
        Cell::Bytes { .. } => "bytes",
    }
}

fn op_name(op: Op) -> &'static str {
    match op {
        Op::Upsert => "upsert",
        Op::Delete => "delete",
    }
}

fn json_string(text: &str) -> Result<String, FaultCode> {
    serde_json::to_string(text).map_err(|_| FaultCode::PayloadInvalid)
}

fn strictly_increasing(values: &[u16]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

pub fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_source(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_table_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn is_qualified(text: &str) -> bool {
    let Some((schema, table)) = text.split_once('.') else {
        return false;
    };
    !table.contains('.') && is_source(schema) && is_table_token(table)
}

fn parse_digits(bytes: &[u8]) -> Result<u32, FaultCode> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return Err(FaultCode::TimestampInvalid);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FaultCode::TimestampInvalid)?;
    text.parse().map_err(|_| FaultCode::TimestampInvalid)
}

fn is_leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

pub fn chunk_name(index: u64) -> Result<String, FaultCode> {
    if index == 0 || index > MAX_FILES {
        return Err(FaultCode::FileCap);
    }
    Ok(format!("chunk-{index:06}"))
}

pub fn is_chunk_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("chunk-") else {
        return false;
    };
    rest.len() == 6 && rest != "000000" && rest.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{canonical_preimage, event_hash_hex, CanonicalData, Cell, Op};

    #[test]
    fn sample_preimage_hash_is_the_literal() {
        let key = vec![Cell::Int {
            ordinal: 0,
            value: "1".to_owned(),
        }];
        let payload = vec![
            Cell::Text {
                ordinal: 0,
                value: "a".to_owned(),
            },
            Cell::Null { ordinal: 1 },
            Cell::Unavailable { ordinal: 2 },
        ];
        let data = CanonicalData {
            txn_id: "inv17",
            global_txn_sequence: 3,
            data_collection_sequence: 2,
            table: "public.lines",
            key: &key,
            op: Op::Upsert,
            payload: &payload,
            unavailable_ordinals: &[2],
        };
        let preimage = canonical_preimage(&data).unwrap();
        assert_eq!(
            preimage,
            "{\"data_collection_sequence\":2,\"global_txn_sequence\":3,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"1\"}],\"op\":\"upsert\",\"payload\":[{\"ordinal\":0,\"type\":\"text\",\"value\":\"a\"},{\"ordinal\":1,\"type\":\"null\"},{\"ordinal\":2,\"type\":\"unavailable\"}],\"table\":\"public.lines\",\"txn_id\":\"inv17\",\"unavailable_ordinals\":[2]}"
        );
        assert_eq!(
            event_hash_hex(&preimage).unwrap(),
            "4864cde4bd37438f0fa3c69808c0adb5ddd244d2450987ad69299330b2b8e8ee"
        );
    }
}
