use std::fs;
use std::path::PathBuf;

use selfbuild_control_api::adapt::{self, BasisPin, ProtocolPin};
use serde_json::json;

fn pin() -> ProtocolPin {
    ProtocolPin {
        catalog: adapt::CAPTURE_CATALOG.to_owned(),
        profile: adapt::CAPTURE_COMPARE.to_owned(),
        semantics: adapt::SEMANTICS_PROFILE.to_owned(),
        source_commit: "4c8c10cf121eaa2390c98d5ee012b0421aab6e80".to_owned(),
        semantics_commit: "8397fd5b984df364cc3638ffa1e69ea06e125d74".to_owned(),
    }
}

fn catalog() -> String {
    r#"{"protocol":"oo.postgres-preview.capture-check/v1","tables":[{"schema":"public","table":"hubs","columns":["hub_id","badge"]}]}"#.to_owned()
}

fn profile() -> String {
    r#"{"protocol":"oo.postgres-preview.capture-compare/v1","rows_compared":100050,"sampled":false,"full_scan":true}"#.to_owned()
}

fn semantics() -> String {
    r#"{"protocol":"oo.builder.source-semantics-profile/v1","consistency_grade":null,"delete_detection":null}"#.to_owned()
}

fn basis(oid_badge: i64) -> String {
    format!(
        r#"{{"protocol":"oo.pipeline.catalog-import-pg/v1","manifest":{{"protocol":"oo.pipeline.pg-sealed-manifest/v1","tables":[{{"logical_schema":"public","logical_table":"hubs","columns":[{{"logical_name":"hub_id","oid":20,"typmod":-1}},{{"logical_name":"badge","oid":{oid_badge},"typmod":-1}}]}}]}}}}"#
    )
}

fn project_with(badge_oid: i64) -> adapt::Projection {
    let bytes = basis(badge_oid).into_bytes();
    let sha = selfbuild_control_api::canonical::sha256_hex(&bytes);
    adapt::project(
        catalog().as_bytes(),
        profile().as_bytes(),
        semantics().as_bytes(),
        &pin(),
        &[BasisPin {
            protocol: adapt::SEALED_MANIFEST.to_owned(),
            outer_protocol: adapt::CATALOG_IMPORT.to_owned(),
            sha256: sha,
            bytes,
        }],
    )
    .expect("legal fixture")
}

#[test]
fn bytea_oid_is_unknown_and_not_boolean() {
    assert_eq!(adapt::oos_from_pg_oid(17, -1), None);
    assert_eq!(adapt::oos_from_pg_oid(16, -1), Some("boolean"));
    assert_eq!(adapt::oos_from_pg_oid(20, -1), Some("bigint"));
    assert_eq!(adapt::oos_from_pg_oid(1700, 2490376), Some("numeric"));
    assert_eq!(adapt::oos_from_pg_oid(2950, -1), Some("uuid"));
    assert_eq!(adapt::oos_from_pg_oid(1184, -1), Some("timestamptz"));
    let projection = project_with(17);
    let columns = projection.receipt["columns"].as_array().unwrap();
    let badge = columns.iter().find(|item| item["column"] == "badge").unwrap();
    assert_eq!(badge["type_status"], "unknown");
    assert_eq!(badge["oos_support"], "oos_unsupported");
    assert_eq!(badge["target_type"], "bytea");
    assert_eq!(badge["data_type"], json!(null));
    assert_eq!(badge["auto_approved"], false);
    assert_eq!(badge["pii"], "unknown");
    assert_eq!(badge["pii_core_level"], false);
    assert_eq!(badge["semantic"], json!(null));
    assert_eq!(badge["oid"], 17);
    assert_eq!(projection.receipt["derive_source_bound_called"], false);
    assert_eq!(projection.receipt["derive_skip_reason"], "engine_not_oracle");
    assert_eq!(projection.receipt["engine"], "postgres");
    assert_eq!(projection.receipt["core_commit"], selfbuild_control_api::corebind::CORE_COMMIT);
    let hub = columns.iter().find(|item| item["column"] == "hub_id").unwrap();
    assert_eq!(hub["data_type"], "bigint");
    assert_eq!(hub["auto_approved"], false);
    assert_eq!(projection.receipt["grade_status"], "unknown");
    assert_eq!(projection.receipt["delete_status"], "unknown");
    assert_eq!(projection.receipt["consistency_grade"], json!(null));
    assert_eq!(projection.receipt["unknown_auto_approved"], false);
    let typed = projection.typed_catalog["tables"][0]["columns"].as_array().unwrap();
    assert!(typed.iter().all(|item| item["name"] != "badge"));
    assert!(typed.iter().any(|item| item["name"] == "hub_id" && item["data_type"] == "bigint"));
}

#[test]
fn a_changed_oid_changes_the_receipt_and_does_not_approve() {
    let left = project_with(17);
    let right = project_with(25);
    assert_ne!(
        selfbuild_control_api::canonical::canonical_sha256(&left.receipt),
        selfbuild_control_api::canonical::canonical_sha256(&right.receipt)
    );
    let badge = right.receipt["columns"].as_array().unwrap().iter().find(|item| item["column"] == "badge").unwrap();
    assert_eq!(badge["data_type"], "text");
    assert_eq!(badge["auto_approved"], false);
}

#[test]
fn wrong_protocol_and_forged_type_are_rejected() {
    let bytes = basis(20).into_bytes();
    let sha = selfbuild_control_api::canonical::sha256_hex(&bytes);
    let basis_pin = BasisPin {
        protocol: adapt::SEALED_MANIFEST.to_owned(),
        outer_protocol: adapt::CATALOG_IMPORT.to_owned(),
        sha256: sha,
        bytes,
    };
    let mut wrong = pin();
    wrong.catalog = "oo.postgres-preview.other/v1".to_owned();
    assert!(adapt::project(catalog().as_bytes(), profile().as_bytes(), semantics().as_bytes(), &wrong, std::slice::from_ref(&basis_pin)).is_err());
    let forged = basis(20).replace("\"oid\":20", "\"oid\":20,\"data_type\":\"boolean\"");
    let forged_bytes = forged.into_bytes();
    let forged_sha = selfbuild_control_api::canonical::sha256_hex(&forged_bytes);
    assert!(adapt::project(
        catalog().as_bytes(),
        profile().as_bytes(),
        semantics().as_bytes(),
        &pin(),
        &[BasisPin {
            protocol: adapt::SEALED_MANIFEST.to_owned(),
            outer_protocol: adapt::CATALOG_IMPORT.to_owned(),
            sha256: forged_sha,
            bytes: forged_bytes,
        }],
    )
    .is_err());
    let mut sampled = profile();
    sampled = sampled.replace("\"sampled\":false", "\"sampled\":true");
    let bytes = basis(20).into_bytes();
    let sha = selfbuild_control_api::canonical::sha256_hex(&bytes);
    assert!(adapt::project(
        catalog().as_bytes(),
        sampled.as_bytes(),
        semantics().as_bytes(),
        &pin(),
        &[BasisPin {
            protocol: adapt::SEALED_MANIFEST.to_owned(),
            outer_protocol: adapt::CATALOG_IMPORT.to_owned(),
            sha256: sha,
            bytes,
        }],
    )
    .is_err());
}

fn evidence_root() -> PathBuf {
    std::env::var("OO_SELFBUILD_REPO_ROOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

#[test]
fn fixed_pg_and_semantics_bytes_map_without_rewriting() {
    let root = evidence_root();
    let pg = root.join("evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product");
    let catalog_path = pg.join("linux/runner/out/capture/phases/catalog-capture-check.json");
    let profile_path = pg.join("linux/runner/out/capture/phases/compare-capture-compare.json");
    let run_basis = pg.join("linux/runner/out/run/stages/catalog-import.request.json");
    let east_basis = pg.join("linux/runner/out/customer-east/stages/catalog-import.request.json");
    let semantics_path = root.join(
        "evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/oracle-source-bound/post-edit-6/cli/batch-positive/profile.json",
    );
    let catalog_bytes = fs::read(&catalog_path).expect("catalog");
    let profile_bytes = fs::read(&profile_path).expect("profile");
    let semantics_bytes = fs::read(&semantics_path).expect("semantics");
    let before = catalog_bytes.clone();
    let bases = [run_basis, east_basis]
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).expect("basis");
            let sha = selfbuild_control_api::canonical::sha256_hex(&bytes);
            BasisPin {
                protocol: adapt::SEALED_MANIFEST.to_owned(),
                outer_protocol: adapt::CATALOG_IMPORT.to_owned(),
                sha256: sha,
                bytes,
            }
        })
        .collect::<Vec<_>>();
    let projection = adapt::project(&catalog_bytes, &profile_bytes, &semantics_bytes, &pin(), &bases).expect("fixed artifacts");
    assert_eq!(fs::read(&catalog_path).unwrap(), before);
    assert_eq!(projection.receipt["rows_compared"], 100050);
    assert_eq!(projection.receipt["grade_status"], "unknown");
    assert_eq!(projection.receipt["delete_status"], "unknown");
    assert_eq!(projection.receipt["source_commit"], "4c8c10cf121eaa2390c98d5ee012b0421aab6e80");
    assert_eq!(projection.receipt["semantics_commit"], "8397fd5b984df364cc3638ffa1e69ea06e125d74");
    assert_eq!(projection.receipt["unknown_auto_approved"], false);
    assert_eq!(projection.receipt["derive_source_bound_called"], false);
    assert_eq!(projection.receipt["derive_skip_reason"], "engine_not_oracle");
    assert_eq!(projection.receipt["core_commit"], selfbuild_control_api::corebind::CORE_COMMIT);
    assert_eq!(projection.receipt["version_binding"]["source_commit"], "4c8c10cf121eaa2390c98d5ee012b0421aab6e80");
    assert_eq!(projection.receipt["version_binding"]["core_commit"], selfbuild_control_api::corebind::CORE_COMMIT);
    let columns = projection.receipt["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 11 + 12 + 9 + 6);
    for name in ["badge", "seal", "payload", "receipt"] {
        let column = columns.iter().find(|item| item["column"] == name).unwrap();
        assert_eq!(column["oid"], 17, "{name}");
        assert_eq!(column["type_status"], "unknown");
        assert_eq!(column["oos_support"], "oos_unsupported");
        assert_eq!(column["target_type"], "bytea");
        assert_eq!(column["data_type"], json!(null));
        assert_eq!(column["auto_approved"], false);
        assert_eq!(column["pii_core_level"], false);
    }
    let hub = columns.iter().find(|item| item["dataset"] == "public.hubs" && item["column"] == "hub_id").unwrap();
    assert_eq!(hub["data_type"], "bigint");
    let amount = columns.iter().find(|item| item["column"] == "amount").unwrap();
    assert_eq!(amount["data_type"], "numeric");
    assert_eq!(amount["auto_approved"], false);
    let typed_names: Vec<&str> = projection.typed_catalog["tables"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|table| table["columns"].as_array().unwrap())
        .map(|column| column["name"].as_str().unwrap())
        .collect();
    assert!(!typed_names.contains(&"badge"));
    assert!(typed_names.contains(&"hub_id"));
}

#[test]
fn postgres_capture_does_not_call_derive_source_bound() {
    let err = selfbuild_control_api::corebind::derive_oracle(catalog().as_str(), profile().as_str()).expect_err("pg capture");
    assert_eq!(err.code, "engine_not_oracle");
    assert!(!selfbuild_control_api::corebind::core_pii_level("unknown"));
    assert!(selfbuild_control_api::corebind::core_pii_level("none"));
    assert!(selfbuild_control_api::corebind::core_pii_level("restricted"));
    assert!(selfbuild_control_api::corebind::core_semantic("identifier"));
    assert!(!selfbuild_control_api::corebind::core_semantic("unknown"));
    assert_eq!(adapt::oos_from_target_name("bytea"), None);
    assert_eq!(adapt::pg_target_name(17, -1), Some("bytea"));
}

#[test]
fn oracle_derive_source_bound_matches_batch_positive_profile() {
    let root = evidence_root();
    let dir = root.join("evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/oracle-source-bound/post-edit-6/cli/batch-positive");
    let request: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("request.json")).expect("oracle request")).expect("request json");
    let profile: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("profile.json")).expect("oracle profile")).expect("profile json");
    let catalog = serde_json::to_string(request.get("catalog").expect("catalog")).expect("catalog json");
    let bound = serde_json::to_string(request.get("source_bound").expect("source_bound")).expect("bound json");
    let receipt = selfbuild_control_api::corebind::derive_oracle(&catalog, &bound).expect("derive_source_bound");
    let profile_identity = profile.get("identity_hash").and_then(|value| value.as_str()).expect("profile identity");
    assert_eq!(profile_identity, "2c2f05e3442a5722ca0cb7eaf46a5a318f87212e4219a1fb87fbd2ea2a8e66be");
    assert_eq!(receipt.identity_hash, profile_identity, "derived identity diverged from the fixed profile");
    assert!(receipt.called);
    assert_eq!(receipt.engine, "oracle");
    assert_eq!(receipt.source_id, "oo04_t_batch");
    assert_eq!(receipt.version, "23.26.3.0.0");
    assert_eq!(receipt.consistency_grade, None);
    assert_eq!(receipt.delete_detection, None);
    assert_eq!(receipt.bytea_columns, vec!["r".to_owned()]);
    assert!(receipt.target_types.iter().any(|(name, target)| name == "r" && target == "bytea"));
    assert!(!receipt.target_types.iter().any(|(name, target)| name == "r" && target == "boolean"));
    if let Ok(path) = std::env::var("OO_SELFBUILD_ORACLE_RECEIPT") {
        let body = serde_json::to_vec_pretty(&selfbuild_control_api::corebind::oracle_receipt_json(&receipt, profile_identity)).expect("receipt json");
        fs::write(&path, body).unwrap_or_else(|err| panic!("oracle receipt write {path}: {err}"));
    }
}
