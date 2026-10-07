#![allow(dead_code)]

use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use cdc_spool::{Limits, Outcome, Spool};

pub const LINEAGE: &str =
    "a123456789abcdefa123456789abcdefa123456789abcdefa123456789abcdef";
pub const LSN: &str = "16/B6E40";
pub const TS: &str = "2026-04-15T08:30:00.123456Z";
pub const POSITION: &str = "18446744168199581248";

pub fn work_dir(name: &str) -> PathBuf {
    let pid = std::process::id();
    let path = std::env::temp_dir().join(format!("oo-a301-{name}-{pid}"));
    fs::create_dir_all(&path).expect("workdir");
    path
}

pub fn seal(
    dir: &Path,
    txn: &str,
    tables: &[&str],
    rows: &[String],
    counts: &[(&str, u64)],
    schema_hash: &str,
    config_hash: &str,
    type_hash: &str,
) -> Outcome {
    let _ = fs::remove_dir_all(dir);
    let mut lines = Vec::new();
    lines.push(bind_line(tables, schema_hash, config_hash, type_hash));
    lines.push(format!(r#"{{"kind":"begin","txn_id":"{txn}"}}"#));
    lines.extend(rows.iter().cloned());
    lines.push(end_line(txn, counts));
    let mut bytes = Vec::new();
    for line in lines {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
    }
    let mut spool = Spool::create_new(dir, Limits::product()).expect("create");
    spool.ingest_reader(std::io::Cursor::new(bytes)).expect("ingest")
}

pub fn seal_feed(dir: &Path, feed: &Path) -> Outcome {
    let _ = fs::remove_dir_all(dir);
    let mut spool = Spool::create_new(dir, Limits::product()).expect("create");
    let file = File::open(feed).expect("feed");
    spool
        .ingest_reader(BufReader::new(file))
        .expect("ingest")
}

pub fn data_line(
    txn: &str,
    global: u64,
    collection: u64,
    table: &str,
    key: &str,
    op: &str,
    payload: &str,
    unavailable: &[u16],
) -> String {
    let missing = unavailable
        .iter()
        .map(|ordinal| ordinal.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"kind":"data","txn_id":"{txn}","global_txn_sequence":{global},"data_collection_sequence":{collection},"table":"{table}","key":[{{"type":"int","ordinal":0,"value":"{key}"}}],"op":"{op}","payload":{payload},"unavailable_ordinals":[{missing}]}}"#
    )
}

fn bind_line(tables: &[&str], schema_hash: &str, config_hash: &str, type_hash: &str) -> String {
    let mut names = tables.to_vec();
    names.sort_unstable();
    let listed = names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"kind":"bind","source":"billing","lineage":"{LINEAGE}","epoch":1,"selected_tables":[{listed}],"schema_hash":"{schema_hash}","config_hash":"{config_hash}","type_hash":"{type_hash}","hook_hashes":[],"commit_position_provenance":"caller_declared"}}"#
    )
}

fn end_line(txn: &str, counts: &[(&str, u64)]) -> String {
    let mut pairs = counts.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    let body = pairs
        .iter()
        .map(|(name, count)| format!("\"{name}\":{count}"))
        .collect::<Vec<_>>()
        .join(",");
    let total: u64 = pairs.iter().map(|(_, count)| *count).sum();
    format!(
        r#"{{"kind":"end","txn_id":"{txn}","count":{total},"per_collection_counts":{{{body}}},"commit_lsn":"{LSN}","source_commit_timestamp":"{TS}"}}"#
    )
}

pub fn write_feed_line(file: &mut File, line: &str) {
    file.write_all(line.as_bytes()).expect("feed line");
    file.write_all(b"\n").expect("feed nl");
}
