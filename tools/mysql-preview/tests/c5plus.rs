use mysql_preview::c5plus::*;
#[test]
fn digest_binds_keys_and_nulls() {
    let sql = digest_sql(
        "s",
        "t",
        &["id"],
        &["id", "v"],
        "`id` > ? AND `id` <= ?",
        5000,
    )
    .unwrap();
    assert!(sql.contains("MD5(CONCAT_WS"));
    assert!(sql.contains("ISNULL(`v`)"));
    assert!(sql.contains("HEX(CAST(`id` AS BINARY))"));
    assert!(!sql.contains("CRC32"));
    assert!(digest_sql("s", "t", &["id"], &["v"], "1", 5000).is_err());
}
#[test]
fn hints_are_only_positive() {
    assert_eq!(hint(None, None, 10), Hint::Unknown);
    assert_eq!(hint(Some(9), Some(9), 10), Hint::Hot);
    assert_eq!(hint(Some(5), Some(8), 10), Hint::Changed);
}
#[test]
fn overnight_window_and_budget() {
    let w = Window::parse("20:00-08:00").unwrap();
    assert!(w.contains(23 * 60));
    assert!(w.contains(7 * 60));
    assert!(!w.contains(12 * 60));
    let mut b = Budget::default();
    assert_eq!(
        b.rest(std::time::Duration::from_millis(100)).as_millis(),
        300
    );
    assert!(b.adapt(1000, 1.0) < 1000);
    assert!(full_read_due(None, 10, 20));
    assert!(!full_read_due(Some(9), 10, 20));
}

fn chunk(id: &str, rows: u64) -> Chunk {
    Chunk {
        chunk_id: id.into(),
        bound: serde_json::json!([0, 10]),
        rows,
        digest: [1, 2],
        scanned_at: 1,
    }
}
#[test]
fn chunk_comparison_and_topology() {
    let a = chunk("a", 1);
    let b = chunk("b", 1);
    let c = chunk("c", 2);
    let mut newer = a.clone();
    newer.scanned_at = 9;
    let d = compare(&[a.clone(), b], &[newer, c]).unwrap();
    assert_eq!(d.unchanged, ["a"]);
    assert_eq!(d.new, ["c"]);
    assert_eq!(d.removed, ["b"]);
    let mut changed = a.clone();
    changed.bound = serde_json::json!([0, 5]);
    assert_eq!(compare(&[a.clone()], &[changed]).unwrap().changed, ["a"]);
    assert!(compare(&[], &[a.clone(), a]).is_err());
    assert_eq!(resize(300, 100), Resize::Split);
    assert_eq!(resize(1, 100), Resize::MergeCandidate);
}
struct Target(Vec<TypedRow>);
impl TargetRows for Target {
    fn current_rows(&self, _: &str) -> Result<Vec<TypedRow>> {
        Ok(self.0.clone())
    }
}
fn row(key: u64, value: u64) -> TypedRow {
    TypedRow {
        key: vec![key.into()],
        values: vec![
            oo_connectors::mysql::values::decode_cell(
                oo_connectors::mysql::map::StoreKind::Int { width: 8 },
                &oo_connectors::mysql::values::RawCell::Uint(value),
                false,
            )
            .unwrap()
            .0,
        ],
    }
}
#[test]
fn typed_diff_and_key_reuse() {
    let out = reconcile(
        &[row(1, 3), row(3, 4)],
        &Target(vec![row(1, 2), row(2, 2)]),
        "a",
        7,
    )
    .unwrap();
    assert_eq!(
        out,
        vec![
            Mutation::Update(row(1, 3)),
            Mutation::Insert(row(3, 4)),
            Mutation::Delete {
                key: vec![2u64.into()],
                tombstone_generation: 7
            }
        ]
    );
    assert_eq!(
        reconcile(&[row(2, 9)], &Target(vec![]), "a", 8).unwrap(),
        vec![Mutation::Insert(row(2, 9))]
    );
}
#[test]
fn state_roundtrip_and_validation() {
    let c = chunk("a", 12);
    let state =
        serde_json::json!({"version":1,"table":"s.t","chunks":[chunk_json(&c)],"next_chunk":"b"});
    let path = std::env::temp_dir().join(format!("oo-wf-c5plus-state-{}.json", std::process::id()));
    save(&path, &state).unwrap();
    let restored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(load_chunks(&restored).unwrap(), vec![c]);
    std::fs::remove_file(path).unwrap();
    assert!(load_chunks(&serde_json::json!({"version":2})).is_err());
}

#[test]
fn interrupted_scan_only_checkpoints_applied_blocks() {
    let a = chunk("a", 2);
    let b = chunk("b", 2);
    let mut progress = ScanProgress {
        table: "s.t".into(),
        previous: vec![a.clone()],
        completed: vec![],
        planned: vec![
            (a.chunk_id.clone(), a.bound.clone()),
            (b.chunk_id.clone(), b.bound.clone()),
        ],
    };
    let path =
        std::env::temp_dir().join(format!("oo-wf-c5plus-progress-{}.json", std::process::id()));
    assert!(progress.changes().is_err());
    assert!(progress.checkpoint_applied(b.clone(), &path).is_err());
    progress.checkpoint_applied(a, &path).unwrap();
    let mut recovered = ScanProgress::load(&path).unwrap();
    assert_eq!(recovered.next_pending().unwrap().0, "b");
    recovered.checkpoint_applied(b, &path).unwrap();
    assert_eq!(recovered.changes().unwrap().new, ["b"]);
    std::fs::remove_file(path).unwrap();
}
