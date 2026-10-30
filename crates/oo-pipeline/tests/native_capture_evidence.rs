//! Replay retained admission evidence without connecting to a source or target.
use oo_pipeline::native_update::trusted_capture;
use serde_json::Value;
use std::path::Path;

#[test]
#[ignore = "requires OO_NATIVE_EVIDENCE_REQUEST and the matching operator registry environment"]
fn retained_capture_admission_matches_without_opening_target() {
    let path = std::env::var("OO_NATIVE_EVIDENCE_REQUEST").unwrap();
    assert!(Path::new(&path).is_absolute());
    let document: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let arrow_path = document["arrow_path"].as_str().unwrap();
    let limit = document["max_arrow_bytes"].as_u64().unwrap();
    assert!(std::fs::metadata(arrow_path).unwrap().len() <= limit);
    let arrow = std::fs::read(arrow_path).unwrap();
    let result = trusted_capture(&document, &arrow).expect("retained admission must remain bound");
    assert!(result.source_bound);
    assert_eq!(result.producer_trust["trusted"], true);
}
