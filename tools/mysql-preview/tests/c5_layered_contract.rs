use mysql_preview::c5_layered::{decode_bound, encode_bound};
use oo_connectors::mysql::chunk::KeyValue;

#[test]
fn typed_boundaries_preserve_integer_and_byte_identity() {
    let keys = vec![
        KeyValue::Int(-7),
        KeyValue::Text(vec![0, 255, 32]),
        KeyValue::Bytes(vec![65]),
    ];
    let json = encode_bound(Some(&keys));
    assert_eq!(json[1]["value"], "00ff20");
    assert_eq!(decode_bound(&json).unwrap(), Some(keys));
    assert_eq!(decode_bound(&serde_json::Value::Null).unwrap(), None);
    for bad in [
        serde_json::json!([]),
        serde_json::json!([{"kind":"text","value":"F"}]),
        serde_json::json!([{"kind":"int","value":1,"sql":"x"}]),
    ] {
        assert!(decode_bound(&bad).is_err());
    }
}

#[test]
fn loader_members_use_typed_arrow_bytes_without_collation_comparison() {
    use oo_pipeline::native_arrow::{DecodedStream, NativeCell};
    use oo_pipeline::native_c5_range::{KeyComponent, RangeScope};
    let decoded = DecodedStream {
        rows: 2,
        fields: vec![],
        columns: vec![vec![
            NativeCell::Utf8("Z ".into()),
            NativeCell::Utf8("é".into()),
        ]],
    };
    let keys = RangeScope::decoded_keys(&decoded, &[0]).unwrap();
    assert_eq!(
        keys,
        vec![
            vec![KeyComponent::Text("5a20".into())],
            vec![KeyComponent::Text("c3a9".into())]
        ]
    );
}
