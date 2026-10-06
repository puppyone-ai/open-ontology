//! Independent review of the CDC-D01 ordinal repair.
//! Contract ordinals stay 0..=32767. This file does not widen that range.
//! The pinned digest is SHA-256 of the preimage literal below.
//! Empty and "abc" are the FIPS 180-4 vectors.

#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use cdc_transaction::{
    canonical_preimage, event_hash, Assembler, CanonicalData, Cell, CompletedTxn, Fault, FaultCode,
    Op, PendingStatus, Progress,
};
use serde_json::{json, Map, Value};

const HEX64: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TS: &str = "2026-04-15T08:30:01Z";
const TXN: &str = "r298";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const PIN_PREIMAGE: &str = "{\"data_collection_sequence\":1,\"global_txn_sequence\":1,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"4\"}],\"op\":\"upsert\",\"payload\":[{\"ordinal\":32767,\"type\":\"text\",\"value\":\"pin\"}],\"table\":\"public.orders\",\"txn_id\":\"r298\",\"unavailable_ordinals\":[]}";
const PIN_HASH: &str = "bcce7305687a58a07e5ea9416487eb2c15b12d2658dffd7dcd81546c33059440";
const PIN_CHARGE: u64 = 4;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Null,
    Unavailable,
    Bool,
    Int,
    Decimal,
    Text,
    Bytes,
    Key,
}

struct Run {
    completed: Vec<CompletedTxn>,
    fault: Option<Fault>,
    progress: Progress,
}

struct Cli {
    code: i32,
    stdout: String,
    stderr: String,
    lines: Vec<Value>,
}

#[test]
fn fips_and_pinned_preimage() {
    assert_eq!(event_hash("").expect("empty"), EMPTY_SHA);
    assert_eq!(event_hash("abc").expect("abc"), ABC_SHA);
    let key = [Cell::Int {
        ordinal: 0,
        value: "4".to_owned(),
    }];
    let payload = [Cell::Text {
        ordinal: 32_767,
        value: "pin".to_owned(),
    }];
    let preimage = canonical_preimage(&CanonicalData {
        txn_id: TXN,
        global_txn_sequence: 1,
        data_collection_sequence: 1,
        table: "public.orders",
        key: &key,
        op: Op::Upsert,
        payload: &payload,
        unavailable_ordinals: &[],
    })
    .expect("preimage");
    assert_eq!(preimage, PIN_PREIMAGE);
    assert_eq!(event_hash(&preimage).expect("pin"), PIN_HASH);
}

#[test]
fn api_32767_selected_unselected_key_and_cells() {
    for kind in all_kinds() {
        let run = apply_feed(&ended(line_for(kind, 32_767, "public.orders", 1, 1)));
        assert!(run.fault.is_none(), "{kind:?} {:?}", run.fault);
        assert_eq!(run.completed.len(), 1, "{kind:?}");
        let txn = &run.completed[0];
        assert_eq!(txn.event_count, 1);
        assert_eq!(txn.selected_row_count, 1);
        assert_eq!(txn.rows.len(), 1);
        assert_closed(txn, &run.progress);
        match kind {
            Kind::Key => assert_eq!(ordinal_of(&txn.rows[0].key[0]), 32_767),
            Kind::Unavailable => {
                assert_eq!(ordinal_of(&txn.rows[0].payload[0]), 32_767);
                assert_eq!(txn.rows[0].unavailable_ordinals, vec![32_767]);
            }
            _ => assert_eq!(ordinal_of(&txn.rows[0].payload[0]), 32_767),
        }
        if let Kind::Text = kind {
            assert_eq!(txn.rows[0].event_hash, PIN_HASH);
        }
        assert_eq!(run.progress.seen.data, 1);
        assert_eq!(run.progress.complete_decoded.transactions, 1);
        assert!(run.progress.pending.is_empty());
    }

    let omitted = apply_feed(&ended(line_for(Kind::Text, 32_767, "public.notes", 1, 1)));
    assert!(omitted.fault.is_none(), "{:?}", omitted.fault);
    assert_eq!(omitted.completed.len(), 1);
    assert_eq!(omitted.completed[0].event_count, 1);
    assert_eq!(omitted.completed[0].selected_row_count, 0);
    assert!(omitted.completed[0].rows.is_empty());
    assert_closed(&omitted.completed[0], &omitted.progress);
    assert_eq!(omitted.progress.seen.data, 1);
}

#[test]
fn api_32768_and_65535_refuse_before_seen_or_buffer() {
    for ordinal in [32_768_u16, 65_535] {
        for kind in all_kinds() {
            let label = format!("selected {kind:?} {ordinal}");
            assert_range(
                &apply_feed(&ended(line_for(kind, ordinal, "public.orders", 1, 1))),
                &label,
            );
        }
        for kind in [Kind::Text, Kind::Key] {
            let label = format!("unselected {kind:?} {ordinal}");
            assert_range(
                &apply_feed(&ended(line_for(kind, ordinal, "public.notes", 1, 1))),
                &label,
            );
        }
    }
}

#[test]
fn api_range_before_key_order_and_unavailable_mismatch() {
    for ordinal in [32_768_u16, 65_535] {
        assert_range(
            &apply_feed(&ended(unavailable_only(ordinal))),
            &format!("unavailable_ordinals {ordinal}"),
        );
        assert_range(
            &apply_feed(&ended(null_key(ordinal))),
            &format!("null key {ordinal} is range before key_invalid"),
        );
    }
    assert_range(
        &apply_feed(&ended(high_then_low())),
        "32768 then 0 is range before ordinal_order",
    );
    assert_range(
        &apply_feed(&ended(in_range_then_high())),
        "in-range cell then 32768 is range",
    );
}

#[test]
fn api_65536_is_malformed_and_stores_nothing() {
    let run = apply_feed(&opened(raw_65536()));
    let fault = run.fault.as_ref().expect("malformed");
    assert_eq!(fault.code, FaultCode::Malformed);
    assert!(fault.txn_id.is_none());
    assert!(run.completed.is_empty());
    assert_eq!(run.progress.seen.begin, 1);
    assert_eq!(run.progress.seen.data, 0);
    assert_eq!(run.progress.seen.end, 0);
    assert_eq!(run.progress.read.lines, 3);
    assert_eq!(run.progress.complete_decoded.transactions, 0);
    assert_eq!(run.progress.pending[0].data_events, 0);
    assert_eq!(run.progress.pending[0].payload_bytes, 0);
    assert_eq!(run.progress.pending[0].status, PendingStatus::Open);
    assert_eq!(run.progress.fault, Some(FaultCode::Malformed));
    assert!(!run.progress.needs_spool);
    assert!(run.progress.applied.is_none());
}

#[test]
fn api_stored_event_remains_and_fresh_retry_matches_hash() {
    let mut assembler = Assembler::new();
    assembler.apply_line(&bind()).expect("bind");
    assembler.apply_line(&begin()).expect("begin");
    assembler
        .apply_line(&line_for(Kind::Text, 32_767, "public.orders", 1, 1))
        .expect("store");
    let before = assembler.progress();
    assert_eq!(before.pending[0].status, PendingStatus::Open);
    assert_eq!(before.pending[0].data_events, 1);
    assert_eq!(before.pending[0].payload_bytes, PIN_CHARGE);
    assert_eq!(before.seen.data, 1);
    assert!(before.fault.is_none());
    assert_eq!(before.read.lines, 3);

    let refused = assembler
        .apply_line(&line_for(Kind::Text, 32_768, "public.notes", 2, 1))
        .expect_err("range");
    assert_eq!(refused.code, FaultCode::OrdinalRange);
    assert_eq!(refused.txn_id.as_deref(), Some(TXN));
    let after = assembler.progress();
    assert_eq!(after.pending[0].status, PendingStatus::Refused);
    assert_eq!(after.pending[0].data_events, 1);
    assert_eq!(after.pending[0].payload_bytes, PIN_CHARGE);
    assert_eq!(after.seen.data, 1);
    assert_eq!(after.seen.begin, before.seen.begin);
    assert_eq!(after.complete_decoded.transactions, 0);
    assert_eq!(after.read.lines, before.read.lines + 1);
    assert_eq!(after.fault, Some(FaultCode::OrdinalRange));
    assert!(!after.needs_spool);

    let stuck = assembler
        .apply_line(&line_for(Kind::Text, 32_767, "public.orders", 2, 2))
        .expect_err("sticky");
    assert_eq!(stuck.code, FaultCode::OrdinalRange);
    assert_eq!(stuck.txn_id.as_deref(), Some(TXN));
    let still = assembler.progress();
    assert_eq!(still.pending[0].data_events, 1);
    assert_eq!(still.pending[0].payload_bytes, PIN_CHARGE);
    assert_eq!(still.seen.data, 1);
    assert_eq!(still.complete_decoded.transactions, 0);
    assert_eq!(still.read.lines, after.read.lines + 1);
    assert_eq!(still.fault, Some(FaultCode::OrdinalRange));

    let mut hashes = Vec::new();
    for _ in 0..2 {
        let run = apply_feed(&ended(line_for(Kind::Text, 32_767, "public.orders", 1, 1)));
        assert!(run.fault.is_none(), "{:?}", run.fault);
        assert_eq!(run.completed.len(), 1);
        hashes.push(run.completed[0].rows[0].event_hash.clone());
    }
    assert_eq!(hashes, vec![PIN_HASH.to_owned(), PIN_HASH.to_owned()]);
}

#[test]
fn api_existing_malformed_fault_stays() {
    let mut assembler = Assembler::new();
    assembler.apply_line(&bind()).expect("bind");
    assembler.apply_line(&begin()).expect("begin");
    let malformed = assembler.apply_line(&raw_65536()).expect_err("malformed");
    assert_eq!(malformed.code, FaultCode::Malformed);
    assert!(malformed.txn_id.is_none());
    let before = assembler.progress();
    assert_eq!(before.pending[0].status, PendingStatus::Open);
    assert_eq!(before.seen.data, 0);
    assert_eq!(before.fault, Some(FaultCode::Malformed));

    let later = assembler
        .apply_line(&line_for(Kind::Text, 32_768, "public.orders", 1, 1))
        .expect_err("stored fault");
    assert_eq!(later.code, FaultCode::Malformed);
    assert!(later.txn_id.is_none());
    let after = assembler.progress();
    assert_eq!(after.fault, Some(FaultCode::Malformed));
    assert_eq!(after.seen.data, 0);
    assert_eq!(after.pending[0].data_events, 0);
    assert_eq!(after.pending[0].payload_bytes, 0);
    assert_eq!(after.pending[0].status, PendingStatus::Open);
    assert_eq!(after.read.lines, before.read.lines + 1);
    assert_eq!(after.complete_decoded.transactions, 0);
}

#[test]
fn cli_32767_positive_and_high_ordinals_do_not_complete() {
    let kept = spawn_cli(&ended(line_for(Kind::Text, 32_767, "public.orders", 1, 1)));
    assert_eq!(kept.code, 0, "stdout {} stderr {}", kept.stdout, kept.stderr);
    assert!(kept.stderr.is_empty(), "{}", kept.stderr);
    let done = completes(&kept);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["rows"][0]["payload"][0]["ordinal"].as_u64(), Some(32_767));
    assert_eq!(done[0]["rows"][0]["payload"][0]["value"].as_str(), Some("pin"));
    assert_eq!(done[0]["rows"][0]["event_hash"].as_str(), Some(PIN_HASH));
    assert_eq!(done[0]["selected_row_count"].as_u64(), Some(1));
    assert_cli_closed(done[0], last(&kept));
    assert_eq!(last(&kept)["seen"]["data"].as_u64(), Some(1));

    let omitted = spawn_cli(&ended(line_for(Kind::Text, 32_767, "public.notes", 1, 1)));
    assert_eq!(omitted.code, 0, "{}", omitted.stdout);
    let quiet = completes(&omitted);
    assert_eq!(quiet.len(), 1);
    assert_eq!(quiet[0]["event_count"].as_u64(), Some(1));
    assert_eq!(quiet[0]["selected_row_count"].as_u64(), Some(0));
    assert!(quiet[0]["rows"].as_array().expect("rows").is_empty());
    assert_cli_closed(quiet[0], last(&omitted));

    for ordinal in [32_768_u16, 65_535] {
        for kind in all_kinds() {
            assert_cli_range(
                &format!("selected {kind:?} {ordinal}"),
                &ended(line_for(kind, ordinal, "public.orders", 1, 1)),
            );
        }
        for kind in [Kind::Text, Kind::Key] {
            assert_cli_range(
                &format!("unselected {kind:?} {ordinal}"),
                &ended(line_for(kind, ordinal, "public.notes", 1, 1)),
            );
        }
        assert_cli_range(
            &format!("unavailable_ordinals {ordinal}"),
            &ended(unavailable_only(ordinal)),
        );
        assert_cli_range(
            &format!("null key {ordinal}"),
            &ended(null_key(ordinal)),
        );
    }
}

#[test]
fn cli_65536_malformed_and_stored_in_range_remains() {
    let bad = spawn_cli(&opened(raw_65536()));
    assert_eq!(bad.code, 2, "stdout {} stderr {}", bad.stdout, bad.stderr);
    assert!(bad.stderr.is_empty(), "{}", bad.stderr);
    assert!(completes(&bad).is_empty());
    assert_eq!(bad.lines[0]["fault"].as_str(), Some("malformed"));
    assert!(bad.lines[0]["txn_id"].is_null());
    let progress = last(&bad);
    assert_eq!(progress["seen"]["data"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["data_events"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["payload_bytes"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["status"].as_str(), Some("open"));
    assert_eq!(progress["complete_decoded"]["transactions"].as_u64(), Some(0));
    assert!(progress["applied"].is_null());

    let mut lines = opened(line_for(Kind::Text, 32_767, "public.orders", 1, 1));
    lines.push(line_for(Kind::Int, 65_535, "public.notes", 2, 1));
    let kept = spawn_cli(&lines);
    assert_eq!(kept.code, 2, "{}", kept.stdout);
    assert!(completes(&kept).is_empty());
    assert_eq!(kept.lines[0]["fault"].as_str(), Some("ordinal_range"));
    assert_eq!(kept.lines[0]["txn_id"].as_str(), Some(TXN));
    let pending = last(&kept);
    assert_eq!(pending["seen"]["data"].as_u64(), Some(1));
    assert_eq!(pending["pending"][0]["data_events"].as_u64(), Some(1));
    assert_eq!(pending["pending"][0]["payload_bytes"].as_u64(), Some(PIN_CHARGE));
    assert_eq!(pending["pending"][0]["status"].as_str(), Some("refused"));
    assert_eq!(pending["complete_decoded"]["transactions"].as_u64(), Some(0));
    assert!(pending["applied"].is_null());
    assert_eq!(pending["needs_spool"].as_bool(), Some(false));
}

fn all_kinds() -> [Kind; 8] {
    [
        Kind::Null,
        Kind::Unavailable,
        Kind::Bool,
        Kind::Int,
        Kind::Decimal,
        Kind::Text,
        Kind::Bytes,
        Kind::Key,
    ]
}

fn apply_feed(lines: &[String]) -> Run {
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
                return Run {
                    completed,
                    fault: Some(fault),
                    progress: assembler.progress(),
                };
            }
        }
    }
    Run {
        completed,
        fault: None,
        progress: assembler.finish(),
    }
}

fn assert_range(run: &Run, label: &str) {
    let Some(fault) = run.fault.as_ref() else {
        panic!("{label}: no fault, completes {}", run.completed.len());
    };
    assert_eq!(fault.code, FaultCode::OrdinalRange, "{label}");
    assert_eq!(fault.txn_id.as_deref(), Some(TXN), "{label}");
    assert!(run.completed.is_empty(), "{label}");
    assert_eq!(run.progress.seen.bind, 1, "{label}");
    assert_eq!(run.progress.seen.begin, 1, "{label}");
    assert_eq!(run.progress.seen.data, 0, "{label}");
    assert_eq!(run.progress.seen.end, 0, "{label}");
    assert_eq!(run.progress.read.lines, 3, "{label}");
    assert_eq!(run.progress.complete_decoded.transactions, 0, "{label}");
    assert_eq!(run.progress.pending.len(), 1, "{label}");
    assert_eq!(run.progress.pending[0].txn_id, TXN, "{label}");
    assert_eq!(run.progress.pending[0].data_events, 0, "{label}");
    assert_eq!(run.progress.pending[0].payload_bytes, 0, "{label}");
    assert_eq!(run.progress.pending[0].status, PendingStatus::Refused, "{label}");
    assert_eq!(run.progress.fault, Some(FaultCode::OrdinalRange), "{label}");
    assert!(run.progress.resume_required, "{label}");
    assert!(!run.progress.needs_spool, "{label}");
    assert!(run.progress.applied.is_none(), "{label}");
    assert!(!run.progress.ack_allowed, "{label}");
    assert!(!run.progress.target_applied, "{label}");
    assert!(!run.progress.debezium_normalizer, "{label}");
}

fn assert_closed(txn: &CompletedTxn, progress: &Progress) {
    let flags = &txn.attestations;
    assert!(!flags.upstream_commit_lsn_verified);
    assert!(!flags.cross_table_consistent);
    assert!(!flags.target_applied);
    assert!(!flags.ack_allowed);
    assert!(!flags.debezium_normalizer);
    assert!(!flags.p2_verified);
    assert!(!flags.p11_verified);
    assert_eq!(txn.commit_position_provenance, "caller_declared");
    assert!(progress.applied.is_none());
    assert!(!progress.ack_allowed);
    assert!(!progress.target_applied);
    assert!(!progress.debezium_normalizer);
    assert!(progress.fault.is_none());
}

fn ordinal_of(cell: &Cell) -> u16 {
    match cell {
        Cell::Null { ordinal }
        | Cell::Unavailable { ordinal }
        | Cell::Bool { ordinal, .. }
        | Cell::Int { ordinal, .. }
        | Cell::Decimal { ordinal, .. }
        | Cell::Text { ordinal, .. }
        | Cell::Bytes { ordinal, .. } => *ordinal,
    }
}

fn bind() -> String {
    json!({
        "kind": "bind",
        "source": "billing",
        "lineage": HEX64,
        "epoch": 1,
        "selected_tables": ["public.orders"],
        "schema_hash": HEX64,
        "config_hash": HEX64,
        "type_hash": HEX64,
        "hook_hashes": [],
        "commit_position_provenance": "caller_declared"
    })
    .to_string()
}

fn begin() -> String {
    json!({"kind": "begin", "txn_id": TXN}).to_string()
}

fn finish(table: &str) -> String {
    let mut counts = Map::new();
    counts.insert(table.to_owned(), json!(1));
    json!({
        "kind": "end",
        "txn_id": TXN,
        "count": 1,
        "per_collection_counts": counts,
        "commit_lsn": "0/21",
        "source_commit_timestamp": TS
    })
    .to_string()
}

fn opened(line: String) -> Vec<String> {
    vec![bind(), begin(), line]
}

fn ended(line: String) -> Vec<String> {
    let table = table_of(&line);
    let mut lines = opened(line);
    lines.push(finish(&table));
    lines
}

fn table_of(line: &str) -> String {
    let value: Value = serde_json::from_str(line).expect("data line");
    value["table"].as_str().expect("table").to_owned()
}

fn line_for(kind: Kind, ordinal: u16, table: &str, global: u64, collection: u64) -> String {
    let (key_ordinal, payload, unavailable): (u16, Value, Vec<u16>) = match kind {
        Kind::Null => (0, json!([{"type": "null", "ordinal": ordinal}]), Vec::new()),
        Kind::Unavailable => (
            0,
            json!([{"type": "unavailable", "ordinal": ordinal}]),
            vec![ordinal],
        ),
        Kind::Bool => (
            0,
            json!([{"type": "bool", "ordinal": ordinal, "value": true}]),
            Vec::new(),
        ),
        Kind::Int => (
            0,
            json!([{"type": "int", "ordinal": ordinal, "value": "11"}]),
            Vec::new(),
        ),
        Kind::Decimal => (
            0,
            json!([{"type": "decimal", "ordinal": ordinal, "value": "8.5"}]),
            Vec::new(),
        ),
        Kind::Text => (
            0,
            json!([{"type": "text", "ordinal": ordinal, "value": "pin"}]),
            Vec::new(),
        ),
        Kind::Bytes => (
            0,
            json!([{"type": "bytes", "ordinal": ordinal, "value": "0a"}]),
            Vec::new(),
        ),
        Kind::Key => (ordinal, json!([]), Vec::new()),
    };
    json!({
        "kind": "data",
        "txn_id": TXN,
        "global_txn_sequence": global,
        "data_collection_sequence": collection,
        "table": table,
        "key": [{"type": "int", "ordinal": key_ordinal, "value": "4"}],
        "op": "upsert",
        "payload": payload,
        "unavailable_ordinals": unavailable
    })
    .to_string()
}

fn unavailable_only(ordinal: u16) -> String {
    json!({
        "kind": "data",
        "txn_id": TXN,
        "global_txn_sequence": 1,
        "data_collection_sequence": 1,
        "table": "public.orders",
        "key": [{"type": "int", "ordinal": 0, "value": "4"}],
        "op": "upsert",
        "payload": [{"type": "text", "ordinal": 0, "value": "a"}],
        "unavailable_ordinals": [ordinal]
    })
    .to_string()
}

fn null_key(ordinal: u16) -> String {
    json!({
        "kind": "data",
        "txn_id": TXN,
        "global_txn_sequence": 1,
        "data_collection_sequence": 1,
        "table": "public.orders",
        "key": [{"type": "null", "ordinal": ordinal}],
        "op": "upsert",
        "payload": [{"type": "text", "ordinal": 0, "value": "a"}],
        "unavailable_ordinals": []
    })
    .to_string()
}

fn high_then_low() -> String {
    json!({
        "kind": "data",
        "txn_id": TXN,
        "global_txn_sequence": 1,
        "data_collection_sequence": 1,
        "table": "public.orders",
        "key": [{"type": "int", "ordinal": 0, "value": "4"}],
        "op": "upsert",
        "payload": [
            {"type": "text", "ordinal": 32768, "value": "a"},
            {"type": "null", "ordinal": 0}
        ],
        "unavailable_ordinals": []
    })
    .to_string()
}

fn in_range_then_high() -> String {
    json!({
        "kind": "data",
        "txn_id": TXN,
        "global_txn_sequence": 1,
        "data_collection_sequence": 1,
        "table": "public.orders",
        "key": [{"type": "int", "ordinal": 0, "value": "4"}],
        "op": "upsert",
        "payload": [
            {"type": "text", "ordinal": 0, "value": "a"},
            {"type": "bool", "ordinal": 65535, "value": false}
        ],
        "unavailable_ordinals": []
    })
    .to_string()
}

fn raw_65536() -> String {
    "{\"kind\":\"data\",\"txn_id\":\"r298\",\"global_txn_sequence\":1,\"data_collection_sequence\":1,\"table\":\"public.orders\",\"key\":[{\"type\":\"int\",\"ordinal\":0,\"value\":\"4\"}],\"op\":\"upsert\",\"payload\":[{\"type\":\"text\",\"ordinal\":65536,\"value\":\"x\"}],\"unavailable_ordinals\":[]}".to_owned()
}

fn spawn_cli(lines: &[String]) -> Cli {
    let mut body = String::new();
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    let mut child = Command::new(executable())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body.as_bytes()).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().expect("status");
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    let parsed = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect();
    Cli {
        code,
        stdout,
        stderr,
        lines: parsed,
    }
}

fn assert_cli_range(label: &str, lines: &[String]) {
    let cli = spawn_cli(lines);
    let fault = cli.lines.first().and_then(|line| line["fault"].as_str());
    let stored = completes(&cli).len();
    let progress = cli.lines.last();
    let pending = progress.and_then(|line| line["pending"][0]["data_events"].as_u64());
    let payload = progress.and_then(|line| line["pending"][0]["payload_bytes"].as_u64());
    let seen = progress.and_then(|line| line["seen"]["data"].as_u64());
    let status = progress.and_then(|line| line["pending"][0]["status"].as_str());
    let read = progress.and_then(|line| line["read"]["lines"].as_u64());
    assert!(
        cli.code == 2
            && cli.stderr.is_empty()
            && fault == Some("ordinal_range")
            && stored == 0
            && pending == Some(0)
            && payload == Some(0)
            && seen == Some(0)
            && status == Some("refused")
            && read == Some(3),
        "{label}: code {} fault {fault:?} completes {stored} pending {pending:?} payload {payload:?} seen {seen:?} status {status:?} read {read:?} stderr {} stdout {}",
        cli.code,
        cli.stderr,
        cli.stdout
    );
}

fn assert_cli_closed(txn: &Value, progress: &Value) {
    for name in [
        "upstream_commit_lsn_verified",
        "cross_table_consistent",
        "target_applied",
        "ack_allowed",
        "debezium_normalizer",
        "p2_verified",
        "p11_verified",
    ] {
        assert_eq!(txn["attestations"][name].as_bool(), Some(false), "{name}");
        assert_eq!(progress[name].as_bool(), Some(false), "{name}");
    }
    assert_eq!(txn["commit_position_provenance"].as_str(), Some("caller_declared"));
    assert!(progress["applied"].is_null());
    assert!(progress["fault"].is_null());
}

fn completes(cli: &Cli) -> Vec<&Value> {
    cli.lines
        .iter()
        .filter(|line| line["kind"] == "complete_transaction")
        .collect()
}

fn last(cli: &Cli) -> &Value {
    cli.lines.last().expect("progress")
}

fn executable() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc-transaction") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc_transaction") {
        return path;
    }
    panic!("missing cdc-transaction executable path");
}
