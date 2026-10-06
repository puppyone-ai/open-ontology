#![forbid(unsafe_code)]
#![allow(clippy::bool_assert_comparison)]

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use common::oracle;

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_cdc-quarantine")
        .or(option_env!("CARGO_BIN_EXE_cdc_quarantine"))
        .expect("cdc-quarantine binary")
}

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("oo-a294-q-{}-{n}-{label}", std::process::id()));
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

fn assert_honest(status: &Value) {
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
}

fn assert_silent_payload(out: &Out) {
    assert!(out.stderr.is_empty(), "stderr {}", out.stderr);
    assert!(!out.stdout.contains("TRUNCATE"));
    assert!(!out.stdout.contains("DROP COLUMN"));
    assert!(!out.stdout.contains("SECRET_a294_NOT_A_PAYLOAD"));
    assert!(!out.stderr.contains("SECRET_a294_NOT_A_PAYLOAD"));
}

fn control(
    source: &str,
    event_id: &str,
    sequence: u64,
    collection: &str,
    reason: &str,
    payload: &str,
    position: &str,
) -> Vec<u8> {
    let digest = oracle::hex(&oracle::sha256(payload.as_bytes()));
    let value = json!({
        "kind": "control",
        "source": source,
        "lineage": oracle::LINEAGE,
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

fn truncate_line() -> Vec<u8> {
    control(
        "billing",
        "truncate-orders-1",
        1,
        "public.orders",
        "truncate",
        oracle::TRUNCATE_PAYLOAD,
        oracle::POSITION,
    )
}

fn schema_line() -> Vec<u8> {
    control(
        "billing",
        "schema-lines-1",
        2,
        "public.lines",
        "schema_changed",
        oracle::SCHEMA_PAYLOAD,
        "18446744168199581249",
    )
}

fn inventory(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .collect::<Result<Vec<_>, _>>()
        .expect("entries");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().expect("file type");
        let rel = path
            .strip_prefix(root)
            .expect("prefix")
            .to_str()
            .expect("utf8 rel")
            .to_owned();
        if kind.is_symlink() {
            out.insert(rel, "symlink".to_owned());
            continue;
        }
        if kind.is_dir() {
            walk(&path, root, out);
            continue;
        }
        assert!(kind.is_file(), "not a regular file {}", path.display());
        let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        out.insert(rel, oracle::hex(&oracle::sha256(&bytes)));
    }
}

fn assert_exists(path: &Path, present: bool) {
    assert_eq!(path.exists(), present, "exists {} want {present}", path.display());
}

#[test]
fn known_answer_oracle_matches_literals() {
    assert_eq!(oracle::hex(&oracle::sha256(b"")), oracle::EMPTY_SHA);
    assert_eq!(oracle::hex(&oracle::sha256(b"abc")), oracle::ABC_SHA);
    assert_eq!(
        oracle::hex(&oracle::sha256(oracle::LINEAGE_PREIMAGE.as_bytes())),
        oracle::LINEAGE
    );
    assert_eq!(
        oracle::hex(&oracle::sha256(oracle::TRUNCATE_PAYLOAD.as_bytes())),
        oracle::TRUNCATE_SHA
    );
    assert_eq!(
        oracle::hex(&oracle::sha256(oracle::SCHEMA_PAYLOAD.as_bytes())),
        oracle::SCHEMA_SHA
    );
    assert_eq!(oracle::identity_dir("truncate-orders-1"), oracle::TRUNCATE_DIR);
    assert_eq!(oracle::identity_dir("schema-lines-1"), oracle::SCHEMA_DIR);
    let line = truncate_line();
    let text = std::str::from_utf8(&line).expect("utf8");
    assert!(text.contains(oracle::TRUNCATE_SHA));
    assert!(!text.contains('\r'));
}

#[test]
fn durable_put_then_recover_keeps_receipt_and_flags() {
    let base = scratch("durable");
    let store = base.join("store");
    let input = truncate_line();
    let put = run("put", &store, Some(&input), &[]);
    assert_eq!(put.code, 0, "{}", put.stdout);
    assert_honest(&put.status);
    assert_silent_payload(&put);
    assert_eq!(put.status["durable_quarantined"], true);
    assert_eq!(put.status["paused"], true);
    assert_eq!(put.status["incomplete"], false);
    assert_eq!(put.status["local_prerequisite"], true);
    assert_eq!(put.status["attempt_durable"], true);
    assert_eq!(put.status["replay"], false);
    assert_eq!(put.status["attempt_collection"], "public.orders");
    assert_eq!(put.status["reason"], "truncate");
    assert_eq!(put.status["payload_sha256"], oracle::TRUNCATE_SHA);
    assert_eq!(put.status["source_position"], oracle::POSITION);
    assert_eq!(put.status["paused_collections"][0], "public.orders");

    let dir = store.join("events").join(oracle::TRUNCATE_DIR);
    let payload = fs::read(dir.join("payload")).expect("payload");
    assert_eq!(payload, oracle::TRUNCATE_PAYLOAD.as_bytes());
    assert_eq!(oracle::hex(&oracle::sha256(&payload)), oracle::TRUNCATE_SHA);
    let manifest = fs::read(dir.join("manifest.json")).expect("manifest");
    let manifest_text = std::str::from_utf8(&manifest).expect("manifest utf8");
    assert!(manifest_text.ends_with('\n'));
    assert!(manifest_text.contains("\"local_prerequisite\":false"));
    assert!(manifest_text.contains("\"target_truncated\":false"));
    assert!(manifest_text.contains("\"schema_applied\":false"));
    assert!(manifest_text.contains("\"target_applied\":null"));
    let parsed: Value = serde_json::from_str(manifest_text.trim_end()).expect("manifest json");
    assert_eq!(parsed["paused_collections"][0], "public.orders");
    let receipt = fs::read(dir.join("RECEIPT")).expect("receipt");
    let receipt_text = std::str::from_utf8(&receipt).expect("receipt utf8");
    let manifest_sha = oracle::hex(&oracle::sha256(&manifest));
    let mut lines = receipt_text.lines();
    assert_eq!(lines.next(), Some("durable_quarantined"));
    assert_eq!(lines.next(), Some(oracle::TRUNCATE_SHA));
    assert_eq!(lines.next(), Some(manifest_sha.as_str()));
    assert_eq!(lines.next(), None);
    let store_text = fs::read_to_string(store.join("STORE")).expect("store");
    assert_eq!(
        store_text,
        format!("cdc-quarantine-v1\nbilling\n{}\n1\n", oracle::LINEAGE)
    );

    let again = run("recover", &store, None, &[]);
    assert_eq!(again.code, 0, "{}", again.stdout);
    assert_honest(&again.status);
    assert_eq!(again.status["local_prerequisite"], true);
    assert_eq!(again.status["paused"], true);
    assert_eq!(again.status["durable_quarantined"], true);
    assert_eq!(again.status["incomplete"], false);
    assert_silent_payload(&again);
}

#[test]
fn both_reasons_pause_sorted_collections() {
    let base = scratch("both");
    let store = base.join("store");
    let first = run("put", &store, Some(&truncate_line()), &[]);
    assert_eq!(first.code, 0, "{}", first.stdout);
    let second = run("put", &store, Some(&schema_line()), &[]);
    assert_eq!(second.code, 0, "{}", second.stdout);
    assert_silent_payload(&second);
    assert_eq!(second.status["paused_collections"][0], "public.lines");
    assert_eq!(second.status["paused_collections"][1], "public.orders");
    assert_eq!(second.status["local_prerequisite"], true);
    assert_eq!(second.status["target_truncated"], false);
    assert_eq!(second.status["schema_applied"], false);
    let recovered = run("recover", &store, None, &[]);
    assert_eq!(recovered.code, 0, "{}", recovered.stdout);
    assert_eq!(recovered.status["durable_events"], 2);
    assert_eq!(recovered.status["paused_collections"][0], "public.lines");
    assert_eq!(recovered.status["paused_collections"][1], "public.orders");
    assert_eq!(recovered.status["paused"], true);
    assert_eq!(recovered.status["local_prerequisite"], true);
    assert!(recovered.status["event_id"].is_null());
    let orders = fs::read_to_string(store.join("events").join(oracle::TRUNCATE_DIR).join("manifest.json")).expect("orders");
    let lines = fs::read_to_string(store.join("events").join(oracle::SCHEMA_DIR).join("manifest.json")).expect("lines");
    assert!(orders.contains("\"paused_collections\":[\"public.orders\"]"));
    assert!(lines.contains("\"paused_collections\":[\"public.lines\",\"public.orders\"]"));
}

#[test]
fn exact_replay_does_not_rewrite_bytes() {
    let base = scratch("replay");
    let store = base.join("store");
    let input = truncate_line();
    assert_eq!(run("put", &store, Some(&input), &[]).code, 0);
    let before = inventory(&store);
    let replay = run("replay", &store, Some(&input), &[]);
    assert_eq!(replay.code, 0, "{}", replay.stdout);
    assert_honest(&replay.status);
    assert_eq!(replay.status["replay"], true);
    assert_eq!(replay.status["durable_quarantined"], true);
    assert_eq!(replay.status["local_prerequisite"], true);
    assert_eq!(replay.status["attempt_durable"], true);
    assert_eq!(inventory(&store), before);
    let mut spaced = input.clone();
    spaced.insert(1, b' ');
    let pretty = run("replay", &store, Some(&spaced), &[]);
    assert_eq!(pretty.code, 0, "{}", pretty.stdout);
    assert_eq!(pretty.status["replay"], true);
    assert_eq!(inventory(&store), before);
    let put_again = run("put", &store, Some(&input), &[]);
    assert_eq!(put_again.code, 0, "{}", put_again.stdout);
    assert_eq!(put_again.status["replay"], true);
    assert_eq!(inventory(&store), before);
}

#[test]
fn payload_conflict_keeps_stored_bytes() {
    let base = scratch("conflict");
    let store = base.join("store");
    assert_eq!(run("put", &store, Some(&truncate_line()), &[]).code, 0);
    let before = inventory(&store);
    let other = control(
        "billing",
        "truncate-orders-1",
        1,
        "public.orders",
        "truncate",
        "TRUNCATE public.orders NOW",
        oracle::POSITION,
    );
    let conflict = run("put", &store, Some(&other), &[]);
    assert_eq!(conflict.code, 2, "{}", conflict.stdout);
    assert_eq!(conflict.status["fault"], "payload_conflict");
    assert_eq!(conflict.status["paused"], true);
    assert_eq!(conflict.status["attempt_collection"], "public.orders");
    assert_eq!(conflict.status["durable_quarantined"], false);
    assert_eq!(conflict.status["local_prerequisite"], false);
    assert_eq!(conflict.status["durable_events"], 1);
    assert_honest(&conflict.status);
    assert_eq!(inventory(&store), before);
    let recovered = run("recover", &store, None, &[]);
    assert_eq!(recovered.code, 0, "{}", recovered.stdout);
    assert_eq!(recovered.status["durable_quarantined"], true);
    assert_eq!(recovered.status["local_prerequisite"], true);
}

#[test]
fn digest_and_oversize_pause_without_a_directory() {
    let base = scratch("digest");
    let store = base.join("store");
    let mut bad = truncate_line();
    let text = String::from_utf8(bad.clone()).expect("utf8");
    let flipped = text.replace(oracle::TRUNCATE_SHA, oracle::EMPTY_SHA);
    bad = flipped.into_bytes();
    let digest = run("put", &store, Some(&bad), &[]);
    assert_eq!(digest.code, 2, "{}", digest.stdout);
    assert_eq!(digest.status["fault"], "digest_mismatch");
    assert_eq!(digest.status["paused"], true);
    assert_eq!(digest.status["attempt_collection"], "public.orders");
    assert_eq!(digest.status["durable_quarantined"], false);
    assert_honest(&digest.status);
    assert!(!store.exists());

    let empty = control("billing", "truncate-orders-1", 1, "public.orders", "truncate", "", oracle::POSITION);
    let over = run("put", &store, Some(&empty), &[]);
    assert_eq!(over.code, 2, "{}", over.stdout);
    assert_eq!(over.status["fault"], "payload_too_large");
    assert_eq!(over.status["paused"], true);
    assert_eq!(over.status["attempt_collection"], "public.orders");
    assert!(!store.exists());

    let limited = run("put", &store, Some(&truncate_line()), &[("CDC_QUARANTINE_MAX_PAYLOAD_BYTES", "4")]);
    assert_eq!(limited.code, 2, "{}", limited.stdout);
    assert_eq!(limited.status["fault"], "payload_too_large");
    assert_eq!(limited.status["paused"], true);
    assert!(!store.exists());
}

#[test]
fn quota_pause_does_not_persist_the_refused_event() {
    let base = scratch("quota");
    let disk = base.join("disk");
    let disk_out = run("put", &disk, Some(&truncate_line()), &[("CDC_QUARANTINE_MAX_DISK_BYTES", "1")]);
    assert_eq!(disk_out.code, 2, "{}", disk_out.stdout);
    assert_eq!(disk_out.status["fault"], "disk_quota");
    assert_eq!(disk_out.status["paused"], true);
    assert_eq!(disk_out.status["attempt_collection"], "public.orders");
    assert!(!disk.exists());

    let zero = base.join("zero");
    let zero_out = run("put", &zero, Some(&truncate_line()), &[("CDC_QUARANTINE_MAX_EVENTS", "0")]);
    assert_eq!(zero_out.code, 2, "{}", zero_out.stdout);
    assert_eq!(zero_out.status["fault"], "record_cap");
    assert!(!zero.exists());

    let raised = base.join("raised");
    let raised_out = run(
        "put",
        &raised,
        Some(&truncate_line()),
        &[("CDC_QUARANTINE_MAX_EVENTS", "1025")],
    );
    assert_eq!(raised_out.code, 2, "{}", raised_out.stdout);
    assert_eq!(raised_out.status["fault"], "limit_raised");
    assert_eq!(raised_out.status["paused"], false);
    assert!(!raised.exists());

    let store = base.join("store");
    assert_eq!(
        run("put", &store, Some(&truncate_line()), &[("CDC_QUARANTINE_MAX_EVENTS", "1")]).code,
        0
    );
    let before = inventory(&store);
    let second = run("put", &store, Some(&schema_line()), &[("CDC_QUARANTINE_MAX_EVENTS", "1")]);
    assert_eq!(second.code, 2, "{}", second.stdout);
    assert_eq!(second.status["fault"], "record_cap");
    assert_eq!(second.status["paused"], true);
    assert_eq!(second.status["attempt_collection"], "public.lines");
    assert_eq!(second.status["durable_quarantined"], false);
    assert!(!store.join("events").join(oracle::SCHEMA_DIR).exists());
    assert_eq!(inventory(&store), before);
    let recovered = run("recover", &store, None, &[]);
    assert_eq!(recovered.code, 0, "{}", recovered.stdout);
    assert_eq!(recovered.status["durable_events"], 1);
    assert_eq!(recovered.status["paused_collections"][0], "public.orders");
    assert_eq!(recovered.status["local_prerequisite"], true);
}

#[test]
fn sigkill_checkpoints_stay_incomplete() {
    let stages = [
        "before_data_fsync",
        "before_manifest",
        "before_rename",
        "before_parent_fsync",
    ];
    for stage in stages {
        let base = scratch(stage);
        let store = base.join("store");
        let input = truncate_line();
        kill_at(stage, &base, &store, &input);
        let dir = store.join("events").join(oracle::TRUNCATE_DIR);
        match stage {
            "before_data_fsync" | "before_manifest" => {
                assert_exists(&dir.join("payload.partial"), true);
                assert_exists(&dir.join("payload"), false);
                assert_exists(&dir.join("manifest.json.partial"), false);
                assert_exists(&dir.join("manifest.json"), false);
                assert_exists(&store.join("STORE.partial"), false);
                assert_exists(&store.join("STORE"), false);
            }
            "before_rename" => {
                assert_exists(&dir.join("payload.partial"), true);
                assert_exists(&dir.join("manifest.json.partial"), true);
                assert_exists(&store.join("STORE.partial"), true);
                assert_exists(&dir.join("payload"), false);
                assert_exists(&dir.join("manifest.json"), false);
                assert_exists(&store.join("STORE"), false);
            }
            "before_parent_fsync" => {
                assert_exists(&dir.join("payload"), true);
                assert_exists(&dir.join("manifest.json"), true);
                assert_exists(&store.join("STORE"), true);
                assert_exists(&dir.join("payload.partial"), false);
                assert_exists(&dir.join("manifest.json.partial"), false);
                assert_exists(&store.join("STORE.partial"), false);
            }
            _ => panic!("stage"),
        }
        assert_exists(&dir.join("RECEIPT"), false);
        let snap = inventory(&store);
        assert_incomplete(&store);
        assert_eq!(inventory(&store), snap);
        assert_incomplete(&store);
        let replay = run("replay", &store, Some(&input), &[]);
        assert_eq!(replay.code, 2, "{}", replay.stdout);
        assert_eq!(replay.status["fault"], "incomplete");
        assert_eq!(replay.status["paused"], true);
        assert_eq!(replay.status["incomplete"], true);
        assert_eq!(replay.status["durable_quarantined"], false);
        assert_eq!(replay.status["ack_eligible"], false);
        assert_honest(&replay.status);
        assert_exists(&dir.join("RECEIPT"), false);
        assert_eq!(inventory(&store), snap);
        let second = run("put", &store, Some(&input), &[]);
        assert_eq!(second.code, 2, "{}", second.stdout);
        assert_eq!(second.status["fault"], "incomplete");
        assert_exists(&dir.join("RECEIPT"), false);
        assert_eq!(inventory(&store), snap);
    }
}

fn assert_incomplete(store: &Path) {
    let out = run("recover", store, None, &[]);
    assert_eq!(out.code, 3, "{}", out.stdout);
    assert_honest(&out.status);
    assert_eq!(out.status["incomplete"], true);
    assert_eq!(out.status["paused"], true);
    assert_eq!(out.status["durable_quarantined"], false);
    assert_eq!(out.status["local_prerequisite"], false);
    assert!(out.status["fault"].is_null());
    assert_eq!(out.status["ack_eligible"], false);
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
    // `Child::kill` sends SIGKILL on Unix. The checkpoint sleeps until that signal.
    child.kill().expect("sigkill");
    let output = child.wait_with_output().expect("wait killed");
    assert!(
        saw,
        "no mark for {stage}; stderr {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.code().is_none() || !output.status.success());
}

#[test]
fn altered_hash_and_missing_manifest_are_not_repaired() {
    let base = scratch("altered");
    let store = base.join("store");
    assert_eq!(run("put", &store, Some(&truncate_line()), &[]).code, 0);
    let payload_path = store.join("events").join(oracle::TRUNCATE_DIR).join("payload");
    let mut payload = fs::read(&payload_path).expect("payload");
    payload[0] ^= 0x01;
    fs::write(&payload_path, &payload).expect("alter");
    let altered = inventory(&store);
    assert_incomplete(&store);
    assert_eq!(inventory(&store), altered);
    assert_incomplete(&store);
    assert_eq!(inventory(&store), altered);

    let base2 = scratch("missing-manifest");
    let store2 = base2.join("store");
    assert_eq!(run("put", &store2, Some(&truncate_line()), &[]).code, 0);
    let manifest = store2.join("events").join(oracle::TRUNCATE_DIR).join("manifest.json");
    fs::remove_file(&manifest).expect("remove manifest in the test");
    assert!(!manifest.exists());
    let missing = inventory(&store2);
    assert_incomplete(&store2);
    assert_eq!(inventory(&store2), missing);
    assert!(!manifest.exists());
    let receipt = fs::read(store2.join("events").join(oracle::TRUNCATE_DIR).join("RECEIPT")).expect("receipt");
    assert_incomplete(&store2);
    assert_eq!(
        fs::read(store2.join("events").join(oracle::TRUNCATE_DIR).join("RECEIPT")).expect("receipt again"),
        receipt
    );
}

#[test]
fn symlink_secret_dotdot_and_foreign_names_are_refused() {
    let base = scratch("symlink");
    let secret = base.join("secret");
    fs::write(&secret, b"SECRET_a294_NOT_A_PAYLOAD").expect("secret");
    let linked = base.join("linked");
    std::os::unix::fs::symlink(&secret, &linked).expect("symlink");
    let refused = run("put", &linked, Some(&truncate_line()), &[]);
    assert_eq!(refused.code, 2, "{}", refused.stdout);
    assert_eq!(refused.status["fault"], "path_refused");
    assert!(!refused.stdout.contains("SECRET_a294_NOT_A_PAYLOAD"));
    assert!(!refused.stderr.contains("SECRET_a294_NOT_A_PAYLOAD"));
    assert_eq!(fs::read(&secret).expect("secret bytes"), b"SECRET_a294_NOT_A_PAYLOAD");

    let store = base.join("store");
    assert_eq!(run("put", &store, Some(&truncate_line()), &[]).code, 0);
    let payload = store.join("events").join(oracle::TRUNCATE_DIR).join("payload");
    fs::remove_file(&payload).expect("test removes payload");
    std::os::unix::fs::symlink(&secret, &payload).expect("payload symlink");
    let recovered = run("recover", &store, None, &[]);
    assert_ne!(recovered.code, 0);
    assert!(!recovered.stdout.contains("SECRET_a294_NOT_A_PAYLOAD"));
    assert!(!recovered.stderr.contains("SECRET_a294_NOT_A_PAYLOAD"));
    assert_eq!(fs::read(&secret).expect("secret still"), b"SECRET_a294_NOT_A_PAYLOAD");
    assert!(payload.symlink_metadata().expect("meta").file_type().is_symlink());

    let outside = format!("{}/../oo-a294-refused-dotdot", base.display());
    let dot = run("put", Path::new(&outside), Some(&truncate_line()), &[]);
    assert_eq!(dot.code, 2, "{}", dot.stdout);
    assert_eq!(dot.status["fault"], "path_refused");
    assert!(!Path::new(&format!("{}/oo-a294-refused-dotdot", std::env::temp_dir().display())).exists());

    let foreign = base.join("foreign");
    fs::create_dir(&foreign).expect("foreign");
    fs::write(foreign.join("alien.txt"), b"KEEP\n").expect("alien");
    let foreign_out = run("put", &foreign, Some(&truncate_line()), &[]);
    assert_eq!(foreign_out.code, 2, "{}", foreign_out.stdout);
    assert_eq!(foreign_out.status["fault"], "foreign_directory");
    assert_eq!(foreign_out.status["paused"], true);
    assert_eq!(fs::read(foreign.join("alien.txt")).expect("alien"), b"KEEP\n");
    assert!(!foreign.join("events").exists());

    let torn = base.join("events-only");
    fs::create_dir(&torn).expect("torn");
    fs::create_dir(torn.join("events")).expect("events");
    let torn_put = run("put", &torn, Some(&truncate_line()), &[]);
    assert_eq!(torn_put.code, 2, "{}", torn_put.stdout);
    assert_eq!(torn_put.status["fault"], "incomplete");
    assert!(!torn.join("STORE").exists());
    let torn_recover = run("recover", &torn, None, &[]);
    assert_eq!(torn_recover.code, 3, "{}", torn_recover.stdout);
    assert_eq!(torn_recover.status["paused"], true);
    assert_eq!(torn_recover.status["incomplete"], true);
    assert!(torn_recover.status["fault"].is_null());
}

#[test]
fn binding_conflict_writes_nothing() {
    let base = scratch("binding");
    let store = base.join("store");
    assert_eq!(run("put", &store, Some(&truncate_line()), &[]).code, 0);
    let before = inventory(&store);
    let other = control(
        "warehouse",
        "truncate-orders-2",
        1,
        "public.orders",
        "truncate",
        oracle::TRUNCATE_PAYLOAD,
        oracle::POSITION,
    );
    let conflict = run("put", &store, Some(&other), &[]);
    assert_eq!(conflict.code, 2, "{}", conflict.stdout);
    assert_eq!(conflict.status["fault"], "binding_conflict");
    assert_eq!(conflict.status["paused"], true);
    assert_eq!(conflict.status["durable_quarantined"], false);
    assert_eq!(inventory(&store), before);
    let mut names = Vec::new();
    for entry in fs::read_dir(store.join("events")).expect("events") {
        names.push(entry.expect("entry").file_name());
    }
    assert_eq!(names.len(), 1);
}

#[test]
fn malformed_inputs_are_not_stored() {
    let base = scratch("malformed");
    let store = base.join("store");
    let duplicate = b"{\"kind\":\"control\",\"kind\":\"control\"}\n";
    let dup = run("put", &store, Some(duplicate), &[]);
    assert_eq!(dup.code, 2, "{}", dup.stdout);
    assert_eq!(dup.status["fault"], "malformed");
    assert_eq!(dup.status["paused"], false);
    assert!(!store.exists());

    let mut password = truncate_line();
    let text = String::from_utf8(password).expect("utf8");
    let with_secret = text.replacen('{', "{\"password\":\"SECRET_a294_NOT_A_PAYLOAD\",", 1);
    password = with_secret.into_bytes();
    let extra = run("put", &store, Some(&password), &[]);
    assert_eq!(extra.code, 2, "{}", extra.stdout);
    assert_eq!(extra.status["fault"], "malformed");
    assert!(!extra.stdout.contains("SECRET_a294_NOT_A_PAYLOAD"));
    assert!(!store.exists());

    let mut long = vec![b'a'; 1_048_577];
    long.push(b'\n');
    let too_long = run("put", &store, Some(&long), &[]);
    assert_eq!(too_long.code, 2, "{}", too_long.stdout);
    assert_eq!(too_long.status["fault"], "line_too_long");
    assert_eq!(too_long.status["paused"], false);
    assert!(!store.exists());

    let mut two = truncate_line();
    two.extend_from_slice(b"{\"kind\":\"control\"}\n");
    let store2 = base.join("two");
    let put = run("put", &store2, Some(&two), &[]);
    assert_eq!(put.code, 0, "{}", put.stdout);
    assert!(store2.join("events").join(oracle::TRUNCATE_DIR).join("RECEIPT").is_file());
    let mut count = 0;
    for entry in fs::read_dir(store2.join("events")).expect("events") {
        count += 1;
        let _ = entry.expect("entry");
    }
    assert_eq!(count, 1);
}

#[test]
fn mark_inside_the_store_returns_instead_of_sleeping() {
    let base = scratch("inside-mark");
    let store = base.join("store");
    let mark = store.join("mark");
    let dir = store.to_str().expect("utf8");
    let mut child = Command::new(bin())
        .arg("put")
        .arg(dir)
        .env("CDC_QUARANTINE_CHECKPOINT", "before_data_fsync")
        .env("CDC_QUARANTINE_MARK", mark.to_str().expect("mark"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(&truncate_line()).expect("write");
    }
    let start = Instant::now();
    let finished = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break Some(status);
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let status = finished.expect("checkpoint with an interior mark must exit");
    assert_eq!(status.code(), Some(1));
    assert!(!mark.exists());
    assert!(!store.join("events").join(oracle::TRUNCATE_DIR).join("RECEIPT").exists());
}

#[test]
fn product_source_has_no_repair_or_unsafe() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = 0;
    for entry in fs::read_dir(&src).expect("src") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        files += 1;
        let text = fs::read_to_string(&path).expect("read src");
        let library = text.split("#[cfg(test)]").next().expect("library text");
        assert!(!library.contains("remove_file"), "{}", path.display());
        assert!(!library.contains("remove_dir"), "{}", path.display());
        assert!(!library.contains("canonicalize"), "{}", path.display());
        assert!(!library.contains("create_dir_all"), "{}", path.display());
        assert!(!library.contains(".unwrap("), "{}", path.display());
        assert!(!library.contains(".expect("), "{}", path.display());
        for line in library.lines() {
            if line.contains("forbid(unsafe_code)") {
                continue;
            }
            assert!(!line.contains("unsafe"), "{}: {line}", path.display());
        }
    }
    assert!(files >= 6);
}
