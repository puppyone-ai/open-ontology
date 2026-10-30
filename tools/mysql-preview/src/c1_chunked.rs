//! Producer v2 for the chunked C1 baseline (`c1-baseline-stream-v2`).
//!
//! Design: `specs/chunked-baseline/DESIGN.md` §3.1–§3.7, §3.13 and §4.3. Frames:
//! `specs/chunked-baseline/FRAMES-V2.md`. The producer never receives target
//! credentials and never returns rows. Each keyset chunk becomes one Arrow part
//! file. CDC transactions are batched into CDC part files. Every part is written
//! and journaled before its frame is sent, and a frame is acknowledged before the
//! next one. The worker's resume information is authoritative after a restart.
//!
//! The binlog stream is subscribed from the L0 position before the first chunk
//! read (DESIGN §3.1 P1). Chunk reads and binlog events run concurrently, so a
//! long chunk does not stop CDC consumption.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::future::{Future, poll_fn};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use std::time::{Duration, Instant};

use arrow_ipc::writer::StreamWriter;
use futures_util::stream::Stream;
use mysql_async::binlog::events::{EventData, RowsEventData, TableMapEvent};
use mysql_async::binlog::row::BinlogRow;
use mysql_async::binlog::value::BinlogValue;
use mysql_async::prelude::Queryable;
use mysql_async::{BinlogStream, BinlogStreamRequest, Conn, Row, Sid, Value as MyValue};
use oo_connectors::mysql::map::{MappedColumn, MysqlColumn, map_table};
use oo_connectors::mysql::values::{self, RawCell};
use oo_connectors::mysql::{
    CHUNK_DEADLINE_MS, CHUNK_MAX_BYTES, CHUNK_MAX_ROWS, CHUNK_STATEMENT_MS, Cancel, ChunkBatch,
    ChunkCursor, ChunkReader, MysqlConfig, MysqlError, ReadConnection,
};
use oo_connectors::types::{ColumnPlan, DecodedCell, RunBinding, TypedBatch, assemble_batch};
use oo_pipeline::c1_parts::{
    PART_MAX_BYTES, PartEntry, PartKind, PartManifest, PartSink, part_entry_for_file,
};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::c1::{
    C1Config, batch_source_config, configured_selection, connect, connect_cdc, format_uuid,
    full_status, gtid_contains, load_column_kinds, load_config, load_pk, mysql_err,
    native_table_schemas, parse_span, preflight_values, replication_grants_ok, rows_first, scalar,
    send_frame, unix_text, visible_ordinals, write_json,
};
use crate::c1_replica::{self, ReadSource, ReplicaSource};
use crate::config::read_secret;
use crate::error::PreviewError;

pub(crate) const PROTO_BOOTSTRAP: &str = "oo.jobs.mysql-c1-bootstrap/v2";
pub(crate) const PROTO_CHUNK: &str = "oo.jobs.mysql-c1-chunk/v2";
pub(crate) const PROTO_TXN_BATCH: &str = "oo.jobs.mysql-c1-txn-batch/v2";
pub(crate) const PROTO_ACK: &str = "oo.jobs.mysql-c1-baseline-ack/v2";
const PROTO_JOURNAL: &str = "oo.mysql.c1-chunk-journal/v2";
const PROTO_RECEIPT: &str = "oo.mysql.c1-baseline-stream/v2";
const PROTO_PARTS: &str = "oo.mysql.c1-parts-manifest/v2";
const FORMAT: &str = "chunked-typed/v2";
const JOURNAL_FILE: &str = "journal.jsonl";
const PARTS_MANIFEST_FILE: &str = "parts-manifest.json";
const RECEIPT_FILE: &str = "receipt.json";

const MAX_TXN_EVENTS_DEFAULT: u64 = 200_000;
const MAX_TXN_BYTES_DEFAULT: u64 = 256 * 1024 * 1024;
const MAX_TABLE_ROWS_DEFAULT: u64 = 50_000_000;
const DEADLINE_MS_DEFAULT: u64 = 43_200_000;
/// Read attempts per chunk. The retry budget (`connect_retry_ms`) normally ends a retry
/// first; this is only the hard bound on attempts.
const MAX_CHUNK_ATTEMPTS_DEFAULT: u32 = 20;
/// Wall-clock budget for transient retries (chunk reads, reconnects, the binlog stream).
/// Default 6 minutes: a tunnel outage of a few minutes is survived inside one producer run.
const CONNECT_RETRY_MS_DEFAULT: u64 = 360_000;
const CONNECT_RETRY_MS_MAX: u64 = 3_600_000;
const BATCH_MAX_TXNS_DEFAULT: usize = 1000;
const BATCH_MAX_BYTES_DEFAULT: u64 = 8 * 1024 * 1024;
const BATCH_MAX_AGE_MS_DEFAULT: u64 = 1000;
/// Catch-up to L_end fails retryably after this long without a new applied position.
const CATCHUP_STALL_MS_DEFAULT: u64 = 300_000;
/// Raw bytes per CDC part. The Arrow encoding stays far below `PART_MAX_BYTES`.
const CDC_PART_RAW_BYTES: u64 = 8 * 1024 * 1024;
/// Poll interval for batch aging and the deadline.
const TICK: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Budgets and batch limits. Every value can come from the bootstrap frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    pub chunk_rows: u64,
    pub chunk_bytes: u64,
    pub chunk_deadline_ms: u64,
    pub statement_ms: u64,
    pub max_chunk_attempts: u32,
    pub connect_retry_ms: u64,
    pub max_txn_events: u64,
    pub max_txn_bytes: u64,
    pub max_table_rows: u64,
    pub deadline_ms: u64,
    pub batch_max_txns: usize,
    pub batch_max_bytes: u64,
    pub batch_max_age_ms: u64,
    pub catchup_stall_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            chunk_rows: CHUNK_MAX_ROWS,
            chunk_bytes: CHUNK_MAX_BYTES,
            chunk_deadline_ms: CHUNK_DEADLINE_MS,
            statement_ms: CHUNK_STATEMENT_MS,
            max_chunk_attempts: MAX_CHUNK_ATTEMPTS_DEFAULT,
            connect_retry_ms: CONNECT_RETRY_MS_DEFAULT,
            max_txn_events: MAX_TXN_EVENTS_DEFAULT,
            max_txn_bytes: MAX_TXN_BYTES_DEFAULT,
            max_table_rows: MAX_TABLE_ROWS_DEFAULT,
            deadline_ms: DEADLINE_MS_DEFAULT,
            batch_max_txns: BATCH_MAX_TXNS_DEFAULT,
            batch_max_bytes: BATCH_MAX_BYTES_DEFAULT,
            batch_max_age_ms: BATCH_MAX_AGE_MS_DEFAULT,
            catchup_stall_ms: CATCHUP_STALL_MS_DEFAULT,
        }
    }
}

fn config_err(fact: &'static str) -> PreviewError {
    PreviewError::new("config", fact)
}

fn worker_err(fact: &'static str) -> PreviewError {
    PreviewError::new("worker", fact)
}

fn u64_of(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
}

fn i128_of(value: &Value) -> Option<i128> {
    match value {
        Value::Null => None,
        Value::Number(number) => number.as_i64().map(i128::from),
        Value::String(text) => text.parse::<i128>().ok(),
        _ => None,
    }
}

/// Reads `baseline` limits from the bootstrap, with defaults. Out-of-range values
/// are refused, never clamped.
pub(crate) fn parse_limits(baseline: &Value) -> Result<Limits, PreviewError> {
    let mut limits = Limits::default();
    let field = |name: &str| -> Result<Option<u64>, PreviewError> {
        match baseline.get(name) {
            None => Ok(None),
            Some(value) => u64_of(value).map(Some).ok_or_else(|| config_err("baseline")),
        }
    };
    if let Some(v) = field("chunk_rows")? {
        limits.chunk_rows = v;
    }
    if let Some(v) = field("chunk_bytes")? {
        limits.chunk_bytes = v;
    }
    if let Some(v) = field("chunk_deadline_ms")? {
        limits.chunk_deadline_ms = v;
    }
    if let Some(v) = field("statement_ms")? {
        limits.statement_ms = v;
    }
    if let Some(v) = field("max_chunk_attempts")? {
        limits.max_chunk_attempts = u32::try_from(v).map_err(|_| config_err("baseline"))?;
    }
    if let Some(v) = field("connect_retry_ms")? {
        limits.connect_retry_ms = v;
    }
    if let Some(v) = field("max_txn_events")? {
        limits.max_txn_events = v;
    }
    if let Some(v) = field("max_txn_bytes")? {
        limits.max_txn_bytes = v;
    }
    if let Some(v) = field("max_table_rows")? {
        limits.max_table_rows = v;
    }
    if let Some(v) = field("deadline_ms")? {
        limits.deadline_ms = v;
    }
    if let Some(v) = field("catchup_stall_ms")? {
        limits.catchup_stall_ms = v;
    }
    if let Some(batch) = baseline.get("txn_batch") {
        let item = |name: &str| batch.get(name).and_then(u64_of);
        if let Some(v) = item("max_txns") {
            limits.batch_max_txns = usize::try_from(v).map_err(|_| config_err("baseline"))?;
        }
        if let Some(v) = item("max_bytes") {
            limits.batch_max_bytes = v;
        }
        if let Some(v) = item("max_age_ms") {
            limits.batch_max_age_ms = v;
        }
    }
    let ok = (1..=1_000_000).contains(&limits.chunk_rows)
        && (1..=33_554_432).contains(&limits.chunk_bytes)
        && (1..=300_000).contains(&limits.chunk_deadline_ms)
        && (1..=30_000).contains(&limits.statement_ms)
        && (1..=20).contains(&limits.max_chunk_attempts)
        && limits.connect_retry_ms <= CONNECT_RETRY_MS_MAX
        && limits.max_txn_events >= 1
        && limits.max_txn_bytes >= 1
        && limits.max_table_rows >= 1
        && limits.deadline_ms >= 1000
        && (1..=10_000).contains(&limits.batch_max_txns)
        && (1..=PART_MAX_BYTES).contains(&limits.batch_max_bytes)
        && (1..=60_000).contains(&limits.batch_max_age_ms)
        && (1_000..=3_600_000).contains(&limits.catchup_stall_ms);
    if !ok {
        return Err(config_err("baseline"));
    }
    Ok(limits)
}

/// Resume information from the worker. It is the only source of truth for what is
/// committed in the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resume {
    pub l0: String,
    pub l0_pos: u64,
    pub applied_pos: u64,
    /// Keyed by `schema.table`.
    pub tables: BTreeMap<String, ResumeTable>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResumeTable {
    pub last_chunk_no: u64,
    pub last_key_hi: Option<i128>,
    pub terminal: bool,
    pub rows: u64,
    pub avg_row_bytes: u64,
}

pub(crate) fn parse_resume(value: &Value) -> Result<Resume, PreviewError> {
    if value.get("state").and_then(Value::as_str) != Some("open") {
        return Err(worker_err("resume_state"));
    }
    let l0 = value
        .get("l0")
        .and_then(Value::as_str)
        .ok_or_else(|| worker_err("resume_l0"))?
        .to_owned();
    let l0_pos = value.get("l0_pos").and_then(u64_of).ok_or_else(|| worker_err("resume_l0"))?;
    let applied_pos = value
        .get("applied_pos")
        .and_then(u64_of)
        .ok_or_else(|| worker_err("resume_applied"))?;
    if applied_pos < l0_pos || l0_pos == 0 {
        return Err(worker_err("resume_applied"));
    }
    let mut tables = BTreeMap::new();
    for item in value
        .get("tables")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let schema = item.get("schema").and_then(Value::as_str);
        let table = item.get("table").and_then(Value::as_str);
        let (Some(schema), Some(table)) = (schema, table) else {
            return Err(worker_err("resume_table"));
        };
        let last_chunk_no = item
            .get("last_chunk_no")
            .and_then(u64_of)
            .ok_or_else(|| worker_err("resume_table"))?;
        let last_key_hi = match item.get("last_key_hi") {
            None | Some(Value::Null) => None,
            Some(v) => Some(i128_of(v).ok_or_else(|| worker_err("resume_table"))?),
        };
        let terminal = item.get("terminal").and_then(Value::as_bool).unwrap_or(false);
        let rows = item.get("rows").and_then(u64_of).unwrap_or(0);
        let avg_row_bytes = item.get("avg_row_bytes").and_then(u64_of).unwrap_or(0);
        // Nothing committed means no key and not terminal. A terminal table has
        // at least its terminal chunk, so it has a chunk number.
        if last_chunk_no == 0 && (last_key_hi.is_some() || terminal) {
            return Err(worker_err("resume_table"));
        }
        if last_chunk_no > 0 && !terminal && last_key_hi.is_none() {
            return Err(worker_err("resume_table"));
        }
        let state = ResumeTable {
            last_chunk_no,
            last_key_hi,
            terminal,
            rows,
            avg_row_bytes,
        };
        if tables.insert(format!("{schema}.{table}"), state).is_some() {
            return Err(worker_err("resume_table"));
        }
    }
    Ok(Resume {
        l0,
        l0_pos,
        applied_pos,
        tables,
    })
}

/// Bootstrap frame (stdin, first line). `read_source` names where chunk reads come
/// from. A replica needs the operator's `read_replica` block in the producer config.
#[derive(Clone, Debug)]
pub(crate) struct Bootstrap {
    pub source_id: String,
    pub generation: u64,
    pub limits: Limits,
    pub read_source: ReadSource,
    pub resume: Option<Resume>,
    pub previous: Option<Value>,
}

pub(crate) fn parse_bootstrap(line: &str) -> Result<Bootstrap, PreviewError> {
    let value: Value = serde_json::from_str(line).map_err(|_| worker_err("bootstrap"))?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTO_BOOTSTRAP) {
        return Err(worker_err("bootstrap"));
    }
    let source_id = value
        .get("source_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| worker_err("bootstrap"))?
        .to_owned();
    let generation = value
        .get("source_generation")
        .and_then(u64_of)
        .filter(|n| *n > 0)
        .ok_or_else(|| worker_err("bootstrap"))?;
    let baseline = value.get("baseline").ok_or_else(|| worker_err("bootstrap"))?;
    if baseline.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(config_err("format"));
    }
    let limits = parse_limits(baseline)?;
    let read_source = c1_replica::parse_read_source(value.get("read_source"))?;
    let resume = match value.get("resume") {
        None | Some(Value::Null) => None,
        Some(item) => Some(parse_resume(item)?),
    };
    let previous = match value.get("previous") {
        None | Some(Value::Null) => None,
        Some(item) => Some(item.clone()),
    };
    Ok(Bootstrap {
        source_id,
        generation,
        limits,
        read_source,
        resume,
        previous,
    })
}

/// Row-level data of one fill part (one snapshot chunk).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FillRecord {
    pub table: String,
    pub chunk_no: u32,
    pub terminal: bool,
    pub rows: u64,
    pub bytes: u64,
    pub avg_row_bytes: u64,
    pub part: PartEntry,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CdcPartRecord {
    pub table: String,
    pub part: PartEntry,
    pub path: String,
}

/// One CDC batch as journaled and framed. Positions are GTID sequence numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BatchRecord {
    pub batch_no: u64,
    pub first_pos: u64,
    pub last_pos: u64,
    pub batch_sha256: String,
    pub txns: Vec<TxnMeta>,
    pub parts: Vec<CdcPartRecord>,
    pub rejects: u64,
}

/// Transaction record inside a batch frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TxnMeta {
    pub txn_id: String,
    pub applied_pos: u64,
    pub rows: u64,
    pub payload_sha256: String,
    pub source_commit_timestamp: String,
}

fn part_json(part: &PartEntry) -> Value {
    json!({
        "part_no": part.part_no,
        "kind": part.kind.as_str(),
        "key_lo": part.key_lo.map(|k| k.to_string()),
        "key_hi": part.key_hi.map(|k| k.to_string()),
        "rows": part.rows,
        "bytes": part.bytes,
        "sha256": part.sha256,
    })
}

fn part_from_json(value: &Value) -> Option<PartEntry> {
    let kind = match value.get("kind").and_then(Value::as_str)? {
        "fill" => PartKind::Fill,
        "cdc" => PartKind::Cdc,
        _ => return None,
    };
    Some(PartEntry {
        part_no: u32::try_from(value.get("part_no").and_then(u64_of)?).ok()?,
        kind,
        key_lo: i128_of(value.get("key_lo")?),
        key_hi: i128_of(value.get("key_hi")?),
        rows: value.get("rows").and_then(u64_of)?,
        bytes: value.get("bytes").and_then(u64_of)?,
        sha256: value.get("sha256").and_then(Value::as_str)?.to_owned(),
    })
}

impl FillRecord {
    fn to_json(&self) -> Value {
        json!({
            "type": "fill",
            "table": self.table,
            "chunk_no": self.chunk_no,
            "terminal": self.terminal,
            "rows": self.rows,
            "bytes": self.bytes,
            "avg_row_bytes": self.avg_row_bytes,
            "part": part_json(&self.part),
            "path": self.path,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            table: value.get("table").and_then(Value::as_str)?.to_owned(),
            chunk_no: u32::try_from(value.get("chunk_no").and_then(u64_of)?).ok()?,
            terminal: value.get("terminal").and_then(Value::as_bool)?,
            rows: value.get("rows").and_then(u64_of)?,
            bytes: value.get("bytes").and_then(u64_of)?,
            avg_row_bytes: value.get("avg_row_bytes").and_then(u64_of)?,
            part: part_from_json(value.get("part")?)?,
            path: value.get("path").and_then(Value::as_str)?.to_owned(),
        })
    }
}

impl BatchRecord {
    fn to_json(&self) -> Value {
        json!({
            "type": "cdc",
            "batch_no": self.batch_no,
            "first_pos": self.first_pos.to_string(),
            "last_pos": self.last_pos.to_string(),
            "batch_sha256": self.batch_sha256,
            "txns": self.txns.iter().map(txn_json).collect::<Vec<_>>(),
            "parts": self.parts.iter().map(|p| json!({
                "table": p.table, "path": p.path, "part": part_json(&p.part)
            })).collect::<Vec<_>>(),
            "rejects": self.rejects,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let txns = value
            .get("txns")?
            .as_array()?
            .iter()
            .map(txn_from_json)
            .collect::<Option<Vec<_>>>()?;
        let parts = value
            .get("parts")?
            .as_array()?
            .iter()
            .map(|p| {
                Some(CdcPartRecord {
                    table: p.get("table").and_then(Value::as_str)?.to_owned(),
                    path: p.get("path").and_then(Value::as_str)?.to_owned(),
                    part: part_from_json(p.get("part")?)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            batch_no: value.get("batch_no").and_then(u64_of)?,
            first_pos: value.get("first_pos").and_then(u64_of)?,
            last_pos: value.get("last_pos").and_then(u64_of)?,
            batch_sha256: value.get("batch_sha256").and_then(Value::as_str)?.to_owned(),
            txns,
            parts,
            rejects: value.get("rejects").and_then(u64_of).unwrap_or(0),
        })
    }
}

fn txn_json(txn: &TxnMeta) -> Value {
    json!({
        "txn_id": txn.txn_id,
        "applied_pos": txn.applied_pos.to_string(),
        "rows": txn.rows,
        "payload_sha256": txn.payload_sha256,
        "source_commit_timestamp": txn.source_commit_timestamp,
    })
}

fn txn_from_json(value: &Value) -> Option<TxnMeta> {
    Some(TxnMeta {
        txn_id: value.get("txn_id").and_then(Value::as_str)?.to_owned(),
        applied_pos: value.get("applied_pos").and_then(u64_of)?,
        rows: value.get("rows").and_then(u64_of)?,
        payload_sha256: value.get("payload_sha256").and_then(Value::as_str)?.to_owned(),
        source_commit_timestamp: value
            .get("source_commit_timestamp")
            .and_then(Value::as_str)?
            .to_owned(),
    })
}

/// Journal line. `Header` is the first line of a fresh run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JournalLine {
    Header(Value),
    Fill(FillRecord),
    Batch(BatchRecord),
}

impl JournalLine {
    fn to_json(&self) -> Value {
        match self {
            Self::Header(value) => {
                let mut value = value.clone();
                value["type"] = json!("header");
                value["protocol"] = json!(PROTO_JOURNAL);
                value
            }
            Self::Fill(record) => record.to_json(),
            Self::Batch(record) => record.to_json(),
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        match value.get("type").and_then(Value::as_str)? {
            "header" => Some(Self::Header(value.clone())),
            "fill" => FillRecord::from_json(value).map(Self::Fill),
            "cdc" => BatchRecord::from_json(value).map(Self::Batch),
            _ => None,
        }
    }
}

/// Result of checking journaled parts against the worker's resume information.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Reconciled {
    /// Kept fill records, by table, in chunk order.
    pub fills: BTreeMap<String, Vec<FillRecord>>,
    /// Kept batches, in batch order.
    pub batches: Vec<BatchRecord>,
    /// Journaled parts the worker has not committed. Their files are removed and
    /// their key ranges or positions are read again.
    pub dropped: Vec<String>,
}

/// Keep only what the worker has committed. A part is committed when its chunk
/// number is at or below the worker's `last_chunk_no`, or when its last position is
/// at or below `applied_pos`. Anything beyond that is an orphan and is dropped.
/// Gaps, a missing committed record, or a row count that disagrees are refused.
pub(crate) fn reconcile(
    fills: Vec<FillRecord>,
    batches: Vec<BatchRecord>,
    resume: &Resume,
    tables: &[String],
) -> Result<Reconciled, PreviewError> {
    // The worker may not hold committed state for a table this run does not read.
    if resume.tables.keys().any(|name| !tables.contains(name)) {
        return Err(worker_err("journal_table"));
    }
    let mut out = Reconciled::default();
    let mut by_table: BTreeMap<String, Vec<FillRecord>> = BTreeMap::new();
    for record in fills {
        by_table.entry(record.table.clone()).or_default().push(record);
    }
    for table in tables {
        let committed = resume.tables.get(table).cloned().unwrap_or_default();
        let mut records = by_table.remove(table).unwrap_or_default();
        records.sort_by_key(|r| r.chunk_no);
        let mut kept = Vec::new();
        for record in records {
            if u64::from(record.chunk_no) <= committed.last_chunk_no {
                kept.push(record);
            } else {
                out.dropped.push(record.path);
            }
        }
        for (index, record) in kept.iter().enumerate() {
            if u64::from(record.chunk_no) != index as u64 + 1 {
                return Err(worker_err("journal_gap"));
            }
        }
        if committed.last_chunk_no > 0 {
            let last = kept
                .last()
                .filter(|r| u64::from(r.chunk_no) == committed.last_chunk_no)
                .ok_or_else(|| worker_err("journal_missing"))?;
            if last.terminal != committed.terminal
                || (!last.terminal && last.part.key_hi != committed.last_key_hi)
            {
                return Err(worker_err("journal_key"));
            }
        }
        let rows: u64 = kept.iter().map(|r| r.rows).sum();
        if committed.last_chunk_no > 0 && rows != committed.rows {
            return Err(worker_err("journal_rows"));
        }
        out.fills.insert(table.clone(), kept);
    }
    let mut batches = batches;
    batches.sort_by_key(|b| b.batch_no);
    let mut expect_pos = resume.l0_pos + 1;
    let mut expect_no = 1u64;
    let mut applied_matches = resume.applied_pos == resume.l0_pos;
    for batch in batches {
        if batch.last_pos <= resume.applied_pos {
            if batch.batch_no != expect_no || batch.first_pos != expect_pos {
                return Err(worker_err("journal_chain"));
            }
            expect_no += 1;
            expect_pos = batch.last_pos + 1;
            applied_matches = batch.last_pos == resume.applied_pos;
            out.batches.push(batch);
        } else {
            if batch.first_pos <= resume.applied_pos {
                return Err(worker_err("journal_chain"));
            }
            out.dropped.extend(batch.parts.iter().map(|p| p.path.clone()));
        }
    }
    if !applied_matches {
        return Err(worker_err("journal_missing"));
    }
    Ok(out)
}

/// Digest that binds one batch: its parts and its transactions, in order.
pub(crate) fn batch_sha256(parts: &[CdcPartRecord], txns: &[TxnMeta]) -> String {
    let mut text = String::from("oo.c1.txn-batch/v2\n");
    for part in parts {
        text.push_str(&format!(
            "part\t{}\t{}\t{}\n",
            part.table, part.part.part_no, part.part.sha256
        ));
    }
    for txn in txns {
        text.push_str(&format!(
            "txn\t{}\t{}\t{}\t{}\n",
            txn.applied_pos, txn.rows, txn.payload_sha256, txn.txn_id
        ));
    }
    sha256_hex(text.as_bytes())
}

/// Chunk frame (`kind:"chunk"`).
pub(crate) fn chunk_frame(record: &FillRecord, attempt: u32, read: &Value) -> Value {
    json!({
        "kind": "chunk",
        "protocol": PROTO_CHUNK,
        "schema": record.table.split_once('.').map(|p| p.0).unwrap_or(""),
        "table": record.table.split_once('.').map(|p| p.1).unwrap_or(""),
        "chunk_no": record.chunk_no,
        "attempt": attempt,
        "terminal": record.terminal,
        "key_lo": record.part.key_lo.map(|k| k.to_string()),
        "key_hi": record.part.key_hi.map(|k| k.to_string()),
        "rows": record.rows,
        "bytes": record.bytes,
        "avg_row_bytes": record.avg_row_bytes,
        "part_path": record.path,
        "part_sha256": record.part.sha256,
        "part_bytes": record.part.bytes,
        "read": read,
    })
}

/// Batch frame (`kind:"txn_batch"`).
pub(crate) fn txn_batch_frame(record: &BatchRecord) -> Value {
    json!({
        "kind": "txn_batch",
        "protocol": PROTO_TXN_BATCH,
        "batch_no": record.batch_no,
        "first_pos": record.first_pos.to_string(),
        "last_pos": record.last_pos.to_string(),
        "batch_sha256": record.batch_sha256,
        "parts": record.parts.iter().map(|p| {
            let (schema, table) = p.table.split_once('.').unwrap_or(("", ""));
            json!({
                "schema": schema,
                "table": table,
                "part_no": p.part.part_no,
                "part_path": p.path,
                "part_sha256": p.part.sha256,
                "part_bytes": p.part.bytes,
                "rows": p.part.rows,
                "key_lo": p.part.key_lo.map(|k| k.to_string()),
                "key_hi": p.part.key_hi.map(|k| k.to_string()),
            })
        }).collect::<Vec<_>>(),
        "txns": record.txns.iter().map(txn_json).collect::<Vec<_>>(),
        "rejects": record.rejects,
    })
}

/// Validates one worker ack line. `durable` must be true and the stage must match.
pub(crate) fn parse_ack(line: &str, stage: &str) -> Result<Value, PreviewError> {
    let value: Value = serde_json::from_str(line).map_err(|_| worker_err("reply"))?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTO_ACK)
        || value.get("stage").and_then(Value::as_str) != Some(stage)
        || value.get("durable").and_then(Value::as_bool) != Some(true)
    {
        return Err(worker_err("reply"));
    }
    Ok(value)
}

fn read_ack(input: &mut impl BufRead, stage: &str) -> Result<Value, PreviewError> {
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|_| worker_err("reply"))?;
    if line.is_empty() {
        return Err(worker_err("reply"));
    }
    parse_ack(line.trim_end(), stage)
}

/// Raw cell text for an integer or a decimal key.
fn raw_key(cell: &RawCell) -> Option<i128> {
    match cell {
        RawCell::Int(v) => Some(i128::from(*v)),
        RawCell::Uint(v) => Some(i128::from(*v)),
        RawCell::Bytes(v) => std::str::from_utf8(v).ok()?.parse::<i128>().ok(),
        _ => None,
    }
}

/// Converts one binlog cell to the raw form the snapshot reader produces, so both
/// paths reach the same `decode_cell` (DESIGN §3.8, one canonical encoding).
fn raw_from_binlog(image: Option<&BinlogValue<'_>>) -> Result<RawCell, PreviewError> {
    let Some(value) = image else {
        return Ok(RawCell::Null);
    };
    let value = MyValue::try_from(value.clone()).map_err(|_| PreviewError::new("c1", "partial_image"))?;
    Ok(match value {
        MyValue::NULL => RawCell::Null,
        MyValue::Int(v) => RawCell::Int(v),
        MyValue::UInt(v) => RawCell::Uint(v),
        MyValue::Float(v) => RawCell::Float(v),
        MyValue::Double(v) => RawCell::Double(v),
        MyValue::Bytes(v) => RawCell::Bytes(v),
        MyValue::Date(year, month, day, hour, minute, second, micros) => RawCell::Date {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
        MyValue::Time(negative, days, hours, minutes, seconds, micros) => RawCell::Time {
            negative,
            days,
            hours,
            minutes,
            seconds,
            micros,
        },
    })
}

/// Bytes charged for one raw cell. Byte cells count their length, others 8.
fn raw_cell_bytes(cell: &RawCell) -> u64 {
    match cell {
        RawCell::Bytes(v) => v.len() as u64,
        _ => 8,
    }
}

/// Stable digest of one raw cell, used in the transaction payload digest.
fn raw_digest(cells: &[RawCell]) -> String {
    let mut bytes = Vec::new();
    for cell in cells {
        match cell {
            RawCell::Null => bytes.push(0),
            RawCell::Int(v) => {
                bytes.push(1);
                bytes.extend_from_slice(&v.to_be_bytes());
            }
            RawCell::Uint(v) => {
                bytes.push(2);
                bytes.extend_from_slice(&v.to_be_bytes());
            }
            RawCell::Float(v) => {
                bytes.push(3);
                bytes.extend_from_slice(&v.to_bits().to_be_bytes());
            }
            RawCell::Double(v) => {
                bytes.push(4);
                bytes.extend_from_slice(&v.to_bits().to_be_bytes());
            }
            RawCell::Bytes(v) => {
                bytes.push(5);
                bytes.extend_from_slice(&(v.len() as u64).to_be_bytes());
                bytes.extend_from_slice(v);
            }
            RawCell::Date {
                year,
                month,
                day,
                hour,
                minute,
                second,
                micros,
            } => {
                bytes.push(6);
                bytes.extend_from_slice(&year.to_be_bytes());
                bytes.extend_from_slice(&[*month, *day, *hour, *minute, *second]);
                bytes.extend_from_slice(&micros.to_be_bytes());
            }
            RawCell::Time {
                negative,
                days,
                hours,
                minutes,
                seconds,
                micros,
            } => {
                bytes.push(7);
                bytes.push(u8::from(*negative));
                bytes.extend_from_slice(&days.to_be_bytes());
                bytes.extend_from_slice(&[*hours, *minutes, *seconds]);
                bytes.extend_from_slice(&micros.to_be_bytes());
            }
        }
    }
    sha256_hex(&bytes)
}

/// One row of a transaction after per-transaction de-duplication. Only the last
/// image of a `(table, key)` survives, so each CDC part has unique `(key, pos)`.
#[derive(Clone, Debug)]
pub(crate) struct TxnRow {
    pub table: usize,
    pub delete: bool,
    pub key: i128,
    pub cells: Vec<DecodedCell>,
    pub digest: String,
    pub bytes: u64,
}

/// Changes of the transaction in progress.
#[derive(Debug, Default)]
pub(crate) struct TxnBuild {
    pub rows: BTreeMap<(usize, i128), TxnRow>,
    pub rejects: u64,
}

impl TxnBuild {
    /// `delete` and `upsert` are keyed by `(table, key)`. Last image wins.
    pub(crate) fn put(&mut self, row: TxnRow) {
        self.rows.insert((row.table, row.key), row);
    }

    pub(crate) fn into_rows(self) -> Vec<TxnRow> {
        self.rows.into_values().collect()
    }
}

/// Transaction digest over its final rows, in `(table, key)` order.
pub(crate) fn payload_digest(tables: &[String], rows: &[TxnRow]) -> String {
    let mut text = String::from("oo.c1.txn-payload/v2\n");
    for row in rows {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            tables[row.table],
            if row.delete { "delete" } else { "upsert" },
            row.key,
            row.digest
        ));
    }
    sha256_hex(text.as_bytes())
}

/// CDC rows of one table in one batch, in transaction order. Each row carries the
/// position of its transaction.
#[derive(Debug)]
pub(crate) struct CdcRow {
    pub pos: u64,
    pub delete: bool,
    pub key: i128,
    pub cells: Vec<DecodedCell>,
    pub bytes: u64,
}

/// Accumulates transactions until the batch limits are reached (DESIGN §3.9,
/// `txn_batch`). Transactions stay in GTID order with no gap.
#[derive(Debug, Default)]
pub(crate) struct CdcBatch {
    pub first_pos: u64,
    pub last_pos: u64,
    pub txns: Vec<TxnMeta>,
    pub rows: BTreeMap<usize, Vec<CdcRow>>,
    pub bytes: u64,
    pub rejects: u64,
    opened: Option<Instant>,
}

impl CdcBatch {
    pub(crate) fn is_empty(&self) -> bool {
        self.txns.is_empty()
    }

    /// Whether `txn_bytes` more bytes or one more transaction would break a limit.
    pub(crate) fn would_exceed(&self, limits: &Limits, txn_bytes: u64) -> bool {
        !self.is_empty()
            && (self.txns.len() + 1 > limits.batch_max_txns
                || self.bytes + txn_bytes > limits.batch_max_bytes)
    }

    pub(crate) fn full(&self, limits: &Limits) -> bool {
        self.txns.len() >= limits.batch_max_txns || self.bytes >= limits.batch_max_bytes
    }

    pub(crate) fn aged(&self, limits: &Limits, now: Instant) -> bool {
        self.opened
            .is_some_and(|t| now.duration_since(t) >= Duration::from_millis(limits.batch_max_age_ms))
    }

    /// Adds one completed transaction. `gno` must be the successor of the last
    /// position in the batch, or the batch is empty.
    pub(crate) fn push(&mut self, gno: u64, meta: TxnMeta, rows: Vec<(usize, CdcRow)>, rejects: u64) {
        if self.is_empty() {
            self.first_pos = gno;
            self.opened = Some(Instant::now());
        }
        self.last_pos = gno;
        for (table, row) in rows {
            self.bytes += row.bytes;
            self.rows.entry(table).or_default().push(row);
        }
        self.rejects += rejects;
        self.txns.push(meta);
    }

    pub(crate) fn take(&mut self) -> CdcBatch {
        std::mem::take(self)
    }
}

/// Splits one table's rows into parts of at most `CDC_PART_RAW_BYTES` raw bytes.
pub(crate) fn split_rows(rows: Vec<CdcRow>) -> Vec<Vec<CdcRow>> {
    let mut parts: Vec<Vec<CdcRow>> = Vec::new();
    let mut current: Vec<CdcRow> = Vec::new();
    let mut size = 0u64;
    for row in rows {
        if !current.is_empty() && size + row.bytes > CDC_PART_RAW_BYTES {
            parts.push(std::mem::take(&mut current));
            size = 0;
        }
        size += row.bytes;
        current.push(row);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// A table of the baseline. The reader and the decoding plans are built once.
pub(crate) struct TableRt {
    pub schema: String,
    pub table: String,
    pub qualified: String,
    /// Selected columns in config order. The key is one of them.
    pub columns: Vec<MappedColumn>,
    pub key_index: usize,
    /// Full-table ordinal of each selected column (0-based), for binlog images.
    pub visible: Vec<usize>,
    /// Columns for CDC parts: selected columns plus `_oo_pos` and `_oo_op`.
    pub cdc_plans: Vec<ColumnPlan>,
    pub cdc_pos_store: MappedColumn,
    pub cdc_op_store: MappedColumn,
    pub last_chunk_no: u64,
    pub last_key_hi: Option<i128>,
    pub terminal: bool,
    pub rows: u64,
    pub avg_row_bytes: u64,
    pub fill: Vec<FillRecord>,
    pub cdc_parts: Vec<CdcPartRecord>,
    pub next_cdc_part_no: u32,
}

/// Synthetic CDC columns appended to every CDC part. Mapped with the same mapper as
/// source columns, so the Arrow types match what `c1_parts` checks.
fn cdc_marker_columns(reserved: &BTreeSet<String>) -> Result<(MappedColumn, MappedColumn), PreviewError> {
    let pos = MysqlColumn {
        name: "_oo_pos".to_owned(),
        ordinal: 1,
        data_type: "decimal".to_owned(),
        column_type: "decimal(38,0)".to_owned(),
        nullable: false,
        char_len: None,
        precision: Some(38),
        scale: Some(0),
        charset: None,
        // The mapper requires exactly one key. The flag is cleared below.
        column_key: "PRI".to_owned(),
    };
    let op = MysqlColumn {
        name: "_oo_op".to_owned(),
        ordinal: 2,
        data_type: "varchar".to_owned(),
        column_type: "varchar(8)".to_owned(),
        nullable: false,
        char_len: Some(8),
        precision: None,
        scale: None,
        charset: Some("utf8mb4".to_owned()),
        column_key: String::new(),
    };
    let mut mapped = map_table(&[pos, op], false, true, reserved)?;
    let op = mapped.remove(1);
    let mut pos = mapped.remove(0);
    pos.key = false;
    Ok((pos, op))
}

impl TableRt {
    /// Builds the table state. `columns` must be the mapped selected columns with
    /// attnums 1..n. Exactly one column must be the key.
    pub(crate) fn new(
        schema: &str,
        table: &str,
        columns: Vec<MappedColumn>,
        visible: Vec<usize>,
        reserved: &BTreeSet<String>,
    ) -> Result<Self, PreviewError> {
        let key_index = columns
            .iter()
            .position(|c| c.key)
            .ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
        let (mut pos, mut op) = cdc_marker_columns(reserved)?;
        let base = columns.len() as i32;
        pos.plan.attnum = base + 1;
        op.plan.attnum = base + 2;
        let mut cdc_plans: Vec<ColumnPlan> = columns.iter().map(|c| c.plan.clone()).collect();
        cdc_plans.push(pos.plan.clone());
        cdc_plans.push(op.plan.clone());
        Ok(Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
            qualified: format!("{schema}.{table}"),
            columns,
            key_index,
            visible,
            cdc_plans,
            cdc_pos_store: pos,
            cdc_op_store: op,
            last_chunk_no: 0,
            last_key_hi: None,
            terminal: false,
            rows: 0,
            avg_row_bytes: 0,
            fill: Vec::new(),
            cdc_parts: Vec::new(),
            next_cdc_part_no: 1,
        })
    }

    /// Cursor for the next chunk, rebuilt from committed state each time.
    pub(crate) fn cursor(&self) -> ChunkCursor {
        match self.last_key_hi {
            Some(key) => ChunkCursor::resume_after_with_hint(key, self.avg_row_bytes),
            None => ChunkCursor::start_with_hint(self.avg_row_bytes),
        }
    }

    /// Number for the next CDC part of this table. Numbers are contiguous per table.
    pub(crate) fn take_cdc_part_no(&mut self) -> u32 {
        let part_no = self.next_cdc_part_no;
        self.next_cdc_part_no += 1;
        part_no
    }

    pub(crate) fn next_chunk_no(&self) -> u32 {
        u32::try_from(self.last_chunk_no + 1).unwrap_or(u32::MAX)
    }

    /// Stores the committed fill record in the in-memory state.
    fn commit_fill(&mut self, record: FillRecord) {
        self.last_chunk_no = u64::from(record.chunk_no);
        if !record.terminal {
            self.last_key_hi = record.part.key_hi;
        }
        self.terminal = record.terminal;
        self.rows += record.rows;
        if record.rows > 0 {
            self.avg_row_bytes = (record.bytes / record.rows).max(1);
        }
        self.fill.push(record);
    }

    /// Rebuilds the in-memory state from journal records the worker committed.
    pub(crate) fn restore(&mut self, fills: Vec<FillRecord>, resume: Option<&ResumeTable>) {
        for record in fills {
            self.commit_fill(record);
        }
        // Observed sizes win. The bootstrap hint wins over the catalog hint when no
        // chunk of this table is committed yet.
        if let Some(resume) = resume
            && resume.avg_row_bytes > 0
            && self.fill.is_empty()
        {
            self.avg_row_bytes = resume.avg_row_bytes;
        }
    }
}

/// Builds the typed batch for one CDC part of one table. The rows must already
/// be in transaction order.
pub(crate) fn cdc_typed_batch(
    binding: &RunBinding,
    table: &TableRt,
    rows: &[CdcRow],
) -> Result<TypedBatch, PreviewError> {
    let mut data = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = row.cells.clone();
        let pos = values::decode_cell(
            table.cdc_pos_store.store,
            &RawCell::Bytes(row.pos.to_string().into_bytes()),
            false,
        )?
        .0;
        let op_text: &[u8] = if row.delete { b"delete" } else { b"upsert" };
        let op = values::decode_cell(
            table.cdc_op_store.store,
            &RawCell::Bytes(op_text.to_vec()),
            false,
        )?
        .0;
        cells.push(pos);
        cells.push(op);
        data.push(cells);
    }
    let key_attnum = table.columns[table.key_index].plan.attnum;
    assemble_batch(binding, table.cdc_plans.clone(), &[key_attnum], data)
        .map_err(|_| PreviewError::new("batch", "assemble"))
}

/// Writes one Arrow IPC part through `PartSink`. The file is created under a
/// temporary name and renamed only when complete, so a crash leaves no half part.
pub(crate) fn write_part(path: &Path, batch: &TypedBatch) -> Result<(), PreviewError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| PreviewError::new("config", "out"))?;
    }
    let record = batch
        .record_batch()
        .map_err(|_| PreviewError::new("batch", "arrow"))?;
    let tmp = PathBuf::from(format!("{}.partial", path.display()));
    let file = File::create(&tmp).map_err(|_| PreviewError::new("config", "out"))?;
    let mut writer = StreamWriter::try_new(PartSink::new(file), &record.schema())
        .map_err(|_| PreviewError::new("batch", "arrow"))?;
    writer
        .write(&record)
        .map_err(|_| PreviewError::new("batch", "arrow"))?;
    let sink = writer
        .into_inner()
        .map_err(|_| PreviewError::new("batch", "arrow"))?;
    let (file, _) = sink.into_parts();
    file.sync_all().map_err(|_| PreviewError::new("config", "out"))?;
    fs::rename(&tmp, path).map_err(|_| PreviewError::new("config", "out"))
}

/// Relative part path under the out directory.
pub(crate) fn part_rel_path(qualified: &str, kind: PartKind, part_no: u32) -> String {
    let name = match kind {
        PartKind::Fill => format!("fill-{part_no:08}.arrow"),
        PartKind::Cdc => format!("cdc-{part_no:08}.arrow"),
    };
    format!("parts/{qualified}/{name}")
}

fn append_journal(file: &mut File, line: &JournalLine) -> Result<(), PreviewError> {
    let mut text = serde_json::to_vec(&line.to_json()).map_err(|_| PreviewError::new("config", "out"))?;
    text.push(b'\n');
    file.write_all(&text).map_err(|_| PreviewError::new("config", "out"))?;
    file.sync_data().map_err(|_| PreviewError::new("config", "out"))
}

pub(crate) fn read_journal(path: &Path) -> Result<Vec<JournalLine>, PreviewError> {
    let text = fs::read_to_string(path).map_err(|_| PreviewError::new("config", "out"))?;
    let mut lines = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(raw).map_err(|_| worker_err("journal"))?;
        lines.push(JournalLine::from_json(&value).ok_or_else(|| worker_err("journal"))?);
    }
    Ok(lines)
}

/// Rewrites the journal with the header and the kept records, then removes every
/// part file that is not kept (orphans of a crash before a frame was sent).
pub(crate) fn rewrite_journal(
    out: &Path,
    header: &JournalLine,
    kept: &Reconciled,
) -> Result<File, PreviewError> {
    let tmp = out.join(format!("{JOURNAL_FILE}.tmp"));
    {
        let mut file = File::create(&tmp).map_err(|_| PreviewError::new("config", "out"))?;
        append_journal(&mut file, header)?;
        for records in kept.fills.values() {
            for record in records {
                append_journal(&mut file, &JournalLine::Fill(record.clone()))?;
            }
        }
        for batch in &kept.batches {
            append_journal(&mut file, &JournalLine::Batch(batch.clone()))?;
        }
    }
    fs::rename(&tmp, out.join(JOURNAL_FILE)).map_err(|_| PreviewError::new("config", "out"))?;
    let mut keep: BTreeSet<PathBuf> = BTreeSet::new();
    for records in kept.fills.values() {
        for record in records {
            keep.insert(out.join(&record.path));
        }
    }
    for batch in &kept.batches {
        for part in &batch.parts {
            keep.insert(out.join(&part.path));
        }
    }
    let mut stale: Vec<PathBuf> = Vec::new();
    collect_files(&out.join("parts"), &mut stale)?;
    for path in stale {
        if !keep.contains(&path) {
            fs::remove_file(&path).map_err(|_| PreviewError::new("config", "out"))?;
        }
    }
    OpenOptions::new()
        .append(true)
        .open(out.join(JOURNAL_FILE))
        .map_err(|_| PreviewError::new("config", "out"))
}

fn collect_files(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), PreviewError> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|_| PreviewError::new("config", "out"))? {
        let entry = entry.map_err(|_| PreviewError::new("config", "out"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// Chunk source. The primary is the default. The replica source (`c1_replica.rs`)
/// implements `prepare` to wait until the replica has executed L0 (DESIGN §3.13 step 2).
pub(crate) trait ChunkSource {
    /// Name recorded in each chunk frame.
    fn kind(&self) -> &'static str;
    /// Runs before each chunk read. Returns the wait time in milliseconds.
    async fn prepare(&mut self, l0_pos: u64) -> Result<u64, PreviewError>;
    /// Reads one chunk after `cursor`. The cursor only advances on success.
    async fn read(
        &mut self,
        reader: &ChunkReader,
        cursor: &mut ChunkCursor,
    ) -> Result<ChunkBatch, MysqlError>;
    /// Replaces the connection after a failed read.
    async fn reconnect(&mut self) -> Result<(), PreviewError>;
}

/// Reads from the primary. Each chunk is one autocommit statement, so no snapshot
/// stays open between chunks (DESIGN §3.2, P6).
pub(crate) struct PrimarySource {
    config: MysqlConfig,
    password: Zeroizing<String>,
    conn: Option<ReadConnection>,
    cancel: Cancel,
}

impl PrimarySource {
    pub(crate) async fn open(config: MysqlConfig, password: Zeroizing<String>) -> Result<Self, PreviewError> {
        let mut source = Self {
            config,
            password,
            conn: None,
            cancel: Cancel::default(),
        };
        source.reconnect().await?;
        Ok(source)
    }
}

impl ChunkSource for PrimarySource {
    fn kind(&self) -> &'static str {
        "primary"
    }

    async fn prepare(&mut self, _l0_pos: u64) -> Result<u64, PreviewError> {
        Ok(0)
    }

    async fn read(
        &mut self,
        reader: &ChunkReader,
        cursor: &mut ChunkCursor,
    ) -> Result<ChunkBatch, MysqlError> {
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| MysqlError::new("cancel", "closed"))?;
        reader.read_chunk(conn, cursor, &self.cancel).await
    }

    async fn reconnect(&mut self) -> Result<(), PreviewError> {
        self.conn = None;
        let conn = oo_connectors::mysql::connect(&self.config, self.password.as_str()).await?;
        self.conn = Some(ReadConnection::new(
            conn,
            self.config.clone(),
            self.password.clone(),
        ));
        Ok(())
    }
}

/// The chunk source chosen for this run. Replica mode is the only other variant.
pub(crate) enum ActiveSource {
    Primary(PrimarySource),
    Replica(ReplicaSource),
}

impl ActiveSource {
    pub(crate) fn replica_identity(&self) -> Option<&str> {
        match self {
            Self::Primary(_) => None,
            Self::Replica(replica) => Some(replica.identity()),
        }
    }

    pub(crate) fn receipt(&self) -> Value {
        match self {
            Self::Primary(_) => Value::Null,
            Self::Replica(replica) => replica.receipt(),
        }
    }

    /// Non-waiting checks on the replica after the last chunk. A no-op on the primary.
    pub(crate) async fn verify(&mut self, l0_pos: u64) -> Result<(), PreviewError> {
        match self {
            Self::Primary(_) => Ok(()),
            Self::Replica(replica) => replica.verify(l0_pos).await,
        }
    }
}

impl ChunkSource for ActiveSource {
    fn kind(&self) -> &'static str {
        match self {
            Self::Primary(source) => source.kind(),
            Self::Replica(source) => source.kind(),
        }
    }

    async fn prepare(&mut self, l0_pos: u64) -> Result<u64, PreviewError> {
        match self {
            Self::Primary(source) => source.prepare(l0_pos).await,
            Self::Replica(source) => source.prepare(l0_pos).await,
        }
    }

    async fn read(
        &mut self,
        reader: &ChunkReader,
        cursor: &mut ChunkCursor,
    ) -> Result<ChunkBatch, MysqlError> {
        match self {
            Self::Primary(source) => source.read(reader, cursor).await,
            Self::Replica(source) => source.read(reader, cursor).await,
        }
    }

    async fn reconnect(&mut self) -> Result<(), PreviewError> {
        match self {
            Self::Primary(source) => source.reconnect().await,
            Self::Replica(source) => source.reconnect().await,
        }
    }
}

/// Catch-up watchdog (FRAMES-V2 §9). Fails when the applied position has not moved
/// for `stall`. The failure is retryable: a resume continues from the journal.
#[derive(Debug)]
pub(crate) struct StallWatch {
    stall: Duration,
    applied: u64,
    since: Instant,
}

impl StallWatch {
    pub(crate) fn new(stall: Duration, applied: u64, now: Instant) -> Self {
        Self {
            stall,
            applied,
            since: now,
        }
    }

    /// A reconnect after an outage is not a stall: the clock starts again from `now`.
    pub(crate) fn restart(&mut self, now: Instant) {
        self.since = now;
    }

    pub(crate) fn observe(&mut self, applied: u64, now: Instant) -> Result<(), PreviewError> {
        if applied != self.applied {
            self.applied = applied;
            self.since = now;
            return Ok(());
        }
        if now.duration_since(self.since) >= self.stall {
            return Err(PreviewError::new("c1", "catchup_stall"));
        }
        Ok(())
    }
}

/// Whether a chunk error is worth retrying on the same chunk after a reconnect.
pub(crate) fn retriable(err: &MysqlError) -> bool {
    match (err.code, err.fact) {
        ("connect", _) | ("cancel", _) => true,
        ("protocol", "query") => true,
        ("budget", "statement_timeout") | ("budget", "chunk_deadline") => true,
        _ => false,
    }
}

/// Fixed backoff: 1 s doubling, capped at `MAX_BACKOFF`.
pub(crate) fn backoff(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(5);
    Duration::from_secs(1u64 << exp).min(MAX_BACKOFF)
}

/// Wall-clock budget for transient retries (`connect_retry_ms`). It starts at the first
/// failure. Each back-off is `backoff()` (1 s, doubling, capped at 30 s), cut to what is
/// left of the budget, so the retries end when the budget is spent.
#[derive(Debug)]
pub(crate) struct ConnectBudget {
    budget: Duration,
    started: Option<Instant>,
    failures: u32,
}

impl ConnectBudget {
    pub(crate) fn new(budget: Duration) -> Self {
        Self {
            budget,
            started: None,
            failures: 0,
        }
    }

    /// Records one failure observed at `now`. Returns the back-off before the next try,
    /// or `None` when the budget is spent.
    pub(crate) fn after_failure(&mut self, now: Instant) -> Option<Duration> {
        let started = *self.started.get_or_insert(now);
        let spent = now.saturating_duration_since(started);
        if spent >= self.budget {
            return None;
        }
        self.failures += 1;
        Some(backoff(self.failures).min(self.budget - spent))
    }

    /// Progress was made. The next failure starts a new budget.
    pub(crate) fn reset(&mut self) {
        self.started = None;
        self.failures = 0;
    }
}

/// Whether a binlog stream error is an outage to reconnect from (a dropped or stalled
/// connection). Positions, lineage and the other consumer refusals are not outages.
pub(crate) fn stream_outage(err: &PreviewError) -> bool {
    matches!((err.code, err.fact), ("protocol", "query") | ("connect", _) | ("c1", "stream_end"))
}

/// The error a transient retry ends with when its budget is spent. Exit code 4 (connect),
/// which the job worker treats as transient.
fn retry_exhausted() -> PreviewError {
    PreviewError::new("connect", "retry_exhausted")
}

/// Wire form of the chunk `read` object. A replica read names the replica identity
/// it was read from (FRAMES-V2 §9).
fn read_info(
    source: &str,
    statement_sha256: &str,
    wait_ms: u64,
    rejects: &[(usize, usize, oo_connectors::mysql::values::RejectNote)],
    replica_identity: Option<&str>,
) -> Value {
    let mut by_reason: BTreeMap<&str, u64> = BTreeMap::new();
    for (_, _, note) in rejects {
        *by_reason.entry(note.reason).or_default() += 1;
    }
    let mut read = json!({
        "source": source,
        "statement_sha256": statement_sha256,
        "wait_ms": wait_ms,
        "rejects": {"count": rejects.len(), "by_reason": by_reason},
    });
    if let Some(identity) = replica_identity {
        read["replica_identity"] = json!(identity);
    }
    read
}

/// Every counter the receipt reports.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    pub chunks: u64,
    pub chunk_retries: u64,
    pub cdc_reconnects: u64,
    pub chunk_rows: u64,
    pub batches: u64,
    pub txns: u64,
    pub cdc_rows: u64,
    pub max_batch_txns: u64,
}

/// Producer state shared by the chunk and binlog paths.
pub(crate) struct Engine {
    pub out: PathBuf,
    pub input: BufReader<std::io::StdinLock<'static>>,
    pub uuid: String,
    pub source_id: String,
    pub generation: u64,
    pub epoch: u32,
    pub l0_pos: u64,
    pub applied: u64,
    pub deadline: Instant,
    pub limits: Limits,
    pub tables: Vec<TableRt>,
    pub by_name: BTreeMap<String, usize>,
    pub journal: File,
    pub pending: Option<(u64, String, TxnBuild)>,
    pub txn_events: u64,
    pub txn_bytes: u64,
    pub batch: CdcBatch,
    pub next_batch_no: u64,
    pub stats: Stats,
    pub binding: RunBinding,
    pub tables_names: Vec<String>,
    /// Retry budget for the binlog stream. Cleared by every event that arrives.
    pub cdc_budget: ConnectBudget,
}

impl Engine {
    pub(crate) fn check_deadline(&self) -> Result<(), PreviewError> {
        if Instant::now() >= self.deadline {
            return Err(PreviewError::new("budget", "deadline"));
        }
        Ok(())
    }

    fn charge(&mut self, bytes: u64) -> Result<(), PreviewError> {
        self.txn_events += 1;
        self.txn_bytes = self.txn_bytes.saturating_add(bytes);
        if self.txn_events > self.limits.max_txn_events || self.txn_bytes > self.limits.max_txn_bytes {
            return Err(PreviewError::new("budget", "large_txn"));
        }
        Ok(())
    }

    fn reset_txn_budget(&mut self) {
        self.txn_events = 0;
        self.txn_bytes = 0;
    }

    /// Processes one binlog event. Same refusals as v1 for DDL, DML statements,
    /// GTID gaps and anonymous GTIDs.
    pub(crate) fn on_event(
        &mut self,
        event: mysql_async::binlog::events::Event,
        stream: &BinlogStream,
    ) -> Result<(), PreviewError> {
        self.charge(u64::from(event.header().event_size()))?;
        let header_ts = event.header().timestamp();
        let data = event
            .read_data()
            .map_err(|_| PreviewError::new("protocol", "event"))?;
        let Some(data) = data else { return Ok(()) };
        match data {
            EventData::RotateEvent(_) => Ok(()),
            EventData::GtidEvent(gtid) => self.on_gtid(gtid.sid(), gtid.gno()),
            EventData::QueryEvent(query) => self.on_query(&query.query(), header_ts),
            EventData::RowsEvent(rows) => {
                if matches!(rows, RowsEventData::PartialUpdateRowsEvent(_)) {
                    return Err(PreviewError::new("c1", "partial_image"));
                }
                let tme = stream
                    .get_tme(rows.table_id())
                    .ok_or_else(|| PreviewError::new("c1", "table_map"))?;
                self.on_rows(&rows, tme)
            }
            EventData::XidEvent(_) => self.finish_txn(header_ts),
            EventData::AnonymousGtidEvent(_) => Err(PreviewError::new("c1", "anonymous_gtid")),
            _ => Ok(()),
        }
    }

    fn on_gtid(&mut self, sid: [u8; 16], gno: u64) -> Result<(), PreviewError> {
        if self.pending.is_some() {
            return Err(PreviewError::new("c1", "unclosed_txn"));
        }
        if format_uuid(&sid) != self.uuid {
            return Err(PreviewError::new("c1", "lineage"));
        }
        if gno != self.applied.saturating_add(1) {
            return Err(PreviewError::new("c1", "gtid_gap"));
        }
        self.pending = Some((gno, format!("{}:{gno}", self.uuid), TxnBuild::default()));
        Ok(())
    }

    fn on_query(&mut self, query: &str, timestamp: u32) -> Result<(), PreviewError> {
        let verb = query
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        match verb.as_str() {
            "BEGIN" | "START" | "COMMIT" | "XA" | "SAVEPOINT" => Ok(()),
            "GRANT" | "REVOKE" => {
                if self.pending.as_ref().is_some_and(|p| !p.2.rows.is_empty()) {
                    return Err(PreviewError::new("c1", "mixed_txn"));
                }
                self.finish_txn(timestamp)
            }
            "ROLLBACK" => {
                self.pending = None;
                self.reset_txn_budget();
                Ok(())
            }
            "ALTER" | "CREATE" | "DROP" | "TRUNCATE" | "RENAME" => {
                Err(PreviewError::new("c1", "schema_pause"))
            }
            "INSERT" | "UPDATE" | "DELETE" | "REPLACE" => {
                Err(PreviewError::new("c1", "statement_dml"))
            }
            _ => Ok(()),
        }
    }

    fn on_rows(&mut self, rows: &RowsEventData<'_>, tme: &TableMapEvent<'_>) -> Result<(), PreviewError> {
        let qualified = format!("{}.{}", tme.database_name(), tme.table_name());
        let Some(&table) = self.by_name.get(&qualified) else {
            return Ok(());
        };
        if self.pending.is_none() {
            return Err(PreviewError::new("c1", "rows_outside_txn"));
        }
        for pair in rows.rows(tme) {
            let (before, after) = pair.map_err(|_| PreviewError::new("protocol", "event"))?;
            self.on_row_pair(table, before.as_ref(), after.as_ref())?;
        }
        Ok(())
    }

    /// Same rule as the v1 producer (DESIGN §3.1 P3): a changed primary key deletes
    /// the old key and upserts the new one.
    fn on_row_pair(
        &mut self,
        table: usize,
        before: Option<&BinlogRow>,
        after: Option<&BinlogRow>,
    ) -> Result<(), PreviewError> {
        match (before, after) {
            (None, Some(after)) => self.stage(table, false, after),
            (Some(before), Some(after)) => {
                let old = self.row_key(table, before)?;
                let new = self.row_key(table, after)?;
                if old != new {
                    self.stage(table, true, before)?;
                }
                self.stage(table, false, after)
            }
            (Some(before), None) => self.stage(table, true, before),
            (None, None) => Err(PreviewError::new("protocol", "event")),
        }
    }

    fn raw_row(&self, table: usize, image: &BinlogRow) -> Result<Vec<RawCell>, PreviewError> {
        self.tables[table]
            .visible
            .iter()
            .zip(&self.tables[table].columns)
            .map(|(index,column)| {
                let raw=raw_from_binlog(image.as_ref(*index))?;
                values::normalize_binlog_integer(&column.plan.mapping.source_type,raw).map_err(Into::into)
            })
            .collect()
    }

    fn row_key(&self, table: usize, image: &BinlogRow) -> Result<i128, PreviewError> {
        let raw = self.raw_row(table, image)?;
        let key_index = self.tables[table].key_index;
        raw_key(&raw[key_index]).ok_or_else(|| PreviewError::new("value", "null_key"))
    }

    fn stage(&mut self, table: usize, delete: bool, image: &BinlogRow) -> Result<(), PreviewError> {
        let raw = self.raw_row(table, image)?;
        let key_index = self.tables[table].key_index;
        let key = raw_key(&raw[key_index]).ok_or_else(|| PreviewError::new("value", "null_key"))?;
        let mut cells = Vec::with_capacity(raw.len());
        let mut rejects = 0u64;
        let mut bytes = 0u64;
        for (column, cell) in self.tables[table].columns.iter().zip(&raw) {
            bytes += raw_cell_bytes(cell);
            let (decoded, reject) = values::decode_cell(column.store, cell, column.plan.mapping.nullable)?;
            if reject.is_some() {
                rejects += 1;
            }
            cells.push(decoded);
        }
        let digest = raw_digest(&raw);
        let row = TxnRow {
            table,
            delete,
            key,
            cells,
            digest,
            bytes,
        };
        let build = &mut self
            .pending
            .as_mut()
            .ok_or_else(|| PreviewError::new("c1", "rows_outside_txn"))?
            .2;
        build.rejects += rejects;
        build.put(row);
        Ok(())
    }

    /// Ends the transaction in progress. The transaction joins the batch, and the
    /// batch is flushed first if it would exceed a limit.
    fn finish_txn(&mut self, timestamp: u32) -> Result<(), PreviewError> {
        self.reset_txn_budget();
        let Some((gno, txn_id, build)) = self.pending.take() else {
            return Ok(());
        };
        let rejects = build.rejects;
        let rows = build.into_rows();
        let stamp = unix_text(timestamp).ok_or_else(|| PreviewError::new("c1", "timestamp"))?;
        let payload_sha256 = payload_digest(&self.tables_names, &rows);
        let txn_bytes: u64 = rows.iter().map(|r| r.bytes).sum();
        let meta = TxnMeta {
            txn_id,
            applied_pos: gno,
            rows: rows.len() as u64,
            payload_sha256,
            source_commit_timestamp: stamp,
        };
        if self.batch.would_exceed(&self.limits, txn_bytes) {
            self.flush_batch()?;
        }
        let staged: Vec<(usize, CdcRow)> = rows
            .into_iter()
            .map(|row| {
                (
                    row.table,
                    CdcRow {
                        pos: gno,
                        delete: row.delete,
                        key: row.key,
                        cells: row.cells,
                        bytes: row.bytes,
                    },
                )
            })
            .collect();
        self.batch.push(gno, meta, staged, rejects);
        self.applied = gno;
        self.stats.txns += 1;
        if self.batch.full(&self.limits) {
            self.flush_batch()?;
        }
        Ok(())
    }

    /// Batch age check. Called on every tick, so a quiet stream still flushes.
    pub(crate) fn tick(&mut self) -> Result<(), PreviewError> {
        self.check_deadline()?;
        if self.batch.aged(&self.limits, Instant::now()) {
            self.flush_batch()?;
        }
        Ok(())
    }

    /// Writes the batch's CDC parts, journals the batch, sends `txn_batch` and
    /// waits for the durable ack.
    pub(crate) fn flush_batch(&mut self) -> Result<(), PreviewError> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let batch = self.batch.take();
        let batch_no = self.next_batch_no;
        let mut parts = Vec::new();
        for (table_index, rows) in batch.rows {
            for group in split_rows(rows) {
                // Each group is its own part. The counter advances per part, so two
                // groups of one table in one batch never share a part number.
                let part_no = self.tables[table_index].take_cdc_part_no();
                let table = &self.tables[table_index];
                let typed = cdc_typed_batch(&self.binding_for(table_index), table, &group)?;
                let rel = part_rel_path(&table.qualified, PartKind::Cdc, part_no);
                let abs = self.out.join(&rel);
                write_part(&abs, &typed)?;
                let key_lo = group.iter().map(|r| r.key).min();
                let key_hi = group.iter().map(|r| r.key).max();
                let entry = part_entry_for_file(
                    &abs,
                    part_no,
                    PartKind::Cdc,
                    key_lo,
                    key_hi,
                    group.len() as u64,
                )
                .map_err(|_| PreviewError::new("c1", "part_entry"))?;
                self.stats.cdc_rows += group.len() as u64;
                let record = CdcPartRecord {
                    table: table.qualified.clone(),
                    part: entry,
                    path: rel,
                };
                self.tables[table_index].cdc_parts.push(record.clone());
                parts.push(record);
            }
        }
        let record = BatchRecord {
            batch_no,
            first_pos: batch.first_pos,
            last_pos: batch.last_pos,
            batch_sha256: batch_sha256(&parts, &batch.txns),
            txns: batch.txns,
            parts,
            rejects: batch.rejects,
        };
        append_journal(&mut self.journal, &JournalLine::Batch(record.clone()))?;
        send_frame(&txn_batch_frame(&record)).map_err(|_| PreviewError::new("protocol", "stdout"))?;
        let ack = read_ack(&mut self.input, "txn_batch")?;
        let outcome = ack.get("outcome").and_then(Value::as_str).unwrap_or("");
        if !matches!(outcome, "applied" | "replay")
            || ack.get("batch_no").and_then(u64_of) != Some(batch_no)
            || ack.get("last_pos").and_then(u64_of) != Some(record.last_pos)
        {
            return Err(worker_err("reply"));
        }
        self.next_batch_no += 1;
        self.stats.batches += 1;
        self.stats.max_batch_txns = self.stats.max_batch_txns.max(record.txns.len() as u64);
        Ok(())
    }

    fn binding_for(&self, table: usize) -> RunBinding {
        let mut binding = self.binding.clone();
        binding.schema = self.tables[table].schema.clone();
        binding.table = self.tables[table].table.clone();
        binding
    }

    /// Commits one chunk: part file, journal, `chunk` frame, durable ack.
    pub(crate) fn commit_fill(
        &mut self,
        table: usize,
        batch: ChunkBatch,
        attempt: u32,
        source: &str,
        wait_ms: u64,
        replica_identity: Option<&str>,
    ) -> Result<(), PreviewError> {
        let rows = batch.rows;
        if batch.terminal != (rows == 0) || (!batch.terminal && (batch.key_first.is_none() || batch.key_hi.is_none())) {
            return Err(PreviewError::new("protocol", "chunk_shape"));
        }
        let chunk_no = self.tables[table].next_chunk_no();
        let qualified = self.tables[table].qualified.clone();
        let rel = part_rel_path(&qualified, PartKind::Fill, chunk_no);
        let abs = self.out.join(&rel);
        let typed = batch.typed;
        write_part(&abs, &typed)?;
        let entry = part_entry_for_file(
            &abs,
            chunk_no,
            PartKind::Fill,
            batch.key_first,
            batch.key_hi,
            rows,
        )
        .map_err(|_| PreviewError::new("c1", "part_entry"))?;
        let total = self.tables[table].rows + rows;
        if total > self.limits.max_table_rows {
            return Err(PreviewError::new("budget", "max_table_rows"));
        }
        let avg = if rows > 0 { (batch.bytes / rows).max(1) } else { 0 };
        let record = FillRecord {
            table: qualified,
            chunk_no,
            terminal: batch.terminal,
            rows,
            bytes: batch.bytes,
            avg_row_bytes: avg,
            part: entry,
            path: rel,
        };
        append_journal(&mut self.journal, &JournalLine::Fill(record.clone()))?;
        let read = read_info(
            source,
            &batch.statement.sha256,
            wait_ms,
            &batch.rejects,
            replica_identity,
        );
        let frame = chunk_frame(&record, attempt, &read);
        send_frame(&frame).map_err(|_| PreviewError::new("protocol", "stdout"))?;
        let ack = read_ack(&mut self.input, "chunk_apply")?;
        if ack.get("chunk_no").and_then(u64_of) != Some(u64::from(chunk_no))
            || !matches!(ack.get("outcome").and_then(Value::as_str), Some("loaded" | "replay"))
        {
            return Err(worker_err("reply"));
        }
        self.stats.chunks += 1;
        self.stats.chunk_rows += rows;
        self.tables[table].commit_fill(record);
        Ok(())
    }
}

/// Subscribes the binlog stream from `uuid:1-applied`. Used at start and after an outage.
pub(crate) async fn subscribe_cdc(
    cfg: &C1Config,
    uuid: &str,
    applied: u64,
) -> Result<Pin<Box<BinlogStream>>, PreviewError> {
    let sid = Sid::from_str(&format!("{uuid}:1-{applied}")).map_err(|_| PreviewError::new("c1", "gtid"))?;
    let password = read_secret(&cfg.password_file)?;
    let conn = connect(cfg, &cfg.user, password.as_str()).await?;
    drop(password);
    let request = BinlogStreamRequest::new(cfg.server_id).with_gtid().with_gtid_set([sid]);
    let stream = conn.get_binlog_stream(request).await.map_err(|err| mysql_err(&err))?;
    Ok(Box::pin(stream))
}

/// Drives the binlog stream and the chunk read in one select. Events are never
/// left unread for longer than one chunk, and the batch ages on every tick.
pub(crate) async fn pump<T>(
    engine: &mut Engine,
    stream: &mut Pin<Box<BinlogStream>>,
    cfg: &C1Config,
    fut: impl Future<Output = T>,
) -> Result<T, PreviewError> {
    tokio::pin!(fut);
    let mut interval = tokio::time::interval(TICK);
    loop {
        engine.check_deadline()?;
        tokio::select! {
            out = &mut fut => return Ok(out),
            polled = poll_fn(|cx| stream.as_mut().poll_next(cx)) => {
                handle_polled(engine, stream, cfg, polled).await?;
            }
            _ = interval.tick() => engine.tick()?,
        }
    }
}

/// One item from the binlog stream. An event is processed. An outage (the stream ends or
/// fails with a dropped connection) reconnects from the applied position. Any other error ends the run.
async fn handle_polled(
    engine: &mut Engine,
    stream: &mut Pin<Box<BinlogStream>>,
    cfg: &C1Config,
    polled: Option<Result<mysql_async::binlog::events::Event, mysql_async::Error>>,
) -> Result<(), PreviewError> {
    match polled {
        Some(Ok(event)) => {
            engine.cdc_budget.reset();
            engine.on_event(event, stream.as_ref().get_ref())
        }
        Some(Err(err)) => reconnect_cdc(engine, stream, cfg, mysql_err(&err)).await,
        None => reconnect_cdc(engine, stream, cfg, PreviewError::new("c1", "stream_end")).await,
    }
}

/// Reconnects the binlog stream after an outage, from `applied`. The partial transaction in
/// progress is dropped. The new stream starts after `applied`, so its events are read once.
/// Each failed attempt waits on `engine.cdc_budget`. Its end is `retry_exhausted`.
async fn reconnect_cdc(
    engine: &mut Engine,
    stream: &mut Pin<Box<BinlogStream>>,
    cfg: &C1Config,
    cause: PreviewError,
) -> Result<(), PreviewError> {
    if !stream_outage(&cause) {
        return Err(cause);
    }
    engine.pending = None;
    engine.reset_txn_budget();
    let mut cause = cause;
    loop {
        let Some(delay) = engine.cdc_budget.after_failure(Instant::now()) else {
            return Err(retry_exhausted());
        };
        eprintln!(
            "mysql-preview: c1 stream retry reason={}/{} applied={} sleep_ms={}",
            cause.code,
            cause.fact,
            engine.applied,
            delay.as_millis()
        );
        tokio::time::sleep(delay).await;
        match subscribe_cdc(cfg, &engine.uuid, engine.applied).await {
            Ok(fresh) => {
                *stream = fresh;
                engine.stats.cdc_reconnects += 1;
                return Ok(());
            }
            Err(err) if stream_outage(&err) => cause = err,
            Err(err) => return Err(err),
        }
    }
}

/// Consumes the stream until `applied` reaches `target` exactly. Fails retryably when
/// the applied position stops moving for `catchup_stall_ms`.
pub(crate) async fn consume_until(
    engine: &mut Engine,
    stream: &mut Pin<Box<BinlogStream>>,
    cfg: &C1Config,
    target: u64,
) -> Result<(), PreviewError> {
    let mut interval = tokio::time::interval(TICK);
    let stall = Duration::from_millis(engine.limits.catchup_stall_ms);
    let mut watch = StallWatch::new(stall, engine.applied, Instant::now());
    while engine.applied < target {
        engine.check_deadline()?;
        let reconnects = engine.stats.cdc_reconnects;
        tokio::select! {
            polled = poll_fn(|cx| stream.as_mut().poll_next(cx)) => {
                handle_polled(engine, stream, cfg, polled).await?;
            }
            _ = interval.tick() => engine.tick()?,
        }
        if engine.stats.cdc_reconnects != reconnects {
            watch.restart(Instant::now());
        }
        watch.observe(engine.applied, Instant::now())?;
    }
    if engine.applied != target {
        return Err(PreviewError::new("c1", "position"));
    }
    Ok(())
}

/// Reads one chunk with retry. A transient failure waits, reconnects, and reads the same
/// chunk again (DESIGN §3.5). The waits and the reconnects share one budget, which is
/// `connect_retry_ms` of wall-clock time (FRAMES-V2 §2). The cursor is rebuilt from committed state.
pub(crate) async fn read_chunk_with_retry<S: ChunkSource>(
    engine: &mut Engine,
    stream: &mut Pin<Box<BinlogStream>>,
    cfg: &C1Config,
    source: &mut S,
    reader: &ChunkReader,
    table: usize,
) -> Result<(ChunkBatch, u32, u64), PreviewError> {
    let mut attempt = 0u32;
    let mut budget = ConnectBudget::new(Duration::from_millis(engine.limits.connect_retry_ms));
    loop {
        attempt += 1;
        engine.check_deadline()?;
        let l0_pos = engine.l0_pos;
        let wait_ms = pump(engine, stream, cfg, source.prepare(l0_pos)).await??;
        let mut cursor = engine.tables[table].cursor();
        let result = pump(engine, stream, cfg, source.read(reader, &mut cursor)).await?;
        match result {
            Ok(batch) => return Ok((batch, attempt, wait_ms)),
            Err(err) if retriable(&err) => {
                engine.stats.chunk_retries += 1;
                if attempt >= engine.limits.max_chunk_attempts {
                    return Err(retry_exhausted());
                }
                let reason = (err.code, err.fact);
                retry_wait(engine, stream, cfg, source, &mut budget, reason, attempt).await?;
            }
            Err(err) => return Err(err.into()),
        }
    }
}

/// Waits out one back-off, then reconnects the chunk source. A reconnect that fails with a
/// connection error waits again on the same budget. Any other error ends the read.
async fn retry_wait<S: ChunkSource>(
    engine: &mut Engine,
    stream: &mut Pin<Box<BinlogStream>>,
    cfg: &C1Config,
    source: &mut S,
    budget: &mut ConnectBudget,
    mut reason: (&'static str, &'static str),
    attempt: u32,
) -> Result<(), PreviewError> {
    loop {
        let Some(delay) = budget.after_failure(Instant::now()) else {
            return Err(retry_exhausted());
        };
        eprintln!(
            "mysql-preview: c1 chunk retry reason={}/{} attempt={attempt} sleep_ms={}",
            reason.0,
            reason.1,
            delay.as_millis()
        );
        pump(engine, stream, cfg, tokio::time::sleep(delay)).await?;
        match pump(engine, stream, cfg, source.reconnect()).await? {
            Ok(()) => return Ok(()),
            Err(err) if err.code == "connect" => reason = (err.code, err.fact),
            Err(err) => return Err(err),
        }
    }
}

/// Row-size hint per table from `information_schema.TABLES.AVG_ROW_LENGTH`. The first
/// chunk of a table is sized from it, so a wide table does not stream and discard
/// `chunk_rows` rows before the byte budget cuts the chunk. Missing values give 0.
async fn row_length_hints(
    conn: &mut Conn,
    tables: &[String],
) -> Result<BTreeMap<String, u64>, PreviewError> {
    let mut out = BTreeMap::new();
    for qualified in tables {
        let (schema, table) = qualified
            .split_once('.')
            .ok_or_else(|| PreviewError::new("catalog", "tables"))?;
        let row: Option<(Option<u64>,)> = conn
            .exec_first(
                "SELECT AVG_ROW_LENGTH FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (schema, table),
            )
            .await
            .map_err(|_| PreviewError::new("catalog", "columns"))?;
        out.insert(qualified.clone(), row.and_then(|r| r.0).unwrap_or(0));
    }
    Ok(out)
}

fn hex32(text: &str) -> Result<[u8; 32], PreviewError> {
    if text.len() != 64 {
        return Err(PreviewError::new("catalog", "schema"));
    }
    let mut out = [0u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16);
        let lo = (pair[1] as char).to_digit(16);
        let (Some(hi), Some(lo)) = (hi, lo) else {
            return Err(PreviewError::new("catalog", "schema"));
        };
        out[index] = (hi * 16 + lo) as u8;
    }
    Ok(out)
}

/// Producer entry point for `c1-baseline-stream-v2`.
///
/// Fresh run: `out` must not exist. Resume run: `out` must hold the journal of the
/// earlier run, and the bootstrap must carry the worker's `resume` state.
/// PostEnd step after the producer's `end` (DESIGN §4.2, FRAMES-V2 §10). Reads the
/// producer's out directory and the worker's `context` file, and returns the final-load
/// request (`oo.pipeline.native-c1-baseline/v2`). The CLI is `c1-materialize-v2`.
pub fn materialize_v2(out: &Path, context: &Path) -> Result<Value, PreviewError> {
    let bytes = fs::read(context).map_err(|_| config_err("out"))?;
    let context: Value = serde_json::from_slice(&bytes).map_err(|_| config_err("out"))?;
    crate::c1_materialize::materialize_from_producer(out, &context)
}

/// `target_columns` of one table (DESIGN §4.3, F1): name, target oid, typmod and not-null,
/// in the order of the table's `columns` (ordinal order). The loader's `open` takes its
/// typing from this list, so it must name exactly the same columns.
pub(crate) fn target_columns(columns: &Value, mapped: &[MappedColumn]) -> Result<Value, PreviewError> {
    let names = columns.as_array().ok_or_else(|| PreviewError::new("catalog", "columns"))?;
    let mut target = Vec::with_capacity(names.len());
    for column in names {
        let name = column["name"].as_str().ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        let mapped = mapped
            .iter()
            .find(|c| c.plan.name == name)
            .ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        target.push(json!({"name": mapped.plan.name, "oid": mapped.oid, "typmod": mapped.typmod, "not_null": mapped.not_null}));
    }
    if target.len() != mapped.len() {
        return Err(PreviewError::new("catalog", "columns"));
    }
    Ok(Value::Array(target))
}

pub async fn baseline_stream_v2(config: &Path, out: &Path) -> Result<(), PreviewError> {
    let cfg = load_config(config)?;
    configured_selection(&cfg)?;
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|_| worker_err("bootstrap"))?;
    let boot = parse_bootstrap(line.trim_end())?;
    let replica_config = c1_replica::load_replica_config(config)?;
    if matches!(boot.read_source, ReadSource::Replica(_)) && replica_config.is_none() {
        return Err(config_err("read_source"));
    }
    if boot.resume.is_none() && out.exists() {
        return Err(PreviewError::new("config", "out_exists"));
    }
    if boot.resume.is_some() && !out.join(JOURNAL_FILE).is_file() {
        return Err(worker_err("journal_missing"));
    }

    let mut probe = connect_cdc(&cfg).await?;
    let vars = preflight_values(&mut probe).await?;
    let source_server: u32 = vars["server_id"]
        .as_str()
        .and_then(|item| item.parse().ok())
        .ok_or_else(|| PreviewError::new("c1", "dba_required"))?;
    if source_server == cfg.server_id {
        return Err(PreviewError::new("config", "server_id"));
    }
    replication_grants_ok(&rows_first(
        &probe
            .query::<Row, _>("SHOW GRANTS FOR CURRENT_USER()")
            .await
            .map_err(|_| PreviewError::new("privilege", "replication"))?,
    )?)?;
    let (pk, names, _) = load_pk(&mut probe, &cfg.tables).await?;
    let kinds = load_column_kinds(&mut probe, &cfg.tables).await?;
    let reserved = oo_connectors::PG17_RESERVED
        .iter()
        .map(|s| (*s).to_string())
        .collect::<BTreeSet<_>>();
    let mut typed_tables = Vec::new();
    let mut notes = Vec::new();
    for qualified in &cfg.tables {
        let (schema, table) = qualified
            .split_once('.')
            .ok_or_else(|| PreviewError::new("catalog", "tables"))?;
        let selected = oo_connectors::mysql::SelectedTable {
            schema: schema.to_owned(),
            name: table.to_owned(),
            columns: cfg
                .columns
                .get(qualified)
                .cloned()
                .ok_or_else(|| PreviewError::new("catalog", "columns"))?,
        };
        let catalog = crate::source::catalog_columns(&mut probe, &mut notes, &selected).await?;
        let mut mapped = map_table(&catalog, false, true, &reserved)?;
        for (index, column) in mapped.iter_mut().enumerate() {
            column.plan.attnum =
                i32::try_from(index + 1).map_err(|_| PreviewError::new("catalog", "columns"))?;
        }
        if mapped.iter().map(|c| c.plan.name.as_str()).collect::<Vec<_>>()
            != selected.columns.iter().map(String::as_str).collect::<Vec<_>>()
        {
            return Err(PreviewError::new("catalog", "columns"));
        }
        typed_tables.push((schema.to_owned(), table.to_owned(), mapped));
    }
    let uuid = scalar(&mut probe, "SELECT @@GLOBAL.server_uuid").await?;
    let (_, _, executed) = full_status(&mut probe).await?;
    let row_hints = row_length_hints(&mut probe, &cfg.tables).await?;
    let replica = match (&boot.read_source, replica_config) {
        (ReadSource::Replica(request), Some(replica)) => {
            let base = batch_source_config(&cfg)?;
            let opened = ReplicaSource::open(
                replica,
                request.clone(),
                &base,
                &mut probe,
                &uuid,
                &cfg.tables,
            );
            Some(opened.await?)
        }
        _ => None,
    };
    drop(probe);

    let visible = visible_ordinals(&cfg, &names)?;
    let schema_preimage = Value::Array(
        typed_tables
            .iter()
            .map(|(_, table, columns)| {
                Ok(json!({"table": table, "columns": crate::c1_materialize::schema_columns(columns)?}))
            })
            .collect::<Result<Vec<_>, PreviewError>>()?,
    );
    let catalog = serde_json::to_vec(&schema_preimage).map_err(|_| PreviewError::new("catalog", "schema"))?;
    let schema_hash = sha256_hex(&catalog);
    let mut table_schemas = native_table_schemas(&cfg.tables, &names, &pk, &kinds)?;
    // `target_columns` (DESIGN §4.3): the target typing of each column, in the same order as
    // `columns`. The worker's loader `open` uses it. It comes from the same mapping as the parts.
    for item in table_schemas.iter_mut() {
        let qualified = format!("{}.{}", item["schema"].as_str().unwrap_or_default(), item["table"].as_str().unwrap_or_default());
        let (_, _, mapped) = typed_tables
            .iter()
            .find(|(schema, table, _)| format!("{schema}.{table}") == qualified)
            .ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        item["target_columns"] = target_columns(&item["columns"], mapped)?;
    }
    let context = json!({
        "source_id": boot.source_id,
        "secret_ref": cfg.secret_ref,
        "source_preflight": vars,
    });
    let table_pairs: Vec<(String, String)> = cfg
        .tables
        .iter()
        .map(|qualified| {
            let (schema, table) = qualified.split_once('.').unwrap_or(("", ""));
            (schema.to_owned(), table.to_owned())
        })
        .collect();
    let admission_config = crate::c1_materialize::admission_config_v2(&context, &table_pairs, boot.limits.max_table_rows);
    let config_digest = sha256_hex(
        &serde_json::to_vec(&admission_config).map_err(|_| PreviewError::new("config", "path"))?,
    );
    let connection_config_sha256 =
        sha256_hex(&fs::read(config).map_err(|_| PreviewError::new("config", "path"))?);
    let lineage = format!("mysql.{uuid}");

    // L0. A resume keeps the worker's original L0. A previous run keeps its l_end.
    let observed_gtid = executed.clone();
    let mut l0 = executed.clone();
    if let Some(resume) = &boot.resume {
        let prior = parse_span(&resume.l0)?;
        if prior.uuid != uuid || prior.end != resume.l0_pos || !gtid_contains(&executed, &uuid, prior.end) {
            return Err(PreviewError::new("c1", "resume_l0"));
        }
        l0 = resume.l0.clone();
    } else if let Some(previous) = &boot.previous {
        if let Some(l_end) = previous.get("l_end").and_then(Value::as_str) {
            let prior = parse_span(l_end)?;
            if prior.uuid != uuid || !gtid_contains(&l0, &uuid, prior.end) {
                return Err(PreviewError::new("c1", "previous_gtid"));
            }
            if previous.get("config_digest").and_then(Value::as_str) != Some(config_digest.as_str())
                || previous.get("schema_digest").and_then(Value::as_str) != Some(schema_hash.as_str())
                || previous.get("measured_id").and_then(Value::as_str) != Some(lineage.as_str())
            {
                return Err(PreviewError::new("c1", "previous_identity"));
            }
            l0 = l_end.to_owned();
            if observed_gtid == l0 {
                send_frame(&json!({"kind": "quiet", "protocol": PROTO_CHUNK, "request": {
                    "protocol": "oo.jobs.mysql-c1-nochange/v2",
                    "source_id": boot.source_id,
                    "source_generation": boot.generation.saturating_sub(1),
                    "previous": previous,
                    "observed_gtid": observed_gtid,
                    "connection_config_sha256": connection_config_sha256,
                }}))?;
                return Ok(());
            }
        }
    }
    let span = parse_span(&l0)?;
    if span.uuid != uuid || span.end == 0 {
        return Err(PreviewError::new("c1", "gtid"));
    }
    let l0_pos = span.end;
    if let Some(resume) = &boot.resume
        && (resume.l0_pos != l0_pos || resume.applied_pos > executed_end_pos(&executed, &uuid)?)
    {
        return Err(PreviewError::new("c1", "resume_l0"));
    }

    let limits = boot.limits.clone();
    let binding = RunBinding {
        source_id: boot.source_id.clone(),
        schema: String::new(),
        table: String::new(),
        run_generation: boot.generation,
        input_digest: hex32(&schema_hash)?,
    };
    let mut tables = Vec::with_capacity(typed_tables.len());
    for (schema, table, columns) in &typed_tables {
        let qualified = format!("{schema}.{table}");
        let ordinals = visible
            .get(&qualified)
            .cloned()
            .ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        let mut table_state = TableRt::new(schema, table, columns.clone(), ordinals, &reserved)?;
        table_state.avg_row_bytes = row_hints.get(&qualified).copied().unwrap_or(0);
        tables.push(table_state);
    }
    let by_name: BTreeMap<String, usize> = tables
        .iter()
        .enumerate()
        .map(|(index, t)| (t.qualified.clone(), index))
        .collect();
    let tables_names: Vec<String> = tables.iter().map(|t| t.qualified.clone()).collect();

    // Journal. A fresh run creates it. A resume reconciles it with the worker.
    let header = JournalLine::Header(json!({
        "l0": l0,
        "l0_pos": l0_pos,
        "source_uuid": uuid,
        "source_id": boot.source_id,
        "generation": boot.generation,
        "config_digest": config_digest,
        "schema_digest": schema_hash,
        "tables": cfg.tables,
    }));
    let mut applied = l0_pos;
    let mut next_batch_no = 1u64;
    let journal = match &boot.resume {
        None => {
            fs::create_dir_all(out.join("parts")).map_err(|_| PreviewError::new("config", "out"))?;
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(out, fs::Permissions::from_mode(0o700))
                    .map_err(|_| PreviewError::new("config", "out"))?;
            }
            let mut file = File::create_new(out.join(JOURNAL_FILE))
                .map_err(|_| PreviewError::new("config", "out"))?;
            let mut text = serde_json::to_vec(&header.to_json()).map_err(|_| PreviewError::new("config", "out"))?;
            text.push(b'\n');
            file.write_all(&text).map_err(|_| PreviewError::new("config", "out"))?;
            file.sync_data().map_err(|_| PreviewError::new("config", "out"))?;
            OpenOptions::new()
                .append(true)
                .open(out.join(JOURNAL_FILE))
                .map_err(|_| PreviewError::new("config", "out"))?
        }
        Some(resume) => {
            let lines = read_journal(&out.join(JOURNAL_FILE))?;
            let mut fills = Vec::new();
            let mut batches = Vec::new();
            let mut stored = None;
            for line in lines {
                match line {
                    JournalLine::Header(value) => stored = Some(value),
                    JournalLine::Fill(record) => fills.push(record),
                    JournalLine::Batch(record) => batches.push(record),
                }
            }
            let stored = stored.ok_or_else(|| worker_err("journal"))?;
            if stored.get("l0").and_then(Value::as_str) != Some(resume.l0.as_str())
                || stored.get("l0_pos").and_then(u64_of) != Some(resume.l0_pos)
                || stored.get("schema_digest").and_then(Value::as_str) != Some(schema_hash.as_str())
                || stored.get("config_digest").and_then(Value::as_str) != Some(config_digest.as_str())
            {
                return Err(worker_err("journal_header"));
            }
            let reconciled = reconcile(fills, batches, resume, &cfg.tables)?;
            for table in tables.iter_mut() {
                let kept = reconciled.fills.get(&table.qualified).cloned().unwrap_or_default();
                table.restore(kept, resume.tables.get(&table.qualified));
            }
            for batch in &reconciled.batches {
                for part in &batch.parts {
                    if let Some(index) = by_name.get(&part.table).copied() {
                        let t = &mut tables[index];
                        t.next_cdc_part_no = part.part.part_no + 1;
                        t.cdc_parts.push(part.clone());
                    }
                }
            }
            if let Some(last) = reconciled.batches.last() {
                next_batch_no = last.batch_no + 1;
            }
            applied = resume.applied_pos;
            for path in &reconciled.dropped {
                let _ = fs::remove_file(out.join(path));
            }
            rewrite_journal(out, &header, &reconciled)?
        }
    };

    // FRAMES-V2 §3.1: the digests this baseline is bound to. Both come from open-time facts (the
    // mapped columns and the admission config), so the worker binds them at open and the final
    // request carries the same values.
    let type_tables: Vec<(String, String, Vec<crate::c1_materialize::V2Column>)> = typed_tables
        .iter()
        .map(|(schema, table, mapped)| {
            let columns = mapped
                .iter()
                .map(|column| {
                    crate::c1_materialize::V2Column::from_mapped(column)
                        .and_then(|v| v.to_facts())
                        .and_then(|facts| crate::c1_materialize::V2Column::from_facts(&facts))
                })
                .collect::<Result<Vec<_>, PreviewError>>()?;
            Ok((format!("{schema}.{table}"), table.clone(), columns))
        })
        .collect::<Result<_, PreviewError>>()?;
    let (_, open_type_digest) = crate::c1_materialize::type_preimage(
        type_tables.iter().map(|(name, table, columns)| (name.as_str(), table.as_str(), columns.as_slice())),
    )?;
    let open_input_digest = crate::c1_materialize::input_digest(&l0, &config_digest, &schema_hash, &open_type_digest);
    // Per-table schema digests of the open frame use the same function materialize uses, so the
    // worker binds the digest the scope and the admission carry.
    for item in table_schemas.iter_mut() {
        let qualified = format!("{}.{}", item["schema"].as_str().unwrap_or_default(), item["table"].as_str().unwrap_or_default());
        let (_, table, columns) = type_tables
            .iter()
            .find(|(name, _, _)| *name == qualified)
            .ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        item["schema_sha256"] = json!(crate::c1_materialize::table_schema_digest(table, columns));
    }

    // Open the target stage, then subscribe L0 before any chunk read (P1).
    send_frame(&json!({
        "kind": "open",
        "protocol": PROTO_CHUNK,
        "l0": l0,
        "l0_pos": l0_pos.to_string(),
        "source_uuid": uuid,
        "config_digest": config_digest,
        "connection_config_sha256": connection_config_sha256,
        "admission_config": admission_config,
        "schema_digest": schema_hash,
        "type_digest": open_type_digest,
        "input_digest": open_input_digest,
        "schema_preimage": schema_preimage,
        "tables": cfg.tables,
        "table_schemas": table_schemas,
        "resume": boot.resume.is_some(),
        "applied_pos": applied.to_string(),
    }))?;
    let opened = read_ack(&mut input, "stage_open")?;
    let epoch = opened
        .get("source_epoch")
        .and_then(u64_of)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| worker_err("source_epoch"))?;
    if !opened.get("sealed").is_none_or(Value::is_null) {
        return Err(PreviewError::new("c1", "already_sealed"));
    }

    let mut stream = subscribe_cdc(&cfg, &uuid, applied).await?;
    send_frame(&json!({
        "kind": "stream_started",
        "protocol": PROTO_CHUNK,
        "l0": l0,
        "applied_pos": applied.to_string(),
        "position_lineage": lineage,
    }))?;
    read_ack(&mut input, "stream_started")?;

    let readers = tables
        .iter()
        .map(|t| {
            let mut binding = binding.clone();
            binding.schema = t.schema.clone();
            binding.table = t.table.clone();
            ChunkReader::new(
                &t.schema,
                &t.table,
                t.columns.clone(),
                binding,
                limits.chunk_rows,
                limits.chunk_bytes,
                limits.chunk_deadline_ms,
                limits.statement_ms,
            )
        })
        .collect::<Result<Vec<_>, MysqlError>>()?;
    let mut source = match replica {
        Some(replica) => ActiveSource::Replica(replica),
        None => ActiveSource::Primary(
            PrimarySource::open(
                batch_source_config(&cfg)?,
                read_secret(&cfg.password_file)?,
            )
            .await?,
        ),
    };

    let mut engine = Engine {
        out: out.to_path_buf(),
        input,
        uuid: uuid.clone(),
        source_id: boot.source_id.clone(),
        generation: boot.generation,
        epoch,
        l0_pos,
        applied,
        deadline: Instant::now() + Duration::from_millis(limits.deadline_ms),
        limits: limits.clone(),
        tables,
        by_name,
        journal,
        pending: None,
        txn_events: 0,
        txn_bytes: 0,
        batch: CdcBatch::default(),
        next_batch_no,
        stats: Stats::default(),
        binding,
        tables_names,
        cdc_budget: ConnectBudget::new(Duration::from_millis(limits.connect_retry_ms)),
    };

    for table in 0..engine.tables.len() {
        while !engine.tables[table].terminal {
            let (batch, attempt, wait_ms) = read_chunk_with_retry(
                &mut engine,
                &mut stream,
                &cfg,
                &mut source,
                &readers[table],
                table,
            )
            .await?;
            let kind = source.kind();
            let identity = source.replica_identity().map(str::to_owned);
            engine.commit_fill(table, batch, attempt, kind, wait_ms, identity.as_deref())?;
        }
    }
    source.verify(l0_pos).await?;

    // L_end is measured after the last chunk. Catch-up stops at exactly that position.
    let mut probe = connect_cdc(&cfg).await?;
    let executed_end = scalar(&mut probe, "SELECT @@GLOBAL.gtid_executed").await?;
    drop(probe);
    let end = parse_span(&executed_end)?;
    if end.uuid != uuid {
        return Err(PreviewError::new("c1", "lineage"));
    }
    consume_until(&mut engine, &mut stream, &cfg, end.end).await?;
    engine.flush_batch()?;

    let mut manifest_tables = Vec::new();
    let mut fill_rows = 0u64;
    let mut cdc_rows = 0u64;
    for t in &engine.tables {
        let mut parts: Vec<PartEntry> = t.fill.iter().map(|r| r.part.clone()).collect();
        parts.extend(t.cdc_parts.iter().map(|p| p.part.clone()));
        let manifest = PartManifest::new(parts).map_err(|_| PreviewError::new("c1", "manifest"))?;
        let table_cdc: u64 = t.cdc_parts.iter().map(|p| p.part.rows).sum();
        fill_rows += t.rows;
        cdc_rows += table_cdc;
        // The column facts let `c1-materialize-v2` run without the database (FRAMES-V2 §4).
        let columns = t
            .columns
            .iter()
            .map(|column| crate::c1_materialize::V2Column::from_mapped(column)?.to_facts())
            .collect::<Result<Vec<_>, PreviewError>>()?;
        manifest_tables.push(json!({
            "schema": t.schema,
            "table": t.table,
            "fill_rows": t.rows,
            "cdc_rows": table_cdc,
            "parts_digest": manifest.parts_digest(),
            "manifest": manifest.to_json(),
            "columns": columns,
        }));
    }
    let manifest_doc = json!({
        "protocol": PROTO_PARTS,
        "source_id": engine.source_id,
        "measured_id": lineage,
        "l0": l0,
        "l0_pos": l0_pos,
        "l_end": executed_end,
        "l_end_pos": end.end,
        "applied_pos": engine.applied,
        "open_digests": {"type_digest": open_type_digest, "input_digest": open_input_digest},
        "materialize": {
            "source_preflight": vars,
            "admission_config": admission_config,
            "max_table_rows": limits.max_table_rows,
            "max_cdc_keys": crate::c1_materialize::DEFAULT_MAX_CDC_KEYS,
        },
        "tables": manifest_tables,
    });
    write_json(&out.join(PARTS_MANIFEST_FILE), &manifest_doc)?;
    let manifest_sha256 = sha256_hex(&fs::read(out.join(PARTS_MANIFEST_FILE)).map_err(|_| PreviewError::new("config", "out"))?);
    send_frame(&json!({
        "kind": "end",
        "protocol": PROTO_CHUNK,
        "l_end": executed_end,
        "l_end_pos": end.end.to_string(),
        "applied_pos": engine.applied.to_string(),
        "parts_manifest": PARTS_MANIFEST_FILE,
        "parts_manifest_sha256": manifest_sha256,
        "fill_rows": fill_rows,
        "cdc_rows": cdc_rows,
    }))?;
    let ack = read_ack(&mut engine.input, "end")?;
    if ack.get("outcome").and_then(Value::as_str) != Some("applied") {
        return Err(worker_err("reply"));
    }
    let stats = &engine.stats;
    write_json(
        &out.join(RECEIPT_FILE),
        &json!({
            "protocol": PROTO_RECEIPT,
            "source_id": engine.source_id,
            "generation": engine.generation,
            "read_source": source.kind(),
            "replica": source.receipt(),
            "source_epoch": engine.epoch,
            "l0": l0,
            "l0_pos": l0_pos,
            "l_end": executed_end,
            "l_end_pos": end.end,
            "applied_pos": engine.applied,
            "resumed": boot.resume.is_some(),
            "chunks": stats.chunks,
            "chunk_retries": stats.chunk_retries,
            "cdc_reconnects": stats.cdc_reconnects,
            "chunk_rows": stats.chunk_rows,
            "fill_rows": fill_rows,
            "batches": stats.batches,
            "txns": stats.txns,
            "cdc_rows": cdc_rows,
            "max_batch_txns": stats.max_batch_txns,
            "parts_manifest_sha256": manifest_sha256,
            "limits": {
                "chunk_rows": limits.chunk_rows,
                "chunk_bytes": limits.chunk_bytes,
                "max_txn_events": limits.max_txn_events,
                "max_txn_bytes": limits.max_txn_bytes,
            },
            "target_applied": false,
            "ack_allowed": false,
        }),
    )?;
    Ok(())
}

/// Largest executed position for the measured uuid. Used to validate that a resume
/// point is not ahead of the source.
fn executed_end_pos(executed: &str, uuid: &str) -> Result<u64, PreviewError> {
    let span = parse_span(executed)?;
    if span.uuid != uuid {
        return Err(PreviewError::new("c1", "lineage"));
    }
    Ok(span.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_pipeline::c1_parts::{PartKind, read_part};

    fn bootstrap_line(extra: Value) -> String {
        let mut base = json!({
            "protocol": PROTO_BOOTSTRAP,
            "source_id": "mysql_t3",
            "source_generation": 4,
            "baseline": {"format": FORMAT},
        });
        if let (Some(map), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        base.to_string()
    }

    fn resume_json(l0_pos: u64, applied: u64, tables: Value) -> Value {
        json!({
            "state": "open",
            "l0": format!("uuid:1-{l0_pos}"),
            "l0_pos": l0_pos.to_string(),
            "applied_pos": applied.to_string(),
            "tables": tables,
        })
    }

    fn fill(table: &str, chunk_no: u32, lo: Option<i128>, hi: Option<i128>, rows: u64) -> FillRecord {
        let terminal = rows == 0;
        FillRecord {
            table: table.to_owned(),
            chunk_no,
            terminal,
            rows,
            bytes: rows * 10,
            avg_row_bytes: if rows > 0 { 10 } else { 0 },
            part: PartEntry {
                part_no: chunk_no,
                kind: PartKind::Fill,
                key_lo: lo,
                key_hi: hi,
                rows,
                bytes: 100,
                sha256: "a".repeat(64),
            },
            path: part_rel_path(table, PartKind::Fill, chunk_no),
        }
    }

    fn batch(batch_no: u64, first: u64, last: u64, parts: Vec<CdcPartRecord>) -> BatchRecord {
        let txns: Vec<TxnMeta> = (first..=last)
            .map(|gno| TxnMeta {
                txn_id: format!("uuid:{gno}"),
                applied_pos: gno,
                rows: 1,
                payload_sha256: "b".repeat(64),
                source_commit_timestamp: "2026-10-17T10:00:00Z".to_owned(),
            })
            .collect();
        let batch_sha256 = batch_sha256(&parts, &txns);
        BatchRecord {
            batch_no,
            first_pos: first,
            last_pos: last,
            batch_sha256,
            txns,
            parts,
            rejects: 0,
        }
    }

    fn cdc_part(table: &str, part_no: u32, rows: u64) -> CdcPartRecord {
        CdcPartRecord {
            table: table.to_owned(),
            part: PartEntry {
                part_no,
                kind: PartKind::Cdc,
                key_lo: Some(1),
                key_hi: Some(9),
                rows,
                bytes: 80,
                sha256: "c".repeat(64),
            },
            path: part_rel_path(table, PartKind::Cdc, part_no),
        }
    }

    // ---- bootstrap, limits, resume ----

    /// F1. `target_columns` names the same columns, in the same order, as `columns`, with the
    /// target oid, typmod and not-null of each mapped column. A missing column is refused.
    #[test]
    fn target_columns_follow_the_column_order_and_the_mapping() {
        let mapped = mapped_columns();
        let names = Value::Array(mapped.iter().rev().map(|c| json!({"name": c.plan.name})).collect());
        let target = target_columns(&names, &mapped).expect("target columns");
        let items = target.as_array().expect("array");
        assert_eq!(items.len(), mapped.len());
        for (item, column) in items.iter().zip(mapped.iter().rev()) {
            assert_eq!(item["name"], json!(column.plan.name));
            assert_eq!(item["oid"], json!(column.oid));
            assert_eq!(item["typmod"], json!(column.typmod));
            assert_eq!(item["not_null"], json!(column.not_null));
        }
        let missing = json!([{"name": "no_such_column"}]);
        assert!(target_columns(&missing, &mapped).is_err());
        assert!(target_columns(&json!([]), &mapped).is_err());
    }

    #[test]
    fn bootstrap_v2_defaults_match_design_and_refuses_v1() {
        let boot = parse_bootstrap(&bootstrap_line(json!({}))).unwrap();
        assert_eq!(boot.source_id, "mysql_t3");
        assert_eq!(boot.generation, 4);
        assert_eq!(boot.limits, Limits::default());
        assert_eq!(boot.limits.chunk_rows, 50_000);
        assert_eq!(boot.limits.chunk_bytes, 8 * 1024 * 1024);
        assert_eq!(boot.limits.max_txn_events, 200_000);
        assert_eq!(boot.limits.deadline_ms, 43_200_000);
        assert!(boot.read_source == ReadSource::Primary);
        assert!(boot.resume.is_none() && boot.previous.is_none());

        let v1 = json!({"protocol": "oo.jobs.mysql-c1-bootstrap/v1"}).to_string();
        assert_eq!(parse_bootstrap(&v1).unwrap_err(), worker_err("bootstrap"));
        let no_format = json!({"protocol": PROTO_BOOTSTRAP, "source_id": "s",
            "source_generation": 1, "baseline": {}})
        .to_string();
        assert_eq!(parse_bootstrap(&no_format).unwrap_err(), config_err("format"));
        let zero_gen = json!({"protocol": PROTO_BOOTSTRAP, "source_id": "s",
            "source_generation": 0, "baseline": {"format": FORMAT}})
        .to_string();
        assert_eq!(parse_bootstrap(&zero_gen).unwrap_err(), worker_err("bootstrap"));
    }

    #[test]
    fn bootstrap_reads_limits_and_replica_request_without_defaulting_silently() {
        let boot = parse_bootstrap(&bootstrap_line(json!({
            "baseline": {"format": FORMAT, "chunk_rows": 1000, "chunk_bytes": 4096,
                "max_chunk_attempts": 3, "deadline_ms": 60000,
                "txn_batch": {"max_txns": 10, "max_bytes": 65536, "max_age_ms": 500}},
            "read_source": {"kind": "replica", "replica_identity": IDENTITY, "wait_timeout_s": 60}
        })))
        .unwrap();
        assert_eq!(boot.limits.chunk_rows, 1000);
        assert_eq!(boot.limits.chunk_bytes, 4096);
        assert_eq!(boot.limits.max_chunk_attempts, 3);
        assert_eq!(boot.limits.batch_max_txns, 10);
        assert_eq!(boot.limits.batch_max_bytes, 65536);
        assert_eq!(boot.limits.batch_max_age_ms, 500);
        assert_eq!(
            boot.read_source,
            ReadSource::Replica(c1_replica::ReplicaRequest {
                identity: Some(IDENTITY.to_owned()),
                wait_timeout_s: Some(60),
            })
        );
        let bad_kind = bootstrap_line(json!({"read_source": {"kind": "elsewhere"}}));
        assert_eq!(parse_bootstrap(&bad_kind).unwrap_err(), config_err("read_source"));
        let zero_wait =
            bootstrap_line(json!({"read_source": {"kind": "replica", "wait_timeout_s": 0}}));
        assert_eq!(parse_bootstrap(&zero_wait).unwrap_err(), config_err("read_source"));
    }

    const IDENTITY: &str = "11111111-2222-3333-4444-555555555555";

    #[test]
    fn catchup_stall_is_configurable_and_bounded() {
        assert_eq!(Limits::default().catchup_stall_ms, 300_000);
        let set = parse_limits(&json!({"format": FORMAT, "catchup_stall_ms": 60_000})).unwrap();
        assert_eq!(set.catchup_stall_ms, 60_000);
        for bad in [json!(0), json!(999), json!(3_600_001), json!("soon")] {
            let baseline = json!({"format": FORMAT, "catchup_stall_ms": bad});
            assert!(parse_limits(&baseline).is_err(), "{bad}");
        }
    }

    #[test]
    fn stall_watch_fails_only_when_the_applied_position_stops_moving() {
        let start = Instant::now();
        let mut watch = StallWatch::new(Duration::from_secs(10), 100, start);
        assert!(watch.observe(100, start + Duration::from_secs(9)).is_ok());
        // Progress resets the clock.
        assert!(watch.observe(101, start + Duration::from_secs(9)).is_ok());
        assert!(watch.observe(101, start + Duration::from_secs(18)).is_ok());
        let err = watch.observe(101, start + Duration::from_secs(19)).unwrap_err();
        assert_eq!(err, PreviewError::new("c1", "catchup_stall"));
    }

    #[test]
    fn a_reconnect_restarts_the_stall_clock() {
        let start = Instant::now();
        let mut watch = StallWatch::new(Duration::from_secs(10), 100, start);
        watch.restart(start + Duration::from_secs(9));
        assert!(watch.observe(100, start + Duration::from_secs(18)).is_ok());
        let err = watch.observe(100, start + Duration::from_secs(19)).unwrap_err();
        assert_eq!(err, PreviewError::new("c1", "catchup_stall"));
    }

    #[test]
    fn out_of_range_limits_are_refused_not_clamped() {
        for baseline in [
            json!({"format": FORMAT, "chunk_rows": 0}),
            json!({"format": FORMAT, "chunk_bytes": 33_554_433}),
            json!({"format": FORMAT, "max_chunk_attempts": 0}),
            json!({"format": FORMAT, "deadline_ms": 10}),
            json!({"format": FORMAT, "chunk_rows": "many"}),
            json!({"format": FORMAT, "txn_batch": {"max_bytes": 0}}),
        ] {
            assert!(parse_limits(&baseline).is_err(), "{baseline}");
        }
    }

    #[test]
    fn resume_must_be_open_and_consistent() {
        let ok = parse_resume(&resume_json(
            100,
            5230,
            json!([{"schema": "s", "table": "t", "last_chunk_no": 12,
                "last_key_hi": "98304", "terminal": false, "rows": 98304}]),
        ))
        .unwrap();
        assert_eq!((ok.l0_pos, ok.applied_pos), (100, 5230));
        assert_eq!(ok.tables["s.t"].last_key_hi, Some(98304));
        assert_eq!(ok.tables["s.t"].last_chunk_no, 12);

        let mut closed = resume_json(100, 100, json!([]));
        closed["state"] = json!("closed");
        assert_eq!(parse_resume(&closed).unwrap_err(), worker_err("resume_state"));
        assert_eq!(
            parse_resume(&resume_json(100, 99, json!([]))).unwrap_err(),
            worker_err("resume_applied")
        );
        let no_key = resume_json(100, 100, json!([{"schema": "s", "table": "t",
            "last_chunk_no": 3, "last_key_hi": null, "terminal": false, "rows": 3}]));
        assert_eq!(parse_resume(&no_key).unwrap_err(), worker_err("resume_table"));
        let key_without_chunk = resume_json(100, 100, json!([{"schema": "s", "table": "t",
            "last_chunk_no": 0, "last_key_hi": "7", "terminal": false, "rows": 0}]));
        assert_eq!(parse_resume(&key_without_chunk).unwrap_err(), worker_err("resume_table"));
        let duplicate = resume_json(100, 100, json!([
            {"schema": "s", "table": "t", "last_chunk_no": 0, "terminal": false, "rows": 0},
            {"schema": "s", "table": "t", "last_chunk_no": 0, "terminal": false, "rows": 0}]));
        assert_eq!(parse_resume(&duplicate).unwrap_err(), worker_err("resume_table"));
    }

    // ---- journal reconciliation (resume) ----

    fn tables() -> Vec<String> {
        vec!["s.t".to_owned()]
    }

    #[test]
    fn reconcile_keeps_only_worker_committed_chunks_and_drops_orphans() {
        // Worker committed chunks 1..=2 (last key 200). Chunk 3 was journaled but never acked.
        let resume = parse_resume(&resume_json(
            100,
            100,
            json!([{"schema": "s", "table": "t", "last_chunk_no": 2,
                "last_key_hi": "200", "terminal": false, "rows": 20}]),
        ))
        .unwrap();
        let fills = vec![
            fill("s.t", 1, Some(1), Some(100), 10),
            fill("s.t", 2, Some(101), Some(200), 10),
            fill("s.t", 3, Some(201), Some(300), 10),
        ];
        let out = reconcile(fills, vec![], &resume, &tables()).unwrap();
        let kept: Vec<u32> = out.fills["s.t"].iter().map(|r| r.chunk_no).collect();
        assert_eq!(kept, vec![1, 2]);
        assert_eq!(out.dropped, vec![part_rel_path("s.t", PartKind::Fill, 3)]);
    }

    #[test]
    fn reconcile_refuses_worker_state_for_a_table_this_run_does_not_read() {
        let resume = parse_resume(&resume_json(
            100,
            100,
            json!([{"schema": "s", "table": "other", "last_chunk_no": 1,
                "last_key_hi": "9", "terminal": false, "rows": 1}]),
        ))
        .unwrap();
        assert_eq!(
            reconcile(vec![], vec![], &resume, &tables()).unwrap_err(),
            worker_err("journal_table")
        );
    }

    #[test]
    fn reconcile_refuses_gaps_missing_committed_records_and_row_disagreement() {
        let resume = parse_resume(&resume_json(
            100,
            100,
            json!([{"schema": "s", "table": "t", "last_chunk_no": 2,
                "last_key_hi": "200", "terminal": false, "rows": 20}]),
        ))
        .unwrap();
        // Chunk 1 is missing, so chunk 2 is a gap.
        let gap = vec![fill("s.t", 2, Some(101), Some(200), 10)];
        assert_eq!(
            reconcile(gap, vec![], &resume, &tables()).unwrap_err(),
            worker_err("journal_gap")
        );
        // Chunk 2 was never journaled, so the committed state is missing.
        let missing = vec![fill("s.t", 1, Some(1), Some(100), 10)];
        assert_eq!(
            reconcile(missing, vec![], &resume, &tables()).unwrap_err(),
            worker_err("journal_missing")
        );
        // Journal key disagrees with the worker's last_key_hi.
        let key = vec![
            fill("s.t", 1, Some(1), Some(100), 10),
            fill("s.t", 2, Some(101), Some(199), 10),
        ];
        assert_eq!(
            reconcile(key, vec![], &resume, &tables()).unwrap_err(),
            worker_err("journal_key")
        );
        // Row totals disagree with the worker's committed rows.
        let rows = vec![
            fill("s.t", 1, Some(1), Some(100), 10),
            fill("s.t", 2, Some(101), Some(200), 9),
        ];
        assert_eq!(
            reconcile(rows, vec![], &resume, &tables()).unwrap_err(),
            worker_err("journal_rows")
        );
    }

    #[test]
    fn reconcile_keeps_committed_batches_and_drops_the_rest_by_position() {
        // Worker applied through position 30. The journal has 10..20 and 21..30 and 31..40.
        let resume = parse_resume(&resume_json(10, 30, json!([]))).unwrap();
        let b1 = batch(1, 11, 20, vec![cdc_part("s.t", 1, 3)]);
        let b2 = batch(2, 21, 30, vec![cdc_part("s.t", 2, 2)]);
        let b3 = batch(3, 31, 40, vec![cdc_part("s.t", 3, 4)]);
        let out = reconcile(vec![], vec![b3, b1, b2], &resume, &tables()).unwrap();
        assert_eq!(out.batches.iter().map(|b| b.batch_no).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(out.dropped, vec![part_rel_path("s.t", PartKind::Cdc, 3)]);
    }

    #[test]
    fn reconcile_refuses_a_broken_batch_chain_or_an_applied_point_inside_a_batch() {
        let resume = parse_resume(&resume_json(10, 30, json!([]))).unwrap();
        let skipped = vec![batch(1, 11, 20, vec![]), batch(2, 22, 30, vec![])];
        assert_eq!(
            reconcile(vec![], skipped, &resume, &tables()).unwrap_err(),
            worker_err("journal_chain")
        );
        // Applied 25 is inside the batch 21..30, which cannot be a durable boundary.
        let resume_inside = parse_resume(&resume_json(10, 25, json!([]))).unwrap();
        let inside = vec![batch(1, 11, 20, vec![]), batch(2, 21, 30, vec![])];
        assert_eq!(
            reconcile(vec![], inside, &resume_inside, &tables()).unwrap_err(),
            worker_err("journal_chain")
        );
        let short = vec![batch(1, 11, 20, vec![])];
        assert_eq!(
            reconcile(vec![], short, &resume, &tables()).unwrap_err(),
            worker_err("journal_missing")
        );
    }

    #[test]
    fn restore_resumes_the_chunk_loop_after_the_committed_key() {
        let mut table = TableRt::new(
            "s",
            "t",
            mapped_columns(),
            vec![0, 1],
            &reserved_words(),
        )
        .unwrap();
        assert_eq!(table.next_chunk_no(), 1);
        assert_eq!(table.cursor().after(), None);
        table.restore(
            vec![fill("s.t", 1, Some(1), Some(100), 10), fill("s.t", 2, Some(101), Some(200), 10)],
            Some(&ResumeTable {
                last_chunk_no: 2,
                last_key_hi: Some(200),
                terminal: false,
                rows: 20,
                avg_row_bytes: 0,
            }),
        );
        assert_eq!(table.next_chunk_no(), 3);
        assert_eq!(table.cursor().after(), Some(200));
        assert!(!table.terminal);
        assert_eq!(table.rows, 20);
        assert_eq!(table.cursor().avg_row_bytes(), 10);
        table.restore(vec![fill("s.t", 3, None, None, 0)], None);
        assert!(table.terminal, "a terminal chunk ends the table loop");
        assert_eq!(table.cursor().after(), Some(200), "terminal keeps the last key");
    }

    // ---- frames and journal encoding ----

    #[test]
    fn journal_records_round_trip_through_json() {
        let record = fill("s.t", 7, Some(-5), Some(9_000_000_000_000_000_000), 3);
        let line = JournalLine::Fill(record.clone()).to_json();
        assert_eq!(JournalLine::from_json(&line), Some(JournalLine::Fill(record)));
        let b = batch(4, 31, 35, vec![cdc_part("s.t", 1, 2)]);
        let line = JournalLine::Batch(b.clone()).to_json();
        assert_eq!(JournalLine::from_json(&line), Some(JournalLine::Batch(b)));
        assert!(JournalLine::from_json(&json!({"type": "mystery"})).is_none());
    }

    #[test]
    fn chunk_frame_carries_keys_as_decimal_strings_and_the_part_identity() {
        let record = fill("bench.shipments", 13, Some(98305), Some(106496), 8192);
        let read = json!({"source": "primary", "statement_sha256": "d".repeat(64), "wait_ms": 0});
        let frame = chunk_frame(&record, 2, &read);
        assert_eq!(frame["kind"], "chunk");
        assert_eq!(frame["protocol"], "oo.jobs.mysql-c1-chunk/v2");
        assert_eq!((frame["schema"].as_str(), frame["table"].as_str()), (Some("bench"), Some("shipments")));
        assert_eq!((frame["chunk_no"].as_u64(), frame["attempt"].as_u64()), (Some(13), Some(2)));
        assert_eq!(frame["key_lo"], "98305");
        assert_eq!(frame["key_hi"], "106496");
        assert_eq!(frame["terminal"], false);
        assert_eq!(frame["part_path"], "parts/bench.shipments/fill-00000013.arrow");
        assert_eq!(frame["part_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(frame["read"]["source"], "primary");

        let terminal = fill("bench.shipments", 14, None, None, 0);
        let frame = chunk_frame(&terminal, 1, &read);
        assert_eq!(frame["terminal"], true);
        assert!(frame["key_lo"].is_null() && frame["key_hi"].is_null());
    }

    #[test]
    fn txn_batch_frame_lists_parts_and_transactions_in_order() {
        let record = batch(77, 5231, 5233, vec![cdc_part("bench.shipments", 3, 5)]);
        let frame = txn_batch_frame(&record);
        assert_eq!(frame["kind"], "txn_batch");
        assert_eq!(frame["protocol"], "oo.jobs.mysql-c1-txn-batch/v2");
        assert_eq!(frame["batch_no"], 77);
        assert_eq!(frame["first_pos"], "5231");
        assert_eq!(frame["last_pos"], "5233");
        assert_eq!(frame["batch_sha256"], record.batch_sha256.as_str());
        let part = &frame["parts"][0];
        assert_eq!((part["schema"].as_str(), part["table"].as_str()), (Some("bench"), Some("shipments")));
        assert_eq!((part["part_no"].as_u64(), part["rows"].as_u64()), (Some(3), Some(5)));
        assert_eq!(part["part_path"], "parts/bench.shipments/cdc-00000003.arrow");
        let txns = frame["txns"].as_array().unwrap();
        assert_eq!(txns.iter().map(|t| t["applied_pos"].as_str().unwrap()).collect::<Vec<_>>(), vec!["5231", "5232", "5233"]);
        assert_eq!(txns[0]["txn_id"], "uuid:5231");
    }

    #[test]
    fn batch_digest_binds_parts_and_transaction_order() {
        let parts = vec![cdc_part("s.t", 1, 2)];
        let a = batch(1, 1, 2, parts.clone());
        let same = batch(1, 1, 2, parts.clone());
        assert_eq!(a.batch_sha256, same.batch_sha256);
        let mut other_part = parts.clone();
        other_part[0].part.sha256 = "e".repeat(64);
        assert_ne!(a.batch_sha256, batch(1, 1, 2, other_part).batch_sha256);
        let mut swapped = a.txns.clone();
        swapped.swap(0, 1);
        assert_ne!(a.batch_sha256, batch_sha256(&parts, &swapped));
    }

    #[test]
    fn worker_acks_must_be_durable_and_name_the_stage() {
        let ok = json!({"protocol": PROTO_ACK, "stage": "chunk_apply", "durable": true, "outcome": "loaded"}).to_string();
        assert!(parse_ack(&ok, "chunk_apply").is_ok());
        assert!(parse_ack(&ok, "txn_batch").is_err());
        let not_durable = json!({"protocol": PROTO_ACK, "stage": "chunk_apply", "durable": false}).to_string();
        assert!(parse_ack(&not_durable, "chunk_apply").is_err());
        let v1 = json!({"protocol": "oo.jobs.mysql-c1-baseline-ack/v1", "stage": "chunk_apply", "durable": true}).to_string();
        assert!(parse_ack(&v1, "chunk_apply").is_err());
    }

    // ---- batching and transaction building ----

    fn meta(gno: u64) -> TxnMeta {
        TxnMeta {
            txn_id: format!("uuid:{gno}"),
            applied_pos: gno,
            rows: 0,
            payload_sha256: "f".repeat(64),
            source_commit_timestamp: "2026-10-17T10:00:00Z".to_owned(),
        }
    }

    #[test]
    fn batch_limits_break_on_count_bytes_and_age() {
        let limits = Limits {
            batch_max_txns: 3,
            batch_max_bytes: 100,
            batch_max_age_ms: 1000,
            ..Limits::default()
        };
        let mut b = CdcBatch::default();
        assert!(!b.would_exceed(&limits, 1_000_000), "an empty batch takes any one transaction");
        b.push(10, meta(10), vec![], 0);
        b.push(11, meta(11), vec![], 0);
        assert!(!b.full(&limits));
        assert!(!b.would_exceed(&limits, 10));
        b.push(12, meta(12), vec![], 0);
        assert!(b.full(&limits), "third transaction reaches the count limit");
        assert!(b.would_exceed(&limits, 0), "a fourth transaction would break it");
        assert_eq!((b.first_pos, b.last_pos), (10, 12));

        let mut sized = CdcBatch::default();
        sized.bytes = 90;
        sized.txns.push(meta(1));
        assert!(sized.would_exceed(&limits, 11), "bytes past the limit break the batch");
        assert!(!sized.would_exceed(&limits, 10));

        let now = Instant::now();
        assert!(!b.aged(&limits, now));
        assert!(b.aged(&limits, now + Duration::from_millis(1000)), "age limit is one second");

        let taken = b.take();
        assert!(b.is_empty() && taken.txns.len() == 3);
    }

    #[test]
    fn split_rows_keeps_order_and_splits_only_past_the_raw_limit() {
        let row = |pos: u64, bytes: u64| CdcRow {
            pos,
            delete: false,
            key: pos as i128,
            cells: vec![],
            bytes,
        };
        let big = CDC_PART_RAW_BYTES / 2 + 1;
        let groups = split_rows(vec![row(1, big), row(2, big), row(3, 10), row(4, big)]);
        let shape: Vec<Vec<u64>> = groups.iter().map(|g| g.iter().map(|r| r.pos).collect()).collect();
        assert_eq!(shape, vec![vec![1], vec![2, 3], vec![4]]);
        assert!(split_rows(vec![]).is_empty());
    }

    #[test]
    fn transaction_keeps_the_last_image_per_key_so_each_part_has_unique_key_and_pos() {
        let image = |table: usize, key: i128, delete: bool| TxnRow {
            table,
            delete,
            key,
            cells: vec![],
            digest: format!("{key}{delete}"),
            bytes: 8,
        };
        // Same key: update then delete. The delete survives.
        let mut b = TxnBuild::default();
        b.put(image(0, 1, false));
        b.put(image(0, 1, true));
        let rows = b.into_rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].delete);
        // Same key: delete then reinsert. The reinsert survives.
        let mut b = TxnBuild::default();
        b.put(image(0, 1, true));
        b.put(image(0, 1, false));
        assert!(!b.into_rows()[0].delete);
        // A key change is a delete of the old key plus an upsert of the new one.
        let mut b = TxnBuild::default();
        b.put(image(0, 1, true));
        b.put(image(0, 2, false));
        assert_eq!(b.into_rows().len(), 2);
        // Different tables with the same key are different rows.
        let mut b = TxnBuild::default();
        b.put(image(0, 1, false));
        b.put(image(1, 1, false));
        assert_eq!(b.into_rows().len(), 2);
    }

    #[test]
    fn payload_digest_changes_with_any_row_change() {
        let tables = vec!["s.t".to_owned()];
        let r = |delete: bool, digest: &str| TxnRow {
            table: 0,
            delete,
            key: 7,
            cells: vec![],
            digest: digest.to_owned(),
            bytes: 0,
        };
        let base = payload_digest(&tables, &[r(false, "x")]);
        assert_eq!(base, payload_digest(&tables, &[r(false, "x")]));
        assert_ne!(base, payload_digest(&tables, &[r(true, "x")]));
        assert_ne!(base, payload_digest(&tables, &[r(false, "y")]));
    }

    #[test]
    fn backoff_doubles_and_is_capped_and_only_transient_errors_retry() {
        let steps: Vec<u64> = (1..=7).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(steps, vec![1, 2, 4, 8, 16, 30, 30]);
        assert!(retriable(&MysqlError::new("connect", "connect_timeout")));
        assert!(retriable(&MysqlError::new("protocol", "query")));
        assert!(retriable(&MysqlError::new("budget", "chunk_deadline")));
        assert!(retriable(&MysqlError::new("cancel", "cleanup_deadline")));
        assert!(!retriable(&MysqlError::new("budget", "row_bytes")));
        assert!(!retriable(&MysqlError::new("value", "null_not_allowed")));
        assert!(!retriable(&MysqlError::new("catalog", "key_type")));
    }

    #[test]
    fn connect_retry_budget_defaults_to_six_minutes_and_is_bounded_not_clamped() {
        assert_eq!(Limits::default().connect_retry_ms, 360_000);
        assert_eq!(Limits::default().max_chunk_attempts, 20);
        let set = parse_limits(&json!({"format": FORMAT, "connect_retry_ms": 0})).unwrap();
        assert_eq!(set.connect_retry_ms, 0, "0 turns the retry budget off");
        let set = parse_limits(&json!({"format": FORMAT, "connect_retry_ms": 3_600_000})).unwrap();
        assert_eq!(set.connect_retry_ms, 3_600_000);
        for bad in [json!(3_600_001), json!("soon"), json!(-1)] {
            let baseline = json!({"format": FORMAT, "connect_retry_ms": bad});
            assert!(parse_limits(&baseline).is_err(), "{bad}");
        }
    }

    #[test]
    fn connect_budget_backs_off_to_the_cap_and_ends_at_the_budget() {
        let start = Instant::now();
        let mut budget = ConnectBudget::new(Duration::from_millis(360_000));
        let mut now = start;
        let mut sleeps = Vec::new();
        while let Some(delay) = budget.after_failure(now) {
            assert!(delay <= Duration::from_secs(30), "one back-off is capped at 30 s");
            sleeps.push(delay.as_secs());
            now += delay;
        }
        assert_eq!(&sleeps[..6], &[1, 2, 4, 8, 16, 30], "exponential, then capped");
        assert_eq!(now.duration_since(start), Duration::from_secs(360), "six minutes, exactly");
        // 1+2+4+8+16 = 31 s, then 30 s sleeps: 331 s spent, so the last one is cut to 29 s.
        assert_eq!(*sleeps.last().unwrap(), 29, "the last back-off is cut to what is left");
        assert!(budget.after_failure(now).is_none(), "a spent budget stays spent");
    }

    #[test]
    fn connect_budget_counts_time_spent_in_attempts_not_only_sleeps() {
        let start = Instant::now();
        let mut budget = ConnectBudget::new(Duration::from_secs(60));
        // A 50 s attempt that fails leaves 10 s: the back-off is cut to it.
        assert_eq!(budget.after_failure(start), Some(Duration::from_secs(1)));
        let later = start + Duration::from_secs(50);
        assert_eq!(budget.after_failure(later), Some(Duration::from_secs(2)));
        assert_eq!(
            budget.after_failure(start + Duration::from_secs(59)),
            Some(Duration::from_secs(1)),
            "the back-off is cut to the time left"
        );
        assert!(budget.after_failure(start + Duration::from_secs(60)).is_none());
    }

    #[test]
    fn connect_budget_reset_starts_a_new_budget_and_zero_means_no_retry() {
        let start = Instant::now();
        let mut zero = ConnectBudget::new(Duration::ZERO);
        assert!(zero.after_failure(start).is_none(), "a zero budget never retries");

        let mut budget = ConnectBudget::new(Duration::from_secs(5));
        assert!(budget.after_failure(start + Duration::from_secs(5)).is_some());
        assert!(budget.after_failure(start + Duration::from_secs(10)).is_none());
        budget.reset();
        let again = start + Duration::from_secs(100);
        assert_eq!(budget.after_failure(again), Some(Duration::from_secs(1)));
    }

    #[test]
    fn only_outages_reconnect_the_binlog_stream_and_refusals_end_the_run() {
        for (code, fact) in [("protocol", "query"), ("connect", "connect_timeout"), ("c1", "stream_end")] {
            assert!(stream_outage(&PreviewError::new(code, fact)), "{code}/{fact} is an outage");
        }
        for (code, fact) in [
            ("c1", "gtid_gap"),
            ("c1", "lineage"),
            ("c1", "position_expired"),
            ("protocol", "event"),
            ("budget", "large_txn"),
        ] {
            assert!(!stream_outage(&PreviewError::new(code, fact)), "{code}/{fact} ends the run");
        }
    }

    #[test]
    fn exhausted_retries_exit_as_transient_connect_errors() {
        // main.rs maps `connect` to exit 4, which the job worker retries.
        assert_eq!(retry_exhausted(), PreviewError::new("connect", "retry_exhausted"));
    }

    // ---- real Arrow parts, read back through the shared part code ----

    fn reserved_words() -> BTreeSet<String> {
        oo_connectors::PG17_RESERVED.iter().map(|w| (*w).to_owned()).collect()
    }

    fn column(name: &str, ordinal: i32, data: &str, column_type: &str, key: bool) -> MysqlColumn {
        MysqlColumn {
            name: name.to_owned(),
            ordinal,
            data_type: data.to_owned(),
            column_type: column_type.to_owned(),
            nullable: false,
            char_len: None,
            precision: None,
            scale: None,
            charset: None,
            column_key: if key { "PRI" } else { "" }.to_owned(),
        }
    }

    fn mapped_columns() -> Vec<MappedColumn> {
        let id = column("id", 1, "bigint", "bigint unsigned", true);
        let mut payload = column("payload", 2, "varchar", "varchar(64)", false);
        payload.char_len = Some(64);
        payload.charset = Some("utf8mb4".to_owned());
        let mut mapped = map_table(&[id, payload], false, true, &reserved_words()).unwrap();
        for (index, column) in mapped.iter_mut().enumerate() {
            column.plan.attnum = index as i32 + 1;
        }
        mapped
    }

    fn decode_row(columns: &[MappedColumn], raw: &[RawCell]) -> Vec<DecodedCell> {
        columns
            .iter()
            .zip(raw)
            .map(|(c, cell)| values::decode_cell(c.store, cell, c.plan.mapping.nullable).unwrap().0)
            .collect()
    }

    fn binding() -> RunBinding {
        RunBinding {
            source_id: "mysql_t3".to_owned(),
            schema: "s".to_owned(),
            table: "t".to_owned(),
            run_generation: 1,
            input_digest: [0; 32],
        }
    }

    fn temp_out(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("oo-cb-T3-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unsigned_primary_key_max_round_trips_as_exact_decimal128() {
        let columns = mapped_columns();
        let plans: Vec<ColumnPlan> = columns.iter().map(|c| c.plan.clone()).collect();
        let rows = vec![decode_row(&columns, &[
            RawCell::Uint(u64::MAX), RawCell::Bytes(b"sample".to_vec()),
        ])];
        let typed = assemble_batch(&binding(), plans, &[1], rows).unwrap();
        let out = temp_out("unsigned-key-max");
        let rel = part_rel_path("s.t", PartKind::Fill, 1);
        write_part(&out.join(&rel), &typed).unwrap();
        let entry = part_entry_for_file(&out.join(&rel), 1, PartKind::Fill,
            Some(i128::from(u64::MAX)), Some(i128::from(u64::MAX)), 1).unwrap();
        let stream = read_part(&out.join("parts/s.t"), &entry).unwrap();
        assert_eq!(stream.fields[0].arrow_type, "decimal128");
        assert_eq!(stream.fields[0].precision, Some(20));
        assert_eq!(stream.fields[0].scale, Some(0));
        assert_eq!(stream.columns[0][0], oo_pipeline::native_arrow::NativeCell::Decimal {
            precision: 20, scale: 0, coefficient: i128::from(u64::MAX),
        });
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn fill_part_round_trips_through_the_shared_part_reader() {
        let columns = mapped_columns();
        let plans: Vec<ColumnPlan> = columns.iter().map(|c| c.plan.clone()).collect();
        let rows = vec![
            decode_row(&columns, &[RawCell::Uint(10), RawCell::Bytes(b"alpha".to_vec())]),
            decode_row(&columns, &[RawCell::Uint(12), RawCell::Bytes("ünï".as_bytes().to_vec())]),
        ];
        let typed = assemble_batch(&binding(), plans, &[1], rows).unwrap();
        let out = temp_out("fill");
        let rel = part_rel_path("s.t", PartKind::Fill, 1);
        write_part(&out.join(&rel), &typed).unwrap();
        let entry = part_entry_for_file(&out.join(&rel), 1, PartKind::Fill, Some(10), Some(12), 2)
            .expect("fill part validates");
        let dir = out.join("parts/s.t");
        let stream = read_part(&dir, &entry).expect("shared reader accepts the fill part");
        assert_eq!(stream.rows, 2);
        assert!(!stream.fields.iter().any(|f| f.name == "_oo_pos" || f.name == "_oo_op"));
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn cdc_part_has_the_marker_columns_the_shared_reader_requires() {
        let columns = mapped_columns();
        let mut table = TableRt::new("s", "t", columns.clone(), vec![0, 1], &reserved_words()).unwrap();
        table.cdc_parts.clear();
        let rows = vec![
            CdcRow {
                pos: 5231,
                delete: false,
                key: 10,
                cells: decode_row(&columns, &[RawCell::Uint(10), RawCell::Bytes(b"a".to_vec())]),
                bytes: 9,
            },
            CdcRow {
                pos: 5232,
                delete: true,
                key: 12,
                cells: decode_row(&columns, &[RawCell::Uint(12), RawCell::Bytes(b"b".to_vec())]),
                bytes: 9,
            },
        ];
        let typed = cdc_typed_batch(&binding(), &table, &rows).unwrap();
        let out = temp_out("cdc");
        let rel = part_rel_path("s.t", PartKind::Cdc, 1);
        write_part(&out.join(&rel), &typed).unwrap();
        let entry = part_entry_for_file(&out.join(&rel), 1, PartKind::Cdc, Some(10), Some(12), 2)
            .expect("CDC part has _oo_pos decimal(38,0) and _oo_op utf8 at the end");
        let stream = read_part(&out.join("parts/s.t"), &entry).unwrap();
        assert_eq!(stream.rows, 2);
        let names: Vec<&str> = stream.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(&names[names.len() - 2..], ["_oo_pos", "_oo_op"]);
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn rewrite_journal_removes_orphan_part_files_and_keeps_committed_ones() {
        let out = temp_out("journal");
        let header = JournalLine::Header(json!({"l0": "uuid:1-10", "l0_pos": 10}));
        fs::create_dir_all(out.join("parts/s.t")).unwrap();
        fs::write(out.join(part_rel_path("s.t", PartKind::Fill, 1)), b"kept").unwrap();
        fs::write(out.join(part_rel_path("s.t", PartKind::Fill, 2)), b"orphan").unwrap();
        fs::write(out.join("parts/s.t/fill-00000001.arrow.partial"), b"half").unwrap();
        let kept = Reconciled {
            fills: BTreeMap::from([("s.t".to_owned(), vec![fill("s.t", 1, Some(1), Some(5), 5)])]),
            batches: vec![],
            dropped: vec![],
        };
        let _file = rewrite_journal(&out, &header, &kept).unwrap();
        assert!(out.join(part_rel_path("s.t", PartKind::Fill, 1)).exists());
        assert!(!out.join(part_rel_path("s.t", PartKind::Fill, 2)).exists());
        assert!(!out.join("parts/s.t/fill-00000001.arrow.partial").exists());
        let lines = read_journal(&out.join(JOURNAL_FILE)).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(matches!(lines[0], JournalLine::Header(_)));
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn first_chunk_is_sized_from_the_catalog_row_hint_and_resume_prefers_observed_sizes() {
        let mut table = TableRt::new("s", "t", mapped_columns(), vec![0, 1], &reserved_words()).unwrap();
        table.avg_row_bytes = 1600;
        // 8 MiB + 12.5 % at 1600 bytes per row: 5,898 rows, not 50,000.
        let limit = ChunkCursor::start_with_hint(table.cursor().avg_row_bytes()).avg_row_bytes();
        assert_eq!(limit, 1600);
        table.restore(vec![fill("s.t", 1, Some(1), Some(100), 10)], Some(&ResumeTable {
            last_chunk_no: 1,
            last_key_hi: Some(100),
            terminal: false,
            rows: 10,
            avg_row_bytes: 4000,
        }));
        assert_eq!(table.cursor().avg_row_bytes(), 10, "the committed chunk's own size wins");
    }

    #[test]
    fn cdc_part_numbers_are_contiguous_per_table_across_groups_and_restarts() {
        let mut table = TableRt::new("s", "t", mapped_columns(), vec![0, 1], &reserved_words()).unwrap();
        assert_eq!((table.take_cdc_part_no(), table.take_cdc_part_no()), (1, 2));
        // A resume restores the next number after the highest kept part.
        table.next_cdc_part_no = 7 + 1;
        assert_eq!(table.take_cdc_part_no(), 8);
    }

    #[test]
    fn limits_default_for_a_minimal_baseline_object() {
        assert_eq!(parse_limits(&json!({"format": FORMAT})).unwrap(), Limits::default());
    }
}
