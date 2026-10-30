use oo_pipeline::masked_rejects::project;
use serde_json::json;

#[test]
fn masks_unclassified_and_sensitive_values_by_construction() {
    let records = json!([{"source_id":"fixture","schema":"fixture","table":"items","column":"payload","row_index":0,"key_digest":"a".repeat(64),"reason":"charset","value_len":2,"run_generation":1,"input_digest":"b".repeat(64)}]);
    let got = project("fixture", "fixture", "items", &records).unwrap();
    assert_eq!(got[0]["raw_value"], json!({"masked":true,"value_len":2}));
    assert_eq!(got[0]["pk"], "a".repeat(64));
    let mut leaked = records.clone();
    leaked[0]["raw_value"] = json!("synthetic-sensitive");
    assert!(project("fixture", "fixture", "items", &leaked).is_err());
}

#[test]
fn row_budget_reject_has_a_masked_boundary_identity_and_known_length() {
    let records = json!([{"source_id":"fixture","schema":"fixture","table":"items","run_generation":1,"reason":"row_budget_exceeded","budget_fact":"max_bytes","boundary_sha256":"c".repeat(64),"key_digest":null,"value_len":8192,"raw_value_recorded":false}]);
    let got = project("fixture", "fixture", "items", &records).unwrap();
    assert_eq!(got[0]["pk"], "c".repeat(64));
    assert_eq!(got[0]["raw_value"], json!({"masked":true,"value_len":8192}));
    assert!(project("other", "fixture", "items", &records).is_err());
}

#[test]
fn keyless_reject_uses_only_a_hashed_occurrence_row_locator() {
    let rows = json!([{"source_id":"fixture","schema":"fixture","table":"items","column":"payload","row_index":0,"key_digest":null,"reason":"charset","value_len":2,"run_generation":1,"input_digest":"b".repeat(64)}]);
    let projected = project("fixture", "fixture", "items", &rows).unwrap();
    assert_eq!(projected[0]["pk"].as_str().unwrap().len(), 64);
    assert_eq!(
        projected[0]["raw_value"],
        json!({"masked":true,"value_len":2})
    );
}
