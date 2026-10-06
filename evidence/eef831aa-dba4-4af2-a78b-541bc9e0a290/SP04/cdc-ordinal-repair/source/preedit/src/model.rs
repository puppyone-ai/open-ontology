//! Wire values, bounds, and the canonical data preimage.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;

use crate::sha256::{hex, sha256};

pub const CONTRACT: &str = "cdc-transaction-v1";
pub const MAX_ACTIVE: usize = 16;
pub const MAX_TXN_EVENTS: u64 = 200_000;
pub const MAX_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const PROVENANCE: &str = "caller_declared";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    CommitPositionConflict,
    CommitPositionRegression,
    RollbackAfterComplete,
    NeedsSpool,
    OpenSetExceeded,
    OpenTxnAtBarrier,
    EpochUnchanged,
    LineageRequired,
    LineageNotAllowed,
    LineageUnchanged,
    BarrierInvalid,
    LineTooLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub code: FaultCode,
    pub txn_id: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarrierReason {
    Epoch,
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cell {
    Null { ordinal: u16 },
    Unavailable { ordinal: u16 },
    Bool { ordinal: u16, value: bool },
    Int { ordinal: u16, value: String },
    Decimal { ordinal: u16, value: String },
    Text { ordinal: u16, value: String },
    Bytes { ordinal: u16, value: String },
}

impl Cell {
    fn ordinal(&self) -> u16 {
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

#[derive(Debug, Clone, Deserialize)]
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
        reason: BarrierReason,
        #[serde(default)]
        lineage: Option<String>,
        epoch: u32,
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
        unavailable_ordinals: Vec<u16>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attestations {
    pub upstream_commit_lsn_verified: bool,
    pub cross_table_consistent: bool,
    pub target_applied: bool,
    pub ack_allowed: bool,
    pub debezium_normalizer: bool,
    pub p2_verified: bool,
    pub p11_verified: bool,
}

impl Attestations {
    pub fn fixed() -> Self {
        Self {
            upstream_commit_lsn_verified: false,
            cross_table_consistent: false,
            target_applied: false,
            ack_allowed: false,
            debezium_normalizer: false,
            p2_verified: false,
            p11_verified: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmittedRow {
    pub global_txn_sequence: u64,
    pub data_collection_sequence: u64,
    pub table: String,
    pub key: Vec<Cell>,
    pub op: Op,
    pub payload: Vec<Cell>,
    pub unavailable_ordinals: Vec<u16>,
    pub event_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletedTxn {
    pub contract: &'static str,
    pub kind: &'static str,
    pub source: String,
    pub lineage: String,
    pub epoch: u32,
    pub schema_hash: String,
    pub config_hash: String,
    pub type_hash: String,
    pub hook_hashes: Vec<String>,
    pub selected_tables: Vec<String>,
    pub commit_position_provenance: &'static str,
    pub txn_id: String,
    pub commit_order: u64,
    pub commit_lsn: String,
    pub position: String,
    pub source_commit_timestamp: String,
    pub event_count: u64,
    pub per_collection_counts: BTreeMap<String, u64>,
    pub selected_row_count: u64,
    pub rows: Vec<EmittedRow>,
    pub attestations: Attestations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingStatus {
    Open,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingTxn {
    pub txn_id: String,
    pub data_events: u64,
    pub payload_bytes: u64,
    pub status: PendingStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadProgress {
    pub lines: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeenProgress {
    pub bind: u64,
    pub barrier: u64,
    pub begin: u64,
    pub data: u64,
    pub end: u64,
    pub rollback: u64,
    pub duplicate_data: u64,
    pub duplicate_begin: u64,
    pub duplicate_end: u64,
    pub duplicate_rollback: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecodedProgress {
    pub transactions: u64,
    pub selected_rows: u64,
    pub last_position: Option<String>,
    pub last_commit_lsn: Option<String>,
    pub last_epoch: Option<u32>,
    pub last_lineage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub contract: &'static str,
    pub kind: &'static str,
    pub source: Option<String>,
    pub read: ReadProgress,
    pub seen: SeenProgress,
    pub complete_decoded: DecodedProgress,
    pub applied: Option<()>,
    pub resume_required: bool,
    pub needs_spool: bool,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
    pub upstream_commit_lsn_verified: bool,
    pub debezium_normalizer: bool,
    pub p2_verified: bool,
    pub p11_verified: bool,
    pub commit_position_provenance: Option<&'static str>,
    pub pending: Vec<PendingTxn>,
    pub fault: Option<FaultCode>,
}

pub fn raw_value_bytes(key: &[Cell], payload: &[Cell]) -> Result<u64, FaultCode> {
    let mut total = 0u64;
    for cell in key.iter().chain(payload) {
        let len = u64::try_from(cell.value_bytes()).map_err(|_| FaultCode::NeedsSpool)?;
        total = total.checked_add(len).ok_or(FaultCode::NeedsSpool)?;
    }
    Ok(total)
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
    if bytes.is_empty() || bytes.len() > 128 {
        return Err(FaultCode::TxnInvalid);
    }
    if !bytes[0].is_ascii_alphanumeric() {
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

pub fn event_hash(preimage: &str) -> Result<String, FaultCode> {
    let digest = sha256(preimage.as_bytes()).ok_or(FaultCode::PayloadInvalid)?;
    Ok(hex(&digest))
}

fn validate_cell_run(cells: &[Cell], key: bool) -> Result<(), FaultCode> {
    if cells.len() > 4096 {
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
            Cell::Null { .. } | Cell::Unavailable { .. } => {}
            Cell::Bool { .. } => {}
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

fn is_hex64(text: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{canonical_preimage, event_hash, validate_timestamp, CanonicalData, Cell, Op};

    #[test]
    fn preimage_keeps_null_distinct_from_unavailable() {
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
        let hash = event_hash(&preimage).unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
    }

    #[test]
    fn timestamp_matches_the_core_stored_spelling() {
        assert!(validate_timestamp("2026-04-15T08:30:00.123456Z").is_ok());
        assert!(validate_timestamp("2026-04-15T08:30:01Z").is_ok());
        assert!(validate_timestamp("2024-02-29T00:00:00Z").is_ok());
        for text in [
            "2026-04-15T08:30:00.123450Z",
            "2026-04-15T08:30:00.123456+00:00",
            "2025-02-29T00:00:00Z",
            "2026-04-15T08:30:60Z",
            "2026-04-15t08:30:00Z",
        ] {
            assert!(validate_timestamp(text).is_err(), "{text}");
        }
    }
}
