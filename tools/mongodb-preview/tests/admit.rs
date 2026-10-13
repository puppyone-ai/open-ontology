use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use mongodb_preview::{
    arrow_matches, authorize_read, carries_client_authority, decide, fixture_input, frozen_columns, frozen_driver,
    gap_document, inputs, manifest_engine, native_import_document, run_shared_probe,
    shared_contract_refused, sha256_hex, work_has_password_file, write_probe_inputs,
    FROZEN_ARROW_BYTES, FROZEN_ARROW_SHA256, FROZEN_DRIVER_DIGEST, MANIFEST_PROTOCOL,
    NATIVE_IMPORT_PROTOCOL, OBSERVED_COMPAT,
};
use source_admission::admit;

#[test]
fn native_import_keeps_capture_types_and_refuses_client_authority() {
    let document = native_import_document("aa", "bb", "cc", "dd", "/frozen/batch.arrow", FROZEN_ARROW_SHA256);
    assert_eq!(document.get("protocol").and_then(|value| value.as_str()), Some(NATIVE_IMPORT_PROTOCOL));
    assert_eq!(document.get("source_engine").and_then(|value| value.as_str()), Some("mongodb"));
    assert!(!carries_client_authority(&document));
    assert!(document.get("admission").is_none());
    let columns = document
        .pointer("/tables/0/columns")
        .and_then(|value| value.as_array())
        .expect("columns");
    assert_eq!(columns.len(), frozen_columns().len());
    let arrow_type = |name: &str| {
        columns.iter().find_map(|column| {
            (column.get("name").and_then(|value| value.as_str()) == Some(name))
                .then(|| column.get("arrow_type").and_then(|value| value.as_str()).unwrap_or(""))
        })
    };
    assert_eq!(arrow_type("_id"), Some("fixedsizebinary(12)"));
    assert_eq!(arrow_type("created"), Some("timestamp(ms,utc)"));
    assert_eq!(arrow_type("marks"), Some("list(utf8)"));
    assert_eq!(arrow_type("pack"), Some("struct(kind:utf8,n:int32)"));
    assert_eq!(arrow_type("lane"), Some("union(dense,0:utf8,1:int64)"));
    let text = document.to_string();
    assert!(!text.contains("\"source_engine\":\"mysql\""));
    assert!(!text.contains("\"source_engine\":\"postgres-native\""));
    assert!(!text.contains("clickhouse_268"));
}

#[test]
fn mongodb_native_read_is_admitted_without_retag() {
    let input = fixture_input();
    assert!(frozen_driver(&input));
    let (report, config) = inputs(&input).expect("inputs");
    assert_eq!(report.compat_mode, OBSERVED_COMPAT);
    assert_eq!(config.compat_mode, OBSERVED_COMPAT);
    assert_eq!(report.permissions[0].name, "read");
    assert_eq!(report.driver.digest, FROZEN_DRIVER_DIGEST);
    assert!(report.capabilities.iter().all(|cap| cap.cdc.is_none()));
    assert!(report.capture_methods.iter().all(|method| method.cdc.is_none()));
    let decision = decide(&input).expect("decision");
    assert!(decision.admitted);
    assert!(decision.blocks.is_empty());
    assert!(!decision.matrix_mutated);
    assert!(!decision.zero_write.proven);
    assert_eq!(decision.zero_write.basis, "not_audited");
    let gap = gap_document(&input, &decision, None);
    assert_eq!(gap.get("import_started").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("builder_started").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("operator_preview_started").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("sealed_import_called").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("role_pack_called").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("model_called").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("pg_budget_protocol_used").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("engine").and_then(|value| value.as_str()), Some("mongodb"));
    assert_eq!(gap.get("shared_probe").and_then(|value| value.as_str()), Some("not_run"));
    assert_eq!(gap.get("source_acceptance").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(
        gap.get("manifest_protocol").and_then(|value| value.as_str()),
        Some(MANIFEST_PROTOCOL)
    );
    let text = gap.to_string();
    assert_eq!(gap.get("engine").and_then(|value| value.as_str()), Some("mongodb"));
    assert_eq!(gap.get("compat_observed").and_then(|value| value.as_str()), Some("mongodb-native"));
    assert!(!text.contains("\"engine\":\"postgres-native\""));
    assert!(!text.contains("\"engine\":\"mysql\""));
    assert_eq!(gap.get("pg_budget_protocol_used").and_then(|value| value.as_bool()), Some(false));
}

#[test]
fn write_unknown_and_bypass_grants_stay_denied() {
    for permission in ["readWrite", "root", "SELECT", "reader", "Read", "readAnyDatabase"] {
        let mut input = fixture_input();
        input.permission = permission.to_string();
        let decision = decide(&input).expect("decision");
        assert!(!decision.admitted, "{permission}");
        assert!(!decision.matrix_mutated);
        let codes: Vec<&str> = decision.blocks.iter().map(|block| block.code.as_str()).collect();
        assert!(
            codes.contains(&"PERMISSION_NOT_READONLY") || codes.contains(&"PERMISSION_UNKNOWN"),
            "{permission} {codes:?}"
        );
    }
}

#[test]
fn unknown_budget_unit_stays_a_denial() {
    let mut input = fixture_input();
    input.budget_unit = "ReadOps".to_string();
    let decision = decide(&input).expect("decision");
    assert!(!decision.admitted);
    let codes: Vec<&str> = decision.blocks.iter().map(|block| block.code.as_str()).collect();
    assert!(codes.contains(&"BUDGET_UNIT_UNKNOWN"));
    assert!(!codes.contains(&"COMPAT_MODE_UNKNOWN"));
    assert_eq!(inputs(&input).expect("inputs").0.compat_mode, OBSERVED_COMPAT);
}

#[test]
fn r1w_without_approval_stays_denied() {
    let input = fixture_input();
    let (report, mut config) = inputs(&input).expect("inputs");
    config.requested_write_class = "R1w".to_string();
    let decision = admit(&report, &config);
    assert!(!decision.admitted);
    assert!(decision.blocks.iter().any(|block| block.code == "R1W_UNAPPROVED"));
}

#[test]
fn credential_material_is_rejected_before_admit() {
    let mut input = fixture_input();
    input.secret_ref = "mongodb://reader:secret@127.0.0.1".to_string();
    assert!(decide(&input).is_err());
    input.secret_ref = "secret://oo-dl01w1-41/reader".to_string();
    input.driver_digest = "password".to_string();
    assert!(decide(&input).is_err());
    input.driver_digest = "a".repeat(64);
    assert!(!frozen_driver(&input));
    let decision = decide(&input).expect("other digest still reaches admit");
    assert!(decision.admitted);
}

#[test]
fn probe_manifest_keeps_mongodb_and_calls_the_pinned_commands() {
    let root = std::env::temp_dir().join(format!("oo-dl01w1-41-probe-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("dir");
    let census = serde_json::json!({
        "sample_not_enforced_schema": true,
        "documents": 16,
        "fields": [
            {"field": "code", "bson_types": ["string"]},
            {"field": "lane", "bson_types": ["string", "int64"]}
        ]
    });
    write_probe_inputs(&root, &fixture_input(), &census, FROZEN_ARROW_SHA256).expect("inputs");
    assert_eq!(manifest_engine(&root).expect("engine"), "mongodb");
    let manifest = fs::read_to_string(root.join("manifest.json")).expect("manifest");
    assert!(manifest.contains(MANIFEST_PROTOCOL));
    assert!(manifest.contains("\"source_type\": \"string\""));
    assert!(manifest.contains("mixed:string+int64"));
    assert!(manifest.contains("\"target_oid\": null"));
    assert!(!manifest.contains("postgres-native"));
    assert!(!manifest.contains("oo.mysql-preview.manifest"));
    assert!(!work_has_password_file(&root));
    let fake = root.join("pg-builder");
    fs::write(
        &fake,
        "#!/bin/sh\nif [ \"$1\" = \"import\" ]; then echo \"input.schema the manifest protocol is not the sealed import manifest\" >&2; exit 1; fi\nif [ \"$1\" = \"import-cards\" ]; then echo \"catalog.schema the catalog source engine is not mysql\" >&2; exit 1; fi\necho \"unexpected\" >&2\nexit 9\n",
    )
    .expect("fake");
    let mut mode = fs::metadata(&fake).expect("meta").permissions();
    mode.set_mode(0o755);
    fs::set_permissions(&fake, mode).expect("chmod");
    let probe = run_shared_probe(&fake, &root).expect("probe");
    assert!(shared_contract_refused(&probe));
    let decision = decide(&fixture_input()).expect("decision");
    let gap = gap_document(&fixture_input(), &decision, Some(&probe));
    assert_eq!(gap.get("shared_probe").and_then(|value| value.as_str()), Some("shared_contract"));
    assert_eq!(gap.get("import_started").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("sealed_import_called").and_then(|value| value.as_bool()), Some(false));
    assert_eq!(gap.get("model_called").and_then(|value| value.as_bool()), Some(false));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn frozen_arrow_identity_is_exact() {
    let mut bytes = vec![0_u8; FROZEN_ARROW_BYTES as usize];
    assert!(!arrow_matches(&bytes));
    bytes = vec![1_u8; 4];
    assert!(!arrow_matches(&bytes));
    assert_eq!(sha256_hex(b"hook\nnone\n").len(), 64);
    let arrow = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow");
    if arrow.is_file() {
        let file = fs::read(&arrow).expect("arrow");
        assert!(arrow_matches(&file), "{}", sha256_hex(&file));
    }
}

#[test]
fn cli_denies_a_write_grant_and_stops_before_import() {
    let input = fixture_input();
    let dir = std::env::temp_dir().join(format!("oo-dl01w1-41-deny-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let out = dir.join("decision.json");
    let gap = dir.join("gap.json");
    let customer = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/customer.shipments.json");
    let customer = customer.to_str().expect("utf8");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_mongodb-preview"))
        .args([
            "--customer",
            customer,
            "--source-id",
            &input.source_id,
            "--secret-ref",
            &input.secret_ref,
            "--driver-digest",
            &input.driver_digest,
            "--permission",
            "readWrite",
            "--budget-unit",
            &input.budget_unit,
            "--budget-limit",
            "16",
            "--out",
            out.to_str().expect("utf8"),
            "--gap-out",
            gap.to_str().expect("utf8"),
        ])
        .status()
        .expect("cli");
    assert_eq!(status.code(), Some(3));
    let text = fs::read_to_string(&out).expect("decision");
    let gap_text = fs::read_to_string(&gap).expect("gap");
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("\"admitted\": false"));
    assert!(text.contains("PERMISSION_NOT_READONLY"));
    assert!(gap_text.contains("\"import_started\": false"));
    assert!(gap_text.contains("\"shared_probe\": \"not_run\""));
    assert!(text.contains("connectionStatus"));
    assert!(text.contains("db.grantRolesToUser"));
    assert!(text.contains("role: \\\"read\\\""));
    assert!(!text.contains("PASSWORD"));
    assert!(!text.contains("GRANT SELECT"));
    assert!(!text.contains("mongodb://"));
    assert!(!text.contains("CANARY-SECRET"));
    assert!(!text.contains("postgres-native"));
    assert!(gap_text.contains("\"engine\": \"mongodb\""));
    assert!(!gap_text.contains("\"engine\": \"postgres-native\""));
}

#[test]
fn cli_admits_read_and_does_not_exit_4_without_a_probe() {
    let input = fixture_input();
    let dir = std::env::temp_dir().join(format!("oo-dl01w1-41-admit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let out = dir.join("decision.json");
    let gap = dir.join("gap.json");
    let customer = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/customer.shipments.json");
    let customer = customer.to_str().expect("utf8");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_mongodb-preview"))
        .args([
            "--customer",
            customer,
            "--source-id",
            &input.source_id,
            "--secret-ref",
            &input.secret_ref,
            "--driver-digest",
            &input.driver_digest,
            "--permission",
            "read",
            "--budget-unit",
            "Rows",
            "--budget-limit",
            "16",
            "--out",
            out.to_str().expect("utf8"),
            "--gap-out",
            gap.to_str().expect("utf8"),
        ])
        .status()
        .expect("cli");
    assert_eq!(status.code(), Some(2));
    let text = fs::read_to_string(&out).expect("decision");
    let gap_text = fs::read_to_string(&gap).expect("gap");
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("\"admitted\": true"));
    assert!(gap_text.contains("\"engine\": \"mongodb\""));
    assert!(gap_text.contains("\"import_started\": false"));
    assert!(gap_text.contains("\"model_called\": false"));
    assert!(gap_text.contains("\"engine\": \"mongodb\""));
    assert!(!text.contains("postgres-native"));
    assert!(!gap_text.contains("\"engine\": \"postgres-native\""));
}

#[test]
fn unapproved_object_and_field_are_refused_before_a_read() {
    let input = fixture_input();
    assert!(authorize_read(&input, "oo_w141.shipments", &input.approved_fields).is_ok());
    assert_eq!(
        authorize_read(&input, "oo_w141.not_approved", &input.approved_fields).unwrap_err(),
        "unapproved_object"
    );
    assert_eq!(
        authorize_read(&input, "oo_w141.shipments", &["notApproved".to_string()]).unwrap_err(),
        "unapproved_field"
    );
    let customer = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/customer.shipments.json");
    let customer = customer.to_str().expect("utf8");
    let dir = std::env::temp_dir().join(format!("oo-dl01w1-41-preread-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let out = dir.join("decision.json");
    let gap = dir.join("gap.json");
    let missing = dir.join("missing.arrow");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_mongodb-preview"))
        .args([
            "--customer",
            customer,
            "--source-id",
            &input.source_id,
            "--secret-ref",
            &input.secret_ref,
            "--driver-digest",
            &input.driver_digest,
            "--permission",
            "read",
            "--budget-unit",
            "Rows",
            "--budget-limit",
            "16",
            "--read-object",
            "oo_w141.not_approved",
            "--arrow",
            missing.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--gap-out",
            gap.to_str().expect("utf8"),
        ])
        .output()
        .expect("cli");
    let stderr = String::from_utf8_lossy(&status.stderr);
    let gap_text = fs::read_to_string(&gap).expect("gap");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(status.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("before read"));
    assert!(!stderr.contains("arrow file could not be read"));
    assert!(gap_text.contains("\"find_sent\": false"));
    assert!(gap_text.contains("\"reason\": \"unapproved_object\""));
    assert!(gap_text.contains("\"password_read\": false"));
}
