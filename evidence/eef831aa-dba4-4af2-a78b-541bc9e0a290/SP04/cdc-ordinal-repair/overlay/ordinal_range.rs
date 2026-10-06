//! Contract ordinals are 0..=32767. u16 also stores 32768..=65535.
//! 65536 never reaches the assembler: serde rejects it as malformed.
//! A refusal sets the assembler fault. A later line on that assembler
//! does not recover. A fresh assembler can still accept the valid event.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use cdc_transaction::{
    canonical_preimage, event_hash, Assembler, CanonicalData, Cell, CompletedTxn, Fault,
    FaultCode, Op, PendingStatus, Progress,
};
use common::{begin, bind, end, TS};
use serde_json::{json, Value};

const TEXT_PREIMAGE: &str = "{\"data_collection_sequence\":1,\"global_txn_sequence\":1,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"4\"}],\"op\":\"upsert\",\"payload\":[{\"ordinal\":32767,\"type\":\"text\",\"value\":\"ok\"}],\"table\":\"public.orders\",\"txn_id\":\"ord\",\"unavailable_ordinals\":[]}";
const TEXT_HASH: &str = "ac9bbf8c7334f138906f25c32c151aef7c63e488263fcc683969b9d9f75e3592";

#[derive(Clone, Copy)]
enum Shape {
    Null,
    Unavailable,
    Bool,
    Text(&'static str),
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
fn ordinal_32767_completes_for_each_in_range_shape() {
    let key = vec![Cell::Int {
        ordinal: 0,
        value: "4".to_owned(),
    }];
    let payload = vec![Cell::Text {
        ordinal: 32767,
        value: "ok".to_owned(),
    }];
    let preimage = canonical_preimage(&CanonicalData {
        txn_id: "ord",
        global_txn_sequence: 1,
        data_collection_sequence: 1,
        table: "public.orders",
        key: &key,
        op: Op::Upsert,
        payload: &payload,
        unavailable_ordinals: &[],
    })
    .expect("preimage");
    assert_eq!(preimage, TEXT_PREIMAGE);
    assert_eq!(event_hash(&preimage).expect("hash"), TEXT_HASH);

    for shape in [
        Shape::Null,
        Shape::Unavailable,
        Shape::Bool,
        Shape::Text("ok"),
        Shape::Key,
    ] {
        let run = run_lines(&feed("public.orders", shape, 32767));
        assert!(
            run.fault.is_none(),
            "32767 {} stored a fault {:?}",
            shape_name(shape),
            run.fault.as_ref().map(|fault| fault.code)
        );
        assert_eq!(run.completed.len(), 1, "{}", shape_name(shape));
        let txn = &run.completed[0];
        assert_eq!(txn.event_count, 1);
        assert_eq!(txn.selected_row_count, 1);
        assert_eq!(txn.rows.len(), 1);
        assert_flags(txn);
        match shape {
            Shape::Key => assert_eq!(cell_ordinal(&txn.rows[0].key[0]), 32767),
            Shape::Unavailable => {
                assert_eq!(cell_ordinal(&txn.rows[0].payload[0]), 32767);
                assert_eq!(txn.rows[0].unavailable_ordinals, vec![32767]);
            }
            _ => assert_eq!(cell_ordinal(&txn.rows[0].payload[0]), 32767),
        }
        if let Shape::Text("ok") = shape {
            assert_eq!(txn.rows[0].event_hash, TEXT_HASH);
        }
        assert_eq!(run.progress.seen.data, 1);
        assert_eq!(run.progress.complete_decoded.transactions, 1);
        assert!(run.progress.pending.is_empty());
        assert!(run.progress.fault.is_none());
        assert_progress_closed(&run.progress);
    }
}

#[test]
fn ordinals_above_32767_refuse_without_storing() {
    for ordinal in [32768_u16, 65535] {
        for shape in [
            Shape::Null,
            Shape::Unavailable,
            Shape::Bool,
            Shape::Text("zzzz"),
            Shape::Key,
        ] {
            let label = format!("{} {ordinal} selected", shape_name(shape));
            assert_range(&run_lines(&feed("public.orders", shape, ordinal)), &label);
        }
    }
    assert_range(
        &run_lines(&ended(later_high_cell())),
        "later payload cell 32768",
    );
    assert_range(
        &run_lines(&ended(high_then_low_cell())),
        "ordinal 32768 before 0 is range, not order",
    );
}

#[test]
fn unavailable_list_above_32767_is_ordinal_range() {
    for ordinal in [32768_u16, 65535] {
        let line = json!({
            "kind": "data",
            "txn_id": "ord",
            "global_txn_sequence": 1,
            "data_collection_sequence": 1,
            "table": "public.orders",
            "key": [{"type": "int", "ordinal": 0, "value": "4"}],
            "op": "upsert",
            "payload": [{"type": "text", "ordinal": 0, "value": "a"}],
            "unavailable_ordinals": [ordinal]
        })
        .to_string();
        assert_range(
            &run_lines(&ended(line)),
            &format!("unavailable_ordinals {ordinal}"),
        );
    }
}

#[test]
fn unselected_high_ordinal_refuses_and_in_range_is_omitted() {
    let kept = run_lines(&feed("public.notes", Shape::Text("ok"), 32767));
    assert!(kept.fault.is_none(), "{:?}", kept.fault);
    assert_eq!(kept.completed.len(), 1);
    assert_eq!(kept.completed[0].event_count, 1);
    assert_eq!(kept.completed[0].selected_row_count, 0);
    assert!(kept.completed[0].rows.is_empty());
    assert_flags(&kept.completed[0]);
    assert_eq!(kept.progress.seen.data, 1);
    assert_progress_closed(&kept.progress);

    assert_range(
        &run_lines(&feed("public.notes", Shape::Text("zzzz"), 32768)),
        "unselected 32768",
    );

    let mixed = run_lines(&[
        bind(&["public.orders"]),
        begin("ord"),
        event("public.orders", 1, 1, Shape::Text("a"), 0),
        event("public.notes", 2, 1, Shape::Text("zzzz"), 32768),
    ]);
    let Some(fault) = mixed.fault.as_ref() else {
        let pending = mixed.progress.pending.first();
        panic!(
            "unselected 32768 mutated the open transaction data_events {} payload {}",
            pending.map(|txn| txn.data_events).unwrap_or(u64::MAX),
            pending.map(|txn| txn.payload_bytes).unwrap_or(u64::MAX)
        );
    };
    assert_eq!(fault.code, FaultCode::OrdinalRange);
    assert_eq!(fault.txn_id.as_deref(), Some("ord"));
    assert!(mixed.completed.is_empty());
    assert_eq!(mixed.progress.pending[0].data_events, 1);
    assert_eq!(mixed.progress.pending[0].payload_bytes, 2);
    assert_eq!(mixed.progress.pending[0].status, PendingStatus::Refused);
    assert_eq!(mixed.progress.seen.data, 1);
    assert_eq!(mixed.progress.complete_decoded.transactions, 0);
    assert_progress_closed(&mixed.progress);
}

#[test]
fn ordinal_65536_is_malformed_and_stores_nothing() {
    let run = run_lines(&with_data(raw_65536()));
    let fault = run.fault.as_ref().expect("malformed");
    assert_eq!(fault.code, FaultCode::Malformed);
    assert!(fault.txn_id.is_none());
    assert!(run.completed.is_empty());
    assert_eq!(run.progress.seen.begin, 1);
    assert_eq!(run.progress.seen.data, 0);
    assert_eq!(run.progress.complete_decoded.transactions, 0);
    assert_eq!(run.progress.pending[0].data_events, 0);
    assert_eq!(run.progress.pending[0].payload_bytes, 0);
    assert_eq!(run.progress.pending[0].status, PendingStatus::Open);
    assert_progress_closed(&run.progress);
}

#[test]
fn stored_event_survives_refusal_and_fresh_retry_keeps_the_hash() {
    let mut assembler = Assembler::new();
    assembler
        .apply_line(&bind(&["public.orders"]))
        .expect("bind");
    assembler.apply_line(&begin("ord")).expect("begin");
    assembler
        .apply_line(&event("public.orders", 1, 1, Shape::Text("a"), 0))
        .expect("stored");
    let before = assembler.progress();
    assert_eq!(before.pending[0].status, PendingStatus::Open);
    assert_eq!(before.pending[0].data_events, 1);
    assert_eq!(before.pending[0].payload_bytes, 2);
    assert_eq!(before.seen.data, 1);

    let err = assembler
        .apply_line(&event("public.orders", 2, 2, Shape::Text("ok"), 32768))
        .expect_err("range");
    assert_eq!(err.code, FaultCode::OrdinalRange);
    assert_eq!(err.txn_id.as_deref(), Some("ord"));
    let after = assembler.progress();
    assert_eq!(after.pending[0].status, PendingStatus::Refused);
    assert_eq!(after.pending[0].data_events, before.pending[0].data_events);
    assert_eq!(after.pending[0].payload_bytes, before.pending[0].payload_bytes);
    assert_eq!(after.seen.data, before.seen.data);
    assert_eq!(after.seen.begin, before.seen.begin);
    assert_eq!(after.complete_decoded.transactions, 0);
    assert_eq!(after.read.lines, before.read.lines + 1);
    assert_eq!(after.fault, Some(FaultCode::OrdinalRange));

    let again = assembler
        .apply_line(&event("public.orders", 2, 2, Shape::Text("ok"), 32767))
        .expect_err("terminal");
    assert_eq!(again.code, FaultCode::OrdinalRange);
    assert_eq!(again.txn_id.as_deref(), Some("ord"));
    let stuck = assembler.progress();
    assert_eq!(stuck.pending[0].data_events, after.pending[0].data_events);
    assert_eq!(stuck.pending[0].payload_bytes, after.pending[0].payload_bytes);
    assert_eq!(stuck.seen.data, after.seen.data);
    assert_eq!(stuck.complete_decoded.transactions, 0);
    assert_eq!(stuck.read.lines, after.read.lines + 1);
    assert_eq!(stuck.fault, Some(FaultCode::OrdinalRange));

    let first = run_lines(&feed("public.orders", Shape::Text("ok"), 32767));
    let second = run_lines(&feed("public.orders", Shape::Text("ok"), 32767));
    assert!(first.fault.is_none() && second.fault.is_none());
    assert_eq!(first.completed[0].rows[0].event_hash, TEXT_HASH);
    assert_eq!(second.completed[0].rows[0].event_hash, TEXT_HASH);
}

#[test]
fn cli_ordinal_32767_exits_zero() {
    let cli = run_cli(&feed("public.orders", Shape::Text("ok"), 32767));
    assert_eq!(cli.code, 0, "stdout {} stderr {}", cli.stdout, cli.stderr);
    let done = completes(&cli);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["event_count"].as_u64(), Some(1));
    assert_eq!(done[0]["selected_row_count"].as_u64(), Some(1));
    assert_eq!(done[0]["rows"][0]["payload"][0]["ordinal"].as_u64(), Some(32767));
    assert_eq!(done[0]["rows"][0]["event_hash"].as_str(), Some(TEXT_HASH));
    assert_cli_flags(done[0]);
    let progress = last(&cli);
    assert!(progress["fault"].is_null());
    assert_eq!(progress["seen"]["data"].as_u64(), Some(1));
    assert_eq!(progress["complete_decoded"]["transactions"].as_u64(), Some(1));
    assert_cli_unapplied(progress);
}

#[test]
fn cli_high_ordinals_exit_ordinal_range_with_no_complete() {
    let mut problems = Vec::new();
    for ordinal in [32768_u16, 65535] {
        for shape in [
            Shape::Null,
            Shape::Unavailable,
            Shape::Bool,
            Shape::Text("zzzz"),
            Shape::Key,
        ] {
            let cli = run_cli(&feed("public.orders", shape, ordinal));
            let fault = cli.lines.first().and_then(|line| line["fault"].as_str());
            let stored = completes(&cli).len();
            let pending = cli
                .lines
                .last()
                .and_then(|line| line["pending"][0]["data_events"].as_u64());
            let payload = cli
                .lines
                .last()
                .and_then(|line| line["pending"][0]["payload_bytes"].as_u64());
            let seen = cli
                .lines
                .last()
                .and_then(|line| line["seen"]["data"].as_u64());
            if cli.code != 2 || fault != Some("ordinal_range") || stored != 0 || pending != Some(0)
                || payload != Some(0) || seen != Some(0)
            {
                problems.push(format!(
                    "{} {ordinal} code {} fault {fault:?} completes {stored} pending {pending:?} payload {payload:?} seen {seen:?} stderr {}",
                    shape_name(shape),
                    cli.code,
                    cli.stderr
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "contract ordinals are 0..=32767; {}",
        problems.join("; ")
    );
}

#[test]
fn cli_ordinal_65536_is_malformed() {
    let cli = run_cli(&with_data(raw_65536()));
    assert_eq!(cli.code, 2, "stdout {} stderr {}", cli.stdout, cli.stderr);
    assert!(completes(&cli).is_empty());
    assert_eq!(cli.lines[0]["fault"].as_str(), Some("malformed"));
    assert!(cli.lines[0]["txn_id"].is_null());
    let progress = last(&cli);
    assert_eq!(progress["seen"]["data"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["data_events"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["payload_bytes"].as_u64(), Some(0));
    assert_eq!(progress["complete_decoded"]["transactions"].as_u64(), Some(0));
    assert_cli_unapplied(progress);
}

#[test]
fn cli_unselected_and_shapes_match_the_bound() {
    let kept = run_cli(&feed("public.notes", Shape::Text("ok"), 32767));
    assert_eq!(kept.code, 0, "stdout {} stderr {}", kept.stdout, kept.stderr);
    let done = completes(&kept);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["event_count"].as_u64(), Some(1));
    assert_eq!(done[0]["selected_row_count"].as_u64(), Some(0));
    assert!(done[0]["rows"].as_array().expect("rows").is_empty());
    assert_cli_flags(done[0]);
    assert_cli_unapplied(last(&kept));

    let refused = run_cli(&feed("public.notes", Shape::Text("zzzz"), 32768));
    assert_eq!(refused.code, 2, "stdout {} stderr {}", refused.stdout, refused.stderr);
    assert!(completes(&refused).is_empty());
    assert_eq!(refused.lines[0]["fault"].as_str(), Some("ordinal_range"));
    assert_eq!(refused.lines[0]["txn_id"].as_str(), Some("ord"));
    let progress = last(&refused);
    assert_eq!(progress["seen"]["data"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["data_events"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["payload_bytes"].as_u64(), Some(0));
    assert_eq!(progress["pending"][0]["status"].as_str(), Some("refused"));
    assert_eq!(progress["complete_decoded"]["transactions"].as_u64(), Some(0));
    assert_cli_unapplied(progress);
}

fn run_lines(lines: &[String]) -> Run {
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

fn feed(table: &str, shape: Shape, ordinal: u16) -> Vec<String> {
    with_data(event(table, 1, 1, shape, ordinal))
        .into_iter()
        .chain(std::iter::once(end("ord", 1, &[(table, 1)], "0/5", TS)))
        .collect()
}

fn with_data(line: String) -> Vec<String> {
    vec![bind(&["public.orders"]), begin("ord"), line]
}

fn ended(line: String) -> Vec<String> {
    let mut lines = with_data(line);
    lines.push(end("ord", 1, &[("public.orders", 1)], "0/5", TS));
    lines
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Null => "null",
        Shape::Unavailable => "unavailable",
        Shape::Bool => "bool",
        Shape::Text(_) => "text",
        Shape::Key => "key",
    }
}

fn event(table: &str, seq: u64, collection: u64, shape: Shape, ordinal: u16) -> String {
    let (key_ordinal, payload, unavailable): (u16, Value, Vec<u16>) = match shape {
        Shape::Null => (0, json!([{"type": "null", "ordinal": ordinal}]), Vec::new()),
        Shape::Unavailable => (
            0,
            json!([{"type": "unavailable", "ordinal": ordinal}]),
            vec![ordinal],
        ),
        Shape::Bool => (
            0,
            json!([{"type": "bool", "ordinal": ordinal, "value": true}]),
            Vec::new(),
        ),
        Shape::Text(text) => (
            0,
            json!([{"type": "text", "ordinal": ordinal, "value": text}]),
            Vec::new(),
        ),
        Shape::Key => (ordinal, json!([]), Vec::new()),
    };
    json!({
        "kind": "data",
        "txn_id": "ord",
        "global_txn_sequence": seq,
        "data_collection_sequence": collection,
        "table": table,
        "key": [{"type": "int", "ordinal": key_ordinal, "value": "4"}],
        "op": "upsert",
        "payload": payload,
        "unavailable_ordinals": unavailable
    })
    .to_string()
}

fn later_high_cell() -> String {
    json!({
        "kind": "data",
        "txn_id": "ord",
        "global_txn_sequence": 1,
        "data_collection_sequence": 1,
        "table": "public.orders",
        "key": [{"type": "int", "ordinal": 0, "value": "4"}],
        "op": "upsert",
        "payload": [
            {"type": "text", "ordinal": 0, "value": "a"},
            {"type": "null", "ordinal": 32768}
        ],
        "unavailable_ordinals": []
    })
    .to_string()
}

fn high_then_low_cell() -> String {
    json!({
        "kind": "data",
        "txn_id": "ord",
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

fn raw_65536() -> String {
    "{\"kind\":\"data\",\"txn_id\":\"ord\",\"global_txn_sequence\":1,\"data_collection_sequence\":1,\"table\":\"public.orders\",\"key\":[{\"type\":\"int\",\"ordinal\":0,\"value\":\"4\"}],\"op\":\"upsert\",\"payload\":[{\"type\":\"unavailable\",\"ordinal\":65536}],\"unavailable_ordinals\":[65536]}".to_owned()
}

fn assert_range(run: &Run, label: &str) {
    let Some(fault) = run.fault.as_ref() else {
        let pending = run.progress.pending.first();
        panic!(
            "{label}: expected ordinal_range, completed {} data_events {} payload {}",
            run.completed.len(),
            pending.map(|txn| txn.data_events).unwrap_or(u64::MAX),
            pending.map(|txn| txn.payload_bytes).unwrap_or(u64::MAX)
        );
    };
    assert_eq!(fault.code, FaultCode::OrdinalRange, "{label}");
    assert_eq!(fault.txn_id.as_deref(), Some("ord"), "{label}");
    assert!(run.completed.is_empty(), "{label}");
    assert_eq!(run.progress.seen.begin, 1, "{label}");
    assert_eq!(run.progress.seen.data, 0, "{label}");
    assert_eq!(run.progress.seen.end, 0, "{label}");
    assert_eq!(run.progress.complete_decoded.transactions, 0, "{label}");
    assert_eq!(run.progress.pending.len(), 1, "{label}");
    assert_eq!(run.progress.pending[0].data_events, 0, "{label}");
    assert_eq!(run.progress.pending[0].payload_bytes, 0, "{label}");
    assert_eq!(run.progress.pending[0].status, PendingStatus::Refused, "{label}");
    assert_eq!(run.progress.fault, Some(FaultCode::OrdinalRange), "{label}");
    assert!(!run.progress.needs_spool, "{label}");
    assert!(run.progress.resume_required, "{label}");
    assert_progress_closed(&run.progress);
}

fn assert_flags(txn: &CompletedTxn) {
    let flags = &txn.attestations;
    assert!(!flags.upstream_commit_lsn_verified);
    assert!(!flags.cross_table_consistent);
    assert!(!flags.target_applied);
    assert!(!flags.ack_allowed);
    assert!(!flags.debezium_normalizer);
    assert!(!flags.p2_verified);
    assert!(!flags.p11_verified);
    assert_eq!(txn.commit_position_provenance, "caller_declared");
}

fn assert_progress_closed(progress: &Progress) {
    assert!(progress.applied.is_none());
    assert!(!progress.ack_allowed);
    assert!(!progress.target_applied);
    assert!(!progress.cross_table_consistent);
    assert!(!progress.upstream_commit_lsn_verified);
    assert!(!progress.debezium_normalizer);
    assert!(!progress.p2_verified);
    assert!(!progress.p11_verified);
    assert_eq!(progress.commit_position_provenance, Some("caller_declared"));
}

fn cell_ordinal(cell: &Cell) -> u64 {
    serde_json::to_value(cell).expect("cell")["ordinal"]
        .as_u64()
        .expect("ordinal")
}

fn run_cli(lines: &[String]) -> Cli {
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
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
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

fn executable() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc-transaction") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc_transaction") {
        return path;
    }
    panic!("missing cdc-transaction executable path");
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

fn assert_cli_flags(txn: &Value) {
    let flags = &txn["attestations"];
    for name in [
        "upstream_commit_lsn_verified",
        "cross_table_consistent",
        "target_applied",
        "ack_allowed",
        "debezium_normalizer",
        "p2_verified",
        "p11_verified",
    ] {
        assert_eq!(flags[name].as_bool(), Some(false), "{name}");
    }
    assert_eq!(txn["commit_position_provenance"].as_str(), Some("caller_declared"));
}

fn assert_cli_unapplied(progress: &Value) {
    assert!(progress["applied"].is_null());
    for name in [
        "ack_allowed",
        "target_applied",
        "cross_table_consistent",
        "upstream_commit_lsn_verified",
        "debezium_normalizer",
        "p2_verified",
        "p11_verified",
    ] {
        assert_eq!(progress[name].as_bool(), Some(false), "{name}");
    }
}
