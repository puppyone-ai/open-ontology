//! Executable planner journeys. The spool and offset decisions call the
//! fixed crates. Nothing here is a mock of those receipts.

#![allow(clippy::bool_assert_comparison)]

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cdc_offset_store::PROVENANCE;
use cdc_spool::{Limits, OutcomeKind, Spool};
use serde_json::{json, Value};
use source_capture_policy::{
    AsOfInput, CaptureMethod, CursorInput, CursorSafety, DeleteDetection, Grade, JoinInput,
    MethodInput, OffsetBinding, OffsetDeclared, OffsetInput, OffsetNext, OffsetReceipt,
    OffsetWatermark, PlanRequest, Profile, RequestedConsistency, RetentionInput, SupportGrade,
    SyncMode, TableInput, TokenOrder, WriteClass, CONTRACT,
};

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const POSITION: &str = "18446744168199581248";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("oo-dl01w1-03-{nanos}-{n}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn digest() -> String {
    "ab".repeat(32)
}

fn run(request: &PlanRequest) -> (i32, Value) {
    let dir = temp();
    let path = dir.join("request.json");
    fs::write(&path, serde_json::to_vec(request).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_source-capture-policy"))
        .arg("plan")
        .arg(&path)
        .output()
        .unwrap();
    let code = output.status.code().unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    (code, value)
}

fn pg_method(position: &str, lag: u64, window: u64) -> MethodInput {
    MethodInput {
        method: CaptureMethod::C1,
        support_grade: SupportGrade::Verified,
        write_class: WriteClass::R1,
        r1w_adr: false,
        r1w_audit: false,
        token_order: TokenOrder::Total,
        delete_detection: DeleteDetection::Native,
        consistency_ceiling: Grade::T,
        transaction_boundary: true,
        token_available: false,
        partition_metadata_trusted: false,
        full_partition_replay: false,
        family: "postgresql".to_owned(),
        position_codec: "pg-commit-lsn/v1".to_owned(),
        position_lineage: "alineage".to_owned(),
        source_epoch: 1,
        applied_pos: Some(position.to_owned()),
        window_low: Some("1".to_owned()),
        window_high: Some(position.to_owned()),
        same_source_uuid: true,
        same_instance: true,
        failover_observed: false,
        failover_position_retained: None,
        soft_delete_only: false,
        precision_truncated: false,
        original_column_retained_as_key: false,
        used_as_primary_key: true,
        reconcile_period_seconds: None,
        delete_visibility_sla_seconds: None,
        cursor: None,
        retention: Some(RetentionInput {
            window_seconds: window,
            lag_seconds: lag,
        }),
        as_of: None,
        history_lost: false,
    }
}

fn table(name: &str, generation: &str, method: MethodInput) -> TableInput {
    TableInput {
        name: name.to_owned(),
        generation: generation.to_owned(),
        stable_key: true,
        declared_unique_key: true,
        row_estimate: 10,
        source_changing_without_token: false,
        budget_exhausted: false,
        explicit_method: None,
        methods: vec![method],
    }
}

fn request(tables: Vec<TableInput>, joins: Vec<JoinInput>) -> PlanRequest {
    PlanRequest {
        contract: CONTRACT.to_owned(),
        profile: Profile::Production,
        sync: SyncMode::Auto,
        requested_consistency: Some(RequestedConsistency::Transactional),
        source_id: "billing".to_owned(),
        volume_limit_rows: 1_000_000,
        repair_requested: true,
        spool_dir: None,
        offset: None,
        tables,
        joins,
    }
}

fn receipt(position: &str) -> OffsetReceipt {
    OffsetReceipt {
        source: "billing".to_owned(),
        table_generation: "orders_g1".to_owned(),
        txn_or_batch_id: "inv17".to_owned(),
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
        source_epoch: 1,
        applied_pos: position.to_owned(),
        partial_txn: false,
        cross_table_consistent: true,
        txn_complete: true,
        segment_index: None,
        segment_count: None,
        segment_bitmap: Vec::new(),
        fence_shard: "billing".to_owned(),
        fence_generation: 1,
        target_commit_at: "2026-10-06T00:00:00.000000Z".to_owned(),
    }
}

fn offset(position: &str, source_gap: bool) -> OffsetInput {
    OffsetInput {
        receipts: vec![receipt(position)],
        selected_generation: "orders_g1".to_owned(),
        watermark: Some(OffsetWatermark {
            position_codec: "pg-commit-lsn/v1".to_owned(),
            position_lineage: "alineage".to_owned(),
            source_epoch: 1,
            applied_pos: "1".to_owned(),
            applied_txn_id: "prevtxn".to_owned(),
            declared_next_txn: "inv17".to_owned(),
            declared_next_pos: position.to_owned(),
        }),
        binding: OffsetBinding {
            provenance: PROVENANCE.to_owned(),
            expected_previous: Some(source_capture_policy::OffsetCheckpointRef {
                position_codec: "pg-commit-lsn/v1".to_owned(),
                position_lineage: "alineage".to_owned(),
                source_epoch: 1,
                applied_pos: "1".to_owned(),
                txn_or_batch_id: "prevtxn".to_owned(),
            }),
            declared_transaction: OffsetDeclared {
                txn_or_batch_id: "inv17".to_owned(),
                applied_pos: position.to_owned(),
                required_table_generations: vec!["orders_g1".to_owned()],
            },
            declared_next: OffsetNext {
                txn_or_batch_id: "nexttxn".to_owned(),
                applied_pos: "2".to_owned(),
            },
            source_gap,
            epoch_barrier: false,
        },
    }
}

fn feed() -> Vec<u8> {
    let lines = [
        json!({
            "kind": "bind",
            "source": "billing",
            "lineage": HASH,
            "epoch": 1,
            "selected_tables": ["public.orders"],
            "schema_hash": HASH,
            "config_hash": HASH,
            "type_hash": HASH,
            "hook_hashes": [],
            "commit_position_provenance": "caller_declared"
        })
        .to_string(),
        json!({"kind": "begin", "txn_id": "inv17"}).to_string(),
        json!({
            "kind": "data",
            "txn_id": "inv17",
            "global_txn_sequence": 1,
            "data_collection_sequence": 1,
            "table": "public.orders",
            "key": [{"type": "int", "ordinal": 0, "value": "17"}],
            "op": "upsert",
            "payload": [{"type": "text", "ordinal": 0, "value": "open"}],
            "unavailable_ordinals": []
        })
        .to_string(),
        json!({
            "kind": "end",
            "txn_id": "inv17",
            "count": 1,
            "per_collection_counts": {"public.orders": 1},
            "commit_lsn": "16/B6E40",
            "source_commit_timestamp": "2026-04-15T08:30:00.123456Z"
        })
        .to_string(),
    ];
    let mut out = Vec::new();
    for line in lines {
        out.extend(line.into_bytes());
        out.push(b'\n');
    }
    out
}

fn seal() -> PathBuf {
    let dir = temp().join("spool");
    let mut spool = Spool::create_new(&dir, Limits::product()).unwrap();
    let outcome = spool.ingest_reader(Cursor::new(feed())).unwrap();
    assert!(matches!(outcome.kind, OutcomeKind::Sealed(_)));
    dir
}

fn rollback_spool() -> PathBuf {
    let dir = temp().join("spool");
    let mut spool = Spool::create_new(&dir, Limits::product()).unwrap();
    let bytes = {
        let lines = [
            json!({
                "kind": "bind",
                "source": "billing",
                "lineage": HASH,
                "epoch": 1,
                "selected_tables": ["public.orders"],
                "schema_hash": HASH,
                "config_hash": HASH,
                "type_hash": HASH,
                "hook_hashes": [],
                "commit_position_provenance": "caller_declared"
            })
            .to_string(),
            json!({"kind": "begin", "txn_id": "inv17"}).to_string(),
            json!({"kind": "rollback", "txn_id": "inv17"}).to_string(),
        ];
        let mut out = Vec::new();
        for line in lines {
            out.extend(line.into_bytes());
            out.push(b'\n');
        }
        out
    };
    let outcome = spool.ingest_reader(Cursor::new(bytes)).unwrap();
    assert!(matches!(outcome.kind, OutcomeKind::RolledBack { .. }));
    dir
}

#[test]
fn positive_c1_uses_real_spool_and_offset_receipt() {
    let dir = seal();
    let mut plan = request(
        vec![table("public.orders", "orders_g1", pg_method(POSITION, 10, 100_000))],
        vec![JoinInput {
            tables: vec!["public.orders".to_owned()],
            source_ids: vec!["billing".to_owned()],
        }],
    );
    plan.spool_dir = Some(dir.to_string_lossy().into_owned());
    plan.offset = Some(offset(POSITION, false));
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["kind"], "decision");
    assert_eq!(body["downgraded"], false);
    assert_eq!(body["configuration_accepted"], true);
    assert_eq!(body["requested_consistency_result"], "accepted");
    assert_eq!(body["source_grade"], "T");
    assert_eq!(body["transactional_allowed"], true);
    assert_eq!(body["repair_allowed"], true);
    assert_eq!(body["consistency_claim_allowed"], true);
    assert_eq!(body["advance_capture_position"], true);
    assert_eq!(body["continue_from_latest"], true);
    assert_eq!(body["operator_action"], "continue");
    assert_eq!(body["loader_integration"], "unaccepted");
    assert_eq!(body["scheduler_integration"], "unaccepted");
    assert_eq!(body["live_offset_mirror"], "not_run");
    assert_eq!(body["tables"][0]["capture_method"], "C1");
    assert_eq!(body["tables"][0]["consistency_grade"], "T");
    assert_eq!(body["tables"][0]["initial_load"], "cdc_first_snapshot_fill");
    assert_eq!(body["tables"][0]["freshness"]["capture_method"], "C1");
    assert_eq!(body["joins"][0]["warning"], Value::Null);
    assert_eq!(body["spool"]["state"], "committed");
    assert_eq!(body["spool"]["commit_position"], POSITION);
    assert_eq!(body["spool"]["ack_allowed"], false);
    assert_eq!(body["spool"]["target_applied"], false);
    assert_eq!(body["spool"]["cross_table_consistent"], false);
    assert_eq!(body["spool"]["applied"], Value::Null);
    assert_eq!(body["spool"]["authorizes_ack"], false);
    assert_eq!(body["spool"]["authorizes_publish"], false);
    assert_eq!(body["offset"]["checkpoint"], "compare_and_swap");
    assert_eq!(body["offset"]["checkpoint_advanced"], true);
    assert_eq!(body["offset"]["receipt_closed"], true);
    assert_eq!(body["offset"]["source_applied"], false);
    assert_eq!(body["offset"]["ack_allowed"], false);
    assert_eq!(body["offset"]["durable_control_mirror"], false);
    assert_eq!(body["offset"]["live_mirror"], "not_run");
    let hash = body["offset"]["receipt_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn transactional_below_t_is_denied_without_downgrade() {
    let mut method = pg_method("100", 1, 1000);
    method.method = CaptureMethod::C3;
    method.consistency_ceiling = Grade::B;
    method.transaction_boundary = false;
    method.token_order = TokenOrder::Opaque;
    method.position_codec = "batch-cursor-us/v1".to_owned();
    method.applied_pos = None;
    method.window_low = None;
    method.window_high = None;
    method.retention = None;
    method.delete_detection = DeleteDetection::Reconcile;
    method.reconcile_period_seconds = Some(86_400);
    method.delete_visibility_sla_seconds = Some(86_400);
    method.cursor = Some(CursorInput {
        safety: CursorSafety::StatementTime,
        overlap_seconds: 30,
        max_transaction_age_seconds: Some(30),
        conservative_max_transaction_age_seconds: None,
        upper_bound_is_commit_safe: false,
        reconcile_configured: true,
        equal_cursor_checkpoint: true,
        admin_confirmed: false,
    });
    let mut plan = request(vec![table("public.orders", "orders_g1", method)], vec![]);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C3);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["capture_method"], "C3");
    assert_eq!(body["tables"][0]["consistency_grade"], "B");
    assert_eq!(body["transactional_allowed"], false);
    assert_eq!(body["requested_consistency_result"], "denied");
    assert_eq!(body["configuration_accepted"], false);
    assert_eq!(body["operator_action"], "refuse");
    assert_eq!(body["downgraded"], false);
    assert_eq!(body["repair_allowed"], false);
    assert!(body["tables"][0]["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "DELETES_LAGGED"));
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "repair_denied"));
}

#[test]
fn explicit_blocked_write_class_does_not_become_c5() {
    let mut method = pg_method("100", 1, 1000);
    method.write_class = WriteClass::R2;
    let mut weaker = method.clone();
    weaker.method = CaptureMethod::C5;
    weaker.consistency_ceiling = Grade::S0;
    weaker.token_order = TokenOrder::None;
    weaker.position_codec = "none/v1".to_owned();
    weaker.write_class = WriteClass::R0;
    weaker.transaction_boundary = false;
    weaker.delete_detection = DeleteDetection::None;
    weaker.applied_pos = None;
    weaker.window_low = None;
    weaker.window_high = None;
    weaker.retention = None;
    weaker.used_as_primary_key = false;
    let mut plan = request(vec![table("public.orders", "orders_g1", method)], vec![]);
    plan.tables[0].methods.push(weaker);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C1);
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "write_class_blocked");
    assert_eq!(body["tables"][0]["capture_method"], Value::Null);
    assert_eq!(body["downgraded"], false);
    assert_eq!(body["operator_action"], "refuse");
}

#[test]
fn retention_thresholds_and_resnapshot_generation() {
    let cases = [
        (50_u64, 100_u64, "continue", false, false),
        (51, 100, "alert_stale_risk", true, false),
        (80, 100, "alert_stale_risk", true, false),
        (81, 100, "pause_publish", true, false),
        (100, 100, "pause_publish", true, false),
        (101, 100, "resnapshot", true, true),
    ];
    for (lag, window, action, stale, expired) in cases {
        let plan = request(
            vec![table("public.orders", "orders_g2", pg_method("100", lag, window))],
            vec![],
        );
        let (code, body) = run(&plan);
        assert_eq!(code, 0, "{lag} {body}");
        assert_eq!(body["tables"][0]["operator_action"], action, "{lag}");
        let statuses = body["tables"][0]["statuses"].as_array().unwrap();
        assert_eq!(statuses.iter().any(|item| item == "STALE_RISK"), stale, "{lag}");
        assert_eq!(
            statuses.iter().any(|item| item == "POSITION_EXPIRED"),
            expired,
            "{lag}"
        );
        if expired {
            assert_eq!(body["tables"][0]["next_generation"], "orders_g3");
            assert_eq!(body["tables"][0]["advance_capture_position"], false);
            assert_eq!(body["tables"][0]["continue_from_latest"], false);
            assert_eq!(body["tables"][0]["old_generation_readable"], true);
            assert_eq!(body["tables"][0]["publish_new_generation"], false);
            assert_eq!(body["repair_allowed"], false);
        }
    }
}

#[test]
fn incomparable_position_denies_repair() {
    let mut method = pg_method("100", 1, 1000);
    method.token_order = TokenOrder::PerPartition;
    method.position_codec = "dynamodb-shard/v1".to_owned();
    method.consistency_ceiling = Grade::B;
    method.method = CaptureMethod::C3;
    method.transaction_boundary = false;
    method.delete_detection = DeleteDetection::None;
    method.applied_pos = Some("7".to_owned());
    method.window_low = None;
    method.window_high = None;
    method.retention = None;
    method.cursor = Some(CursorInput {
        safety: CursorSafety::CommitOrdered,
        overlap_seconds: 0,
        max_transaction_age_seconds: None,
        conservative_max_transaction_age_seconds: None,
        upper_bound_is_commit_safe: true,
        reconcile_configured: false,
        equal_cursor_checkpoint: true,
        admin_confirmed: false,
    });
    let mut plan = request(vec![table("public.orders", "orders_g1", method)], vec![]);
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C3);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["capture_method"], "C3");
    assert_eq!(body["repair_allowed"], false);
    assert_eq!(body["consistency_claim_allowed"], false);
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "repair_denied"));
    assert_eq!(body["tables"][0]["statuses"][0], "DELETES_UNDETECTED");
}

#[test]
fn short_overlap_and_coarse_cursor_are_refused() {
    let mut method = pg_method("100", 1, 1000);
    method.method = CaptureMethod::C3;
    method.consistency_ceiling = Grade::B;
    method.transaction_boundary = false;
    method.token_order = TokenOrder::Opaque;
    method.position_codec = "batch-cursor-us/v1".to_owned();
    method.applied_pos = None;
    method.window_low = None;
    method.window_high = None;
    method.retention = None;
    method.delete_detection = DeleteDetection::Reconcile;
    method.reconcile_period_seconds = Some(10);
    method.delete_visibility_sla_seconds = Some(10);
    method.cursor = Some(CursorInput {
        safety: CursorSafety::StatementTime,
        overlap_seconds: 1,
        max_transaction_age_seconds: Some(30),
        conservative_max_transaction_age_seconds: None,
        upper_bound_is_commit_safe: false,
        reconcile_configured: true,
        equal_cursor_checkpoint: true,
        admin_confirmed: false,
    });
    let mut plan = request(vec![table("public.orders", "orders_g1", method.clone())], vec![]);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C3);
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "overlap_short");
    assert_eq!(body["downgraded"], false);

    method.cursor.as_mut().unwrap().safety = CursorSafety::Coarse;
    method.cursor.as_mut().unwrap().overlap_seconds = 30;
    plan.tables[0].methods[0] = method;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "coarse_cursor");
}

#[test]
fn joins_below_t_or_across_sources_warn() {
    let mut other = pg_method("100", 1, 1000);
    other.method = CaptureMethod::C5;
    other.consistency_ceiling = Grade::B;
    other.token_order = TokenOrder::None;
    other.position_codec = "none/v1".to_owned();
    other.write_class = WriteClass::R0;
    other.transaction_boundary = false;
    other.delete_detection = DeleteDetection::None;
    other.applied_pos = None;
    other.window_low = None;
    other.window_high = None;
    other.retention = None;
    other.used_as_primary_key = false;
    let mut plan = request(
        vec![
            table("public.orders", "orders_g1", pg_method("100", 1, 1000)),
            table("public.lines", "lines_g1", other),
        ],
        vec![JoinInput {
            tables: vec!["public.orders".to_owned(), "public.lines".to_owned()],
            source_ids: vec!["billing".to_owned()],
        }],
    );
    plan.requested_consistency = Some(RequestedConsistency::PerTable);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["joins"][0]["warning"], "CROSS_TABLE_SKEW");
    assert_eq!(body["source_grade"], "B");
    assert_eq!(body["same_instant_claim_allowed"], false);

    plan.tables[1].methods[0] = pg_method("100", 1, 1000);
    plan.joins[0].source_ids = vec!["billing".to_owned(), "archive".to_owned()];
    plan.requested_consistency = Some(RequestedConsistency::Transactional);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["joins"][0]["warning"], "CROSS_TABLE_SKEW");
    assert_eq!(body["source_grade"], "T");
}

#[test]
fn offset_gap_does_not_advance_or_repair() {
    let mut plan = request(
        vec![table("public.orders", "orders_g1", pg_method("100", 1, 1000))],
        vec![],
    );
    plan.offset = Some(offset("100", true));
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "skip");
    assert_eq!(body["offset"]["watermark_hold"], "source_gap");
    assert_eq!(body["offset"]["checkpoint_advanced"], false);
    assert_eq!(body["advance_capture_position"], false);
    assert_eq!(body["repair_allowed"], false);
    assert_eq!(body["continue_from_latest"], false);
}

#[test]
fn rolled_back_spool_authorizes_nothing() {
    let dir = rollback_spool();
    let mut plan = request(
        vec![table("public.orders", "orders_g1", pg_method("100", 1, 1000))],
        vec![],
    );
    plan.spool_dir = Some(dir.to_string_lossy().into_owned());
    plan.requested_consistency = Some(RequestedConsistency::PerTable);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["spool"]["state"], "rolled_back");
    assert_eq!(body["spool"]["authorizes_ack"], false);
    assert_eq!(body["spool"]["authorizes_publish"], false);
    assert_eq!(body["spool"]["applied"], Value::Null);
    assert_eq!(body["advance_capture_position"], false);
}

#[test]
fn as_of_window_and_auto_order() {
    let mut c2 = pg_method("100", 1, 10_000);
    c2.method = CaptureMethod::C2;
    c2.consistency_ceiling = Grade::S1;
    c2.transaction_boundary = false;
    c2.token_available = true;
    c2.position_codec = "snowflake-timestamp/v1".to_owned();
    c2.family = "snowflake".to_owned();
    c2.as_of = Some(AsOfInput {
        snapshot_token: "t0".to_owned(),
        read_seconds: 100,
        token_retention_seconds: 100,
        same_token: true,
    });
    let mut plan = request(vec![table("public.orders", "orders_g1", c2.clone())], vec![]);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C2);
    plan.requested_consistency = Some(RequestedConsistency::Snapshot);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "as_of_exceeds_retention");

    c2.as_of.as_mut().unwrap().read_seconds = 301;
    c2.as_of.as_mut().unwrap().token_retention_seconds = 10_000;
    plan.tables[0].methods[0] = c2.clone();
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "snapshot_exceeds_300s");

    c2.as_of.as_mut().unwrap().read_seconds = 300;
    c2.support_grade = SupportGrade::Experimental;
    let mut c5 = pg_method("100", 1, 1000);
    c5.method = CaptureMethod::C5;
    c5.support_grade = SupportGrade::Verified;
    c5.consistency_ceiling = Grade::S0;
    c5.token_order = TokenOrder::None;
    c5.position_codec = "none/v1".to_owned();
    c5.write_class = WriteClass::R0;
    c5.transaction_boundary = false;
    c5.delete_detection = DeleteDetection::None;
    c5.applied_pos = None;
    c5.window_low = None;
    c5.window_high = None;
    c5.retention = None;
    c5.used_as_primary_key = false;
    plan.sync = SyncMode::Auto;
    plan.tables[0].explicit_method = None;
    plan.tables[0].methods = vec![c2, c5];
    plan.profile = Profile::Production;
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["capture_method"], "C5");
    assert_eq!(body["tables"][0]["statuses"][0], "DELETES_UNDETECTED");
}

#[test]
fn auto_prefers_verified_c1_and_skips_app_cursor() {
    let mut c3 = pg_method("100", 1, 1000);
    c3.method = CaptureMethod::C3;
    c3.consistency_ceiling = Grade::B;
    c3.transaction_boundary = false;
    c3.token_order = TokenOrder::Opaque;
    c3.position_codec = "opaque-cursor/v1".to_owned();
    c3.applied_pos = None;
    c3.window_low = None;
    c3.window_high = None;
    c3.retention = None;
    c3.delete_detection = DeleteDetection::None;
    c3.cursor = Some(CursorInput {
        safety: CursorSafety::AppMaintained,
        overlap_seconds: 100,
        max_transaction_age_seconds: Some(10),
        conservative_max_transaction_age_seconds: None,
        upper_bound_is_commit_safe: false,
        reconcile_configured: true,
        equal_cursor_checkpoint: true,
        admin_confirmed: true,
    });
    let c1 = pg_method("100", 1, 1000);
    let mut plan = request(vec![table("public.orders", "orders_g1", c3)], vec![]);
    plan.tables[0].methods.push(c1);
    plan.requested_consistency = Some(RequestedConsistency::Transactional);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["capture_method"], "C1");
    assert_eq!(body["transactional_allowed"], true);
}

#[test]
fn failover_soft_delete_precision_and_sla() {
    let mut method = pg_method("100", 1, 1000);
    method.failover_observed = true;
    let plan = request(vec![table("public.orders", "orders_g1", method.clone())], vec![]);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["operator_action"], "resnapshot");
    assert!(body["tables"][0]["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "POSITION_EXPIRED"));
    assert_eq!(body["repair_allowed"], false);

    method.failover_observed = false;
    method.position_codec = "mysql-binlog-pos/v1".to_owned();
    method.family = "mysql".to_owned();
    method.failover_observed = true;
    let plan = request(vec![table("public.orders", "orders_g1", method.clone())], vec![]);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["operator_action"], "resnapshot");
    assert_eq!(body["repair_allowed"], false);

    method.failover_observed = false;
    method.position_codec = "pg-commit-lsn/v1".to_owned();
    method.soft_delete_only = true;
    let mut plan = request(vec![table("public.orders", "orders_g1", method.clone())], vec![]);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C1);
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "soft_delete_not_physical");

    method.soft_delete_only = false;
    method.precision_truncated = true;
    method.original_column_retained_as_key = false;
    plan.tables[0].methods[0] = method.clone();
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "precision_truncated");

    method.precision_truncated = false;
    method.delete_detection = DeleteDetection::Reconcile;
    method.reconcile_period_seconds = Some(100);
    method.delete_visibility_sla_seconds = Some(10);
    plan.tables[0].methods[0] = method;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "delete_sla_unmet");
}

#[test]
fn equal_cursor_checkpoint_and_volume_without_key() {
    let mut method = pg_method("100", 1, 1000);
    method.method = CaptureMethod::C3;
    method.consistency_ceiling = Grade::B;
    method.transaction_boundary = false;
    method.token_order = TokenOrder::Opaque;
    method.position_codec = "batch-cursor-us/v1".to_owned();
    method.applied_pos = None;
    method.window_low = None;
    method.window_high = None;
    method.retention = None;
    method.delete_detection = DeleteDetection::None;
    method.cursor = Some(CursorInput {
        safety: CursorSafety::CommitOrdered,
        overlap_seconds: 0,
        max_transaction_age_seconds: None,
        conservative_max_transaction_age_seconds: None,
        upper_bound_is_commit_safe: true,
        reconcile_configured: false,
        equal_cursor_checkpoint: false,
        admin_confirmed: false,
    });
    let mut plan = request(vec![table("public.orders", "orders_g1", method)], vec![]);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C3);
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "equal_cursor_checkpoint_required");

    let mut full = pg_method("100", 1, 1000);
    full.method = CaptureMethod::C5;
    full.consistency_ceiling = Grade::S0;
    full.token_order = TokenOrder::None;
    full.position_codec = "none/v1".to_owned();
    full.write_class = WriteClass::R0;
    full.transaction_boundary = false;
    full.delete_detection = DeleteDetection::Reconcile;
    full.reconcile_period_seconds = Some(86_400);
    full.delete_visibility_sla_seconds = Some(86_400);
    full.applied_pos = None;
    full.window_low = None;
    full.window_high = None;
    full.retention = None;
    full.used_as_primary_key = false;
    plan.tables[0].methods = vec![full];
    plan.tables[0].explicit_method = Some(CaptureMethod::C5);
    plan.tables[0].stable_key = false;
    plan.tables[0].declared_unique_key = false;
    plan.tables[0].row_estimate = 50;
    plan.volume_limit_rows = 10;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "volume_exceeds_limit");
    assert_eq!(body["downgraded"], false);
}

#[test]
fn same_token_s1_join_still_warns_and_snapshot_is_accepted() {
    let mut method = pg_method("100", 1, 10_000);
    method.method = CaptureMethod::C2;
    method.consistency_ceiling = Grade::S1;
    method.transaction_boundary = false;
    method.token_available = true;
    method.position_codec = "bigquery-timestamp/v1".to_owned();
    method.family = "bigquery".to_owned();
    method.as_of = Some(AsOfInput {
        snapshot_token: "t0".to_owned(),
        read_seconds: 20,
        token_retention_seconds: 1_000,
        same_token: true,
    });
    let mut second = method.clone();
    second.position_lineage = "blineage".to_owned();
    let mut plan = request(
        vec![
            table("public.orders", "orders_g1", method),
            table("public.lines", "lines_g1", second),
        ],
        vec![JoinInput {
            tables: vec!["public.orders".to_owned(), "public.lines".to_owned()],
            source_ids: vec!["billing".to_owned()],
        }],
    );
    plan.requested_consistency = Some(RequestedConsistency::Snapshot);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["requested_consistency_result"], "accepted");
    assert_eq!(body["source_grade"], "S1");
    assert_eq!(body["same_instant_claim_allowed"], true);
    assert_eq!(body["joins"][0]["warning"], "CROSS_TABLE_SKEW");
    assert_eq!(body["transactional_allowed"], false);
    assert_eq!(body["tables"][0]["initial_load"], "token_anchored_snapshot_replay");
}

#[test]
fn spool_and_offset_position_disagreement_refuses_repair() {
    let dir = seal();
    let mut plan = request(
        vec![table("public.orders", "orders_g1", pg_method("100", 1, 1000))],
        vec![],
    );
    plan.spool_dir = Some(dir.to_string_lossy().into_owned());
    plan.offset = Some(offset("100", false));
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["spool"]["commit_position"], POSITION);
    assert_eq!(body["operator_action"], "refuse");
    assert_eq!(body["repair_allowed"], false);
    assert_eq!(body["advance_capture_position"], false);
    assert_eq!(body["configuration_accepted"], false);
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "position_disagreement"));
}

#[test]
fn c5_change_during_read_discloses_b() {
    let mut method = pg_method("100", 1, 1000);
    method.method = CaptureMethod::C5;
    method.consistency_ceiling = Grade::S0;
    method.token_order = TokenOrder::None;
    method.position_codec = "none/v1".to_owned();
    method.write_class = WriteClass::R0;
    method.transaction_boundary = false;
    method.delete_detection = DeleteDetection::Reconcile;
    method.reconcile_period_seconds = Some(100);
    method.delete_visibility_sla_seconds = Some(100);
    method.applied_pos = None;
    method.window_low = None;
    method.window_high = None;
    method.retention = None;
    method.used_as_primary_key = false;
    let mut plan = request(vec![table("public.orders", "orders_g1", method)], vec![]);
    plan.tables[0].source_changing_without_token = true;
    plan.requested_consistency = Some(RequestedConsistency::BestEffort);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["capture_method"], "C5");
    assert_eq!(body["tables"][0]["consistency_grade"], "B");
    assert_eq!(body["tables"][0]["initial_load"], "shadow_diff");
    assert_eq!(body["downgraded"], false);
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "source_changed_during_read"));
}

#[test]
fn secret_field_and_unknown_field_are_refused() {
    let dir = temp();
    let path = dir.join("request.json");
    fs::write(
        &path,
        br#"{"contract":"source-capture-policy/v1","password":"sentinel-secret-value"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_source-capture-policy"))
        .arg("plan")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("sentinel-secret-value"));
    assert!(text.contains("secret_field"));

    fs::write(&path, br#"{"contract":"source-capture-policy/v1","extra":1}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_source-capture-policy"))
        .arg("plan")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["fault"], "input");
    assert_eq!(body["downgraded"], false);
}

#[test]
fn two_total_tables_have_no_skew() {
    let plan = request(
        vec![
            table("public.orders", "orders_g1", pg_method("100", 1, 1000)),
            table("public.lines", "lines_g1", pg_method("100", 1, 1000)),
        ],
        vec![JoinInput {
            tables: vec!["public.orders".to_owned(), "public.lines".to_owned()],
            source_ids: vec!["billing".to_owned()],
        }],
    );
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["joins"][0]["warning"], Value::Null);
    assert_eq!(body["transactional_allowed"], true);
    assert_eq!(body["same_instant_claim_allowed"], true);
    assert_eq!(body["operator_action"], "continue");
}

fn maria_method(lineage: &str, pos: &str) -> MethodInput {
    let mut method = pg_method(pos, 1, 864000);
    method.family = "mariadb".to_owned();
    method.position_codec = "mariadb-gtid/v1".to_owned();
    method.position_lineage = lineage.to_owned();
    method.same_source_uuid = false;
    method.window_low = Some("2".to_owned());
    method.window_high = Some("10".to_owned());
    method
}

#[test]
fn mariadb_gtid_compares_one_domain_and_mysql_uuid_does_not() {
    let plan = request(
        vec![table("logistics.shipments", "shipments_g1", maria_method("d0.s1", "10"))],
        vec![],
    );
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["capture_method"], "C1");
    assert_eq!(body["tables"][0]["consistency_grade"], "T");
    assert_eq!(body["transactional_allowed"], true);
    assert!(!body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "position_compare_unimplemented"));
    assert!(!body["tables"][0]["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "POSITION_EXPIRED"));

    let mut raw = maria_method("d0.s1", "6");
    raw.applied_pos = Some("0-1-6".to_owned());
    let mut plan = request(vec![table("logistics.shipments", "shipments_g1", raw)], vec![]);
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C1);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "gtid_identity");
    assert_eq!(body["transactional_allowed"], false);

    let mut boundary = maria_method("d0.s1", "6");
    boundary.transaction_boundary = false;
    plan.tables[0].methods[0] = boundary;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "transaction_boundary_required");
    assert_eq!(body["transactional_allowed"], false);

    let mut file_pos = maria_method("d0.s1", "6");
    file_pos.position_codec = "mysql-binlog-pos/v1".to_owned();
    file_pos.position_lineage = "same-instance".to_owned();
    file_pos.applied_pos = Some("328".to_owned());
    file_pos.window_low = None;
    file_pos.window_high = None;
    plan.tables[0].methods[0] = file_pos;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "binlog_pos_not_durable");

    let mut mysql = pg_method("23", 1, 1000);
    mysql.family = "mysql".to_owned();
    mysql.position_codec = "mysql-gtid/v1".to_owned();
    mysql.position_lineage = "src.3e11fa47-71ca-11e1-9e33-c80aa9429562".to_owned();
    mysql.same_source_uuid = true;
    mysql.window_low = None;
    mysql.window_high = None;
    let plan = request(vec![table("shop.orders", "orders_g1", mysql)], vec![]);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "position_compare_unimplemented"));
    assert_eq!(body["transactional_allowed"], false);
    assert_eq!(body["repair_allowed"], false);

    let mut expired = maria_method("d0.s1", "6");
    expired.failover_observed = true;
    expired.failover_position_retained = None;
    let plan = request(vec![table("logistics.shipments", "shipments_g1", expired)], vec![]);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["operator_action"], "resnapshot");
    assert_eq!(body["repair_allowed"], false);
    assert!(body["tables"][0]["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "POSITION_EXPIRED"));
}

fn mongo_method(grade: Grade) -> MethodInput {
    let mut method = pg_method("11", 1, 864000);
    method.family = "mongodb".to_owned();
    method.position_codec = "mongo-cluster-time/v1".to_owned();
    method.position_lineage = "rs0".to_owned();
    method.consistency_ceiling = grade;
    method.transaction_boundary = false;
    method.same_source_uuid = false;
    method.source_epoch = 4;
    method.window_low = None;
    method.window_high = None;
    method
}

#[test]
fn explicit_mongo_c1_stays_weak_and_auto_does_not_promote() {
    let auto = request(
        vec![table("shop.items", "items_g1", mongo_method(Grade::S0))],
        vec![],
    );
    let (code, body) = run(&auto);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "no_eligible_method");
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "c1_requires_t"));
    assert_eq!(body["transactional_allowed"], false);

    let mut plan = request(
        vec![table("shop.items", "items_g1", mongo_method(Grade::S0))],
        vec![],
    );
    plan.sync = SyncMode::Explicit;
    plan.tables[0].explicit_method = Some(CaptureMethod::C1);
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], Value::Null);
    assert_eq!(body["tables"][0]["capture_method"], "C1");
    assert_eq!(body["tables"][0]["consistency_grade"], "S0");
    assert_eq!(body["transactional_allowed"], false);
    assert_eq!(body["tables"][0]["resume_opaque"], false);

    plan.tables[0].methods[0].consistency_ceiling = Grade::T;
    plan.tables[0].methods[0].transaction_boundary = true;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "mongo_t_unverified");
    assert_eq!(body["transactional_allowed"], false);

    plan.tables[0].methods[0] = mongo_method(Grade::S0);
    plan.tables[0].methods[0].history_lost = true;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["consistency_grade"], "S0");
    assert_eq!(body["tables"][0]["advance_capture_position"], false);
    assert_eq!(body["tables"][0]["continue_from_latest"], false);
    assert_eq!(body["tables"][0]["resume_opaque"], false);
    assert_eq!(body["transactional_allowed"], false);
    assert!(body["tables"][0]["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "POSITION_EXPIRED"));
    assert!(body["tables"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "history_lost"));

    let mut token = mongo_method(Grade::S0);
    token.position_codec = "mongo-resume-token/v1".to_owned();
    token.token_order = TokenOrder::Opaque;
    token.applied_pos = None;
    plan.tables[0].methods[0] = token;
    let (code, body) = run(&plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["tables"][0]["fault"], "resume_token_not_ordered");
}
