//! Closed rules that do not need PostgreSQL.

use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cdc_offset_store::{
    checkpoint, cmp_pos, hex, migration_sha256, pos_ok, receipt_document, receipt_hash, same_batch,
    sha256, validate_mirror, Checkpoint, CheckpointRef, OrderBinding, Output, Receipt, Request,
    Watermark, BOOTSTRAP_SHA256, FENCE_SHA256, HISTORICAL_V2_SHA256, PREFLIGHT_SHA256, PROVENANCE,
};

fn digest() -> String {
    "ab".repeat(32)
}

fn receipt(
    table: &str,
    txn: &str,
    pos: &str,
    epoch: i32,
    partial: bool,
    cross: bool,
    complete: bool,
) -> Receipt {
    let segmented = partial;
    Receipt {
        source: "billing".to_owned(),
        table_generation: table.to_owned(),
        txn_or_batch_id: txn.to_owned(),
        config_version: "config/v1".to_owned(),
        config_digest: digest(),
        type_policy_version: "type-policy/v1".to_owned(),
        type_digest: digest(),
        hook_digest: digest(),
        schema_digest: digest(),
        payload_digest: digest(),
        protocol_version: "batch/v1".to_owned(),
        position_codec: "pg-commit-lsn/v1".to_owned(),
        position_lineage: "alineage".to_owned(),
        source_epoch: epoch,
        applied_pos: pos.to_owned(),
        partial_txn: partial,
        cross_table_consistent: cross,
        txn_complete: complete,
        segment_index: segmented.then_some(0),
        segment_count: segmented.then_some(2),
        segment_bitmap: if segmented { vec![0x01] } else { Vec::new() },
        fence_shard: "billing".to_owned(),
        fence_generation: 1,
        target_commit_at: "2026-10-06T00:00:00.000000Z".to_owned(),
    }
}

fn previous(txn: &str, pos: &str, epoch: i32) -> CheckpointRef {
    CheckpointRef {
        position_codec: "pg-commit-lsn/v1".to_owned(),
        position_lineage: "alineage".to_owned(),
        source_epoch: epoch,
        applied_pos: pos.to_owned(),
        txn_or_batch_id: txn.to_owned(),
    }
}

fn binding(
    expected: Option<CheckpointRef>,
    tables: &[&str],
    txn: &str,
    pos: &str,
    next_txn: &str,
    next_pos: &str,
) -> OrderBinding {
    serde_json::from_value(serde_json::json!({
        "provenance": PROVENANCE,
        "expected_previous": expected.map(|item| serde_json::json!({
            "position_codec": item.position_codec,
            "position_lineage": item.position_lineage,
            "source_epoch": item.source_epoch,
            "applied_pos": item.applied_pos,
            "txn_or_batch_id": item.txn_or_batch_id,
        })),
        "declared_transaction": {
            "txn_or_batch_id": txn,
            "applied_pos": pos,
            "required_table_generations": tables,
        },
        "declared_next": {"txn_or_batch_id": next_txn, "applied_pos": next_pos},
        "source_gap": false,
        "epoch_barrier": false,
    }))
    .unwrap()
}

fn mark(pos: &str, txn: &str, next_txn: &str, next_pos: &str) -> Watermark {
    Watermark {
        position_codec: "pg-commit-lsn/v1".to_owned(),
        position_lineage: "alineage".to_owned(),
        source_epoch: 1,
        applied_pos: pos.to_owned(),
        applied_txn_id: txn.to_owned(),
        declared_next_txn: next_txn.to_owned(),
        declared_next_pos: next_pos.to_owned(),
    }
}

#[test]
fn sha256_known_answers() {
    let empty = sha256(b"").unwrap();
    assert_eq!(hex(&empty), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    let abc = sha256(b"abc").unwrap();
    assert_eq!(hex(&abc), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn receipt_document_is_stable() {
    let row = receipt("orders", "txn-1", "10", 1, false, true, true);
    let text = receipt_document(&row);
    assert!(text.starts_with("cdc-offset-receipt/v1\n"));
    assert!(text.contains("applied_pos=10\n"));
    assert!(text.contains("segment_index=-\n"));
    assert!(text.contains("segment_bitmap=-\n"));
    let again = receipt_hash(&row).unwrap();
    assert_eq!(again, hex(&sha256(text.as_bytes()).unwrap()));
    let mut other = row.clone();
    other.payload_digest = "cd".repeat(32);
    assert_ne!(receipt_hash(&other).unwrap(), again);
}

#[test]
fn position_order_is_numeric_text() {
    assert!(pos_ok("10"));
    assert!(pos_ok("0"));
    assert!(!pos_ok("099"));
    assert_eq!(cmp_pos("10", "2"), Ordering::Greater);
    assert_eq!(cmp_pos("10", "10"), Ordering::Equal);
    assert_eq!(cmp_pos("099", "100"), Ordering::Less);
}

#[test]
fn checkpoint_holds_and_never_maxes_a_hole() {
    let complete = receipt("orders", "txn-1", "10", 1, false, true, true);
    let lines = receipt("lines", "txn-1", "10", 1, false, true, true);
    let first = binding(None, &["lines", "orders"], "txn-1", "10", "txn-2", "20");
    assert_eq!(
        checkpoint(&complete, &[complete.clone(), lines], None, &first).unwrap(),
        Checkpoint::InsertFirst
    );

    let partial = receipt("orders", "txn-p", "15", 1, true, false, false);
    let partial_binding = binding(None, &["orders"], "txn-p", "15", "txn-p2", "16");
    assert_eq!(
        checkpoint(&partial, std::slice::from_ref(&partial), None, &partial_binding).unwrap(),
        Checkpoint::Skip("partial_segment")
    );

    let missing = binding(None, &["lines", "orders"], "txn-1", "10", "txn-2", "20");
    assert_eq!(
        checkpoint(&complete, std::slice::from_ref(&complete), None, &missing).unwrap(),
        Checkpoint::Skip("required_incomplete")
    );

    let stored = mark("10", "txn-1", "txn-2", "20");
    let later = receipt("orders", "txn-3", "40", 1, false, true, true);
    let gap = binding(
        Some(previous("txn-1", "10", 1)),
        &["orders"],
        "txn-3",
        "40",
        "txn-4",
        "50",
    );
    assert_eq!(
        checkpoint(&later, std::slice::from_ref(&later), Some(&stored), &gap).unwrap(),
        Checkpoint::Skip("source_gap")
    );

    let epoch2 = receipt("orders", "txn-e2", "100", 2, false, true, true);
    let barrier = binding(
        Some(previous("txn-1", "10", 1)),
        &["orders"],
        "txn-e2",
        "100",
        "txn-e3",
        "110",
    );
    assert_eq!(
        checkpoint(&epoch2, std::slice::from_ref(&epoch2), Some(&stored), &barrier).unwrap(),
        Checkpoint::Skip("epoch_barrier")
    );

    let next = receipt("orders", "txn-2", "20", 1, false, true, true);
    let cas = binding(
        Some(previous("txn-1", "10", 1)),
        &["orders"],
        "txn-2",
        "20",
        "txn-4",
        "50",
    );
    assert_eq!(
        checkpoint(&next, std::slice::from_ref(&next), Some(&stored), &cas).unwrap(),
        Checkpoint::CompareAndSwap
    );
}

#[test]
fn same_hash_replays_and_a_different_hash_conflicts() {
    let row = receipt("orders", "txn-1", "10", 1, false, true, true);
    let hash = receipt_hash(&row).unwrap();
    assert!(same_batch(&hash, &hash));
    assert!(!same_batch(&hash, &"cd".repeat(32)));
}

#[test]
fn output_does_not_certify_ack_or_fence() {
    let refused = Output::refused("input");
    assert!(!refused.durable_control_mirror);
    assert!(!refused.checkpoint_advanced);
    assert!(!refused.source_applied);
    assert!(!refused.fence_proof);
    assert!(!refused.ack_allowed);
    assert!(!refused.external_xack);
    assert!(!refused.external_xtrim);
    assert!(!refused.routing_verified);
    assert!(!refused.real_source_provenance_verified);
    assert!(!refused.debezium_normalizer);
    assert_eq!(refused.checkpoint_provenance, PROVENANCE);
    let unknown = Output::unknown();
    assert_eq!(unknown.kind, "commit_unknown");
    assert!(!unknown.durable_control_mirror);
    assert!(!unknown.external_xack);
    assert!(!unknown.external_xtrim);
}

#[test]
fn caller_cannot_supply_a_receipt_body() {
    let mut value = sample_request();
    value["payload_digest"] = serde_json::json!(digest());
    assert!(serde_json::from_value::<Request>(value).is_err());
}

#[test]
fn same_database_on_one_host_is_refused() {
    let mut value = sample_request();
    value["target"]["database"] = serde_json::json!("oo_same");
    value["control"]["database"] = serde_json::json!("oo_same");
    value["target"]["host"] = serde_json::json!("db.example");
    value["target"]["server_name"] = serde_json::json!("db.example");
    value["control"]["host"] = serde_json::json!("db.example");
    value["control"]["server_name"] = serde_json::json!("db.example");
    let request: Request = serde_json::from_value(value).unwrap();
    assert_eq!(validate_mirror(&request), Err("input"));
}

#[test]
fn migration_constants_match_the_fixed_scripts() {
    assert_eq!(BOOTSTRAP_SHA256, "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208");
    assert_eq!(FENCE_SHA256, "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9");
    assert_eq!(PREFLIGHT_SHA256, "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3");
    assert_eq!(HISTORICAL_V2_SHA256, "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512");
    let Some(dir) = std::env::var_os("CDC_OFFSET_MIGRATIONS") else {
        return;
    };
    let dir = PathBuf::from(dir);
    for (name, want) in [
        ("0000_bootstrap_roles.sql", BOOTSTRAP_SHA256),
        ("0001_target_fence_expand.sql", FENCE_SHA256),
        ("0003_target_segment_preflight_expand.sql", PREFLIGHT_SHA256),
    ] {
        let bytes = fs::read(dir.join(name)).unwrap_or_else(|_| panic!("missing {name}"));
        assert_eq!(hex(&sha256(&bytes).unwrap()), want, "{name}");
    }
}

#[test]
fn control_sql_is_immutable_and_has_no_max() {
    let sql = include_migration();
    assert!(!sql.contains("IF NOT EXISTS"));
    assert!(!sql.contains("CREATE OR REPLACE"));
    assert!(!sql.contains("DROP"));
    assert!(!sql.to_lowercase().contains("max("));
    assert!(sql.contains(PROVENANCE));
    assert!(!sql.contains("apply_receipts"));
    let hash = migration_sha256().unwrap();
    assert_eq!(hash, hex(&sha256(sql.as_bytes()).unwrap()));
    let mut source = String::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for name in ["lib.rs", "main.rs", "model.rs", "rules.rs", "sql.rs", "store.rs", "tls.rs"] {
        source.push_str(&fs::read_to_string(root.join(name)).unwrap());
    }
    assert!(!source.contains("XACK"));
    assert!(!source.contains("XTRIM"));
    assert!(!source.contains(".max("));
}

fn include_migration() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations/oo_cdc_offset_control_v1.sql"),
    )
    .unwrap()
}

#[test]
fn invalid_json_exits_refused() {
    let path = std::env::temp_dir().join("cdc-offset-store-bad.json");
    fs::write(&path, b"{\"contract\":").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cdc-offset-store"))
        .arg("mirror")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["fault"], "input");
    assert_eq!(value["durable_control_mirror"], false);
    assert_eq!(value["external_xack"], false);
    assert_eq!(value["external_xtrim"], false);
    let usage = Command::new(env!("CARGO_BIN_EXE_cdc-offset-store")).output().unwrap();
    assert_eq!(usage.status.code(), Some(1));
}

fn sample_request() -> serde_json::Value {
    serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "target": sample_endpoint("oo_target"),
        "control": sample_endpoint("oo_control"),
        "source": "billing",
        "table_generation": "orders",
        "txn_or_batch_id": "txn-1",
        "order_binding": {
            "provenance": PROVENANCE,
            "expected_previous": null,
            "declared_transaction": {
                "txn_or_batch_id": "txn-1",
                "applied_pos": "10",
                "required_table_generations": ["orders"]
            },
            "declared_next": {"txn_or_batch_id": "txn-2", "applied_pos": "20"},
            "source_gap": false,
            "epoch_barrier": false
        }
    })
}

fn maria(table: &str, txn: &str, lineage: &str, pos: &str, epoch: i32) -> Receipt {
    let mut row = receipt(table, txn, pos, epoch, false, true, true);
    row.position_codec = "mariadb-gtid/v1".to_owned();
    row.position_lineage = lineage.to_owned();
    row
}

fn maria_previous(txn: &str, lineage: &str, pos: &str, epoch: i32) -> CheckpointRef {
    let mut row = previous(txn, pos, epoch);
    row.position_codec = "mariadb-gtid/v1".to_owned();
    row.position_lineage = lineage.to_owned();
    row
}

fn maria_mark(lineage: &str, pos: &str, txn: &str, next_txn: &str, next_pos: &str) -> Watermark {
    let mut row = mark(pos, txn, next_txn, next_pos);
    row.position_codec = "mariadb-gtid/v1".to_owned();
    row.position_lineage = lineage.to_owned();
    row
}

#[test]
fn mariadb_gtid_does_not_max_a_set_or_another_domain() {
    let row = maria("orders", "txn-1", "d0.s1", "6", 1);
    let first = binding(None, &["orders"], "txn-1", "6", "txn-2", "7");
    assert_eq!(
        checkpoint(&row, std::slice::from_ref(&row), None, &first).unwrap(),
        Checkpoint::InsertFirst
    );

    let stored = maria_mark("d0.s1", "6", "txn-1", "txn-2", "7");
    let next = maria("orders", "txn-2", "d0.s1", "7", 1);
    let cas = binding(
        Some(maria_previous("txn-1", "d0.s1", "6", 1)),
        &["orders"],
        "txn-2",
        "7",
        "txn-3",
        "8",
    );
    assert_eq!(
        checkpoint(&next, std::slice::from_ref(&next), Some(&stored), &cas).unwrap(),
        Checkpoint::CompareAndSwap
    );

    let hole = maria("orders", "txn-8", "d0.s1", "8", 1);
    let skipped = binding(
        Some(maria_previous("txn-1", "d0.s1", "6", 1)),
        &["orders"],
        "txn-8",
        "8",
        "txn-9",
        "9",
    );
    assert_eq!(
        checkpoint(&hole, std::slice::from_ref(&hole), Some(&stored), &skipped).unwrap(),
        Checkpoint::Skip("source_gap")
    );

    let other_domain = maria("orders", "txn-9", "d1.s1", "100", 1);
    let crossed = binding(
        Some(maria_previous("txn-1", "d0.s1", "6", 1)),
        &["orders"],
        "txn-9",
        "100",
        "txn-10",
        "101",
    );
    assert_eq!(
        checkpoint(&other_domain, std::slice::from_ref(&other_domain), Some(&stored), &crossed).unwrap(),
        Checkpoint::Skip("lineage_barrier")
    );

    let other_server = maria("orders", "txn-9", "d0.s2", "7", 1);
    let server_cross = binding(
        Some(maria_previous("txn-1", "d0.s1", "6", 1)),
        &["orders"],
        "txn-9",
        "7",
        "txn-10",
        "8",
    );
    assert_eq!(
        checkpoint(&other_server, std::slice::from_ref(&other_server), Some(&stored), &server_cross).unwrap(),
        Checkpoint::Skip("lineage_barrier")
    );

    let replay = binding(
        Some(maria_previous("txn-1", "d0.s1", "6", 1)),
        &["orders"],
        "txn-1",
        "6",
        "txn-2",
        "7",
    );
    assert_eq!(
        checkpoint(&row, std::slice::from_ref(&row), Some(&stored), &replay).unwrap(),
        Checkpoint::Skip("already_checkpoint")
    );

    let epoch = maria("orders", "txn-e", "d0.s1", "7", 2);
    let failover = binding(
        Some(maria_previous("txn-1", "d0.s1", "6", 1)),
        &["orders"],
        "txn-e",
        "7",
        "txn-f",
        "8",
    );
    assert_eq!(
        checkpoint(&epoch, std::slice::from_ref(&epoch), Some(&stored), &failover).unwrap(),
        Checkpoint::Skip("epoch_barrier")
    );

    let mut raw = row.clone();
    raw.applied_pos = "0-1-6".to_owned();
    assert_eq!(
        checkpoint(&raw, std::slice::from_ref(&raw), None, &first),
        Err("receipt_rule")
    );
    let mut set = row.clone();
    set.position_lineage = "d0.s1,d1.s1".to_owned();
    assert_eq!(
        checkpoint(&set, std::slice::from_ref(&set), None, &first),
        Err("receipt_rule")
    );
    let mut file_pos = row.clone();
    file_pos.position_codec = "mysql-binlog-pos/v1".to_owned();
    file_pos.position_lineage = "alineage".to_owned();
    file_pos.applied_pos = "mariadb-bin.000001".to_owned();
    assert_eq!(
        checkpoint(&file_pos, std::slice::from_ref(&file_pos), None, &first),
        Err("receipt_rule")
    );
    let mut mysql = row.clone();
    mysql.position_codec = "mysql-gtid/v1".to_owned();
    mysql.position_lineage = "src.3e11fa47-71ca-11e1-9e33-c80aa9429562".to_owned();
    mysql.applied_pos = "23".to_owned();
    assert_eq!(
        checkpoint(&mysql, std::slice::from_ref(&mysql), None, &first),
        Err("receipt_rule")
    );
}

fn sample_endpoint(database: &str) -> serde_json::Value {
    serde_json::json!({
        "host": "db.example",
        "port": 5432,
        "database": database,
        "user": "oo_reader",
        "password_file": "/tmp/reader.pass",
        "ca_file": "/tmp/ca.crt",
        "server_name": "db.example"
    })
}
