use oo_builder::{compile_captured_pg_catalog, finalize_pg_capture};
use serde_json::{Value, json};

fn catalog() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/captured-pg-review/catalog-before.json"
    ))
    .unwrap()
}

#[test]
fn finalize_view_capture_preserves_evidence_and_seals_it() {
    let mut capture = catalog();
    capture["tables"].as_array_mut().unwrap().truncate(1);
    let table = &mut capture["tables"][0];
    table["relkind"] = json!("v");
    table["primary_key"] = json!([]);
    table["primary_key_nulls_not_distinct"] = Value::Null;
    table["schema"] = json!("oo_load_data");
    table["name"] = json!("published");
    let comment = table["columns"][2]["comment"].clone();
    let result = finalize_pg_capture(capture).unwrap();
    assert_eq!(result["tables"][0]["relkind"], "v");
    assert_eq!(result["tables"][0]["schema_ident"], "oo_load_data");
    assert_eq!(result["tables"][0]["columns"][2]["comment"], comment);
    compile_captured_pg_catalog(&result.to_string(), None, None).unwrap();
}

#[test]
fn finalize_does_not_drop_keys_or_accept_malformed_columns() {
    let mut original = catalog();
    original["tables"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|table| table["name"].as_str().unwrap().to_owned());
    let result = finalize_pg_capture(original.clone()).unwrap();
    assert_eq!(result, original);
    let mut malformed = original;
    malformed["tables"][0]["columns"][0]["attnum"] = json!(0);
    assert!(finalize_pg_capture(malformed).is_err());
}
