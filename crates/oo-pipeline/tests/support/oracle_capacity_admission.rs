//! Uses retained real capture receipts; never connects to a source or target.
use std::path::PathBuf;
use crate::native_binding::{self, OperatorEnv};
use serde_json::{Value, json};

fn read(path: impl AsRef<std::path::Path>) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn resolve(request: &Value, entry: &Value, validator: &str, registry_sha: &str) -> Result<Option<native_binding::ResolvedCapture>, &'static str> {
    native_binding::with_operator_env(OperatorEnv::Fixed {
        bin: Some(validator.into()),
        registry: Some(entry["admission_registry_path"].as_str().unwrap().into()),
        registry_sha256: Some(registry_sha.into()),
    }, || native_binding::resolve(
        request["source_engine"].as_str().unwrap(), request["source_id"].as_str().unwrap(),
        request["source_generation"].as_i64().unwrap(),
        request["snapshot"]["kind"].as_str().unwrap(), request["snapshot"]["value"].as_str().unwrap(),
        request["config_digest"].as_str().unwrap(), request["schema_digest"].as_str().unwrap(),
        request["type_digest"].as_str().unwrap(), request["input_digest"].as_str().unwrap()))
}

#[test]
fn capacity_validator_is_an_exact_consumer_owned_approval() {
    let approved = native_binding::ADMISSION_BIN_SHA256_ORACLE_CAPACITY_20261030;
    assert!(native_binding::approved_admission_bin(approved));
    assert!(native_binding::approved_admission_bin(
        native_binding::ADMISSION_BIN_SHA256_ORACLE_CONTINUOUS_20261027));
    assert!(!native_binding::approved_admission_bin(&approved.to_uppercase()));
    assert!(!native_binding::approved_admission_bin(&"0".repeat(64)));
}

#[test]
#[ignore = "OO_ORACLE_CAPACITY_ADMISSION_FIXTURE names pinned retained real capture and validator; no DB"]
fn retained_real_oracle_receipts_use_the_normal_consumer_trust_boundary() {
    let fixture = read(std::env::var("OO_ORACLE_CAPACITY_ADMISSION_FIXTURE").unwrap());
    let validator = fixture["validator"]["path"].as_str().unwrap();
    let old = fixture["previous_validator"].as_str().unwrap();
    let request = read(fixture["request"].as_str().unwrap());
    assert_eq!(fixture["validator"]["sha256"],
        native_binding::ADMISSION_BIN_SHA256_ORACLE_CAPACITY_20261030);
    let validator_bytes = std::fs::read(validator).unwrap();
    assert_eq!(crate::target_installer::sha256_hex(&validator_bytes),
        native_binding::ADMISSION_BIN_SHA256_ORACLE_CAPACITY_20261030);
    let output = PathBuf::from(fixture["output"].as_str().unwrap());
    std::fs::create_dir(&output).unwrap();
    let unknown = output.join("unapproved-validator");
    let marker = output.join("unapproved-executed");
    std::fs::write(&unknown, format!("#!/bin/sh\nprintf executed > '{}'\n", marker.display())).unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&unknown, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut accepted = Vec::new();
    for (native, entry) in request["native_requests"].as_array().unwrap().iter()
        .zip(request["entries"].as_array().unwrap())
    {
        let sha = entry["admission_registry_sha256"].as_str().unwrap();
        let before = resolve(native, entry, old, sha).unwrap().unwrap();
        let current = resolve(native, entry, validator, sha).unwrap().unwrap();
        assert!(current.source_bound && before.source_bound);
        assert_eq!(current.source_engine, "oracle");
        assert_eq!(current.decision_sha256, before.decision_sha256);
        assert_eq!(current.receipt_sha256, before.receipt_sha256);
        assert_eq!(current.baseline_scope, before.baseline_scope);
        assert_eq!(current.baseline_scope.as_ref().unwrap()["buffer"]["source_offset_write"], false);
        let arrow = std::fs::read(native["arrow_path"].as_str().unwrap()).unwrap();
        let admitted = native_binding::with_operator_env(OperatorEnv::Fixed {
            bin: Some(validator.into()),
            registry: Some(entry["admission_registry_path"].as_str().unwrap().into()),
            registry_sha256: Some(sha.into()),
        }, || crate::native_update::trusted_capture(native, &arrow)).unwrap();
        assert!(admitted.source_bound);
        assert_eq!(admitted.producer_trust["trusted"], true);
        let mut changed_arrow = arrow.clone();
        changed_arrow[0] ^= 1;
        assert!(native_binding::with_operator_env(OperatorEnv::Fixed {
            bin: Some(validator.into()),
            registry: Some(entry["admission_registry_path"].as_str().unwrap().into()),
            registry_sha256: Some(sha.into()),
        }, || crate::native_update::trusted_capture(native, &changed_arrow)).is_err());
        assert!(resolve(native, entry, validator, &"0".repeat(64)).is_err());
        assert!(resolve(native, entry, unknown.to_str().unwrap(), sha).is_err());
        for (field, wrong) in [("source_engine", json!("mysql")), ("source_id", json!("other")),
            ("schema_digest", json!("0".repeat(64))), ("input_digest", json!("0".repeat(64)))]
        {
            let mut bad = native.clone();
            bad[field] = wrong;
            assert!(!matches!(resolve(&bad, entry, validator, sha), Ok(Some(_))), "{field}");
        }
        accepted.push(json!({"source":current.source_id,"source_bound":current.source_bound,
            "decision_sha256":current.decision_sha256,"receipt_sha256":current.receipt_sha256,
            "baseline_scope":current.baseline_scope}));
    }
    assert_eq!(accepted.len(), 2);
    assert!(!marker.exists(), "unapproved programs must never execute");
    std::fs::write(output.join("receipt.json"), serde_json::to_vec_pretty(&json!({
        "passed":true,"tables":accepted,"new_validator":fixture["validator"],
        "normal_native_binding_resolve":true,"old_decisions_unchanged":true,
        "normal_loader_trusted_capture":true,"changed_arrow_refused":true,
        "identity_and_digest_negatives_refused":true,"unapproved_binary_not_executed":true,
        "source_connections":0,"database_connections":0,"target_writes":0,
    })).unwrap()).unwrap();
}
