#![forbid(unsafe_code)]

use std::path::Path;

use session_control_api::config::{self, CONTRACT};
use session_control_api::startup_gate;

#[test]
#[allow(clippy::assertions_on_constants)]
fn localprocess_listens_and_production_does_not_flip_into_it() {
    assert!(startup_gate("localprocess").is_ok());
    let production = startup_gate("production").expect_err("production must refuse");
    let runsc = Path::new("/usr/local/bin/runsc").is_file() || Path::new("/usr/bin/runsc").is_file();
    let expected = if runsc { "production_driver_absent" } else { "runtime_unavailable" };
    assert_eq!(production, expected);
    assert_eq!(startup_gate("sandbox"), Err("profile"));
    assert!(!session_control_api::PRODUCTION_SIGNED);
    assert_eq!(session_control_api::DEPLOYMENT, "localprocess");
    assert_eq!(CONTRACT, "session-control-api-v1");
}

#[test]
fn client_identity_fields_are_self_asserted() {
    let body = serde_json::json!({"turn_id": "t1", "text": "hello"});
    assert!(!config::self_asserted(&body));
    for key in ["principal_id", "project_id", "role", "claims", "claims_hash", "identity"] {
        let mut object = serde_json::Map::new();
        object.insert(key.to_owned(), serde_json::json!("x"));
        assert!(config::self_asserted(&serde_json::Value::Object(object)), "{key}");
    }
    assert!(config::self_asserted(&serde_json::json!(["x"])));
}

#[test]
fn claims_hash_is_order_independent() {
    let left = vec![("dept".to_owned(), "sales".to_owned()), ("region".to_owned(), "eu".to_owned())];
    let mut right = left.clone();
    right.reverse();
    assert_eq!(config::claims_hash(&left).unwrap(), config::claims_hash(&right).unwrap());
    let empty = config::digest_text("").unwrap();
    assert_eq!(empty, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(
        config::digest_text("abc").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
