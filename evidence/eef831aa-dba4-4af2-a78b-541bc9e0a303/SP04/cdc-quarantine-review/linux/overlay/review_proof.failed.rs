//! Independent review of cdc-quarantine-v1.
//! The SHA-256 below is this file's oracle. It is not the product implementation.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cdc_quarantine::{
    hex as product_hex, identity_preimage, parse_control, put, recover, replay, sha256 as product_sha,
    FaultCode, Limits, Status,
};
use serde_json::{json, Value};

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const LINEAGE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const POSITION: &str = "18446744168199581248";
const TRUNCATE_PAYLOAD: &str = "qa303-truncate-opaque";
const SCHEMA_PAYLOAD: &str = "qa303-schema-opaque";
const TRUNCATE_ID: &str = "revtrunc1";
const SCHEMA_ID: &str = "revschema1";

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let bit_len = u64::try_from(input.len()).expect("length").checked_mul(8).expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_cdc-quarantine")
        .or(option_env!("CARGO_BIN_EXE_cdc_quarantine"))
        .expect("cdc-quarantine binary")
}

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("oo-a303-q-{}-{n}-{label}", std::process::id()));
    fs::create_dir(&path).unwrap_or_else(|err| panic!("scratch {}: {err}", path.display()));
    path
}

struct Out {
    code: i32,
    status: Value,
    stdout: String,
    stderr: String,
}

fn run(command: &str, store: &Path, input: Option<&[u8]>, env: &[(&str, &str)]) -> Out {
    let dir = store.to_str().expect("utf8 store path");
    let mut cmd = Command::new(bin());
    cmd.arg(command)
        .arg(dir)
        .env_remove("CDC_QUARANTINE_CHECKPOINT")
        .env_remove("CDC_QUARANTINE_MARK")
        .env_remove("CDC_QUARANTINE_MAX_DISK_BYTES")
        .env_remove("CDC_QUARANTINE_MAX_EVENTS")
        .env_remove("CDC_QUARANTINE_MAX_PAYLOAD_BYTES")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().expect("spawn");
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(bytes).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let code = output.status.code().expect("exit code");
    let status = if stdout.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(stdout.trim_end()).unwrap_or_else(|err| panic!("status json: {err} in {stdout}"))
    };
    Out {
        code,
        status,
        stdout,
        stderr,
    }
}

fn control_line(event_id: &str, sequence: u64, collection: &str, reason: &str, payload: &str, position: &str) -> Vec<u8> {
    let digest = hex(&sha256(payload.as_bytes()));
    let value = json!({
        "kind": "control",
        "source": "billing",
        "lineage": LINEAGE,
        "epoch": 1,
        "event_id": event_id,
        "sequence": sequence,
        "collection": collection,
        "reason": reason,
        "payload": payload,
        "payload_sha256": digest,
        "source_position": position,
    });
    let mut bytes = serde_json::to_vec(&value).expect("encode");
    bytes.push(b'\n');
    bytes
}

fn parse_line(bytes: &[u8]) -> cdc_quarantine::ControlEvent {
    let text = std::str::from_utf8(bytes).expect("utf8").trim_end_matches('\n');
    parse_control(text, &Limits::product()).expect("parse control")
}

fn identity_dir(event_id: &str) -> String {
    let preimage = format!(
        "{{\"epoch\":1,\"event_id\":\"{event_id}\",\"lineage\":\"{LINEAGE}\",\"source\":\"billing\"}}"
    );
    hex(&sha256(preimage.as_bytes()))
}

struct Expect {
    event_id: &'static str,
    sequence: u64,
    collection: &'static str,
    reason: &'static str,
    payload: &'static str,
    position: &'static str,
    paused: &'static [&'static str],
}

fn manifest_bytes(expect: &Expect, payload_sha: &str, payload_len: u64) -> Vec<u8> {
    let mut list = String::from("[");
    for (index, name) in expect.paused.iter().enumerate() {
        if index > 0 {
            list.push(',');
        }
        list.push('"');
        list.push_str(name);
        list.push('"');
    }
    list.push(']');
    let mut text = String::new();
    text.push_str("{\"contract\":\"cdc-quarantine-v1\",\"kind\":\"control_manifest\",\"source\":\"billing\",\"lineage\":\"");
    text.push_str(LINEAGE);
    text.push_str("\",\"epoch\":1,\"event_id\":\"");
    text.push_str(expect.event_id);
    text.push_str("\",\"sequence\":");
    text.push_str(&expect.sequence.to_string());
    text.push_str(",\"collection\":\"");
    text.push_str(expect.collection);
    text.push_str("\",\"reason\":\"");
    text.push_str(expect.reason);
    text.push_str("\",\"payload_sha256\":\"");
    text.push_str(payload_sha);
    text.push_str("\",\"payload_bytes\":");
    text.push_str(&payload_len.to_string());
    text.push_str(",\"source_position\":\"");
    text.push_str(expect.position);
    text.push_str("\",\"source_position_provenance\":\"caller_declared\",\"paused_collections\":");
    text.push_str(&list);
    text.push_str(",\"schema_applied\":false,\"target_truncated\":false,\"target_applied\":null,\"cross_table_consistent\":false,\"ack_allowed\":false,\"ack_eligible\":false,\"debezium_router\":false,\"upstream_commit_verified\":false,\"local_prerequisite\":false}\n");
    text.into_bytes()
}

fn honest_status(status: &Status) {
    assert_eq!(status.contract, "cdc-quarantine-v1");
    assert_eq!(status.kind, "quarantine_status");
    assert!(!status.ack_eligible);
    assert!(!status.ack_allowed);
    assert!(!status.schema_applied);
    assert!(!status.target_truncated);
    assert!(status.target_applied.is_none());
    assert!(!status.cross_table_consistent);
    assert!(!status.debezium_router);
    assert!(!status.upstream_commit_verified);
    assert_eq!(status.source_position_provenance, "caller_declared");
    let durable = status.durable_quarantined && !status.incomplete && status.fault.is_none();
    assert_eq!(status.local_prerequisite, durable);
}

fn honest_json(status: &Value) {
    assert_eq!(status["contract"], "cdc-quarantine-v1");
    assert_eq!(status["kind"], "quarantine_status");
    assert_eq!(status["ack_eligible"], false);
    assert_eq!(status["ack_allowed"], false);
    assert_eq!(status["schema_applied"], false);
    assert_eq!(status["target_truncated"], false);
    assert!(status["target_applied"].is_null());
    assert_eq!(status["cross_table_consistent"], false);
    assert_eq!(status["debezium_router"], false);
    assert_eq!(status["upstream_commit_verified"], false);
    assert_eq!(status["source_position_provenance"], "caller_declared");
    let durable = status["durable_quarantined"] == true && status["incomplete"] == false && status["fault"].is_null();
    assert_eq!(status["local_prerequisite"], durable);
}

fn silent(out: &Out) {
    assert!(out.stderr.is_empty(), "stderr {}", out.stderr);
    assert!(!out.stdout.contains(TRUNCATE_PAYLOAD));
    assert!(!out.stdout.contains(SCHEMA_PAYLOAD));
    assert!(!out.stderr.contains(TRUNCATE_PAYLOAD));
    assert!(!out.stderr.contains(SCHEMA_PAYLOAD));
}

fn inventory(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .collect::<Result<Vec<_>, _>>()
        .expect("entries");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).unwrap_or_else(|err| panic!("meta {}: {err}", path.display()));
        let rel = path
            .strip_prefix(root)
            .expect("prefix")
            .components()
            .map(|component| component.as_os_str().to_str().expect("utf8"))
            .collect::<Vec<_>>()
            .join("/");
        assert!(!meta.file_type().is_symlink(), "symlink {rel}");
        if meta.is_dir() {
            walk(&path, root, out);
            continue;
        }
        assert!(meta.is_file(), "not a regular file {rel}");
        let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        out.insert(rel, bytes);
    }
}

fn bind_event(store: &Path, expect: &Expect) {
    let dir = store.join("events").join(identity_dir(expect.event_id));
    let payload_bytes = fs::read(dir.join("payload")).expect("payload");
    assert_eq!(payload_bytes, expect.payload.as_bytes());
    let payload_sha = hex(&sha256(&payload_bytes));
    assert_eq!(product_hex(&product_sha(&payload_bytes).expect("product sha")), payload_sha);
    let manifest = fs::read(dir.join("manifest.json")).expect("manifest");
    let expected = manifest_bytes(expect, &payload_sha, u64::try_from(payload_bytes.len()).expect("len"));
    assert_eq!(manifest, expected, "manifest spelling for {}", expect.event_id);
    let text = std::str::from_utf8(&manifest).expect("manifest utf8");
    assert!(text.contains("\"local_prerequisite\":false"));
    assert!(!text.contains("\"local_prerequisite\":true"));
    assert!(!text.contains("\"ordinal\""));
    assert!(!text.contains("\"lsn\""));
    let manifest_sha = hex(&sha256(&manifest));
    let receipt = format!("durable_quarantined\n{payload_sha}\n{manifest_sha}\n");
    assert_eq!(fs::read(dir.join("RECEIPT")).expect("receipt"), receipt.as_bytes());
}

#[test]
fn oracle_known_answers_and_position_is_decimal_text() {
    assert_eq!(LINEAGE.len(), 64);
    assert_eq!(hex(&sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&sha256(b"abc")), ABC_SHA);
    assert!(POSITION.parse::<u64>().is_err());
    assert_eq!(POSITION.len(), 20);
    let event = parse_line(&control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION));
    let preimage = format!(
        "{{\"epoch\":1,\"event_id\":\"{TRUNCATE_ID}\",\"lineage\":\"{LINEAGE}\",\"source\":\"billing\"}}"
    );
    assert_eq!(identity_preimage(&event), preimage);
    assert_eq!(event.source_position, POSITION);
    assert_eq!(event.reason.as_str(), "truncate");
}

#[test]
fn api_and_cli_bind_bytes_and_union_paused_collections() {
    let base = scratch("bind");
    let store = base.join("store");
    let truncate = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION);
    let event = parse_line(&truncate);
    let stored = put(&store, &event, &Limits::product());
    assert!(stored.fault.is_none(), "{:?}", stored.fault);
    assert_eq!(stored.exit_code(), 0);
    assert!(stored.durable_quarantined);
    assert!(stored.paused);
    honest_status(&stored);
    assert_eq!(stored.local_prerequisite, true);
    assert_eq!(stored.source_position.as_deref(), Some(POSITION));
    assert_eq!(stored.reason.as_deref(), Some("truncate"));
    let store_file = format!("cdc-quarantine-v1\nbilling\n{LINEAGE}\n1\n");
    assert_eq!(fs::read(store.join("STORE")).expect("STORE"), store_file.as_bytes());
    bind_event(
        &store,
        &Expect {
            event_id: TRUNCATE_ID,
            sequence: 4,
            collection: "public.orders",
            reason: "truncate",
            payload: TRUNCATE_PAYLOAD,
            position: POSITION,
            paused: &["public.orders"],
        },
    );
    let first_manifest = fs::read(store.join("events").join(identity_dir(TRUNCATE_ID)).join("manifest.json")).expect("first");

    let schema = control_line(SCHEMA_ID, 9, "public.lines", "schema_changed", SCHEMA_PAYLOAD, "0");
    let cli = run("put", &store, Some(&schema), &[]);
    assert_eq!(cli.code, 0, "{}", cli.stdout);
    silent(&cli);
    honest_json(&cli.status);
    assert_eq!(cli.status["local_prerequisite"], true);
    assert_eq!(cli.status["reason"], "schema_changed");
    assert_eq!(cli.status["source_position"], "0");
    assert_eq!(cli.status["paused_collections"], json!(["public.lines", "public.orders"]));
    assert_eq!(
        fs::read(store.join("events").join(identity_dir(TRUNCATE_ID)).join("manifest.json")).expect("first again"),
        first_manifest
    );
    assert_eq!(fs::read(store.join("STORE")).expect("STORE again"), store_file.as_bytes());
    bind_event(
        &store,
        &Expect {
            event_id: SCHEMA_ID,
            sequence: 9,
            collection: "public.lines",
            reason: "schema_changed",
            payload: SCHEMA_PAYLOAD,
            position: "0",
            paused: &["public.lines", "public.orders"],
        },
    );

    let recovered = run("recover", &store, None, &[]);
    assert_eq!(recovered.code, 0, "{}", recovered.stdout);
    silent(&recovered);
    honest_json(&recovered.status);
    assert_eq!(recovered.status["durable_quarantined"], true);
    assert_eq!(recovered.status["paused"], true);
    assert_eq!(recovered.status["incomplete"], false);
    assert_eq!(recovered.status["local_prerequisite"], true);
    assert_eq!(recovered.status["paused_collections"], json!(["public.lines", "public.orders"]));
    assert_eq!(recovered.status["durable_events"], 2);
    let api = recover(&store);
    assert_eq!(api.exit_code(), 0);
    assert_eq!(api.paused_collections, vec!["public.lines".to_owned(), "public.orders".to_owned()]);
    honest_status(&api);
}

#[test]
fn exact_replay_and_conflict_keep_stored_bytes() {
    let base = scratch("replay");
    let store = base.join("store");
    let line = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION);
    let put_out = run("put", &store, Some(&line), &[]);
    assert_eq!(put_out.code, 0, "{}", put_out.stdout);
    let snap = inventory(&store);
    let replay_out = run("replay", &store, Some(&line), &[]);
    assert_eq!(replay_out.code, 0, "{}", replay_out.stdout);
    assert_eq!(replay_out.status["replay"], true);
    assert_eq!(replay_out.status["local_prerequisite"], true);
    honest_json(&replay_out.status);
    silent(&replay_out);
    assert_eq!(inventory(&store), snap);
    let event = parse_line(&line);
    let api = replay(&store, &event);
    assert!(api.replay);
    assert_eq!(api.exit_code(), 0);
    honest_status(&api);
    assert_eq!(inventory(&store), snap);

    let mut conflict = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", "qa303-conflict-opaque", POSITION);
    assert_ne!(conflict, line);
    let cli = run("put", &store, Some(&conflict), &[]);
    assert_eq!(cli.code, 2, "{}", cli.stdout);
    assert_eq!(cli.status["fault"], "payload_conflict");
    assert_eq!(cli.status["durable_quarantined"], false);
    honest_json(&cli.status);
    silent(&cli);
    assert_eq!(inventory(&store), snap);
    conflict.pop();
    let text = std::str::from_utf8(&conflict).expect("utf8");
    let other = parse_control(text, &Limits::product()).expect("conflict parses");
    let api_conflict = put(&store, &other, &Limits::product());
    assert_eq!(api_conflict.fault, Some("payload_conflict"));
    assert_eq!(api_conflict.exit_code(), 2);
    honest_status(&api_conflict);
    assert_eq!(inventory(&store), snap);
}

#[test]
fn counter_quota_is_not_an_os_quota() {
    let base = scratch("quota");
    let store = base.join("store");
    let line = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION);
    let event = parse_line(&line);
    let mut limits = Limits::product();
    limits.max_disk_bytes = 0;
    let refused = put(&store, &event, &limits);
    assert_eq!(refused.fault, Some("disk_quota"));
    assert_eq!(refused.exit_code(), 2);
    assert!(refused.paused);
    assert_eq!(refused.attempt_collection.as_deref(), Some("public.orders"));
    assert!(!refused.durable_quarantined);
    honest_status(&refused);
    assert!(!store.exists(), "counter refusal must not create the store");
    let marker = base.join("os-write");
    fs::write(&marker, b"os-accepted").expect("os write");
    assert_eq!(fs::read(&marker).expect("os read"), b"os-accepted");

    let cli_store = base.join("cli-store");
    let cli = run("put", &cli_store, Some(&line), &[("CDC_QUARANTINE_MAX_DISK_BYTES", "0")]);
    assert_eq!(cli.code, 2, "{}", cli.stdout);
    assert_eq!(cli.status["fault"], "disk_quota");
    assert_eq!(cli.status["paused"], true);
    assert_eq!(cli.status["attempt_collection"], "public.orders");
    assert_eq!(cli.status["durable_quarantined"], false);
    honest_json(&cli.status);
    silent(&cli);
    assert!(!cli_store.exists());
}

#[test]
fn record_cap_does_not_persist_the_refused_event() {
    let base = scratch("cap");
    let store = base.join("store");
    let first = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION);
    let second = control_line(SCHEMA_ID, 9, "public.lines", "schema_changed", SCHEMA_PAYLOAD, "0");
    let mut limits = Limits::product();
    limits.max_events = 1;
    let stored = put(&store, &parse_line(&first), &limits);
    assert!(stored.fault.is_none(), "{:?}", stored.fault);
    let snap = inventory(&store);
    let refused = put(&store, &parse_line(&second), &limits);
    assert_eq!(refused.fault, Some("record_cap"));
    assert_eq!(refused.exit_code(), 2);
    assert_eq!(refused.attempt_collection.as_deref(), Some("public.lines"));
    honest_status(&refused);
    assert_eq!(inventory(&store), snap);
    assert!(!store.join("events").join(identity_dir(SCHEMA_ID)).exists());

    let empty = base.join("empty");
    let mut zero = Limits::product();
    zero.max_events = 0;
    let blocked = put(&empty, &parse_line(&first), &zero);
    assert_eq!(blocked.fault, Some("record_cap"));
    assert!(!empty.exists());
    let cli = run("put", &base.join("cli-empty"), Some(&first), &[("CDC_QUARANTINE_MAX_EVENTS", "0")]);
    assert_eq!(cli.code, 2, "{}", cli.stdout);
    assert_eq!(cli.status["fault"], "record_cap");
    assert!(!base.join("cli-empty").exists());
}

#[test]
fn ordinal_and_lsn_are_not_fields_of_this_contract() {
    let base = scratch("ordinal");
    for key in ["ordinal", "lsn"] {
        let mut value = json!({
            "kind": "control",
            "source": "billing",
            "lineage": LINEAGE,
            "epoch": 1,
            "event_id": TRUNCATE_ID,
            "sequence": 4,
            "collection": "public.orders",
            "reason": "truncate",
            "payload": TRUNCATE_PAYLOAD,
            "payload_sha256": hex(&sha256(TRUNCATE_PAYLOAD.as_bytes())),
            "source_position": POSITION,
        });
        value.as_object_mut().expect("object").insert(key.to_owned(), json!(1));
        let text = serde_json::to_string(&value).expect("encode");
        let rejected = parse_control(&text, &Limits::product()).expect_err(key);
        assert_eq!(rejected.code, FaultCode::Malformed);
        assert!(rejected.event.is_none());
        let mut line = text.into_bytes();
        line.push(b'\n');
        let store = base.join(key);
        let cli = run("put", &store, Some(&line), &[]);
        assert_eq!(cli.code, 2, "{}", cli.stdout);
        assert_eq!(cli.status["fault"], "malformed");
        assert_eq!(cli.status["paused"], false);
        assert!(cli.status["attempt_collection"].is_null());
        honest_json(&cli.status);
        silent(&cli);
        assert!(!store.exists());
    }
}

#[test]
fn sigkill_checkpoints_recover_incomplete_without_ack() {
    let stages = ["before_data_fsync", "before_manifest", "before_rename", "before_parent_fsync"];
    let line = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION);
    for stage in stages {
        let base = scratch(stage);
        let store = base.join("store");
        kill_at(stage, &base, &store, &line);
        let dir = store.join("events").join(identity_dir(TRUNCATE_ID));
        match stage {
            "before_data_fsync" | "before_manifest" => {
                assert!(dir.join("payload.partial").is_file());
                assert!(!dir.join("payload").exists());
                assert!(!dir.join("manifest.json").exists());
                assert!(!dir.join("manifest.json.partial").exists());
                assert!(!store.join("STORE").exists());
                assert!(!store.join("STORE.partial").exists());
            }
            "before_rename" => {
                assert!(dir.join("payload.partial").is_file());
                assert!(dir.join("manifest.json.partial").is_file());
                assert!(store.join("STORE.partial").is_file());
                assert!(!dir.join("payload").exists());
                assert!(!dir.join("manifest.json").exists());
                assert!(!store.join("STORE").exists());
            }
            "before_parent_fsync" => {
                assert!(dir.join("payload").is_file());
                assert!(dir.join("manifest.json").is_file());
                assert!(store.join("STORE").is_file());
                assert!(!dir.join("payload.partial").exists());
                assert!(!dir.join("manifest.json.partial").exists());
                assert!(!store.join("STORE.partial").exists());
            }
            _ => panic!("stage"),
        }
        assert!(!dir.join("RECEIPT").exists());
        let snap = inventory(&store);
        assert_killed_recover(&store, stage);
        assert_eq!(inventory(&store), snap);
        assert_killed_recover(&store, stage);
        let replay_out = run("replay", &store, Some(&line), &[]);
        assert_eq!(replay_out.code, 2, "{}", replay_out.stdout);
        assert_eq!(replay_out.status["fault"], "incomplete");
        assert_eq!(replay_out.status["paused"], true);
        assert_eq!(replay_out.status["ack_eligible"], false);
        assert!(!dir.join("RECEIPT").exists());
        assert_eq!(inventory(&store), snap);
        let second = run("put", &store, Some(&line), &[]);
        assert_eq!(second.code, 2, "{}", second.stdout);
        assert_eq!(second.status["fault"], "incomplete");
        assert!(!dir.join("RECEIPT").exists());
        assert_eq!(inventory(&store), snap);
    }
}

fn assert_killed_recover(store: &Path, stage: &str) {
    let out = run("recover", store, None, &[]);
    assert_eq!(out.code, 3, "{}", out.stdout);
    honest_json(&out.status);
    assert_eq!(out.status["incomplete"], true);
    assert_eq!(out.status["paused"], true);
    assert_eq!(out.status["durable_quarantined"], false);
    assert_eq!(out.status["local_prerequisite"], false);
    assert!(out.status["fault"].is_null());
    assert_eq!(out.status["ack_eligible"], false);
    assert_eq!(out.status["ack_allowed"], false);
    let api = recover(store);
    assert_eq!(api.exit_code(), 3);
    assert!(api.incomplete);
    assert!(api.paused);
    assert!(!api.durable_quarantined);
    honest_status(&api);
    match stage {
        "before_data_fsync" | "before_manifest" => {
            assert!(out.status["unknown_incomplete"] == true);
            assert!(api.unknown_incomplete);
        }
        "before_rename" | "before_parent_fsync" => {
            assert_eq!(out.status["unknown_incomplete"], false);
            assert!(!api.unknown_incomplete);
            assert_eq!(out.status["paused_collections"], json!(["public.orders"]));
        }
        _ => panic!("stage"),
    }
}

fn kill_at(stage: &str, base: &Path, store: &Path, input: &[u8]) {
    let mark = base.join(format!("{stage}.mark"));
    let dir = store.to_str().expect("utf8");
    let mut child = Command::new(bin())
        .arg("put")
        .arg(dir)
        .env("CDC_QUARANTINE_CHECKPOINT", stage)
        .env("CDC_QUARANTINE_MARK", mark.to_str().expect("utf8 mark"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn checkpoint");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(input).expect("write");
    }
    let start = Instant::now();
    let mut saw = false;
    while start.elapsed() < Duration::from_secs(30) {
        if fs::read(&mark).ok().as_deref() == Some(b"waiting\n") {
            saw = true;
            break;
        }
        if let Some(status) = child.try_wait().expect("try_wait") {
            panic!("checkpoint {stage} exited before the mark: {status}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().expect("sigkill");
    let output = child.wait_with_output().expect("wait killed");
    assert!(saw, "no mark for {stage}; stderr {}", String::from_utf8_lossy(&output.stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(output.status.signal(), Some(9), "child signal for {stage}");
    }
}

#[test]
fn damaged_manifest_receipt_and_hash_are_not_repaired() {
    let line = control_line(TRUNCATE_ID, 4, "public.orders", "truncate", TRUNCATE_PAYLOAD, POSITION);
    let dir_name = identity_dir(TRUNCATE_ID);

    let missing_manifest = durable_store("miss-manifest", &line);
    let manifest = missing_manifest.join("events").join(&dir_name).join("manifest.json");
    let payload = fs::read(missing_manifest.join("events").join(&dir_name).join("payload")).expect("payload");
    fs::remove_file(&manifest).expect("harness removes manifest");
    let snap = inventory(&missing_manifest);
    assert_damaged(&missing_manifest, true);
    assert_eq!(inventory(&missing_manifest), snap);
    assert!(!manifest.exists());
    assert_eq!(fs::read(missing_manifest.join("events").join(&dir_name).join("payload")).expect("payload stays"), payload);
    refused_put_keeps(&missing_manifest, &line, &snap);

    let torn = durable_store("torn-manifest", &line);
    let torn_path = torn.join("events").join(&dir_name).join("manifest.json");
    fs::write(&torn_path, b"not-json").expect("harness tears manifest");
    let torn_snap = inventory(&torn);
    assert_damaged(&torn, true);
    assert_eq!(inventory(&torn), torn_snap);
    assert_eq!(fs::read(&torn_path).expect("torn stays"), b"not-json");
    refused_put_keeps(&torn, &line, &torn_snap);

    let missing_receipt = durable_store("miss-receipt", &line);
    let receipt = missing_receipt.join("events").join(&dir_name).join("RECEIPT");
    fs::remove_file(&receipt).expect("harness removes receipt");
    let receipt_snap = inventory(&missing_receipt);
    assert_damaged(&missing_receipt, false);
    assert_eq!(inventory(&missing_receipt), receipt_snap);
    assert!(!receipt.exists());
    refused_put_keeps(&missing_receipt, &line, &receipt_snap);
    assert!(!receipt.exists());

    let wrong = durable_store("wrong-hash", &line);
    let wrong_path = wrong.join("events").join(&dir_name).join("RECEIPT");
    let mut bytes = fs::read(&wrong_path).expect("receipt");
    let last = bytes.len() - 2;
    bytes[last] ^= 0x01;
    fs::write(&wrong_path, &bytes).expect("harness flips receipt");
    let wrong_snap = inventory(&wrong);
    assert_damaged(&wrong, false);
    assert_eq!(inventory(&wrong), wrong_snap);
    assert_eq!(fs::read(&wrong_path).expect("flipped stays"), bytes);
    refused_put_keeps(&wrong, &line, &wrong_snap);
}

fn durable_store(label: &str, line: &[u8]) -> PathBuf {
    let store = scratch(label).join("store");
    let out = run("put", &store, Some(line), &[]);
    assert_eq!(out.code, 0, "{}", out.stdout);
    store
}

fn assert_damaged(store: &Path, unknown: bool) {
    let out = run("recover", store, None, &[]);
    assert_eq!(out.code, 3, "{}", out.stdout);
    honest_json(&out.status);
    assert_eq!(out.status["incomplete"], true);
    assert_eq!(out.status["paused"], true);
    assert_eq!(out.status["durable_quarantined"], false);
    assert_eq!(out.status["local_prerequisite"], false);
    assert_eq!(out.status["ack_eligible"], false);
    assert_eq!(out.status["unknown_incomplete"], unknown);
    silent(&out);
    let api = recover(store);
    assert!(api.incomplete);
    assert!(api.paused);
    assert!(!api.durable_quarantined);
    assert_eq!(api.unknown_incomplete, unknown);
    honest_status(&api);
}

fn refused_put_keeps(store: &Path, line: &[u8], snap: &BTreeMap<String, Vec<u8>>) {
    let out = run("put", store, Some(line), &[]);
    assert_eq!(out.code, 2, "{}", out.stdout);
    assert_eq!(out.status["fault"], "incomplete");
    assert_eq!(out.status["ack_eligible"], false);
    honest_json(&out.status);
    assert_eq!(&inventory(store), snap);
    let replay_out = run("replay", store, Some(line), &[]);
    assert_eq!(replay_out.code, 2, "{}", replay_out.stdout);
    assert_eq!(&inventory(store), snap);
}
