use super::*;
use arrow_array::Int32Array;
use arrow_schema::{DataType, Field, Schema};
use std::sync::Arc;

fn table() -> TableRef {
    TableRef {
        schema: "fixture".into(),
        name: "rows".into(),
    }
}
fn row(n: i32) -> RecordBatch {
    RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("id", DataType::Int32, false)])),
        vec![Arc::new(Int32Array::from(vec![n]))],
    )
    .unwrap()
}
fn pos(n: u64) -> Watermark {
    Watermark::MysqlFilePos {
        file: "binlog.000001".into(),
        position: n,
    }
}
fn budget() -> RepairBudget {
    RepairBudget {
        max_keys: 100,
        max_bytes: 1_000_000,
        required_history_seconds: 0,
    }
}
fn fake_repair(source: &str, bindings: &[&str]) -> SourceRepair {
    SourceRepair {
        source_id: source.into(),
        bindings: bindings.iter().map(|b| ((*b).into(), table())).collect(),
        l0: pos(4),
        end: pos(123),
        rows: BTreeMap::new(),
        empty_batches: BTreeMap::new(),
        receipts: Vec::new(),
        during_read: 0,
    }
}

#[test]
fn window_log_wins_and_tombstones_prevent_snapshot_resurrection() {
    let key = |n| (table(), vec![KeyValue::Int(n)]);
    let mut overlay = Overlay::default();
    overlay.snapshot(key(1), row(1), budget()).unwrap();
    overlay
        .log(
            &ChangeBatch {
                position: pos(20),
                changes: vec![
                    RowChange::Update {
                        table: table(),
                        key: key(3).1,
                        old_key: Some(key(1).1),
                    },
                    RowChange::Delete {
                        table: table(),
                        key: key(2).1,
                    },
                ],
                images: vec![row(3), row(2)],
            },
            budget(),
        )
        .unwrap();
    // Later chunks may contain old snapshot rows, even after the log won.
    overlay.snapshot(key(1), row(1), budget()).unwrap();
    overlay.snapshot(key(2), row(2), budget()).unwrap();
    overlay.snapshot(key(3), row(30), budget()).unwrap();
    assert!(overlay.rows[&key(1)].is_none());
    assert!(overlay.rows[&key(2)].is_none());
    assert_eq!(overlay.rows[&key(3)], Some(row(3)));
    // A later committed insertion legitimately replaces a tombstone.
    overlay
        .log(
            &ChangeBatch {
                position: pos(25),
                changes: vec![RowChange::Insert {
                    table: table(),
                    key: key(2).1,
                }],
                images: vec![row(2)],
            },
            budget(),
        )
        .unwrap();
    assert_eq!(overlay.rows[&key(2)], Some(row(2)));
}

#[test]
fn repair_budget_counts_tombstones_and_update_churn() {
    let mut overlay = Overlay::default();
    let limit = RepairBudget {
        max_keys: 1,
        max_bytes: 1_000_000,
        required_history_seconds: 0,
    };
    let changes = ChangeBatch {
        position: pos(10),
        changes: vec![
            RowChange::Delete {
                table: table(),
                key: vec![KeyValue::Int(1)],
            },
            RowChange::Delete {
                table: table(),
                key: vec![KeyValue::Int(2)],
            },
        ],
        images: vec![row(1), row(2)],
    };
    assert_eq!(
        overlay.log(&changes, limit).unwrap_err().kind(),
        SourceErrorKind::Budget
    );
    let mut overlay = Overlay::default();
    assert_eq!(
        overlay
            .snapshot(
                (table(), vec![KeyValue::Int(1)]),
                row(1),
                RepairBudget {
                    max_keys: 10,
                    max_bytes: 1,
                    required_history_seconds: 0,
                }
            )
            .unwrap_err()
            .kind(),
        SourceErrorKind::Budget
    );
}

#[test]
fn all_sources_and_bindings_must_confirm_the_exact_end() {
    let scope = BTreeMap::from([
        ("a".into(), BTreeSet::from(["x".into(), "y".into()])),
        ("b".into(), BTreeSet::from(["z".into()])),
    ]);
    let mut barrier = HandoffBarrier::new(scope).unwrap();
    assert!(barrier.stage(fake_repair("a", &["x"])).is_err());
    barrier.stage(fake_repair("a", &["x", "y"])).unwrap();
    assert!(barrier.confirm_applied("a", "x", &pos(123)).is_err());
    assert!(!barrier.ready());
    barrier.stage(fake_repair("b", &["z"])).unwrap();
    assert!(barrier.stage(fake_repair("b", &["z"])).is_err());
    assert!(barrier.confirm_applied("a", "x", &pos(124)).is_err());
    assert!(barrier.confirm_applied("a", "z", &pos(123)).is_err());
    barrier.confirm_applied("a", "x", &pos(123)).unwrap();
    barrier.confirm_applied("b", "z", &pos(123)).unwrap();
    assert!(!barrier.ready());
    barrier.confirm_applied("a", "y", &pos(123)).unwrap();
    assert!(barrier.ready());
    let cursors = barrier.switch().unwrap();
    assert_eq!(
        cursors,
        BTreeMap::from([("a".into(), pos(123)), ("b".into(), pos(123))])
    );
    let empty =
        HandoffBarrier::new(BTreeMap::from([("a".into(), BTreeSet::from(["x".into()]))])).unwrap();
    assert!(empty.switch().is_err());
}

#[test]
fn positions_are_compared_within_a_lineage_only() {
    assert!(covers(&pos(200), &pos(100)).unwrap());
    assert!(!covers(&pos(100), &pos(200)).unwrap());
    let alien = Watermark::MysqlFilePos {
        file: "another.000001".into(),
        position: 4,
    };
    assert!(covers(&pos(100), &alien).is_err());
    let id = "3e11fa47-71ca-11e1-9e33-c80aa9429562";
    let lo = Watermark::MysqlGtidSet {
        gtid_set: format!("{id}:1-2:tag:1"),
    };
    let hi = Watermark::MysqlGtidSet {
        gtid_set: format!("{id}:1-3:tag:1-2"),
    };
    assert!(covers(&hi, &lo).unwrap());
    assert!(!covers(&lo, &hi).unwrap());
    assert!(covers(&lo, &pos(100)).is_err());
}
