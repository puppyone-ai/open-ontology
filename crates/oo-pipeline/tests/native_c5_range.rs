use oo_pipeline::native_c5_range::{RangeScope, TypedKey};
use serde_json::{Value, json};

fn scope() -> Value {
    json!({"protocol":"oo.pipeline.native-c5-range/v1", "source_engine":"mysql",
        "capture_method":"c5_diff", "source_id":"source", "schema":"app", "table":"t_orders",
        "schema_fingerprint":"a".repeat(64), "chunk_topology_revision":1, "chunk_id":"block-1",
        "lo":null, "hi":[{"kind":"text","value":"61"}], "prior_membership_revision":0,
        "parent_generation":0, "source_generation":1, "arrow_sha256":"b".repeat(64),
        "row_count":1, "delete_detection":"reconcile", "position_codec":"snapshot-zero/v1",
        "applied_pos":"0", "members":[[{"kind":"text","value":"5a"}]]})
}

#[test]
fn closed_scope_rejects_every_unknown_field() {
    let mut document = scope();
    assert!(RangeScope::parse(&document).is_ok());
    for field in ["where", "full_snapshot", "previous_signature", "unexpected"] {
        document[field] = json!("anything");
        assert!(RangeScope::parse(&document).is_err(), "{field}");
        document.as_object_mut().unwrap().remove(field);
    }
    document["members"][0][0]["extra"] = json!(true);
    assert!(RangeScope::parse(&document).is_err());
}

#[test]
fn scope_equality_binds_every_field() {
    let document = scope();
    let parsed = RangeScope::parse(&document).unwrap();
    parsed.require_bound(&document).unwrap();
    for field in document.as_object().unwrap().keys() {
        let mut changed = document.clone();
        changed.as_object_mut().unwrap().remove(field);
        assert!(parsed.require_bound(&changed).is_err(), "{field}");
    }
}

#[test]
fn text_membership_is_byte_identity_not_ordering() {
    let mut document = scope();
    document["hi"] = json!([{"kind":"text","value":"41"}]);
    document["members"] = json!([[{"kind":"text","value":"7a"}]]);
    let parsed = RangeScope::parse(&document).unwrap();
    let keys: Vec<TypedKey> = serde_json::from_value(document["members"].clone()).unwrap();
    parsed.validate_keys(&keys).unwrap();
    let wrong: Vec<TypedKey> =
        serde_json::from_value(json!([[{"kind":"text","value":"5a"}]])).unwrap();
    assert!(parsed.validate_keys(&wrong).is_err());
}

#[test]
fn malformed_and_duplicate_members_are_rejected() {
    for value in [json!("Z0"), json!("a"), json!("AA"), json!(3), Value::Null] {
        let mut document = scope();
        document["members"][0][0]["value"] = value;
        assert!(RangeScope::parse(&document).is_err());
    }
    let mut document = scope();
    document["row_count"] = json!(2);
    document["members"] = json!([document["members"][0], document["members"][0]]);
    assert!(RangeScope::parse(&document).is_err());
}

#[test]
fn integer_keys_and_composite_shapes_are_strict() {
    let mut document = scope();
    document["hi"] = json!([{"kind":"int","value":i64::MAX}]);
    document["members"] = json!([[{"kind":"int","value":i64::MIN}]]);
    assert!(RangeScope::parse(&document).is_ok());
    for bad in [json!(1.0), json!("1"), json!(18446744073709551615u64)] {
        document["members"][0][0]["value"] = bad;
        assert!(RangeScope::parse(&document).is_err());
    }
    document["members"] = json!([[{"kind":"int","value":1},{"kind":"text","value":"61"}]]);
    assert!(RangeScope::parse(&document).is_err());
}

#[test]
fn empty_block_still_requires_digest_and_no_members() {
    let mut document = scope();
    document["row_count"] = json!(0);
    document["members"] = json!([]);
    RangeScope::parse(&document)
        .unwrap()
        .validate_keys(&[])
        .unwrap();
    document["arrow_sha256"] = json!("");
    assert!(RangeScope::parse(&document).is_err());
}

#[test]
fn generation_and_semantics_are_not_coerced() {
    for (field, value) in [
        ("source_engine", json!("postgres")),
        ("capture_method", json!("c1_cdc")),
        ("delete_detection", json!("none")),
        ("position_codec", json!("mysql-gtid/v1")),
        ("applied_pos", json!(0)),
        ("source_generation", json!(2)),
        ("chunk_topology_revision", json!(0)),
        ("prior_membership_revision", json!(-1)),
    ] {
        let mut document = scope();
        document[field] = value;
        assert!(RangeScope::parse(&document).is_err(), "{field}");
    }
}

#[test]
fn decoded_arrow_keys_are_complete_and_unique() {
    use oo_pipeline::native_arrow::{DecodedStream, NativeCell};
    let parsed = RangeScope::parse(&scope()).unwrap();
    let mut stream = DecodedStream {
        rows: 1,
        fields: vec![],
        columns: vec![vec![NativeCell::Utf8("Z".to_string())]],
    };
    parsed.validate_decoded_keys(&stream, &[0]).unwrap();
    stream.columns[0][0] = NativeCell::Utf8("z".to_string());
    assert!(parsed.validate_decoded_keys(&stream, &[0]).is_err());
    stream.columns[0][0] = NativeCell::Null;
    assert!(parsed.validate_decoded_keys(&stream, &[0]).is_err());
    assert!(parsed.validate_decoded_keys(&stream, &[1]).is_err());
    assert!(parsed.validate_decoded_keys(&stream, &[]).is_err());
    stream.rows = 2;
    stream.columns[0] = vec![NativeCell::Utf8("Z".to_string()); 2];
    assert!(parsed.validate_decoded_keys(&stream, &[0]).is_err());
}

#[test]
fn full_arrow_digest_is_required_even_for_empty_blocks() {
    use sha2::{Digest, Sha256};
    let bytes = b"immutable Arrow bytes including schema and empty record batch";
    let mut document = scope();
    document["row_count"] = json!(0);
    document["members"] = json!([]);
    document["arrow_sha256"] = json!(format!("{:x}", Sha256::digest(bytes)));
    let parsed = RangeScope::parse(&document).unwrap();
    parsed.validate_arrow_digest(bytes).unwrap();
    assert!(parsed.validate_arrow_digest(b"").is_err());
    assert!(
        parsed
            .validate_arrow_digest(&bytes[..bytes.len() - 1])
            .is_err()
    );
}

#[test]
fn empty_decoded_block_still_checks_key_column_indexes() {
    use oo_pipeline::native_arrow::DecodedStream;
    let mut document = scope();
    document["row_count"] = json!(0);
    document["members"] = json!([]);
    let parsed = RangeScope::parse(&document).unwrap();
    let decoded = DecodedStream {
        rows: 0,
        fields: vec![],
        columns: vec![vec![]],
    };
    parsed.validate_decoded_keys(&decoded, &[0]).unwrap();
    assert!(parsed.validate_decoded_keys(&decoded, &[1]).is_err());
    assert!(parsed.validate_decoded_keys(&decoded, &[0, 0]).is_err());
}
