//! Independent CLI review of cdc-transaction. Not an author test.
//! Value-byte figures are summed key and payload lengths compared with
//! `pending.payload_bytes`. This file does not read process RSS.

use std::io::Write;
use std::process::{Command, Stdio};

use cdc_transaction::{encode_position, event_hash, Cell};
use serde_json::{json, Map, Value};

const TS: &str = "2024-11-02T03:04:05.5Z";
const TS_WHOLE: &str = "2024-11-02T03:04:06Z";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const MINT_PREIMAGE: &str = "{\"data_collection_sequence\":1,\"global_txn_sequence\":1,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"4\"}],\"op\":\"upsert\",\"payload\":[{\"ordinal\":0,\"type\":\"text\",\"value\":\"mint\"}],\"table\":\"public.tickets\",\"txn_id\":\"west\",\"unavailable_ordinals\":[]}";

struct Out {
    code: i32,
    stdout: String,
    lines: Vec<Value>,
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

fn run(input: &[u8]) -> Out {
    let mut child = Command::new(executable())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().expect("status");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    let lines = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect();
    if code == 0 {
        assert!(stderr.is_empty(), "{stderr}");
    }
    Out { code, stdout, lines }
}

fn feed(parts: &[String]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(part.as_bytes());
        body.push(b'\n');
    }
    body
}

fn hex64(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn bind(tables: &[&str]) -> String {
    let lineage = hex64(0x11);
    let digest = hex64(0xab);
    json!({
        "kind": "bind",
        "source": "ledger",
        "lineage": lineage,
        "epoch": 1,
        "selected_tables": tables,
        "schema_hash": digest,
        "config_hash": digest,
        "type_hash": digest,
        "hook_hashes": [],
        "commit_position_provenance": "caller_declared"
    })
    .to_string()
}

fn begin(txn: &str) -> String {
    json!({"kind": "begin", "txn_id": txn}).to_string()
}

fn rollback(txn: &str) -> String {
    json!({"kind": "rollback", "txn_id": txn}).to_string()
}

fn end(txn: &str, count: u64, counts: &[(&str, u64)], lsn: &str, timestamp: &str) -> String {
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

fn data_row(
    txn: &str,
    global: u64,
    collection: u64,
    table: &str,
    key: &str,
    payload: Value,
) -> String {
    json!({
        "kind": "data",
        "txn_id": txn,
        "global_txn_sequence": global,
        "data_collection_sequence": collection,
        "table": table,
        "key": [{"type": "int", "ordinal": 0, "value": key}],
        "op": "upsert",
        "payload": payload,
        "unavailable_ordinals": []
    })
    .to_string()
}

fn position(epoch: u32, lsn: u64) -> String {
    let text = encode_position(epoch, lsn).expect("position");
    let numeric = (u128::from(epoch) << 64) + u128::from(lsn);
    assert_eq!(text, numeric.to_string(), "epoch {epoch} lsn {lsn}");
    text
}

fn last(out: &Out) -> &Value {
    out.lines.last().expect("progress")
}

fn assert_unapplied(progress: &Value) {
    assert!(progress["applied"].is_null(), "{progress}");
    for key in [
        "ack_allowed",
        "target_applied",
        "cross_table_consistent",
        "upstream_commit_lsn_verified",
        "debezium_normalizer",
        "p2_verified",
        "p11_verified",
    ] {
        assert_eq!(progress[key], Value::Bool(false), "{key} {progress}");
    }
}

fn assert_attestations(txn: &Value) {
    assert_eq!(txn["commit_position_provenance"], "caller_declared");
    for key in [
        "upstream_commit_lsn_verified",
        "cross_table_consistent",
        "target_applied",
        "ack_allowed",
        "debezium_normalizer",
        "p2_verified",
        "p11_verified",
    ] {
        assert_eq!(txn["attestations"][key], Value::Bool(false), "{key}");
    }
}

fn completes(out: &Out) -> Vec<&Value> {
    out.lines
        .iter()
        .filter(|line| line["kind"] == "complete_transaction")
        .collect()
}

#[test]
fn sha256_known_vectors() {
    assert_eq!(event_hash("").expect("empty"), EMPTY_SHA);
    assert_eq!(event_hash("abc").expect("abc"), ABC_SHA);
}

#[test]
fn begin_data_end_count_filter_and_attestations() {
    let notes = data_row("west", 2, 1, "public.notes", "9", json!([]));
    let tickets = data_row(
        "west",
        1,
        1,
        "public.tickets",
        "4",
        json!([{"type": "text", "ordinal": 0, "value": "mint"}]),
    );
    let input = feed(&[
        bind(&["public.tickets"]),
        begin("west"),
        notes,
        tickets,
        end("west", 2, &[("public.notes", 1), ("public.tickets", 1)], "0/15", TS),
    ]);
    let out = run(&input);
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert_eq!(out.lines.len(), 2);
    let txn = &out.lines[0];
    assert_eq!(txn["kind"], "complete_transaction");
    assert_eq!(txn["txn_id"], "west");
    assert_eq!(txn["commit_order"], 1);
    assert_eq!(txn["commit_lsn"], "0/15");
    assert_eq!(txn["position"], position(1, 0x15));
    assert_eq!(txn["event_count"], 2);
    assert_eq!(txn["selected_row_count"], 1);
    assert_eq!(txn["per_collection_counts"]["public.notes"], 1);
    assert_eq!(txn["per_collection_counts"]["public.tickets"], 1);
    assert_eq!(txn["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(txn["rows"][0]["table"], "public.tickets");
    assert_eq!(txn["rows"][0]["global_txn_sequence"], 1);
    assert_eq!(txn["rows"][0]["data_collection_sequence"], 1);
    assert_eq!(
        txn["rows"][0]["event_hash"],
        event_hash(MINT_PREIMAGE).expect("hash")
    );
    assert_eq!(txn["lineage"], hex64(0x11));
    assert_attestations(txn);
    let progress = last(&out);
    assert_eq!(progress["kind"], "progress");
    assert_eq!(progress["seen"]["data"], 2);
    assert_eq!(progress["seen"]["end"], 1);
    assert_eq!(progress["complete_decoded"]["transactions"], 1);
    assert_eq!(progress["complete_decoded"]["selected_rows"], 1);
    assert_eq!(progress["complete_decoded"]["last_commit_lsn"], "0/15");
    assert_eq!(progress["resume_required"], false);
    assert_eq!(progress["fault"], Value::Null);
    assert_unapplied(progress);
}

#[test]
fn interleaved_end_order_keeps_each_sequence() {
    let input = feed(&[
        bind(&["public.tickets"]),
        begin("west"),
        data_row("west", 1, 1, "public.tickets", "1", json!([])),
        begin("east"),
        data_row("east", 1, 1, "public.tickets", "2", json!([])),
        end("east", 1, &[("public.tickets", 1)], "0/2A", TS_WHOLE),
        end("west", 1, &[("public.tickets", 1)], "0/2B", TS_WHOLE),
    ]);
    let out = run(&input);
    assert_eq!(out.code, 0, "{}", out.stdout);
    let done = completes(&out);
    assert_eq!(done.len(), 2);
    assert_eq!(done[0]["txn_id"], "east");
    assert_eq!(done[0]["commit_order"], 1);
    assert_eq!(done[0]["commit_lsn"], "0/2A");
    assert_eq!(done[0]["position"], position(1, 0x2a));
    assert_eq!(done[0]["rows"][0]["global_txn_sequence"], 1);
    assert_eq!(done[1]["txn_id"], "west");
    assert_eq!(done[1]["commit_order"], 2);
    assert_eq!(done[1]["commit_lsn"], "0/2B");
    assert_eq!(done[1]["position"], position(1, 0x2b));
    assert_eq!(done[1]["rows"][0]["global_txn_sequence"], 1);
    assert_attestations(done[0]);
    assert_attestations(done[1]);
    assert_unapplied(last(&out));
    assert_eq!(last(&out)["complete_decoded"]["transactions"], 2);
}

#[test]
fn rollback_does_not_complete_and_replay_is_idempotent() {
    let open = feed(&[
        bind(&["public.tickets"]),
        begin("back"),
        data_row("back", 1, 1, "public.tickets", "3", json!([])),
        rollback("back"),
        rollback("back"),
    ]);
    let out = run(&open);
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert!(completes(&out).is_empty());
    let progress = last(&out);
    assert_eq!(progress["pending"].as_array().expect("pending").len(), 0);
    assert_eq!(progress["resume_required"], false);
    assert_eq!(progress["seen"]["rollback"], 1);
    assert_eq!(progress["seen"]["duplicate_rollback"], 1);
    assert_eq!(progress["seen"]["data"], 1);
    assert_eq!(progress["complete_decoded"]["transactions"], 0);
    assert_unapplied(progress);

    let after = feed(&[
        bind(&["public.tickets"]),
        begin("back"),
        data_row("back", 1, 1, "public.tickets", "3", json!([])),
        end("back", 1, &[("public.tickets", 1)], "0/7", TS_WHOLE),
        rollback("back"),
    ]);
    let refused = run(&after);
    assert_eq!(refused.code, 2, "{}", refused.stdout);
    assert_eq!(completes(&refused).len(), 1);
    assert_eq!(refused.lines[1]["kind"], "refused");
    assert_eq!(refused.lines[1]["fault"], "rollback_after_complete");
    assert_eq!(refused.lines[1]["txn_id"], "back");
    assert_unapplied(last(&refused));
    assert!(last(&refused)["applied"].is_null());
}

#[test]
fn duplicate_payload_conflicts_and_identical_does_not_double() {
    let first = data_row(
        "dup",
        1,
        1,
        "public.tickets",
        "8",
        json!([{"type": "text", "ordinal": 0, "value": "mint"}]),
    );
    let changed = data_row(
        "dup",
        1,
        1,
        "public.tickets",
        "8",
        json!([{"type": "text", "ordinal": 0, "value": "rose"}]),
    );
    let conflict = run(&feed(&[
        bind(&["public.tickets"]),
        begin("dup"),
        first.clone(),
        changed,
    ]));
    assert_eq!(conflict.code, 2, "{}", conflict.stdout);
    assert!(completes(&conflict).is_empty());
    assert_eq!(conflict.lines[0]["fault"], "duplicate_conflict");
    assert_eq!(last(&conflict)["pending"][0]["data_events"], 1);
    assert_eq!(last(&conflict)["pending"][0]["status"], "refused");
    assert_unapplied(last(&conflict));

    let same = run(&feed(&[
        bind(&["public.tickets"]),
        begin("dup"),
        first.clone(),
        first,
        end("dup", 1, &[("public.tickets", 1)], "0/8", TS_WHOLE),
    ]));
    assert_eq!(same.code, 0, "{}", same.stdout);
    assert_eq!(completes(&same)[0]["event_count"], 1);
    assert_eq!(last(&same)["seen"]["duplicate_data"], 1);
    assert_eq!(last(&same)["seen"]["data"], 1);
    assert_unapplied(last(&same));
}

#[test]
fn eof_pending_keeps_completed_and_open() {
    let input = feed(&[
        bind(&["public.tickets"]),
        begin("kept"),
        data_row("kept", 1, 1, "public.tickets", "5", json!([])),
        end("kept", 1, &[("public.tickets", 1)], "0/9", TS_WHOLE),
        begin("open1"),
        data_row("open1", 1, 1, "public.tickets", "6", json!([])),
    ]);
    let out = run(&input);
    assert_eq!(out.code, 3, "{}", out.stdout);
    assert_eq!(completes(&out).len(), 1);
    assert_eq!(completes(&out)[0]["txn_id"], "kept");
    assert_attestations(completes(&out)[0]);
    let progress = last(&out);
    assert_eq!(progress["resume_required"], true);
    assert_eq!(progress["fault"], Value::Null);
    assert_eq!(progress["pending"][0]["txn_id"], "open1");
    assert_eq!(progress["pending"][0]["status"], "open");
    assert_eq!(progress["pending"][0]["data_events"], 1);
    assert_eq!(progress["pending"][0]["payload_bytes"], 1);
    assert_eq!(progress["seen"]["begin"], 2);
    assert_eq!(progress["seen"]["data"], 2);
    assert_eq!(progress["seen"]["end"], 1);
    assert_eq!(progress["complete_decoded"]["transactions"], 1);
    assert_unapplied(progress);
}

#[test]
fn lineage_reset_clears_floor_and_keeps_position_identity() {
    let blocked = run(&feed(&[
        bind(&["public.tickets"]),
        begin("alpha"),
        json!({"kind": "barrier", "reason": "epoch", "epoch": 2}).to_string(),
    ]));
    assert_eq!(blocked.code, 2, "{}", blocked.stdout);
    assert_eq!(blocked.lines[0]["fault"], "open_txn_at_barrier");
    assert!(completes(&blocked).is_empty());
    assert_unapplied(last(&blocked));

    let next = hex64(0x22);
    let input = feed(&[
        bind(&["public.tickets"]),
        begin("alpha"),
        data_row("alpha", 1, 1, "public.tickets", "1", json!([])),
        end("alpha", 1, &[("public.tickets", 1)], "0/30", TS_WHOLE),
        json!({
            "kind": "barrier",
            "reason": "reset",
            "lineage": next,
            "epoch": 1
        })
        .to_string(),
        begin("beta"),
        data_row("beta", 1, 1, "public.tickets", "2", json!([])),
        end("beta", 1, &[("public.tickets", 1)], "0/10", TS_WHOLE),
        begin("gamma"),
        data_row("gamma", 1, 1, "public.tickets", "3", json!([])),
        end("gamma", 1, &[("public.tickets", 1)], "0/30", TS_WHOLE),
    ]);
    let out = run(&input);
    assert_eq!(out.code, 2, "{}", out.stdout);
    let done = completes(&out);
    assert_eq!(done.len(), 2);
    assert_eq!(done[0]["txn_id"], "alpha");
    assert_eq!(done[0]["lineage"], hex64(0x11));
    assert_eq!(done[0]["position"], position(1, 0x30));
    assert_eq!(done[1]["txn_id"], "beta");
    assert_eq!(done[1]["lineage"], next);
    assert_eq!(done[1]["position"], position(1, 0x10));
    assert_eq!(out.lines[2]["fault"], "commit_position_conflict");
    assert_eq!(out.lines[2]["txn_id"], "gamma");
    assert_eq!(last(&out)["complete_decoded"]["last_commit_lsn"], "0/10");
    assert_unapplied(last(&out));
    assert_attestations(done[0]);
    assert_attestations(done[1]);
}

#[test]
fn count_overflow_does_not_wrap_and_early_end_keeps_pending() {
    let overflow = run(&feed(&[
        bind(&["public.notes", "public.tickets"]),
        begin("ov"),
        end(
            "ov",
            0,
            &[("public.notes", 1), ("public.tickets", u64::MAX)],
            "0/1",
            TS_WHOLE,
        ),
    ]));
    assert_eq!(overflow.code, 2, "{}", overflow.stdout);
    assert!(completes(&overflow).is_empty());
    assert_eq!(overflow.lines[0]["fault"], "count_overflow");
    assert_eq!(last(&overflow)["complete_decoded"]["transactions"], 0);
    assert_eq!(last(&overflow)["pending"][0]["data_events"], 0);
    assert_unapplied(last(&overflow));

    let early = run(&feed(&[
        bind(&["public.tickets"]),
        begin("hole"),
        data_row("hole", 1, 1, "public.tickets", "1", json!([])),
        end("hole", 2, &[("public.tickets", 2)], "0/2", TS_WHOLE),
    ]));
    assert_eq!(early.code, 2, "{}", early.stdout);
    assert!(completes(&early).is_empty());
    assert_eq!(early.lines[0]["fault"], "end_too_early");
    assert_eq!(last(&early)["pending"][0]["data_events"], 1);
    assert_eq!(last(&early)["pending"][0]["status"], "refused");
    assert_eq!(last(&early)["complete_decoded"]["transactions"], 0);
    assert_unapplied(last(&early));
}

#[test]
fn row_lsn_is_not_a_commit_fallback() {
    let mut bad = data_row("row", 1, 1, "public.tickets", "1", json!([]));
    bad.insert_str(1, "\"row_lsn\":\"0/FF\",");
    let input = feed(&[
        bind(&["public.tickets"]),
        begin("kept"),
        data_row("kept", 1, 1, "public.tickets", "4", json!([])),
        end("kept", 1, &[("public.tickets", 1)], "0/11", TS_WHOLE),
        begin("row"),
        bad,
    ]);
    let out = run(&input);
    assert_eq!(out.code, 2, "{}", out.stdout);
    let done = completes(&out);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["txn_id"], "kept");
    assert_eq!(done[0]["commit_lsn"], "0/11");
    assert_eq!(done[0]["position"], position(1, 0x11));
    assert_ne!(done[0]["position"], position(1, 0xff));
    assert_eq!(out.lines[1]["fault"], "malformed");
    assert_eq!(last(&out)["complete_decoded"]["last_commit_lsn"], "0/11");
    assert_eq!(last(&out)["complete_decoded"]["last_position"], position(1, 0x11));
    assert_unapplied(last(&out));
    assert_attestations(done[0]);
}

#[test]
fn u16_decoder_accepts_32768_and_65535() {
    let high: Cell =
        serde_json::from_str(r#"{"type":"unavailable","ordinal":32768}"#).expect("32768 decodes");
    match high {
        Cell::Unavailable { ordinal } => assert_eq!(u32::from(ordinal), 32768),
        other => panic!("unexpected {other:?}"),
    }
    let max: Cell =
        serde_json::from_str(r#"{"type":"unavailable","ordinal":65535}"#).expect("65535 decodes");
    match max {
        Cell::Unavailable { ordinal } => assert_eq!(u32::from(ordinal), 65535),
        other => panic!("unexpected {other:?}"),
    }
    assert!(serde_json::from_str::<Cell>(r#"{"type":"unavailable","ordinal":65536}"#).is_err());
}

fn unavailable_feed(ordinal: u32) -> Vec<u8> {
    let data = format!(
        "{{\"kind\":\"data\",\"txn_id\":\"ord\",\"global_txn_sequence\":1,\"data_collection_sequence\":1,\"table\":\"public.tickets\",\"key\":[{{\"type\":\"int\",\"ordinal\":0,\"value\":\"4\"}}],\"op\":\"upsert\",\"payload\":[{{\"type\":\"unavailable\",\"ordinal\":{ordinal}}}],\"unavailable_ordinals\":[{ordinal}]}}"
    );
    feed(&[
        bind(&["public.tickets"]),
        begin("ord"),
        data,
        end("ord", 1, &[("public.tickets", 1)], "0/5", TS_WHOLE),
    ])
}

#[test]
fn ordinal_32767_completes_inside_the_contract() {
    let out = run(&unavailable_feed(32767));
    assert_eq!(out.code, 0, "{}", out.stdout);
    let txn = completes(&out)[0];
    assert_eq!(txn["rows"][0]["unavailable_ordinals"][0], 32767);
    assert_eq!(txn["rows"][0]["payload"][0]["ordinal"], 32767);
    assert_eq!(txn["rows"][0]["payload"][0]["type"], "unavailable");
    assert_attestations(txn);
    assert_unapplied(last(&out));
}

#[test]
fn ordinals_32768_and_65535_are_outside_the_contract() {
    let mut problems = Vec::new();
    for ordinal in [32768_u32, 65535] {
        let out = run(&unavailable_feed(ordinal));
        let fault = out
            .lines
            .first()
            .map(|line| line["fault"].to_string())
            .unwrap_or_else(|| "none".to_owned());
        let stored = completes(&out).len();
        if out.code != 2 || stored != 0 || fault != "\"ordinal_range\"" {
            problems.push(format!(
                "ordinal {ordinal} code {} fault {fault} completes {stored}",
                out.code
            ));
        }
        if stored == 0 && out.code == 2 {
            assert_unapplied(last(&out));
        }
    }
    assert!(
        problems.is_empty(),
        "contract ordinals are 0..=32767 and FaultCode::OrdinalRange is the refusal; {}",
        problems.join("; ")
    );
}

#[test]
fn ordinal_65536_is_malformed_because_it_does_not_fit_u16() {
    let out = run(&unavailable_feed(65536));
    assert_eq!(out.code, 2, "{}", out.stdout);
    assert!(completes(&out).is_empty());
    assert_eq!(out.lines[0]["fault"], "malformed", "{}", out.stdout);
    assert_unapplied(last(&out));
}

fn text_line(seq: u64, text: &str) -> String {
    format!(
        "{{\"kind\":\"data\",\"txn_id\":\"bulk\",\"global_txn_sequence\":{seq},\"data_collection_sequence\":{seq},\"table\":\"public.audit\",\"key\":[{{\"type\":\"bool\",\"ordinal\":0,\"value\":true}}],\"op\":\"upsert\",\"payload\":[{{\"type\":\"text\",\"ordinal\":0,\"value\":\"{text}\"}}],\"unavailable_ordinals\":[]}}"
    )
}

fn value_plan() -> (Vec<String>, u64) {
    let mut remaining = 33_554_432_u64;
    let mut seq = 1_u64;
    let mut lines = Vec::new();
    let mut charged = 0_u64;
    while remaining > 0 {
        let overhead = text_line(seq, "").len();
        assert!(overhead < 1_048_576, "{overhead}");
        let room = u64::try_from(1_048_576 - overhead).expect("room");
        let take = remaining.min(room);
        let text = "a".repeat(usize::try_from(take).expect("take"));
        let line = text_line(seq, &text);
        assert_eq!(line.len(), overhead + text.len());
        assert!(line.len() <= 1_048_576);
        lines.push(line);
        charged += take;
        remaining -= take;
        seq += 1;
        assert!(seq < 1_000, "plan did not converge");
    }
    assert_eq!(charged, 33_554_432);
    let events = u64::try_from(lines.len()).expect("events");
    assert!(events > 0 && events < 200_000, "{events}");
    (lines, events)
}

fn bulk_feed(lines: &[String], finish: bool, extra: bool) -> Vec<u8> {
    let mut parts = vec![bind(&["public.tickets"]), begin("bulk")];
    parts.extend(lines.iter().cloned());
    if extra {
        let seq = u64::try_from(lines.len()).expect("len") + 1;
        parts.push(text_line(seq, "z"));
    }
    if finish {
        let count = u64::try_from(lines.len()).expect("len");
        parts.push(end(
            "bulk",
            count,
            &[("public.audit", count)],
            "0/3",
            TS_WHOLE,
        ));
    }
    feed(&parts)
}

#[test]
fn value_byte_budget_is_separate_from_line_length_and_the_event_cap() {
    let one = text_line(1, "q");
    assert!(one.len() > 1);
    let probe = run(&feed(&[
        bind(&["public.tickets"]),
        begin("bulk"),
        one,
    ]));
    assert_eq!(probe.code, 3, "{}", probe.stdout);
    assert_eq!(last(&probe)["pending"][0]["payload_bytes"], 1);
    assert_eq!(last(&probe)["pending"][0]["data_events"], 1);
    assert_unapplied(last(&probe));

    let mut cells = String::from("[");
    for ordinal in 0..32 {
        if ordinal > 0 {
            cells.push(',');
        }
        cells.push_str(&format!(
            "{{\"type\":\"bool\",\"ordinal\":{ordinal},\"value\":true}}"
        ));
    }
    cells.push(']');
    let decoy = format!(
        "{{\"kind\":\"data\",\"txn_id\":\"decoy\",\"global_txn_sequence\":1,\"data_collection_sequence\":1,\"table\":\"public.audit\",\"key\":[{{\"type\":\"bool\",\"ordinal\":0,\"value\":false}}],\"op\":\"upsert\",\"payload\":{cells},\"unavailable_ordinals\":[]}}"
    );
    assert!(decoy.len() > 32);
    let zeros = run(&feed(&[bind(&["public.tickets"]), begin("decoy"), decoy]));
    assert_eq!(zeros.code, 3, "{}", zeros.stdout);
    assert_eq!(
        last(&zeros)["pending"][0]["payload_bytes"], 0,
        "bool cells add no key or payload value bytes"
    );
    assert_eq!(last(&zeros)["pending"][0]["data_events"], 1);

    let (lines, events) = value_plan();
    let open = run(&bulk_feed(&lines, false, false));
    assert_eq!(open.code, 3, "{}", open.stdout);
    assert_eq!(
        last(&open)["pending"][0]["payload_bytes"], 33_554_432,
        "pending.payload_bytes is the summed value length, not a resident-set figure"
    );
    assert_eq!(last(&open)["pending"][0]["data_events"], events);
    assert_eq!(last(&open)["needs_spool"], false);
    assert_eq!(last(&open)["pending"][0]["status"], "open");
    assert_unapplied(last(&open));

    let done = run(&bulk_feed(&lines, true, false));
    assert_eq!(done.code, 0, "{}", done.stdout);
    assert_eq!(completes(&done)[0]["event_count"], events);
    assert_eq!(completes(&done)[0]["selected_row_count"], 0);
    assert_eq!(completes(&done)[0]["rows"].as_array().expect("rows").len(), 0);
    assert_attestations(completes(&done)[0]);
    assert_unapplied(last(&done));

    let over = run(&bulk_feed(&lines, false, true));
    assert_eq!(over.code, 2, "{}", over.stdout);
    assert!(completes(&over).is_empty());
    assert_eq!(over.lines[0]["fault"], "needs_spool");
    assert_eq!(last(&over)["needs_spool"], true);
    assert_eq!(last(&over)["pending"][0]["data_events"], events);
    assert_eq!(last(&over)["pending"][0]["payload_bytes"], 33_554_432);
    assert_eq!(last(&over)["pending"][0]["status"], "refused");
    assert_eq!(last(&over)["complete_decoded"]["transactions"], 0);
    assert_unapplied(last(&over));
}

fn zero_value_line(seq: u64) -> String {
    format!(
        "{{\"kind\":\"data\",\"txn_id\":\"cap\",\"global_txn_sequence\":{seq},\"data_collection_sequence\":{seq},\"table\":\"public.audit\",\"key\":[{{\"type\":\"bool\",\"ordinal\":0,\"value\":false}}],\"op\":\"delete\",\"payload\":[],\"unavailable_ordinals\":[]}}"
    )
}

fn event_feed(count: u64, extra: bool, finish: bool) -> Vec<u8> {
    let mut body = Vec::new();
    for line in [bind(&["public.tickets"]), begin("cap")] {
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    for seq in 1..=count {
        let line = zero_value_line(seq);
        assert!(line.len() <= 1_048_576);
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    if extra {
        let line = zero_value_line(count + 1);
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    if finish {
        let line = end("cap", count, &[("public.audit", count)], "0/4", TS_WHOLE);
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    body
}

#[test]
fn event_cap_200000_completes_with_zero_value_bytes() {
    let done = run(&event_feed(200_000, false, true));
    assert_eq!(done.code, 0, "{}", done.stdout);
    assert_eq!(completes(&done)[0]["event_count"], 200_000);
    assert_eq!(completes(&done)[0]["selected_row_count"], 0);
    assert_eq!(
        completes(&done)[0]["rows"].as_array().expect("rows").len(),
        0
    );
    assert_eq!(last(&done)["seen"]["data"], 200_000);
    assert_eq!(last(&done)["complete_decoded"]["transactions"], 1);
    assert_unapplied(last(&done));
    assert_attestations(completes(&done)[0]);

    let over = run(&event_feed(200_000, true, false));
    assert_eq!(over.code, 2, "{}", over.stdout);
    assert!(completes(&over).is_empty());
    assert_eq!(over.lines[0]["fault"], "needs_spool");
    assert_eq!(last(&over)["needs_spool"], true);
    assert_eq!(last(&over)["pending"][0]["data_events"], 200_000);
    assert_eq!(
        last(&over)["pending"][0]["payload_bytes"], 0,
        "200000 bool-key events charge no value bytes"
    );
    assert_eq!(last(&over)["pending"][0]["status"], "refused");
    assert_eq!(last(&over)["complete_decoded"]["transactions"], 0);
    assert_unapplied(last(&over));
}
