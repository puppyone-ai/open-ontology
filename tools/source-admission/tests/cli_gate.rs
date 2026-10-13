//! Executes `oo-source-admission`. These checks are not mocks.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
        "oo-dl01w1-02-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
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

fn load_pair() -> (Value, Value) {
    let report = fs::read(manifest().join("fixtures/admit-ok-report.json")).unwrap();
    let config = fs::read(manifest().join("fixtures/admit-ok-config.json")).unwrap();
    (
        serde_json::from_slice(&report).unwrap(),
        serde_json::from_slice(&config).unwrap(),
    )
}

fn validate_values(report: &Value, config: &Value) -> (i32, Value, String) {
    let report_path = scratch("report.json");
    let config_path = scratch("config.json");
    write_json(&report_path, report);
    write_json(&config_path, config);
    let (code, stdout, stderr) = run(&[
        "validate",
        "--report",
        report_path.to_str().unwrap(),
        "--config",
        config_path.to_str().unwrap(),
    ]);
    if code == 1 {
        panic!("validate invalid: {stderr}");
    }
    let doc: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{stdout} {stderr}"));
    (code, doc, stderr)
}

fn codes(doc: &Value) -> Vec<&str> {
    doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect()
}

fn expect_block(label: &str, code: &str, mutate: impl FnOnce(&mut Value, &mut Value)) {
    let (mut report, mut config) = load_pair();
    mutate(&mut report, &mut config);
    let (exit, doc, stderr) = validate_values(&report, &config);
    assert_eq!(exit, 2, "{label} stderr={stderr} doc={doc}");
    assert_eq!(doc["admitted"], json!(false), "{label}");
    assert_eq!(doc["matrix_mutated"], json!(false), "{label}");
    assert_eq!(doc["zero_write"]["proven"], json!(false), "{label}");
    assert_eq!(doc["zero_write"]["basis"], json!("not_audited"), "{label}");
    let found = codes(&doc);
    assert!(found.contains(&code), "{label} codes={found:?}");
}

#[test]
fn positive_r0_is_admitted_with_redacted_checklist_and_no_zero_write_claim() {
    let (report, config) = load_pair();
    let (exit, doc, stderr) = validate_values(&report, &config);
    assert_eq!(exit, 0, "{stderr} {doc}");
    assert_eq!(doc["protocol"], json!("oo.source-admission.decision/v1"));
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["matrix_mutated"], json!(false));
    assert_eq!(doc["blocks"], json!([]));
    assert_eq!(doc["zero_write"]["proven"], json!(false));
    assert_eq!(doc["zero_write"]["basis"], json!("not_audited"));
    assert_eq!(doc["identity"]["source_id"], json!("pg_selfmanaged"));
    assert_eq!(
        doc["identity"]["secret_ref"],
        json!("secret://example/reader")
    );
    let names: Vec<&str> = doc["model"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Catalog"));
    assert!(names.contains(&"BatchFull"));
    assert!(names.contains(&"Budget"));
    let items = doc["dba_checklist"].as_array().unwrap();
    assert!(!items.is_empty());
    for item in items {
        assert!(!item["rollback"].as_str().unwrap().is_empty());
        assert!(!item["verification"].as_str().unwrap().is_empty());
        assert!(!item["risk"].as_str().unwrap().is_empty());
    }
    let text = serde_json::to_string(&doc).unwrap();
    assert!(text.contains("<由 DBA 设置>"));
    assert!(!text.contains("CANARY-SECRET-DL01W102"));
}

#[test]
fn rejects_unknown_permission() {
    expect_block("permission", "PERMISSION_UNKNOWN", |report, _| {
        report["permissions"][0]["name"] = json!("DROP");
    });
}

#[test]
fn rejects_granted_insert() {
    expect_block("insert", "PERMISSION_NOT_READONLY", |report, _| {
        report["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "INSERT", "status": "known", "granted": true}));
    });
}

#[test]
fn rejects_unknown_compat_mode() {
    expect_block("compat", "COMPAT_MODE_UNKNOWN", |report, config| {
        report["compat_mode"] = json!("tidb");
        config["compat_mode"] = json!("tidb");
    });
}

#[test]
fn rejects_unknown_budget_unit() {
    expect_block("budget", "BUDGET_UNIT_UNKNOWN", |report, config| {
        report["cost_model"]["unit"] = json!("RU");
        report["capabilities"][2]["budget_unit"] = json!("RU");
        config["budget"]["unit"] = json!("RU");
    });
}

#[test]
fn rejects_write_class_r2() {
    expect_block("r2", "WRITE_CLASS_R2", |_report, config| {
        config["requested_write_class"] = json!("R2");
    });
}

#[test]
fn rejects_write_class_r3() {
    expect_block("r3", "WRITE_CLASS_R3", |_report, config| {
        config["requested_write_class"] = json!("R3");
    });
}

#[test]
fn rejects_unapproved_r1w() {
    expect_block("r1w", "R1W_UNAPPROVED", |_report, config| {
        config["requested_write_class"] = json!("R1w");
    });
}

#[test]
fn rejects_r1w_approval_without_request() {
    expect_block("r1w-extra", "R1W_UNAPPROVED", |_report, config| {
        config["r1w"] = json!({
            "approved": true,
            "adr": "DEC-17",
            "audit_ref": "audit/scope"
        });
    });
}

#[test]
fn rejects_signal_write() {
    expect_block("signal", "SIGNAL_WRITE", |_report, config| {
        config["write_options"]["signal_table"] = json!(true);
    });
}

#[test]
fn rejects_debezium_snapshot_mode() {
    expect_block("snapshot", "SIGNAL_WRITE", |_report, config| {
        config["debezium"]["snapshot_mode"] = json!("initial");
    });
}

#[test]
fn rejects_heartbeat_write() {
    expect_block("heartbeat", "HEARTBEAT_WRITE", |_report, config| {
        config["write_options"]["heartbeat_write"] = json!(true);
    });
}

#[test]
fn rejects_stream_write() {
    expect_block("stream", "STREAM_WRITE", |_report, config| {
        config["write_options"]["create_stream"] = json!(true);
    });
}

#[test]
fn rejects_trigger_write() {
    expect_block("trigger", "TRIGGER_WRITE", |_report, config| {
        config["write_options"]["create_trigger"] = json!(true);
    });
}

#[test]
fn duplicate_key_is_invalid_input() {
    let path = scratch("dup.json");
    fs::write(
        &path,
        b"{\"protocol\":\"oo.source-admission.report/v1\",\"protocol\":\"other\"}\n",
    )
    .unwrap();
    let config = scratch("config.json");
    fs::copy(manifest().join("fixtures/admit-ok-config.json"), &config).unwrap();
    let (code, _stdout, stderr) = run(&[
        "validate",
        "--report",
        path.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("duplicate_key"), "{stderr}");
}

#[test]
fn adapt_pg_preserves_identity_and_redacts_checklist() {
    let path = manifest().join("fixtures/pg-access-min.json");
    let bytes = fs::read(&path).unwrap();
    let (code, stdout, stderr) = run(&["adapt-pg", "--report", path.to_str().unwrap()]);
    assert_eq!(code, 2, "{stderr} {stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["admitted"], json!(false));
    assert_eq!(doc["matrix_mutated"], json!(false));
    assert_eq!(doc["zero_write"]["proven"], json!(false));
    assert_eq!(doc["zero_write"]["basis"], json!("not_audited"));
    assert!(codes(&doc).contains(&"BUDGET_UNIT_UNKNOWN"));
    assert_eq!(doc["identity"]["source_id"], json!("oo_dl01w1_02"));
    assert_eq!(
        doc["identity"]["secret_ref"],
        json!("secret://oo-dl01w1-02/reader")
    );
    assert_eq!(
        doc["identity"]["source_report_protocol"],
        json!("oo.source.pgsql/v1")
    );
    assert_eq!(
        doc["identity"]["core_contract"],
        json!("oo.core.contract/fixture")
    );
    let digest = Sha256::digest(&bytes);
    let mut hex = String::new();
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    assert_eq!(doc["identity"]["source_report_sha256"], json!(hex));
    let names: Vec<&str> = doc["model"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Catalog"));
    assert!(names.contains(&"BatchFull"));
    assert!(!names.contains(&"Cdc"));
    assert!(!stdout.contains("CANARY-SECRET-DL01W102"));
    assert!(stdout.contains("<redacted>"));
    assert!(stdout.contains("<由 DBA 设置>"));
    for item in doc["dba_checklist"].as_array().unwrap() {
        assert!(!item["rollback"].as_str().unwrap().is_empty());
        assert!(!item["verification"].as_str().unwrap().is_empty());
    }
}

#[test]
fn audit_log_executes_empty_select_and_insert() {
    let empty = scratch("empty.csv");
    fs::write(&empty, b"").unwrap();
    let (code, stdout, stderr) = run(&[
        "audit-log",
        "--reader",
        "oo_reader",
        "--log",
        empty.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["zero_write"]["basis"], json!("audit_empty"));
    assert_eq!(doc["zero_write"]["proven"], json!(false));

    let ok = scratch("ok.csv");
    fs::write(
        &ok,
        b"2026-10-09 00:00:00.000 UTC,oo_reader,src,1,[local],s,1,,,0/1,0,LOG,00000,\"statement: SELECT 1\"\n2026-10-09 00:00:01.000 UTC,oo_dba,src,2,[local],s,1,,,0/1,0,LOG,00000,\"statement: INSERT INTO app.item VALUES (1, 'x')\"\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(&[
        "audit-log",
        "--reader",
        "oo_reader",
        "--log",
        ok.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr} {stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["zero_write"]["proven"], json!(true));
    assert_eq!(doc["zero_write"]["basis"], json!("pg_csvlog"));
    assert_eq!(doc["other_users_with_forbidden"], json!(1));

    let bad = scratch("bad.csv");
    fs::write(
        &bad,
        b"2026-10-09 00:00:00.000 UTC,oo_reader,src,1,[local],s,1,,,0/1,0,LOG,00000,\"statement: INSERT INTO app.item VALUES (1, 'x')\"\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(&[
        "audit-log",
        "--reader",
        "oo_reader",
        "--log",
        bad.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{stderr}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["zero_write"]["basis"], json!("audit_write"));
    assert_eq!(doc["forbidden"], json!(1));
}
