//! Policy checks for admit-pg-budget. These documents are fixtures.
//! They are not the oo-connectors producer.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use source_admission::{Decision, admit_pg_budget};

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn doc_bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn fact(status: &str, value: Value) -> Value {
    json!({"status": status, "value": value, "reason": null})
}

fn access_doc() -> Value {
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "oo_dl01w1_38",
        "secret_ref": "secret://oo-dl01w1-38/reader",
        "tls": {
            "mode": "verify-full",
            "trust": "caller-ca-only",
            "hostname_verification": true,
            "negotiated_version": "TLSv1.3",
            "server_observation": "observed"
        },
        "version": {
            "server_version_num": 170010,
            "server_version": "17.10",
            "major": 17,
            "minor": 10,
            "declared_minimum_major": 13,
            "tested_versions": ["17.10"],
            "admission": "tested"
        },
        "registration_allowed": true,
        "block_reasons": [],
        "principal": fact("known", json!({
            "current_user": "reader",
            "session_user": "reader",
            "database": "src"
        })),
        "in_recovery": fact("known", json!(false)),
        "role": fact("known", json!({
            "name": "reader",
            "superuser": false,
            "inherit": true,
            "create_role": false,
            "create_db": false,
            "can_login": true,
            "replication": false,
            "bypass_rls": false
        })),
        "memberships": [],
        "write": {"state": "read_only", "findings": []},
        "security_definers": [],
        "rls": [],
        "publications": fact("known", json!([])),
        "replication_slots": fact("known", json!([])),
        "cdc": {
            "wal_level": fact("known", json!("replica")),
            "replication_attribute": fact("known", json!(false)),
            "publication_for_selected": fact("known", json!(false)),
            "implemented": false
        },
        "capabilities": [
            {"name": "scan", "supported": true, "reason": "reader"},
            {"name": "bounded_read", "supported": true, "reason": "reader"},
            {"name": "cdc", "supported": false, "reason": "not_in_this_slice"},
            {"name": "type_conversion", "supported": false, "reason": "not_in_this_slice"},
            {"name": "registry_api", "supported": false, "reason": "not_in_this_slice"},
            {"name": "source_ddl", "supported": false, "reason": "forbidden"},
            {"name": "source_write", "supported": false, "reason": "forbidden"}
        ],
        "dba_checklist": [{
            "id": "role",
            "level": "required",
            "risk": "login",
            "rollback": "DROP ROLE reader;",
            "statement": "CREATE ROLE reader WITH LOGIN PASSWORD '<由 DBA 设置>'; -- CANARY-SECRET-DL01W102"
        }]
    })
}

fn setting(name: &str, configured: &str, observed: &str, unit: &str, status: &str) -> Value {
    json!({
        "name": name,
        "status": status,
        "configured": configured,
        "setting": observed,
        "unit": unit
    })
}

fn bound(
    unit: &str,
    limit: u64,
    server_enforced: bool,
    server_setting: Option<&str>,
    observed: Option<u64>,
    server_status: &str,
) -> Value {
    json!({
        "unit": unit,
        "configured_limit": limit,
        "configured_status": "known",
        "client_enforced": true,
        "server_enforced": server_enforced,
        "server_setting": server_setting,
        "server_observed": observed,
        "server_status": server_status
    })
}

fn object(temp: bool) -> Value {
    json!({
        "schema": "app",
        "table": "item",
        "connect": true,
        "temp": temp,
        "database_create": false,
        "usage": true,
        "schema_create": false,
        "select": true,
        "insert": false,
        "update": false,
        "delete": false,
        "truncate": false,
        "references": false,
        "trigger": false
    })
}

fn evidence_doc(access: &Value) -> Value {
    let digest = sha256(&doc_bytes(access));
    json!({
        "protocol": "oo.source.pgsql.budget/v1",
        "core_contract": "oo.config/v1",
        "source_id": "oo_dl01w1_38",
        "secret_ref": "secret://oo-dl01w1-38/reader",
        "companion": {"protocol": "oo.source.pgsql/v1", "sha256": digest},
        "identity": {
            "status": "known",
            "current_user": "reader",
            "session_user": "reader",
            "database": "src",
            "aligned": true
        },
        "tls": access["tls"].clone(),
        "version": {
            "server_version_num": 170010,
            "server_version": "17.10",
            "major": 17,
            "minor": 10,
            "admission": "tested"
        },
        "driver": {
            "kind": "in_process",
            "name": "sqlx-postgres",
            "version": "0.9.0",
            "features": "_tls-rustls-ring-no-roots",
            "digest": "d510f5df0eddeb651367bbdf62f30b795c0d747f379104d990f307f8fd0664a5"
        },
        "shadow": "none",
        "compat_mode": "postgres-native",
        "compat_status": "known",
        "target_id": "postgres-17-native",
        "registration_allowed": true,
        "block_reasons": [],
        "permission": {"status": "known", "read_only": true, "objects": [object(false)]},
        "settings": {
            "statement_timeout": setting("statement_timeout", "7000", "7000", "ms", "known"),
            "lock_timeout": setting("lock_timeout", "3000", "3000", "ms", "known"),
            "idle_in_transaction_session_timeout": setting("idle_in_transaction_session_timeout", "7000", "7000", "ms", "known"),
            "default_transaction_read_only": setting("default_transaction_read_only", "on", "on", "", "known"),
            "connect_timeout_ms": 5000,
            "connect_timeout_client_enforced": true,
            "connect_timeout_server_enforced": false,
            "row_rate_per_sec": 50000,
            "row_rate_client_enforced": true,
            "row_rate_server_enforced": false
        },
        "bounds": [
            bound("Rows", 11, false, None, None, "not_applicable"),
            bound("Bytes", 4096, false, None, None, "not_applicable"),
            bound("WallClock", 7000, true, Some("statement_timeout"), Some(7000), "known")
        ],
        "cost_model": {
            "family": "oltp_accounting",
            "units": ["Rows", "Bytes", "WallClock"],
            "estimate_confidence": "unknown",
            "estimate_basis": "not_collected",
            "billing": "none"
        },
        "blocks": []
    })
}

fn config_doc() -> Value {
    json!({
        "protocol": "oo.source-admission.pg-budget/v1",
        "target_id": "postgres-17-native",
        "source_id": "oo_dl01w1_38",
        "secret_ref": "secret://oo-dl01w1-38/reader",
        "compat_mode": "postgres-native",
        "current_user": "reader",
        "database": "src",
        "hard_caps": [
            {"unit": "Rows", "hard_limit": 11, "client_cap": 11, "confidence": "unknown"},
            {"unit": "Bytes", "hard_limit": 4096, "client_cap": 4096, "confidence": "unknown"},
            {"unit": "WallClock", "hard_limit": 7000, "client_cap": 7000, "confidence": "unknown"}
        ],
        "write_options": {
            "signal_table": false,
            "heartbeat_write": false,
            "create_stream": false,
            "create_trigger": false
        },
        "debezium": {
            "signal_data_collection": null,
            "heartbeat_action_query": null,
            "snapshot_mode": "never",
            "create_stream": false,
            "create_trigger": false
        },
        "requested_write_class": "R0",
        "r1w": null
    })
}

fn triple() -> (Value, Value, Value) {
    let access = access_doc();
    let evidence = evidence_doc(&access);
    (access, evidence, config_doc())
}

fn decide(
    access: &Value,
    evidence: &Value,
    config: &Value,
) -> Result<Decision, source_admission::ParseError> {
    admit_pg_budget(&doc_bytes(evidence), &doc_bytes(access), &doc_bytes(config))
}

fn codes(decision: &Decision) -> Vec<&str> {
    decision
        .blocks
        .iter()
        .map(|block| block.code.as_str())
        .collect()
}

fn assert_block(label: &str, decision: &Decision, code: &str) {
    assert!(!decision.admitted, "{label}");
    assert!(!decision.matrix_mutated, "{label}");
    assert!(!decision.zero_write.proven, "{label}");
    assert_eq!(decision.zero_write.basis, "not_audited", "{label}");
    assert!(
        codes(decision).contains(&code),
        "{label} codes={:?}",
        codes(decision)
    );
    assert!(
        decision
            .model
            .capabilities
            .iter()
            .all(|cap| cap.name != "Cdc"),
        "{label}"
    );
}

fn set_blocks(evidence: &mut Value, items: &[(&str, &str)]) {
    evidence["blocks"] = json!(
        items
            .iter()
            .map(|(code, fact)| json!({"code": code, "fact": fact}))
            .collect::<Vec<_>>()
    );
}

#[test]
fn policy_fixture_admits_only_when_budget_facts_are_known() {
    let (access, evidence, config) = triple();
    let decision = decide(&access, &evidence, &config).unwrap();
    assert!(decision.admitted, "{:?}", decision.blocks);
    assert!(!decision.matrix_mutated);
    assert!(!decision.zero_write.proven);
    assert_eq!(decision.zero_write.basis, "not_audited");
    assert_eq!(
        decision.identity.source_report_protocol.as_deref(),
        Some("oo.source.pgsql.budget/v1")
    );
    assert_eq!(
        decision.identity.core_contract.as_deref(),
        Some("oo.config/v1")
    );
    assert_eq!(
        decision.identity.source_report_sha256.as_deref(),
        Some(sha256(&doc_bytes(&evidence)).as_str())
    );
    let names: Vec<_> = decision
        .model
        .capabilities
        .iter()
        .map(|cap| cap.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["Catalog", "BatchFull", "Budget", "Budget", "Budget"]
    );
    let units: Vec<_> = decision
        .model
        .capabilities
        .iter()
        .filter_map(|cap| cap.budget_unit.as_deref())
        .collect();
    assert_eq!(units, vec!["Rows", "Bytes", "WallClock"]);
    assert!(
        decision
            .model
            .capabilities
            .iter()
            .all(|cap| cap.level == "designed" && cap.write_class == "R0")
    );
    let statement = &decision.dba_checklist[0].statement;
    assert!(
        statement.contains("PASSWORD '<由 DBA 设置>'"),
        "{statement}"
    );
    assert!(statement.contains("<redacted>"), "{statement}");
    assert!(!statement.contains("CANARY-SECRET-DL01W102"), "{statement}");
}

#[test]
fn hard_cap_unit_and_identity_failures_block() {
    let (access, evidence, mut config) = triple();
    config["hard_caps"][0]["hard_limit"] = json!(10);
    let decision = decide(&access, &evidence, &config).unwrap();
    assert_block("mismatch", &decision, "BUDGET_UNIT_MISMATCH");

    let (access, evidence, mut config) = triple();
    config["hard_caps"] = json!([
        {"unit": "Bytes", "hard_limit": 4096, "client_cap": 4096, "confidence": "unknown"},
        {"unit": "WallClock", "hard_limit": 7000, "client_cap": 7000, "confidence": "unknown"}
    ]);
    assert_block(
        "missing",
        &decide(&access, &evidence, &config).unwrap(),
        "BUDGET_UNIT_UNKNOWN",
    );

    let (access, mut evidence, config) = triple();
    evidence["cost_model"]["estimate_confidence"] = json!("known");
    assert_block(
        "known estimate",
        &decide(&access, &evidence, &config).unwrap(),
        "BUDGET_UNIT_UNKNOWN",
    );

    let (access, mut evidence, config) = triple();
    evidence["cost_model"]["units"] = json!(["Rows", "Bytes", "Credits"]);
    assert_block(
        "credits",
        &decide(&access, &evidence, &config).unwrap(),
        "BUDGET_UNIT_UNKNOWN",
    );

    let (access, evidence, mut config) = triple();
    config["current_user"] = json!("other");
    assert_block(
        "user",
        &decide(&access, &evidence, &config).unwrap(),
        "IDENTITY_MISMATCH",
    );

    let (access, mut evidence, config) = triple();
    evidence["identity"]["status"] = json!("unknown");
    set_blocks(&mut evidence, &[("IDENTITY_MISSING", "identity.status")]);
    assert_block(
        "missing identity",
        &decide(&access, &evidence, &config).unwrap(),
        "IDENTITY_MISSING",
    );

    let (access, mut evidence, config) = triple();
    evidence["companion"]["sha256"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_block(
        "companion",
        &decide(&access, &evidence, &config).unwrap(),
        "IDENTITY_MISMATCH",
    );

    let (mut access, mut evidence, config) = triple();
    access["core_contract"] = json!("oo.core.contract/fixture");
    evidence["core_contract"] = json!("oo.core.contract/fixture");
    evidence["companion"]["sha256"] = json!(sha256(&doc_bytes(&access)));
    assert_block(
        "fixture contract",
        &decide(&access, &evidence, &config).unwrap(),
        "IDENTITY_MISMATCH",
    );
}

#[test]
fn server_caps_permission_and_compat_fail_closed() {
    let (access, mut evidence, config) = triple();
    evidence["bounds"][0]["server_enforced"] = json!(true);
    set_blocks(&mut evidence, &[("SERVER_CAP_MISSTATED", "bounds")]);
    assert_block(
        "row server cap",
        &decide(&access, &evidence, &config).unwrap(),
        "SERVER_CAP_MISSTATED",
    );

    let (access, mut evidence, config) = triple();
    evidence["settings"]["statement_timeout"]["setting"] = json!("1");
    evidence["bounds"][2]["server_enforced"] = json!(false);
    evidence["bounds"][2]["server_observed"] = json!(1);
    evidence["bounds"][2]["server_status"] = json!("mismatch");
    set_blocks(&mut evidence, &[("SERVER_SETTING_MISMATCH", "settings")]);
    assert_block(
        "wall mismatch",
        &decide(&access, &evidence, &config).unwrap(),
        "SERVER_SETTING_MISMATCH",
    );

    let (access, mut evidence, config) = triple();
    evidence["settings"]["statement_timeout"]["status"] = json!("unknown");
    evidence["settings"]["statement_timeout"]["unit"] = json!("s");
    evidence["settings"]["statement_timeout"]["setting"] = json!("7");
    evidence["bounds"][2]["server_enforced"] = json!(false);
    evidence["bounds"][2]["server_observed"] = Value::Null;
    evidence["bounds"][2]["server_status"] = json!("unknown");
    set_blocks(&mut evidence, &[("SERVER_SETTING_UNKNOWN", "settings")]);
    assert_block(
        "unit",
        &decide(&access, &evidence, &config).unwrap(),
        "SERVER_SETTING_UNKNOWN",
    );

    let (access, mut evidence, config) = triple();
    evidence["permission"]["objects"][0]["temp"] = json!(true);
    evidence["permission"]["read_only"] = json!(false);
    set_blocks(&mut evidence, &[("PERMISSION_NOT_READONLY", "permission")]);
    assert_block(
        "temp",
        &decide(&access, &evidence, &config).unwrap(),
        "PERMISSION_NOT_READONLY",
    );

    let (access, mut evidence, config) = triple();
    evidence["permission"] = json!({"status": "unknown", "read_only": false, "objects": []});
    set_blocks(&mut evidence, &[("PERMISSION_UNKNOWN", "permission")]);
    assert_block(
        "permission unknown",
        &decide(&access, &evidence, &config).unwrap(),
        "PERMISSION_UNKNOWN",
    );

    let (access, mut evidence, config) = triple();
    evidence["shadow"] = json!("local-pg");
    set_blocks(&mut evidence, &[("NATIVE_SOURCE_RETAG", "shadow")]);
    assert_block(
        "retag",
        &decide(&access, &evidence, &config).unwrap(),
        "NATIVE_SOURCE_RETAG",
    );

    let (mut access, mut evidence, mut config) = triple();
    access["version"]["server_version"] = json!("17.10 KingbaseES");
    evidence["version"]["server_version"] = json!("17.10 KingbaseES");
    evidence["compat_mode"] = json!("unknown");
    evidence["compat_status"] = json!("unknown");
    evidence["target_id"] = json!("unknown");
    config["compat_mode"] = json!("unknown");
    config["target_id"] = json!("unknown");
    evidence["companion"]["sha256"] = json!(sha256(&doc_bytes(&access)));
    set_blocks(&mut evidence, &[("COMPAT_MODE_UNKNOWN", "compat_mode")]);
    assert_block(
        "kingbase",
        &decide(&access, &evidence, &config).unwrap(),
        "COMPAT_MODE_UNKNOWN",
    );

    let (mut access, mut evidence, config) = triple();
    access["tls"]["server_observation"] = json!("unknown");
    evidence["tls"]["server_observation"] = json!("unknown");
    evidence["companion"]["sha256"] = json!(sha256(&doc_bytes(&access)));
    set_blocks(&mut evidence, &[("TLS_UNOBSERVED", "tls")]);
    assert_block(
        "tls",
        &decide(&access, &evidence, &config).unwrap(),
        "TLS_UNOBSERVED",
    );

    let (access, mut evidence, config) = triple();
    evidence["driver"]["digest"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    assert_block(
        "driver",
        &decide(&access, &evidence, &config).unwrap(),
        "DRIVER_MISSING",
    );
}

#[test]
fn write_class_and_write_options_still_deny() {
    for (class, code) in [
        ("R2", "WRITE_CLASS_R2"),
        ("R3", "WRITE_CLASS_R3"),
        ("R1w", "R1W_UNAPPROVED"),
    ] {
        let (access, evidence, mut config) = triple();
        config["requested_write_class"] = json!(class);
        assert_block(class, &decide(&access, &evidence, &config).unwrap(), code);
    }
    let cases = [
        ("signal_table", "write_options", "SIGNAL_WRITE"),
        ("heartbeat_write", "write_options", "HEARTBEAT_WRITE"),
        ("create_stream", "write_options", "STREAM_WRITE"),
        ("create_trigger", "write_options", "TRIGGER_WRITE"),
    ];
    for (field, parent, code) in cases {
        let (access, evidence, mut config) = triple();
        config[parent][field] = json!(true);
        assert_block(field, &decide(&access, &evidence, &config).unwrap(), code);
    }
}

#[test]
fn malformed_evidence_is_invalid_not_admitted() {
    let (access, _evidence, config) = triple();
    let err = admit_pg_budget(b"{}\n", &doc_bytes(&access), &doc_bytes(&config)).unwrap_err();
    assert!(!err.fact.is_empty());
}

fn bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_oo_source_admission") {
        return PathBuf::from(path);
    }
    let mut exe = std::env::current_exe().expect("current_exe");
    exe.pop();
    exe.pop();
    exe.push("oo-source-admission");
    exe
}

fn scratch(name: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "oo-dl01w1-38-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
}

#[test]
fn cli_admits_the_policy_fixture_and_denies_r2() {
    let (access, evidence, config) = triple();
    let access_path = scratch("access.json");
    let evidence_path = scratch("evidence.json");
    let config_path = scratch("config.json");
    write(&access_path, &doc_bytes(&access));
    write(&evidence_path, &doc_bytes(&evidence));
    write(&config_path, &doc_bytes(&config));
    let output = Command::new(bin())
        .args([
            "admit-pg-budget",
            "--evidence",
            evidence_path.to_str().unwrap(),
            "--access",
            access_path.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (access, evidence, mut config) = triple();
    config["requested_write_class"] = json!("R2");
    let access_path = scratch("access.json");
    let evidence_path = scratch("evidence.json");
    let config_path = scratch("config.json");
    write(&access_path, &doc_bytes(&access));
    write(&evidence_path, &doc_bytes(&evidence));
    write(&config_path, &doc_bytes(&config));
    let output = Command::new(bin())
        .args([
            "admit-pg-budget",
            "--evidence",
            evidence_path.to_str().unwrap(),
            "--access",
            access_path.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let missing = Command::new(bin()).arg("admit-pg-budget").output().unwrap();
    assert_eq!(missing.status.code(), Some(1));
}
