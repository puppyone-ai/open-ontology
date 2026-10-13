//! Native report interpretation through `admit` and the `validate` CLI.
//! Fixture bytes are the preview protocols. These checks are not mocks.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use source_admission::{SourceConfig, TargetReport, admit, parse_strict};

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

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "oo-dl01w1-38-native-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn fixture(name: &str) -> PathBuf {
    manifest().join("fixtures/native").join(name)
}

fn write_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .output()
        .expect("run oo-source-admission");
    let code = output.status.code().unwrap_or(127);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    (code, stdout, stderr)
}

fn validate_paths(report: &Path, config: &Path) -> (i32, Value, String) {
    let (code, stdout, stderr) = run(&[
        "validate",
        "--report",
        report.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
    ]);
    if code == 1 {
        panic!("validate invalid: {stderr}");
    }
    let doc: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{stdout} {stderr}"));
    (code, doc, stderr)
}

fn validate_values(report: &Value, config: &Value) -> (i32, Value, String) {
    let report_path = scratch("report.json");
    let config_path = scratch("config.json");
    write_json(&report_path, report);
    write_json(&config_path, config);
    validate_paths(&report_path, &config_path)
}

fn load_native(stem: &str) -> (Value, Value) {
    let report = fs::read(fixture(&format!("{stem}-report.json"))).unwrap();
    let config = fs::read(fixture(&format!("{stem}-config.json"))).unwrap();
    (
        serde_json::from_slice(&report).unwrap(),
        serde_json::from_slice(&config).unwrap(),
    )
}

fn load_sql() -> (Value, Value) {
    let report = fs::read(manifest().join("fixtures/admit-ok-report.json")).unwrap();
    let config = fs::read(manifest().join("fixtures/admit-ok-config.json")).unwrap();
    (
        serde_json::from_slice(&report).unwrap(),
        serde_json::from_slice(&config).unwrap(),
    )
}

fn codes(doc: &Value) -> Vec<String> {
    doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap().to_string())
        .collect()
}

fn facts(doc: &Value) -> Vec<String> {
    doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{} {}",
                item["code"].as_str().unwrap(),
                item["fact"].as_str().unwrap()
            )
        })
        .collect()
}

fn assert_closed(doc: &Value, label: &str) {
    assert_eq!(doc["matrix_mutated"], json!(false), "{label}");
    assert_eq!(doc["zero_write"]["proven"], json!(false), "{label}");
    assert_eq!(doc["zero_write"]["basis"], json!("not_audited"), "{label}");
    assert_eq!(
        doc["protocol"],
        json!("oo.source-admission.decision/v1"),
        "{label}"
    );
}

fn set_permissions(report: &mut Value, config: &mut Value, name: &str, granted: bool) {
    let item = json!({"name": name, "status": "known", "granted": granted});
    report["permissions"] = json!([item.clone()]);
    config["permissions"] = json!([item]);
}

fn checklist_text(doc: &Value) -> String {
    serde_json::to_string(&doc["dba_checklist"]).unwrap()
}

#[test]
fn mongodb_product_report_is_admitted_by_library_and_cli() {
    let report_path = fixture("mongodb-product-report.json");
    let config_path = fixture("mongodb-product-config.json");
    let report: TargetReport = parse_strict(&fs::read(&report_path).unwrap()).unwrap();
    let config: SourceConfig = parse_strict(&fs::read(&config_path).unwrap()).unwrap();
    let decision = admit(&report, &config);
    assert!(decision.admitted);
    assert!(decision.blocks.is_empty());
    assert!(!decision.matrix_mutated);
    assert!(!decision.zero_write.proven);
    assert_eq!(decision.zero_write.basis, "not_audited");
    assert_eq!(decision.identity.source_id, "oo_w141");
    assert_eq!(
        decision.identity.target_id,
        "mongodb-8.0.32-community-standalone"
    );
    assert_eq!(decision.identity.secret_ref, "secret://oo-dl01w1-41/reader");
    assert!(decision.identity.source_report_protocol.is_none());
    assert!(decision.identity.source_report_sha256.is_none());
    assert!(decision.identity.core_contract.is_none());

    let (exit, doc, stderr) = validate_paths(&report_path, &config_path);
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["blocks"], json!([]));
    assert_closed(&doc, "mongo");
    assert_eq!(doc["identity"]["source_id"], json!("oo_w141"));
    assert_eq!(
        doc["identity"]["secret_ref"],
        json!("secret://oo-dl01w1-41/reader")
    );
    assert_eq!(
        doc["identity"]["target_id"],
        json!("mongodb-8.0.32-community-standalone")
    );
    assert!(doc["identity"]["source_report_sha256"].is_null());
    let text = checklist_text(&doc);
    assert!(text.contains("connectionStatus"));
    assert!(text.contains("role: \\\"read\\\""));
    assert!(text.contains("db.grantRolesToUser"));
    assert!(text.contains("<database>"));
    assert!(text.contains("oo_w141"));
    assert!(!text.contains("pg_roles"));
    assert!(!text.contains("GRANT SELECT"));
    assert!(!text.contains("PASSWORD"));
}

#[test]
fn mongodb_writable_and_bypass_roles_are_denied() {
    for name in [
        "readWrite",
        "root",
        "readAnyDatabase",
        "dbOwner",
        "clusterMonitor",
    ] {
        let (mut report, mut config) = load_native("mongodb-product");
        set_permissions(&mut report, &mut config, name, true);
        let (exit, doc, stderr) = validate_values(&report, &config);
        assert_eq!(exit, 2, "{name} {stderr} {doc}");
        assert_eq!(doc["admitted"], json!(false), "{name}");
        assert!(
            codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()),
            "{name} {:?}",
            codes(&doc)
        );
        assert_closed(&doc, name);
    }
}

#[test]
fn mongodb_missing_read_grant_is_unknown() {
    let (mut report, mut config) = load_native("mongodb-product");
    set_permissions(&mut report, &mut config, "readWrite", false);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
    assert!(!codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()));

    let (mut report, mut config) = load_native("mongodb-product");
    report["permissions"] = json!([]);
    config["permissions"] = json!([]);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
}

#[test]
fn cross_engine_permission_names_stay_unknown() {
    let (mut report, mut config) = load_native("mongodb-product");
    set_permissions(&mut report, &mut config, "SELECT", true);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).iter().all(|code| code == "PERMISSION_UNKNOWN"));
    assert!(!codes(&doc).contains(&"COMPAT_MODE_UNKNOWN".to_string()));

    let (mut report, mut config) = load_sql();
    set_permissions(&mut report, &mut config, "read", true);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
    assert_eq!(doc["identity"]["source_id"], json!("pg_selfmanaged"));

    let (mut report, mut config) = load_native("doris-preview");
    set_permissions(&mut report, &mut config, "read", true);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
}

#[test]
fn malformed_mongo_names_and_status_are_unknown() {
    for name in ["reader", "Read", "read ", "readonly"] {
        let (mut report, mut config) = load_native("mongodb-product");
        set_permissions(&mut report, &mut config, name, true);
        let (exit, doc, _) = validate_values(&report, &config);
        assert_eq!(exit, 2, "{name}");
        assert!(
            codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()),
            "{name}"
        );
        assert!(
            !codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()),
            "{name}"
        );
    }
    let (mut report, mut config) = load_native("mongodb-product");
    report["permissions"][0]["status"] = json!("observed");
    config["permissions"][0]["status"] = json!("observed");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
}

#[test]
fn mongodb_r1w_stays_unapproved() {
    let (report, mut config) = load_native("mongodb-product");
    config["requested_write_class"] = json!("R1w");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert_eq!(doc["admitted"], json!(false));
    assert!(codes(&doc).contains(&"R1W_UNAPPROVED".to_string()));
    assert!(!codes(&doc).contains(&"COMPAT_MODE_UNKNOWN".to_string()));
    assert!(!codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
}

#[test]
fn clickhouse_preview_stays_denied_on_permission_contract() {
    let (exit, doc, stderr) = validate_paths(
        &fixture("clickhouse-preview-report.json"),
        &fixture("clickhouse-preview-config.json"),
    );
    assert_eq!(exit, 2, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(false));
    assert_closed(&doc, "clickhouse");
    assert_eq!(doc["identity"]["source_id"], json!("clickhouse_268"));
    assert_eq!(
        doc["identity"]["secret_ref"],
        json!("secret://oo-dl01w1-40/reader")
    );
    let found = codes(&doc);
    assert!(
        found.iter().all(|code| code == "PERMISSION_UNKNOWN"),
        "{found:?}"
    );
    assert!(!found.contains(&"COMPAT_MODE_UNKNOWN".to_string()));
    let detail = facts(&doc);
    assert!(
        detail
            .iter()
            .any(|fact| fact.contains("report.permissions[0].scope"))
    );
    assert!(
        detail
            .iter()
            .any(|fact| fact.contains("report.native.clickhouse"))
    );
    let text = checklist_text(&doc);
    assert!(text.contains("<readonly>1</readonly>"));
    assert!(text.contains("max_execution_time"));
    assert!(text.contains("system.settings"));
    assert!(!text.contains("pg_roles"));
    assert!(!text.contains("app.item"));
    assert!(!text.contains("PASSWORD"));

    let (mut report, mut config) = load_native("clickhouse-preview");
    set_permissions(&mut report, &mut config, "readonly", true);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
}

#[test]
fn opensearch_actions_stay_denied() {
    let (exit, doc, stderr) = validate_paths(
        &fixture("opensearch-preview-report.json"),
        &fixture("opensearch-preview-config.json"),
    );
    assert_eq!(exit, 2, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(false));
    assert_closed(&doc, "opensearch");
    assert_eq!(doc["identity"]["source_id"], json!("oo_dl01w1_44"));
    assert_eq!(doc["identity"]["target_id"], json!("oo_dl01w1_44_local_pg"));
    let found = codes(&doc);
    assert!(
        found.iter().all(|code| code == "PERMISSION_UNKNOWN"),
        "{found:?}"
    );
    assert!(!found.contains(&"COMPAT_MODE_UNKNOWN".to_string()));
    let detail = facts(&doc);
    assert!(
        detail
            .iter()
            .any(|fact| fact.contains("report.permissions[0].scope"))
    );
    assert!(
        detail
            .iter()
            .any(|fact| fact.contains("report.permissions[1].scope"))
    );
    let text = checklist_text(&doc);
    assert!(text.contains("indices:data/read/search"));
    assert!(text.contains("indices:data/read/mget"));
    assert!(text.contains("scope.observed"));
    assert!(!text.contains("pg_roles"));
}

#[test]
fn starrocks_and_doris_mysql_protocol_reports_are_admitted() {
    let (exit, doc, stderr) = validate_paths(
        &fixture("starrocks-preview-report.json"),
        &fixture("starrocks-preview-config.json"),
    );
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["blocks"], json!([]));
    assert_closed(&doc, "starrocks");
    assert_eq!(doc["identity"]["source_id"], json!("sr_w45"));
    assert_eq!(doc["identity"]["target_id"], json!("starrocks_w45"));
    let text = checklist_text(&doc);
    assert!(text.contains("GRANT SELECT"));
    assert!(text.contains("SHOW GRANTS"));
    assert!(text.contains("<由 DBA 设置>"));
    assert!(!text.contains("pg_roles"));
    assert!(!text.contains("mysql-native"));

    let (exit, doc, stderr) = validate_paths(
        &fixture("doris-preview-report.json"),
        &fixture("doris-preview-config.json"),
    );
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["identity"]["source_id"], json!("dorispreview"));
    assert_closed(&doc, "doris");
    assert!(checklist_text(&doc).contains("SHOW GRANTS"));
    assert!(!checklist_text(&doc).contains("pg_roles"));
}

#[test]
fn doris_unknown_unit_and_granted_insert_stay_denied() {
    let (mut report, mut config) = load_native("doris-preview");
    report["cost_model"]["unit"] = json!("DorisScan");
    config["budget"]["unit"] = json!("DorisScan");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"BUDGET_UNIT_UNKNOWN".to_string()));
    assert!(!codes(&doc).contains(&"COMPAT_MODE_UNKNOWN".to_string()));

    let (mut report, mut config) = load_native("doris-preview");
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "INSERT", "status": "known", "granted": true}));
    config["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "INSERT", "status": "known", "granted": true}));
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()));
}

#[test]
fn sql_family_modes_still_admit_and_keep_the_postgres_checklist() {
    for mode in ["mysql-native", "mariadb-native", "sqlserver", "db2"] {
        let (mut report, mut config) = load_sql();
        report["compat_mode"] = json!(mode);
        config["compat_mode"] = json!(mode);
        let (exit, doc, stderr) = validate_values(&report, &config);
        assert_eq!(exit, 0, "{mode} {stderr} {doc}");
        assert_eq!(doc["admitted"], json!(true), "{mode}");
        assert_closed(&doc, mode);
        let text = checklist_text(&doc);
        assert!(text.contains("pg_roles"), "{mode}");
        assert!(text.contains("<由 DBA 设置>"), "{mode}");
        assert!(text.contains("GRANT SELECT"), "{mode}");
    }

    let (mut report, mut config) = load_sql();
    report["compat_mode"] = json!("oracle");
    config["compat_mode"] = json!("oracle");
    let (exit, doc, stderr) = validate_values(&report, &config);
    assert_eq!(exit, 2, "unscoped oracle {stderr} {doc}");
    assert_eq!(doc["admitted"], json!(false));
    let found = codes(&doc);
    assert!(
        found.contains(&"PERMISSION_UNKNOWN".to_string()),
        "{found:?}"
    );
    let text = checklist_text(&doc);
    assert!(text.contains("CREATE SESSION"), "{text}");
    assert!(text.contains("AS OF TIMESTAMP"), "{text}");
    assert!(!text.contains("pg_roles"), "{text}");
}

#[test]
fn unknown_engine_tokens_stay_compat_unknown() {
    for mode in ["tidb", "elasticsearch", "mongodb", "doris-native"] {
        let (mut report, mut config) = load_sql();
        report["compat_mode"] = json!(mode);
        config["compat_mode"] = json!(mode);
        let (exit, doc, _) = validate_values(&report, &config);
        assert_eq!(exit, 2, "{mode}");
        assert_eq!(
            codes(&doc),
            vec![
                "COMPAT_MODE_UNKNOWN".to_string(),
                "COMPAT_MODE_UNKNOWN".to_string()
            ],
            "{mode} {:?}",
            facts(&doc)
        );
    }
}

#[test]
fn sql_credits_budget_unit_still_admits() {
    let (mut report, mut config) = load_sql();
    report["cost_model"]["unit"] = json!("Credits");
    report["capabilities"][2]["budget_unit"] = json!("Credits");
    config["budget"]["unit"] = json!("Credits");
    let (exit, doc, stderr) = validate_values(&report, &config);
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_closed(&doc, "credits");
}

#[test]
fn clickhouse_attempt06_grants_and_profile_are_admitted() {
    let report_path = fixture("clickhouse-attempt06-report.json");
    let config_path = fixture("clickhouse-attempt06-config.json");
    let report: TargetReport = parse_strict(&fs::read(&report_path).unwrap()).unwrap();
    let config: SourceConfig = parse_strict(&fs::read(&config_path).unwrap()).unwrap();
    let decision = admit(&report, &config);
    assert!(decision.admitted, "{:?}", decision.blocks);
    let (exit, doc, stderr) = validate_paths(&report_path, &config_path);
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["blocks"], json!([]));
    assert_closed(&doc, "clickhouse-attempt06");
    assert_eq!(doc["identity"]["source_id"], json!("clickhouse_268"));
    assert_eq!(
        doc["identity"]["target_id"],
        json!("clickhouse-26-8-selfmanaged")
    );
    assert_eq!(
        doc["identity"]["secret_ref"],
        json!("secret://oo-dl01w1-40/reader")
    );
    assert!(doc["identity"]["source_report_sha256"].is_null());
    let text = checklist_text(&doc);
    assert!(text.contains("GRANT SELECT ON oo_dl01w40.*"));
    assert!(text.contains("GRANT SELECT ON system.columns"));
    assert!(text.contains("GRANT SHOW DATABASES ON *.*"));
    assert!(text.contains("<readonly>1</readonly>"));
    assert!(text.contains("<max_execution_time>8</max_execution_time>"));
    assert!(text.contains("<queries>400</queries>"));
    assert!(!text.contains("pg_roles"));
    assert!(!text.contains("GRANT readonly"));
}

#[test]
fn opensearch_os44_search_is_admitted_and_unobserved_mget_is_not_a_grant() {
    let report_path = fixture("opensearch-os44-report.json");
    let config_path = fixture("opensearch-os44-config.json");
    let report: TargetReport = parse_strict(&fs::read(&report_path).unwrap()).unwrap();
    let config: SourceConfig = parse_strict(&fs::read(&config_path).unwrap()).unwrap();
    assert!(admit(&report, &config).admitted);
    let (exit, doc, stderr) = validate_paths(&report_path, &config_path);
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_closed(&doc, "opensearch-os44");
    assert_eq!(doc["identity"]["source_id"], json!("oo_dl01w1_44"));
    assert_eq!(doc["identity"]["target_id"], json!("oo_dl01w1_44_local_pg"));
    let text = checklist_text(&doc);
    assert!(text.contains("oo-dl01w1-44-synthetic"));
    assert!(text.contains("indices:data/read/search"));
    assert!(!text.contains("pg_roles"));
    assert!(!text.contains("GRANT SELECT"));
}

#[test]
fn clickhouse_writes_missing_settings_and_wildcard_scopes_deny() {
    let (mut report, mut config) = load_native("clickhouse-attempt06");
    grant_name(&mut report, &mut config, "INSERT", true);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()));
    assert!(!codes(&doc).contains(&"COMPAT_MODE_UNKNOWN".to_string()));

    let (mut report, mut config) = load_native("clickhouse-attempt06");
    report["native"]["clickhouse"]["readonly"] = json!("2");
    config["native"]["clickhouse"]["readonly"] = json!("2");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(facts(&doc).iter().any(|fact| fact.contains("readonly")));
    assert!(codes(&doc).iter().all(|code| code == "PERMISSION_UNKNOWN"));

    let (mut report, mut config) = load_native("clickhouse-attempt06");
    report["native"]["clickhouse"]["quota"]["queries"] = json!(0);
    config["native"]["clickhouse"]["quota"]["queries"] = json!(0);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(facts(&doc).iter().any(|fact| fact.contains("quota")));

    let (mut report, mut config) = load_native("clickhouse-attempt06");
    report["native"] = json!(null);
    config["native"] = json!(null);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(
        facts(&doc)
            .iter()
            .any(|fact| fact.contains("native.clickhouse"))
    );

    let (mut report, mut config) = load_native("clickhouse-attempt06");
    report["permissions"][1]["scope"]["database"] = json!("*");
    config["permissions"][1]["scope"]["database"] = json!("*");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(facts(&doc).iter().any(|fact| fact.contains(".scope")));

    let (mut report, mut config) = load_native("clickhouse-attempt06");
    report["permissions"][2]["scope"]["table"] = json!("columns*");
    config["permissions"][2]["scope"]["table"] = json!("columns*");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(facts(&doc).iter().any(|fact| fact.contains(".scope")));

    let (mut report, mut config) = load_native("clickhouse-attempt06");
    report["cost_model"]["unit"] = json!("Scan");
    config["budget"]["unit"] = json!("Scan");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"BUDGET_UNIT_UNKNOWN".to_string()));

    let (report, mut config) = load_native("clickhouse-attempt06");
    config["requested_write_class"] = json!("R1w");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert_eq!(codes(&doc), vec!["R1W_UNAPPROVED".to_string()]);
}

#[test]
fn opensearch_wildcard_admin_write_and_unobserved_mget_deny() {
    let (mut report, mut config) = load_native("opensearch-os44");
    report["permissions"][1]["granted"] = json!(true);
    config["permissions"][1]["granted"] = json!(true);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(facts(&doc).iter().any(|fact| fact.contains(".observed")));
    assert!(codes(&doc).iter().all(|code| code == "PERMISSION_UNKNOWN"));

    for name in [
        "indices:data/read/search*",
        "indices:admin/create",
        "indices:data/write/index",
        "cluster:admin/reroute",
        "create_index",
    ] {
        let (mut report, mut config) = load_native("opensearch-os44");
        let extra = json!({
            "name": name,
            "status": "known",
            "granted": true,
            "scope": {"index": "oo-dl01w1-44-synthetic", "observed": true}
        });
        report["permissions"]
            .as_array_mut()
            .unwrap()
            .push(extra.clone());
        config["permissions"].as_array_mut().unwrap().push(extra);
        let (exit, doc, _) = validate_values(&report, &config);
        assert_eq!(exit, 2, "{name} {doc}");
        assert!(
            codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()),
            "{name} {:?}",
            facts(&doc)
        );
    }

    let (mut report, mut config) = load_native("opensearch-os44");
    report["permissions"][0]["scope"]["index"] = json!("oo-*");
    config["permissions"][0]["scope"]["index"] = json!("oo-*");
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(facts(&doc).iter().any(|fact| fact.contains(".scope")));

    let (mut report, mut config) = load_native("opensearch-os44");
    let extra = json!({
        "name": "indices:data/read/get",
        "status": "known",
        "granted": true,
        "scope": {"index": "oo-dl01w1-44-synthetic", "observed": true}
    });
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(extra.clone());
    config["permissions"].as_array_mut().unwrap().push(extra);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
    assert!(!codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()));
}

#[test]
fn cross_engine_names_and_profiles_stay_unknown() {
    let (mut report, mut config) = load_native("clickhouse-attempt06");
    let extra = json!({
        "name": "indices:data/read/search",
        "status": "known",
        "granted": true,
        "scope": {"index": "oo-dl01w1-44-synthetic", "observed": true}
    });
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(extra.clone());
    config["permissions"].as_array_mut().unwrap().push(extra);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));

    let (mut report, mut config) = load_native("opensearch-os44");
    let extra = json!({"name": "SELECT", "status": "known", "granted": true});
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(extra.clone());
    config["permissions"].as_array_mut().unwrap().push(extra);
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));

    let (mut report, mut config) = load_native("mongodb-product");
    let profile = json!({"clickhouse": {"readonly": "1", "max_execution_time": "8", "quota": {
        "duration_seconds": 3600, "queries": 400, "errors": 80, "result_rows": 1,
        "read_rows": 1, "execution_time_seconds": 1
    }}});
    report["native"] = profile.clone();
    config["native"] = profile;
    let (exit, doc, _) = validate_values(&report, &config);
    assert_eq!(exit, 2);
    assert!(codes(&doc).contains(&"PERMISSION_UNKNOWN".to_string()));
    assert!(!codes(&doc).contains(&"COMPAT_MODE_UNKNOWN".to_string()));
}

#[test]
fn corroborated_mget_on_one_index_admits() {
    let (mut report, mut config) = load_native("opensearch-os44");
    report["permissions"] = json!([{
        "name": "indices:data/read/mget",
        "status": "known",
        "granted": true,
        "scope": {"index": "oo-dl01w1-44-synthetic", "observed": true}
    }]);
    config["permissions"] = report["permissions"].clone();
    let (exit, doc, stderr) = validate_values(&report, &config);
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["admitted"], json!(true));
}

fn grant_name(report: &mut Value, config: &mut Value, name: &str, granted: bool) {
    let item = json!({"name": name, "status": "known", "granted": granted});
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(item.clone());
    config["permissions"].as_array_mut().unwrap().push(item);
}
