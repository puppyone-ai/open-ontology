#![cfg(feature = "mysql")]
use oo_connectors::mysql::chunk::{ChunkBound, KeyColumn, KeyType, KeyValue, range_where};

#[test]
fn composite_ranges_expand_in_key_order() {
    let columns = vec![
        KeyColumn {
            name: "a".into(),
            kind: KeyType::Text,
            collation: Some("utf8_general_ci".into()),
        },
        KeyColumn {
            name: "b".into(),
            kind: KeyType::Int,
            collation: None,
        },
    ];
    let lo = vec![KeyValue::Text(vec![0xff]), KeyValue::Int(2)];
    let hi = vec![KeyValue::Text(b"z".to_vec()), KeyValue::Int(9)];
    let (sql, params) = range_where(
        &columns,
        &ChunkBound {
            lo: Some(lo.clone()),
            hi: Some(hi.clone()),
        },
    )
    .unwrap();
    assert_eq!(
        sql,
        " WHERE ((`a` > ?) OR (`a` = ? AND `b` > ?)) AND ((`a` < ?) OR (`a` = ? AND `b` <= ?))"
    );
    assert_eq!(
        params,
        vec![
            lo[0].clone(),
            lo[0].clone(),
            lo[1].clone(),
            hi[0].clone(),
            hi[0].clone(),
            hi[1].clone()
        ]
    );
}

#[test]
fn malformed_bounds_fail_closed() {
    let columns = vec![KeyColumn {
        name: "id".into(),
        kind: KeyType::Int,
        collation: None,
    }];
    assert!(
        range_where(
            &columns,
            &ChunkBound {
                lo: Some(vec![]),
                hi: None
            }
        )
        .is_err()
    );
    assert!(
        range_where(
            &columns,
            &ChunkBound {
                lo: Some(vec![KeyValue::Text(vec![0])]),
                hi: None
            }
        )
        .is_err()
    );
    assert!(range_where(&[], &ChunkBound::default()).is_err());
}

#[test]
fn adaptive_rows_obey_both_caps() {
    use oo_connectors::mysql::chunk::AdaptiveRows;
    use std::time::Duration;
    let mut sizing = AdaptiveRows::new(100, 1000, 10_000).unwrap();
    sizing.observe(100, 5000, Duration::from_millis(100));
    assert_eq!(sizing.rows(), 200);
    sizing.observe(100, 100, Duration::from_secs(2));
    assert!(sizing.rows() <= 1000);
    assert!(AdaptiveRows::new(0, 1, 1).is_err());
}

#[test]
fn boundaries_keep_order_inside_the_limited_subquery() {
    use oo_connectors::mysql::chunk::next_boundary_sql;
    let columns = vec![KeyColumn {
        name: "id".into(),
        kind: KeyType::Text,
        collation: Some("utf8_general_ci".into()),
    }];
    let (sql, params) = next_boundary_sql("fixture", "items", &columns, None, 100).unwrap();
    assert_eq!(
        sql,
        "SELECT MAX(`id`) FROM (SELECT `id` FROM `fixture`.`items` ORDER BY `id` LIMIT 100) x"
    );
    assert!(params.is_empty());
    let value = KeyValue::Text(vec![0, 255, 128]);
    assert_eq!(
        serde_json::from_str::<KeyValue>(&serde_json::to_string(&value).unwrap()).unwrap(),
        value
    );
}

#[test]
fn resume_refuses_corrupt_arrow() {
    use oo_connectors::mysql::chunk::verify_arrow;
    let path = std::env::temp_dir().join(format!("oo-wf-chunk-hash-{}", std::process::id()));
    std::fs::write(&path, b"abc").unwrap();
    assert!(
        verify_arrow(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        )
        .is_ok()
    );
    std::fs::write(&path, b"abd").unwrap();
    assert_eq!(
        verify_arrow(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        )
        .unwrap_err()
        .fact,
        "arrow_sha256"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn manifest_resume_checks_chain_and_files() {
    use oo_connectors::mysql::chunk::{ChunkEntry, load_manifest, save_manifest};
    let dir = std::env::temp_dir().join(format!("oo-wf-chunk-manifest-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("0.arrow"), b"abc").unwrap();
    let mut entry = ChunkEntry {
        table: "items".into(),
        chunk_id: 0,
        lo: None,
        hi: Some(vec![KeyValue::Text(vec![255])]),
        rows: 1,
        bytes: 3,
        arrow_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
    };
    save_manifest(&dir, "items", std::slice::from_ref(&entry)).unwrap();
    assert_eq!(load_manifest(&dir, "items").unwrap().len(), 1);
    entry.chunk_id = 1;
    std::fs::write(
        dir.join("chunks.json"),
        serde_json::to_vec(&vec![&entry]).unwrap(),
    )
    .unwrap();
    assert!(load_manifest(&dir, "items").is_err());
    std::fs::remove_file(dir.join("chunks.json")).unwrap();
    std::fs::remove_file(dir.join("0.arrow")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn shrinking_a_chunk_is_bounded_and_eventually_reports_a_single_row() {
    use oo_connectors::mysql::chunk::AdaptiveRows;
    let mut sizing = AdaptiveRows::new(9, 100, 1000).unwrap();
    assert!(sizing.shrink());
    assert_eq!(sizing.rows(), 4);
    assert!(sizing.shrink());
    assert_eq!(sizing.rows(), 2);
    assert!(sizing.shrink());
    assert_eq!(sizing.rows(), 1);
    assert!(!sizing.shrink());
    assert_eq!(sizing.rows(), 1);
}

#[test]
fn per_column_conclusions_cover_transcoding_temporal_and_exact_decimal() {
    use oo_connectors::mysql::map::column_mapping_conclusion;
    let base = serde_json::json!({"COLUMN_NAME":"value","ORDINAL_POSITION":1,"DATA_TYPE":"text","COLUMN_TYPE":"text","IS_NULLABLE":"YES","CHARACTER_SET_NAME":"gbk","COLUMN_KEY":""});
    for charset in ["gbk", "ucs2", "utf8", "utf8mb3", "utf8mb4"] {
        let mut row = base.clone();
        row["CHARACTER_SET_NAME"] = serde_json::json!(charset);
        let result = column_mapping_conclusion(&row, true);
        assert_eq!(result["accepted"], true, "{result}");
        assert_eq!(result["target_oid"], 25);
    }
    let mut row = base.clone();
    row["DATA_TYPE"] = serde_json::json!("timestamp");
    row["COLUMN_TYPE"] = serde_json::json!("timestamp(6)");
    row["CHARACTER_SET_NAME"] = serde_json::Value::Null;
    assert_eq!(column_mapping_conclusion(&row, true)["target_oid"], 1184);
    assert_eq!(column_mapping_conclusion(&row, false)["reason"], "timezone");
    row["DATA_TYPE"] = serde_json::json!("datetime");
    row["COLUMN_TYPE"] = serde_json::json!("datetime(6)");
    assert_eq!(column_mapping_conclusion(&row, true)["target_oid"], 1114);
    row["DATA_TYPE"] = serde_json::json!("decimal");
    row["COLUMN_TYPE"] = serde_json::json!("decimal(38,6)");
    row["NUMERIC_PRECISION"] = serde_json::json!(38);
    row["NUMERIC_SCALE"] = serde_json::json!(6);
    assert_eq!(column_mapping_conclusion(&row, true)["target_oid"], 1700);
    row["DATA_TYPE"] = serde_json::json!("geometry");
    assert_eq!(column_mapping_conclusion(&row, true)["accepted"], false);
    assert_eq!(
        column_mapping_conclusion(&serde_json::json!({}), true)["accepted"],
        false
    );
}
