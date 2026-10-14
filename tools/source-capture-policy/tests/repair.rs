//! Receipt scope. Each decision calls `cdc_offset_store::checkpoint`.
//! A closed receipt binds only the generations it names. Positions stay
//! decimal strings; the 20-digit fixture is never parsed as f64.
//! The offset store requires equal `applied_pos` on every required sibling,
//! so the fully bound pair shares one applied position.

#![allow(clippy::bool_assert_comparison)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cdc_offset_store::{
    checkpoint, Checkpoint, DeclaredNext, DeclaredTxn, OrderBinding, Receipt as StoreReceipt,
    PROVENANCE,
};
use serde_json::Value;
use source_capture_policy::{
    CaptureMethod, DeleteDetection, Grade, JoinInput, MethodInput, OffsetBinding, OffsetDeclared,
    OffsetInput, OffsetNext, OffsetReceipt, PlanRequest, Profile, RequestedConsistency,
    RetentionInput, SupportGrade, SyncMode, TableInput, TokenOrder, WriteClass, CONTRACT,
};

const LEDGER_POS: &str = "36893488160304112307";
const PAYMENT_POS: &str = "77";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("oo-dl01w1-03-repair-{nanos}-{n}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn digest() -> String {
    "ab".repeat(32)
}

fn other_digest() -> String {
    "cd".repeat(32)
}

fn run_named(name: &str, request: &PlanRequest) -> (i32, Value) {
    let dir = temp();
    let path = dir.join("request.json");
    let bytes = serde_json::to_vec_pretty(request).unwrap();
    fs::write(&path, &bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_source-capture-policy"))
        .arg("plan")
        .arg(&path)
        .output()
        .unwrap();
    let code = output.status.code().unwrap();
    if let Ok(sink) = std::env::var("REPAIR_CLI_DIR") {
        let sink = PathBuf::from(sink);
        fs::create_dir_all(&sink).unwrap();
        fs::write(sink.join(format!("{name}.request.json")), &bytes).unwrap();
        fs::write(sink.join(format!("{name}.stdout.json")), &output.stdout).unwrap();
        fs::write(sink.join(format!("{name}.stderr.txt")), &output.stderr).unwrap();
        fs::write(sink.join(format!("{name}.exit")), format!("{code}\n")).unwrap();
    }
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    (code, value)
}

fn method(position: &str, epoch: i32) -> MethodInput {
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
        source_epoch: epoch,
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
            window_seconds: 1000,
            lag_seconds: 1,
        }),
        as_of: None,
        history_lost: false,
    }
}

fn table(name: &str, generation: &str, item: MethodInput) -> TableInput {
    TableInput {
        name: name.to_owned(),
        generation: generation.to_owned(),
        stable_key: true,
        declared_unique_key: true,
        row_estimate: 10,
        source_changing_without_token: false,
        budget_exhausted: false,
        explicit_method: None,
        methods: vec![item],
    }
}

struct Pair {
    ledger_pos: &'static str,
    payment_pos: &'static str,
    ledger_epoch: i32,
    payment_epoch: i32,
    consistency: RequestedConsistency,
}

fn pair() -> Pair {
    Pair {
        ledger_pos: LEDGER_POS,
        payment_pos: PAYMENT_POS,
        ledger_epoch: 1,
        payment_epoch: 1,
        consistency: RequestedConsistency::Transactional,
    }
}

fn finance(spec: &Pair, offset: OffsetInput) -> PlanRequest {
    PlanRequest {
        contract: CONTRACT.to_owned(),
        profile: Profile::Production,
        sync: SyncMode::Auto,
        requested_consistency: Some(spec.consistency),
        source_id: "finance".to_owned(),
        volume_limit_rows: 1_000_000,
        repair_requested: true,
        spool_dir: None,
        offset: Some(offset),
        tables: vec![
            table(
                "finance.ledger",
                "ledger_g4",
                method(spec.ledger_pos, spec.ledger_epoch),
            ),
            table(
                "finance.payment",
                "payment_g2",
                method(spec.payment_pos, spec.payment_epoch),
            ),
        ],
        joins: vec![JoinInput {
            tables: vec!["finance.ledger".to_owned(), "finance.payment".to_owned()],
            source_ids: vec!["finance".to_owned()],
        }],
    }
}

struct Row<'a> {
    generation: &'a str,
    position: &'a str,
    epoch: i32,
    source: &'a str,
    schema: String,
}

fn row(spec: Row<'_>) -> OffsetReceipt {
    OffsetReceipt {
        source: spec.source.to_owned(),
        table_generation: spec.generation.to_owned(),
        txn_or_batch_id: "fin17".to_owned(),
        config_version: "config/v1".to_owned(),
        config_digest: digest(),
        type_policy_version: "type-policy/v1".to_owned(),
        type_digest: digest(),
        hook_digest: digest(),
        schema_digest: spec.schema,
        payload_digest: digest(),
        protocol_version: "batch/v1".to_owned(),
        position_codec: "pg-commit-lsn/v1".to_owned(),
        position_lineage: "alineage".to_owned(),
        source_epoch: spec.epoch,
        applied_pos: spec.position.to_owned(),
        partial_txn: false,
        cross_table_consistent: true,
        txn_complete: true,
        segment_index: None,
        segment_count: None,
        segment_bitmap: Vec::new(),
        fence_shard: "finance".to_owned(),
        fence_generation: 1,
        target_commit_at: "2026-10-06T00:00:00.000000Z".to_owned(),
    }
}

fn ledger_row(position: &str) -> OffsetReceipt {
    row(Row {
        generation: "ledger_g4",
        position,
        epoch: 1,
        source: "finance",
        schema: digest(),
    })
}

fn payment_row(position: &str, epoch: i32, source: &str, schema: String) -> OffsetReceipt {
    row(Row {
        generation: "payment_g2",
        position,
        epoch,
        source,
        schema,
    })
}

fn offset_of(receipts: Vec<OffsetReceipt>, required: &[&str], applied: &str) -> OffsetInput {
    OffsetInput {
        receipts,
        selected_generation: "ledger_g4".to_owned(),
        watermark: None,
        binding: OffsetBinding {
            provenance: PROVENANCE.to_owned(),
            expected_previous: None,
            declared_transaction: OffsetDeclared {
                txn_or_batch_id: "fin17".to_owned(),
                applied_pos: applied.to_owned(),
                required_table_generations: required.iter().map(|item| (*item).to_owned()).collect(),
            },
            declared_next: OffsetNext {
                txn_or_batch_id: "fin18".to_owned(),
                applied_pos: "2".to_owned(),
            },
            source_gap: false,
            epoch_barrier: false,
        },
    }
}

fn to_store(input: &OffsetReceipt) -> StoreReceipt {
    StoreReceipt {
        source: input.source.clone(),
        table_generation: input.table_generation.clone(),
        txn_or_batch_id: input.txn_or_batch_id.clone(),
        config_version: input.config_version.clone(),
        config_digest: input.config_digest.clone(),
        type_policy_version: input.type_policy_version.clone(),
        type_digest: input.type_digest.clone(),
        hook_digest: input.hook_digest.clone(),
        schema_digest: input.schema_digest.clone(),
        payload_digest: input.payload_digest.clone(),
        protocol_version: input.protocol_version.clone(),
        position_codec: input.position_codec.clone(),
        position_lineage: input.position_lineage.clone(),
        source_epoch: input.source_epoch,
        applied_pos: input.applied_pos.clone(),
        partial_txn: input.partial_txn,
        cross_table_consistent: input.cross_table_consistent,
        txn_complete: input.txn_complete,
        segment_index: input.segment_index,
        segment_count: input.segment_count,
        segment_bitmap: input.segment_bitmap.clone(),
        fence_shard: input.fence_shard.clone(),
        fence_generation: input.fence_generation,
        target_commit_at: input.target_commit_at.clone(),
    }
}

fn store_result(input: &OffsetInput) -> Result<Checkpoint, &'static str> {
    let receipts: Vec<StoreReceipt> = input.receipts.iter().map(to_store).collect();
    let selected_index = receipts
        .iter()
        .position(|item| item.table_generation == input.selected_generation)
        .expect("selected receipt");
    let selected = receipts[selected_index].clone();
    let siblings: Vec<StoreReceipt> = receipts
        .into_iter()
        .enumerate()
        .filter(|(index, _)| *index != selected_index)
        .map(|(_, item)| item)
        .collect();
    let binding = &input.binding;
    let order = OrderBinding {
        provenance: binding.provenance.clone(),
        expected_previous: None,
        declared_transaction: DeclaredTxn {
            txn_or_batch_id: binding.declared_transaction.txn_or_batch_id.clone(),
            applied_pos: binding.declared_transaction.applied_pos.clone(),
            required_table_generations: binding
                .declared_transaction
                .required_table_generations
                .clone(),
        },
        declared_next: DeclaredNext {
            txn_or_batch_id: binding.declared_next.txn_or_batch_id.clone(),
            applied_pos: binding.declared_next.applied_pos.clone(),
        },
        source_gap: binding.source_gap,
        epoch_barrier: binding.epoch_barrier,
    };
    checkpoint(&selected, &siblings, None, &order)
}

fn has_reason(body: &Value, index: usize, reason: &str) -> bool {
    body["tables"][index]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == reason)
}

fn assert_source_closed(body: &Value) {
    assert_eq!(body["repair_allowed"], false);
    assert_eq!(body["advance_capture_position"], false);
    assert_eq!(body["transactional_allowed"], false);
    assert_eq!(body["consistency_claim_allowed"], false);
    assert_eq!(body["same_instant_claim_allowed"], false);
    assert_eq!(body["operator_action"], "refuse");
    assert_eq!(body["configuration_accepted"], false);
    assert_eq!(body["downgraded"], false);
    assert_eq!(body["joins"][0]["warning"], "CROSS_TABLE_SKEW");
    assert_eq!(body["loader_integration"], "unaccepted");
    assert_eq!(body["scheduler_integration"], "unaccepted");
    assert_eq!(body["live_offset_mirror"], "not_run");
}

fn assert_table_closed(body: &Value, index: usize) {
    let table = &body["tables"][index];
    assert_eq!(table["repair_allowed"], false);
    assert_eq!(table["advance_capture_position"], false);
    assert_eq!(table["transactional_allowed"], false);
    assert_eq!(table["consistency_claim_allowed"], false);
    assert!(!table["reasons"].as_array().unwrap().is_empty());
}

fn assert_table_open(body: &Value, index: usize) {
    let table = &body["tables"][index];
    assert_eq!(table["repair_allowed"], true);
    assert_eq!(table["advance_capture_position"], true);
    assert_eq!(table["transactional_allowed"], true);
    assert_eq!(table["consistency_claim_allowed"], true);
}

fn assert_position_string(body: &Value, index: usize, position: &str) {
    assert_eq!(body["tables"][index]["freshness"]["position"], position);
    assert!(position.bytes().all(|byte| byte.is_ascii_digit()));
    assert!(!position.bytes().any(|byte| matches!(byte, b'e' | b'E' | b'.')));
}

#[test]
fn fully_bound_pair_inserts_first_for_every_table() {
    let spec = Pair {
        payment_pos: LEDGER_POS,
        ..pair()
    };
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(LEDGER_POS, 1, "finance", digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let (code, body) = run_named("fully-bound", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_eq!(body["offset"]["checkpoint_advanced"], true);
    assert_eq!(body["offset"]["receipt_closed"], true);
    assert_table_open(&body, 0);
    assert_table_open(&body, 1);
    assert_eq!(body["repair_allowed"], true);
    assert_eq!(body["advance_capture_position"], true);
    assert_eq!(body["transactional_allowed"], true);
    assert_eq!(body["consistency_claim_allowed"], true);
    assert_eq!(body["same_instant_claim_allowed"], true);
    assert_eq!(body["operator_action"], "continue");
    assert_eq!(body["configuration_accepted"], true);
    assert_eq!(body["joins"][0]["warning"], Value::Null);
    assert_eq!(body["downgraded"], false);
    assert_position_string(&body, 0, LEDGER_POS);
    assert_position_string(&body, 1, LEDGER_POS);
    assert_eq!(body["live_offset_mirror"], "not_run");
}

#[test]
fn selected_only_ledger_receipt_does_not_cover_payment() {
    let offset = offset_of(vec![ledger_row(LEDGER_POS)], &["ledger_g4"], LEDGER_POS);
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let (code, body) = run_named("selected-only", &finance(&pair(), offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_eq!(body["offset"]["checkpoint_advanced"], true);
    assert_eq!(body["offset"]["watermark_hold"], Value::Null);
    assert_table_open(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "receipt_unbound"));
    assert_source_closed(&body);
    assert_eq!(body["requested_consistency_result"], "denied");
    assert_position_string(&body, 0, LEDGER_POS);
    assert_position_string(&body, 1, PAYMENT_POS);
}

#[test]
fn selected_only_per_table_request_still_refuses() {
    let mut spec = pair();
    spec.consistency = RequestedConsistency::PerTable;
    let offset = offset_of(vec![ledger_row(LEDGER_POS)], &["ledger_g4"], LEDGER_POS);
    let (code, body) = run_named("selected-only-per-table", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_eq!(body["requested_consistency_result"], "accepted");
    assert_table_open(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "receipt_unbound"));
    assert_source_closed(&body);
}

#[test]
fn sibling_applied_position_mismatch_skips() {
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(PAYMENT_POS, 1, "finance", digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(
        store_result(&offset),
        Ok(Checkpoint::Skip("position_unbound"))
    );
    let (code, body) = run_named("wrong-position", &finance(&pair(), offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "skip");
    assert_eq!(body["offset"]["watermark_hold"], "position_unbound");
    assert_eq!(body["offset"]["checkpoint_advanced"], false);
    assert_table_closed(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 0, "position_unbound"));
    assert!(has_reason(&body, 1, "position_unbound"));
    assert_source_closed(&body);
    assert_position_string(&body, 0, LEDGER_POS);
    assert_position_string(&body, 1, PAYMENT_POS);
}

#[test]
fn method_position_disagrees_with_its_receipt() {
    let spec = pair();
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(LEDGER_POS, 1, "finance", digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let (code, body) = run_named("method-position", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_table_open(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "position_disagreement"));
    assert_source_closed(&body);
    assert_position_string(&body, 1, PAYMENT_POS);
}

#[test]
fn missing_required_payment_receipt_is_incomplete() {
    let offset = offset_of(
        vec![ledger_row(LEDGER_POS)],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(
        store_result(&offset),
        Ok(Checkpoint::Skip("required_incomplete"))
    );
    let (code, body) = run_named("missing-table", &finance(&pair(), offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "skip");
    assert_eq!(body["offset"]["watermark_hold"], "required_incomplete");
    assert_eq!(body["offset"]["checkpoint_advanced"], false);
    assert_table_closed(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 0, "required_incomplete"));
    assert!(has_reason(&body, 1, "required_incomplete"));
    assert_source_closed(&body);
}

#[test]
fn sibling_source_epoch_mismatch_fails_closed() {
    let mut spec = pair();
    spec.payment_pos = LEDGER_POS;
    spec.payment_epoch = 2;
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(LEDGER_POS, 2, "finance", digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(
        store_result(&offset),
        Ok(Checkpoint::Skip("position_unbound"))
    );
    let (code, body) = run_named("source-epoch", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "skip");
    assert_eq!(body["offset"]["checkpoint_advanced"], false);
    assert_table_closed(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "receipt_epoch_mismatch"));
    assert_source_closed(&body);
}

#[test]
fn method_epoch_mismatch_fails_closed_after_insert_first() {
    let mut spec = pair();
    spec.payment_pos = LEDGER_POS;
    spec.payment_epoch = 2;
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(LEDGER_POS, 1, "finance", digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let (code, body) = run_named("method-epoch", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_eq!(body["offset"]["checkpoint_advanced"], true);
    assert_table_open(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "receipt_epoch_mismatch"));
    assert_source_closed(&body);
}

#[test]
fn schema_digest_mismatch_fails_closed_after_insert_first() {
    let spec = Pair {
        payment_pos: LEDGER_POS,
        ..pair()
    };
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(LEDGER_POS, 1, "finance", other_digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let (code, body) = run_named("schema-digest", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_table_open(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "receipt_digest_mismatch"));
    assert_source_closed(&body);
}

#[test]
fn source_id_mismatch_fails_closed_after_insert_first() {
    let spec = Pair {
        payment_pos: LEDGER_POS,
        ..pair()
    };
    let offset = offset_of(
        vec![
            ledger_row(LEDGER_POS),
            payment_row(LEDGER_POS, 1, "billing", digest()),
        ],
        &["ledger_g4", "payment_g2"],
        LEDGER_POS,
    );
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let (code, body) = run_named("source-id", &finance(&spec, offset));
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_table_open(&body, 0);
    assert_table_closed(&body, 1);
    assert!(has_reason(&body, 1, "receipt_source_mismatch"));
    assert_source_closed(&body);
}

#[test]
fn mariadb_gtid_receipt_repairs_only_the_stored_sequence() {
    let mut item = method("6", 1);
    item.family = "mariadb".to_owned();
    item.position_codec = "mariadb-gtid/v1".to_owned();
    item.position_lineage = "d0.s1".to_owned();
    item.same_source_uuid = false;
    item.window_high = Some("6".to_owned());
    let mut row = ledger_row("6");
    row.table_generation = "shipments_g1".to_owned();
    row.source = "finance".to_owned();
    row.position_codec = "mariadb-gtid/v1".to_owned();
    row.position_lineage = "d0.s1".to_owned();
    let offset = OffsetInput {
        receipts: vec![row.clone()],
        selected_generation: "shipments_g1".to_owned(),
        watermark: None,
        binding: OffsetBinding {
            provenance: PROVENANCE.to_owned(),
            expected_previous: None,
            declared_transaction: OffsetDeclared {
                txn_or_batch_id: "fin17".to_owned(),
                applied_pos: "6".to_owned(),
                required_table_generations: vec!["shipments_g1".to_owned()],
            },
            declared_next: OffsetNext {
                txn_or_batch_id: "fin18".to_owned(),
                applied_pos: "7".to_owned(),
            },
            source_gap: false,
            epoch_barrier: false,
        },
    };
    assert_eq!(store_result(&offset), Ok(Checkpoint::InsertFirst));
    let plan = PlanRequest {
        contract: CONTRACT.to_owned(),
        profile: Profile::Production,
        sync: SyncMode::Auto,
        requested_consistency: Some(RequestedConsistency::Transactional),
        source_id: "finance".to_owned(),
        volume_limit_rows: 1_000_000,
        repair_requested: true,
        spool_dir: None,
        offset: Some(offset),
        tables: vec![table("finance.shipments", "shipments_g1", item)],
        joins: vec![],
    };
    let (code, body) = run_named("maria-gtid", &plan);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "insert_first");
    assert_eq!(body["tables"][0]["repair_allowed"], true);
    assert_eq!(body["tables"][0]["freshness"]["position"], "6");
    assert_eq!(body["offset"]["ack_allowed"], false);

    let mut raw = row;
    raw.applied_pos = "0-1-6".to_owned();
    let mut refused = plan;
    let mut bad = refused.offset.take().unwrap();
    bad.receipts = vec![raw];
    bad.binding.declared_transaction.applied_pos = "0-1-6".to_owned();
    refused.offset = Some(bad);
    refused.tables[0].methods[0].applied_pos = Some("0-1-6".to_owned());
    let (code, body) = run_named("maria-gtid-raw", &refused);
    assert_eq!(code, 0, "{body}");
    assert_eq!(body["offset"]["checkpoint"], "refused");
    assert_eq!(body["tables"][0]["repair_allowed"], false);
    assert_eq!(body["offset"]["ack_allowed"], false);
}
