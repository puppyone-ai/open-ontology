use cdc_transaction::{Assembler, CompletedTxn, Fault, Progress};
use serde_json::{json, Map, Value};

pub const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub const HASH_B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
pub const TS: &str = "2026-04-15T08:30:00.123456Z";
pub const TS_Z: &str = "2026-04-15T08:30:01Z";

pub struct Drive {
    pub completed: Vec<CompletedTxn>,
    pub progress: Progress,
    pub fault: Option<Fault>,
}

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

pub fn drive(lines: &[String]) -> Drive {
    let mut assembler = Assembler::new();
    let mut completed = Vec::new();
    for line in lines {
        match assembler.apply_line(line) {
            Ok(out) => {
                if let Some(txn) = out.completed {
                    completed.push(txn);
                }
            }
            Err(fault) => {
                let _ = assembler.note_newline();
                return Drive {
                    completed,
                    progress: assembler.finish(),
                    fault: Some(fault),
                };
            }
        }
        let _ = assembler.note_newline();
    }
    Drive {
        completed,
        progress: assembler.finish(),
        fault: None,
    }
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
    let mut map = Map::new();
    for (name, value) in counts {
        map.insert((*name).to_owned(), json!(value));
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

pub fn upsert<'a>(txn: &'a str, global: u64, collection: u64, table: &'a str, key: &'a str) -> Row<'a> {
    Row {
        txn,
        global,
        collection,
        table,
        key,
        op: "upsert",
        payload: json!([]),
        unavailable: &[],
    }
}
