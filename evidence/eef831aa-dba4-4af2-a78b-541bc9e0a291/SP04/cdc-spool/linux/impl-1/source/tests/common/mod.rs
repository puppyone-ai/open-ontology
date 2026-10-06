#![allow(dead_code)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cdc_spool::{Fault, Limits, Outcome, Spool, INDEX_ALLOWANCE, PROGRESS_ALLOWANCE};
use serde_json::{json, Map, Value};

pub mod oracle;

pub const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub const TS: &str = "2026-04-15T08:30:00.123456Z";
pub const TS_Z: &str = "2026-04-15T08:30:01Z";
pub const POSITION: &str = "18446744168199581248";

static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct Row<'a> {
    pub txn: &'a str,
    pub global: u64,
    pub collection: u64,
    pub table: &'a str,
    pub key: &'a str,
    pub op: &'a str,
    pub payload: Value,
    pub unavailable: &'a [u16],
}

pub fn tempdir() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("oo-a291-{}-{n}", std::process::id()));
    fs::create_dir_all(&path).expect("tempdir");
    path
}

pub fn feed(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

pub fn bind(tables: &[&str]) -> String {
    json!({
        "kind": "bind",
        "source": "billing",
        "lineage": HASH,
        "epoch": 1,
        "selected_tables": tables,
        "schema_hash": HASH,
        "config_hash": HASH,
        "type_hash": HASH,
        "hook_hashes": [],
        "commit_position_provenance": "caller_declared"
    })
    .to_string()
}

pub fn begin(txn: &str) -> String {
    json!({"kind": "begin", "txn_id": txn}).to_string()
}

pub fn rollback(txn: &str) -> String {
    json!({"kind": "rollback", "txn_id": txn}).to_string()
}

pub fn data(row: &Row<'_>) -> String {
    json!({
        "kind": "data",
        "txn_id": row.txn,
        "global_txn_sequence": row.global,
        "data_collection_sequence": row.collection,
        "table": row.table,
        "key": [{"type": "int", "ordinal": 0, "value": row.key}],
        "op": row.op,
        "payload": row.payload,
        "unavailable_ordinals": row.unavailable
    })
    .to_string()
}

pub fn end(txn: &str, count: u64, counts: &[(&str, u64)], lsn: &str, timestamp: &str) -> String {
    let mut pairs: Vec<(&str, u64)> = counts.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    let mut map = Map::new();
    for (name, value) in pairs {
        map.insert(name.to_owned(), json!(value));
    }
    json!({
        "kind": "end",
        "txn_id": txn,
        "count": count,
        "per_collection_counts": Value::Object(map),
        "commit_lsn": lsn,
        "source_commit_timestamp": timestamp
    })
    .to_string()
}

pub fn ingest(dir: &Path, limits: Limits, bytes: &[u8]) -> Result<Outcome, Fault> {
    let mut spool = Spool::create_new(dir, limits)?;
    spool.ingest_reader(Cursor::new(bytes))
}

pub fn chunk_one(limits: Limits) -> Limits {
    let mut limits = limits;
    limits.max_chunk_events = 1;
    limits
}

pub fn first_event_disk(line_len: u64) -> u64 {
    line_len
        .saturating_add(INDEX_ALLOWANCE)
        .saturating_add(PROGRESS_ALLOWANCE)
}

pub fn file_sha(path: &Path) -> String {
    let bytes = fs::read(path).expect("read");
    oracle::hex(&oracle::sha256(&bytes))
}
