use std::path::PathBuf;

use mongodb_preview::docseal;
use serde_json::Value;

fn repo(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn baseline_lines() -> Vec<String> {
    let path = repo(
        "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-64-fulldoc/probe/full-approved.bson.hex",
    );
    std::fs::read_to_string(path)
        .expect("baseline hex")
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn report(raw: &[u8]) -> Value {
    docseal::decode_report(raw).expect("decode")
}

#[test]
fn baseline_sixteen_keep_canonical_weight() {
    let lines = baseline_lines();
    assert_eq!(lines.len(), 16);
    for (index, line) in lines.iter().enumerate() {
        let raw = docseal::decode_hex(line).expect("hex");
        let body = report(&raw);
        assert_eq!(body["outcome"], "decoded");
        assert_eq!(body["source_acceptance"], false);
        assert_eq!(body["weight_encoding"], "$numberDouble");
        assert_eq!(body["bson_types"]["_id"], serde_json::json!({"variant":"07"}));
        assert_eq!(body["bson_types"]["uid"], serde_json::json!({"variant":"05","subtype":"04"}));
        assert_eq!(body["bson_types"]["blob"], serde_json::json!({"variant":"05","subtype":"00"}));
        let weight = body["weight"].as_str().expect("weight");
        assert_eq!(weight, format!("{}.25", 41 + index));
        assert!(docseal::weight_bits_match(weight).is_ok());
        let extended = &body["extended_json"];
        assert!(extended["weight"].get("$numberDouble").is_some());
        assert!(extended["weight"].as_number().is_none());
        assert!(extended.get("blob_presence").is_none());
        assert!(extended.get("memo_presence").is_none());
        assert!(extended.get("hiddenNote").is_none());
        assert!(extended["units"].get("$numberLong").is_some());
        assert!(extended["price"].get("$numberDecimal").is_some());
        assert!(extended["units"].as_number().is_none());
        assert!(extended["price"].as_number().is_none());
    }
    let first = docseal::decode_hex(&lines[0]).expect("row0");
    let first_body = report(&first);
    assert_eq!(first_body["weight"], "41.25");
    assert_eq!(first_body["weight_bits"], "4044a00000000000");
}

#[test]
fn replacing_the_same_code_keeps_the_server_bytes() {
    let raw = docseal::decode_hex(&baseline_lines()[0]).expect("hex");
    let again = docseal::set_root_string(&raw, "code", "S41-000").expect("set");
    assert_eq!(again, raw);
}

#[test]
fn hidden_note_stays_unsealed_and_weight_stays() {
    let raw = docseal::decode_hex(&baseline_lines()[0]).expect("hex");
    let mutated = docseal::set_root_string(&raw, "memo", "memo-c5-period").expect("memo");
    let mutated = docseal::set_root_string(&mutated, "hiddenNote", "not-approved-field").expect("hidden");
    let body = report(&mutated);
    assert_eq!(body["outcome"], "scope_refused");
    assert_eq!(body["weight"], "41.25");
    assert_eq!(body["raw_doc_authorized"], false);
    assert_eq!(body["extended_json"]["memo"], "memo-c5-period");
    assert_eq!(body["extended_json"]["hiddenNote"], "not-approved-field");
    let (text, pg) = docseal::logical_jsonb_file(&body["extended_json"]).expect("logical");
    let envelope = docseal::document_envelope(&mutated, &pg, &text);
    let err = docseal::seal_value(&envelope).expect_err("scope");
    assert_eq!(err.code, "oo.document.bson-jsonb/v1/field.scope");
}

#[test]
fn logical_seal_keeps_weight_as_number_double() {
    let raw = docseal::decode_hex(&baseline_lines()[0]).expect("hex");
    let body = report(&raw);
    let (text, pg) = docseal::logical_jsonb_file(&body["extended_json"]).expect("logical");
    let envelope = docseal::document_envelope(&raw, &pg, &text);
    let sealed = docseal::seal_value(&envelope).expect("seal");
    assert_eq!(sealed["weight"], "41.25");
    assert_eq!(sealed["oid"], 3802);
    assert_eq!(sealed["typmod"], -1);
    assert_eq!(sealed["double_encoding"], "canonical_number_double");
    assert_eq!(sealed["source_acceptance"], false);
    assert_eq!(sealed["generation_created"], false);
    assert_eq!(sealed["extended_json"]["weight"]["$numberDouble"], "41.25");
}

#[test]
fn fixed_customer_catalog_profiles_the_weight_token() {
    let command: Value = serde_json::from_str(
        &std::fs::read_to_string(repo(
            "evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/bson-double/seal/command.json",
        ))
        .expect("command"),
    )
    .expect("command json");
    let envelope = command["document"].clone();
    let catalog = serde_json::to_string(&command["catalog"]).expect("catalog");
    let profile = docseal::profile_document(&catalog, &envelope).expect("profile");
    assert_eq!(profile["compiler"], "compile_jsonb_document_profile");
    assert_eq!(profile["ready"], false);
    assert_eq!(profile["source_acceptance"], false);
    assert!(profile["document"].as_str().expect("document").contains("41.25"));
    assert!(profile["document"].as_str().expect("document").contains("canonical_number_double"));
}

#[test]
fn importer_probe_catalog_parses_and_does_not_create_a_generation() {
    let raw = docseal::decode_hex(&baseline_lines()[0]).expect("hex");
    let body = report(&raw);
    let (text, pg) = docseal::logical_jsonb_file(&body["extended_json"]).expect("logical");
    let envelope = docseal::document_envelope(&raw, &pg, &text);
    let request = docseal::importer_request(&envelope).expect("request");
    assert_eq!(request["protocol"], "oo.pipeline.jsonb-document-import/v1");
    assert_eq!(request["generation_created"], false);
    assert_eq!(request["import_invoked"], false);
    assert_eq!(request["document"]["dataset"], "oo_w141.shipments");
}
