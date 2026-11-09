use oo_builder::adjudication::adjudication_summary_sha256;
use oo_core::api::payload_sha256;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[test]
fn shared_vector_matches_builder_and_tui_preimage() {
    let raw = include_str!("adjudication-hash-vector.json");
    assert_eq!(
        raw,
        include_str!("../../../execution/v0.3/contracts/hash-vector.json")
    );
    let vector: Value = serde_json::from_str(raw).unwrap();
    let doc = &vector["document"];
    assert_eq!(adjudication_summary_sha256(doc), vector["expected_sha256"]);
    // Independently mirror the TUI's BTreeMap + serde_json serialization.
    let decisions: BTreeMap<&str, &str> = doc["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["id"].as_str().unwrap(), i["decision"].as_str().unwrap()))
        .collect();
    let preimage = json!({"decisions":decisions,"graph":doc["graph"]}).to_string();
    assert_eq!(preimage, vector["preimage"]);
    assert_eq!(
        payload_sha256(preimage.as_bytes()),
        vector["expected_sha256"]
    );
    let mut annotated = doc.clone();
    annotated["items"][0]["reason"] = json!("Reworded evidence");
    assert_eq!(
        adjudication_summary_sha256(&annotated),
        vector["expected_sha256"]
    );
    annotated["items"][0]["decision"] = json!("visible");
    assert_ne!(
        adjudication_summary_sha256(&annotated),
        vector["expected_sha256"]
    );
}
