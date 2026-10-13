//! Oracle admission through the validate CLI.
//!
//! `admission-1/report.json` and `config.json` are the frozen FREEPDB1 reader
//! inputs from e502e421 and 47d71d3d. The minimal grant file is built here by
//! deleting `EXECUTE`. It is a policy fixture, not an Oracle field capture.
//! This test does not open a database.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const REAL_REPORT_SHA256: &str = "5c8212ddd431b5cd27773135149e4a19337d94832e5e402bdb983d10eb95f1d0";
const REAL_CONFIG_SHA256: &str = "11d89765b447367f3f6cc4200c5fb6f8c94540517919c799b94c97c7f7f0fe82";

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

fn real_dir() -> PathBuf {
    manifest()
        .join("../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1")
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn read_pinned(name: &str, expect: &str) -> Value {
    let bytes = fs::read(real_dir().join(name)).unwrap_or_else(|_| panic!("missing {name}"));
    assert_eq!(sha256(&bytes), expect, "{name}");
    serde_json::from_slice(&bytes).unwrap()
}

fn validate(report: &Value, config: &Value) -> (i32, Value) {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "oo-dl01w1-38-oracle-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let report_path = dir.join("report.json");
    let config_path = dir.join("config.json");
    fs::write(&report_path, serde_json::to_vec_pretty(report).unwrap()).unwrap();
    fs::write(&config_path, serde_json::to_vec_pretty(config).unwrap()).unwrap();
    let output = Command::new(bin())
        .args([
            "validate",
            "--report",
            report_path.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
        ])
        .output()
        .expect("run oo-source-admission");
    let code = output.status.code().unwrap_or(127);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    if code == 1 {
        panic!("validate invalid: {stderr}");
    }
    let doc: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{stdout} {stderr}"));
    let _ = fs::remove_file(&report_path);
    let _ = fs::remove_file(&config_path);
    (code, doc)
}

fn pairs(doc: &Value) -> Vec<(String, String)> {
    doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["code"].as_str().unwrap().to_string(),
                item["fact"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn drop_execute(value: &mut Value) {
    value["permissions"]
        .as_array_mut()
        .unwrap()
        .retain(|item| item["name"] != "EXECUTE");
}

fn policy_pair() -> (Value, Value) {
    let mut report = read_pinned("report.json", REAL_REPORT_SHA256);
    let mut config = read_pinned("config.json", REAL_CONFIG_SHA256);
    drop_execute(&mut report);
    drop_execute(&mut config);
    (report, config)
}

fn checklist_text(doc: &Value) -> String {
    serde_json::to_string(&doc["dba_checklist"]).unwrap()
}

#[test]
fn real_freepdb1_report_stays_denied_on_package_execute() {
    let report = read_pinned("report.json", REAL_REPORT_SHA256);
    let config = read_pinned("config.json", REAL_CONFIG_SHA256);
    assert_eq!(report["permissions"][0]["name"], json!("EXECUTE"));
    assert_eq!(
        report["permissions"][0]["scope"]["table"],
        json!("DBMS_FLASHBACK")
    );
    assert_eq!(report["permissions"][7]["name"], json!("CREATE SESSION"));
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 2);
    assert_eq!(doc["admitted"], json!(false));
    assert_eq!(doc["matrix_mutated"], json!(false));
    assert_eq!(doc["zero_write"]["proven"], json!(false));
    assert_eq!(
        pairs(&doc),
        vec![
            (
                "PERMISSION_NOT_READONLY".to_string(),
                "report.permissions[0]".to_string()
            ),
            (
                "PERMISSION_NOT_READONLY".to_string(),
                "config.permissions[0]".to_string()
            ),
        ]
    );
    let text = checklist_text(&doc);
    let statements: String = doc["dba_checklist"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["statement"].as_str().unwrap())
        .collect();
    assert!(text.contains("TRANSACTION_BACKOUT"));
    assert!(statements.contains("CREATE SESSION"));
    assert!(statements.contains("AS OF TIMESTAMP"));
    assert!(!statements.contains("NOSUPERUSER"));
    assert!(!statements.contains("GRANT EXECUTE"));
    assert!(!statements.contains("GET_SYSTEM_CHANGE_NUMBER"));
}

#[test]
fn policy_fixture_without_package_execute_is_admitted() {
    let (report, config) = policy_pair();
    assert!(
        report["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["name"] != "EXECUTE")
    );
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 0, "{doc}");
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["blocks"], json!([]));
    assert_eq!(doc["matrix_mutated"], json!(false));
    assert_eq!(doc["zero_write"]["basis"], json!("not_audited"));
    let grants = doc["dba_checklist"][1]["statement"].as_str().unwrap();
    assert!(grants.contains("GRANT CREATE SESSION TO \"oo04_t_batch_ro\""));
    assert!(grants.contains("GRANT SELECT ON \"OO04_FIX\".\"T_BATCH\""));
    assert!(grants.contains("GRANT FLASHBACK ON \"OO04_FIX\".\"T_TS\""));
    assert!(grants.contains("GRANT FLASHBACK ON \"OO04_FIX\".\"T_WIDE\""));
    assert!(grants.contains("SELECT * FROM \"OO04_FIX\".\"T_BATCH\" AS OF TIMESTAMP SYSTIMESTAMP"));
    assert!(!grants.contains("EXECUTE"));
    let risk = doc["dba_checklist"][1]["risk"].as_str().unwrap();
    assert!(risk.contains("TRANSACTION_BACKOUT"));
    assert!(risk.contains("INSERT, DELETE, and ALTER"));
}

#[test]
fn flashback_plus_insert_on_the_same_table_is_not_readonly() {
    let (mut report, mut config) = policy_pair();
    let insert = json!({
        "name": "INSERT",
        "status": "known",
        "granted": true,
        "scope": {"database": "OO04_FIX", "table": "T_BATCH"}
    });
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(insert.clone());
    config["permissions"].as_array_mut().unwrap().push(insert);
    let flash = report["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|item| item["name"] == "FLASHBACK" && item["scope"]["table"] == "T_BATCH")
        .unwrap();
    let insert_at = report["permissions"].as_array().unwrap().len() - 1;
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 2);
    let found = pairs(&doc);
    for fact in [
        format!("report.permissions[{flash}]"),
        format!("report.permissions[{insert_at}]"),
        format!("config.permissions[{flash}]"),
        format!("config.permissions[{insert_at}]"),
    ] {
        assert!(
            found.contains(&("PERMISSION_NOT_READONLY".to_string(), fact.clone())),
            "{fact} in {found:?}"
        );
    }
}

#[test]
fn flashback_any_table_and_other_package_execute_stay_denied() {
    let (mut report, mut config) = policy_pair();
    let any = json!({"name": "FLASHBACK ANY TABLE", "status": "known", "granted": true});
    let package = json!({
        "name": "EXECUTE",
        "status": "known",
        "granted": true,
        "scope": {"database": "SYS", "table": "DBMS_SQL"}
    });
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(any.clone());
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(package.clone());
    config["permissions"].as_array_mut().unwrap().push(any);
    config["permissions"].as_array_mut().unwrap().push(package);
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 2);
    let found = pairs(&doc);
    let codes: Vec<&str> = found.iter().map(|(code, _)| code.as_str()).collect();
    assert!(codes.iter().all(|code| *code == "PERMISSION_NOT_READONLY"));
    assert_eq!(codes.len(), 4);
}

#[test]
fn subprogram_name_is_not_a_read_only_execute() {
    let (mut report, mut config) = policy_pair();
    let name = json!({
        "name": "DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER",
        "status": "known",
        "granted": true
    });
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(name.clone());
    config["permissions"].as_array_mut().unwrap().push(name);
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 2);
    let found = pairs(&doc);
    assert!(found.iter().any(|(code, _)| code == "PERMISSION_UNKNOWN"));
    assert!(found.iter().all(|(code, _)| code == "PERMISSION_UNKNOWN"));
}

#[test]
fn scoped_create_session_is_unknown_and_ungranted_execute_is_not_a_block() {
    let (mut report, mut config) = policy_pair();
    report["permissions"].as_array_mut().unwrap().push(json!({
        "name": "CREATE SESSION",
        "status": "known",
        "granted": true,
        "scope": {"database": "SYS", "table": "DUAL"}
    }));
    config["permissions"].as_array_mut().unwrap().push(json!({
        "name": "EXECUTE",
        "status": "known",
        "granted": false,
        "scope": {"database": "SYS", "table": "DBMS_FLASHBACK"}
    }));
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 2, "{doc}");
    assert_eq!(
        pairs(&doc),
        vec![(
            "PERMISSION_UNKNOWN".to_string(),
            "report.permissions[7].scope".to_string()
        )]
    );
}

#[test]
fn postgres_create_session_stays_unknown() {
    let report: Value = serde_json::from_slice(
        &fs::read(manifest().join("fixtures/admit-ok-report.json")).unwrap(),
    )
    .unwrap();
    let config: Value = serde_json::from_slice(
        &fs::read(manifest().join("fixtures/admit-ok-config.json")).unwrap(),
    )
    .unwrap();
    let mut report = report;
    let mut config = config;
    report["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "CREATE SESSION", "status": "known", "granted": true}));
    config["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "CREATE SESSION", "status": "known", "granted": true}));
    let (exit, doc) = validate(&report, &config);
    assert_eq!(exit, 2);
    assert!(pairs(&doc).iter().any(|(code, fact)| {
        code == "PERMISSION_UNKNOWN" && fact.starts_with("report.permissions")
    }));
}

#[test]
fn real_inputs_are_not_the_policy_fixture() {
    let real = fs::read(real_dir().join("report.json")).unwrap();
    let (policy, _) = policy_pair();
    let names: Vec<&str> = policy["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(sha256(&real), REAL_REPORT_SHA256);
    assert_ne!(
        sha256(&serde_json::to_vec(&policy).unwrap()),
        REAL_REPORT_SHA256
    );
    assert!(String::from_utf8(real).unwrap().contains("\"EXECUTE\""));
    assert!(!names.contains(&"EXECUTE"));
}
