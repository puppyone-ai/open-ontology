//! Closed-budget and refusal checks. These tests do not open a database.

#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use local_pg_mirror::{
    copy, parse_config_text, seal_plan, store_plan, ColumnShape, CopyRequest, CopyStop, InputConfig,
    MirrorError, TableShape,
};

const HOST: &str = "closed.invalid";

#[test]
fn mysql_is_unsupported_before_credentials() {
    let dir = workdir("mysql");
    let config = dir.join("source.json");
    let plan = dir.join("plan.json");
    let missing = dir.join("absent-secret");
    fs::write(
        &config,
        r#"{"engine":"mysql","statement_timeout_ms":30001}"#,
    )
    .unwrap_or_else(|_| panic!("config write"));
    let (code, stdout, stderr) = run(&[
        "discover",
        path(&config),
        path(&plan),
        path(&missing),
        path(&dir.join("ca.crt")),
    ]);
    assert_clean(&stdout, &stderr);
    assert_eq!(code, 3);
    assert_eq!(stderr.trim(), "unsupported_engine engine");
    assert!(!missing.exists());
    assert!(!plan.exists());
}

#[test]
fn over_budget_is_refused_before_credentials() {
    let dir = workdir("budget");
    let config = dir.join("source.json");
    let plan = dir.join("plan.json");
    let missing = dir.join("absent-secret");
    fs::write(&config, document("postgresql", 30001, 1000, 1000, 300000)).unwrap_or_else(|_| panic!("config write"));
    let (code, stdout, stderr) = run(&[
        "discover",
        path(&config),
        path(&plan),
        path(&missing),
        path(&dir.join("ca.crt")),
    ]);
    assert_clean(&stdout, &stderr);
    assert_eq!(code, 1);
    assert_eq!(stderr.trim(), "budget statement_timeout");
    assert!(!missing.exists());
}

#[test]
fn missing_secret_is_not_a_connect() {
    let dir = workdir("secret");
    let config = dir.join("source.json");
    let plan = dir.join("plan.json");
    let missing = dir.join("absent-secret");
    fs::write(&config, document("postgresql", 30000, 1000, 1000, 300000)).unwrap_or_else(|_| panic!("config write"));
    let (code, stdout, stderr) = run(&[
        "discover",
        path(&config),
        path(&plan),
        path(&missing),
        path(&dir.join("ca.crt")),
    ]);
    assert_clean(&stdout, &stderr);
    assert_eq!(code, 1);
    assert_eq!(stderr.trim(), "credential_path file");
    assert!(!stdout.contains("connect"));
    assert!(!stderr.contains("connect"));
}

#[test]
fn forbidden_key_stays_off_the_output() {
    let dir = workdir("forbidden");
    let config = dir.join("source.json");
    let mut body = document("postgresql", 30000, 1000, 1000, 300000);
    assert!(body.ends_with('}'));
    body.pop();
    body.push_str(",\"password\":\"x\"}");
    fs::write(&config, body).unwrap_or_else(|_| panic!("config write"));
    let (code, stdout, stderr) = run(&[
        "discover",
        path(&config),
        path(&dir.join("plan.json")),
        path(&dir.join("absent-secret")),
        path(&dir.join("ca.crt")),
    ]);
    assert_clean(&stdout, &stderr);
    assert_eq!(code, 1);
    assert_eq!(stderr.trim(), "config forbidden_key");
}

#[test]
fn closed_budget_edges() {
    let over_batch = parse_config_text(&document("postgresql", 30000, 1000, 1001, 300000));
    assert_budget(&over_batch, "batch_rows");
    let over_lock = parse_config_text(&document("postgresql", 30000, 1001, 1000, 300000));
    assert_budget(&over_lock, "lock_timeout");
    let over_deadline = parse_config_text(&document("postgresql", 30000, 1000, 1000, 300001));
    assert_budget(&over_deadline, "snapshot_deadline");
}

#[test]
fn generation_changes_the_plan_hash() {
    let first = sample_plan(1);
    let second = sample_plan(2);
    assert_eq!(first.plan_sha256.len(), 64);
    assert!(first.plan_sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    assert_ne!(first.plan_sha256, second.plan_sha256);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn tamper_does_not_read_a_secret() {
    let dir = workdir("tamper");
    let plan_path = dir.join("plan.json");
    let missing = dir.join("absent-secret");
    let plan = sample_plan(1);
    store_plan(&plan_path, &plan).unwrap_or_else(|err| panic!("{} {}", err.code, err.fact));
    let mut permissions = fs::metadata(&plan_path).unwrap_or_else(|_| panic!("plan meta")).permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&plan_path, permissions).unwrap_or_else(|_| panic!("plan mode"));
    let mut text = fs::read_to_string(&plan_path).unwrap_or_else(|_| panic!("plan read"));
    let marked = text.replace("\"generation\": 1", "\"generation\": 2");
    assert_ne!(text, marked);
    text = marked;
    fs::write(&plan_path, text).unwrap_or_else(|_| panic!("plan write"));
    let err = copy(CopyRequest {
        plan_path,
        source_password: missing.clone(),
        source_ca: dir.join("ca.crt"),
        admin_password: missing.clone(),
        runtime_password: missing.clone(),
        manifest_path: dir.join("manifest.json"),
        target_config_path: dir.join("target.json"),
        target_ca: dir.join("ca.crt"),
        stop: CopyStop::Finish,
    })
    .await
    .expect_err("tampered plan");
    assert_eq!(err.code, "plan_tamper");
    assert!(!missing.exists());
    assert!(!dir.join("manifest.json").exists());
}

fn assert_budget(result: &Result<InputConfig, MirrorError>, fact: &str) {
    match result {
        Err(err) => {
            assert_eq!(err.code, "budget");
            assert_eq!(err.fact, fact);
        }
        Ok(_) => panic!("budget accepted"),
    }
}

fn sample_plan(generation: u64) -> local_pg_mirror::Plan {
    let config = InputConfig {
        source_id: "fixa".to_string(),
        host: HOST.to_string(),
        port: 9,
        database: "srcdb".to_string(),
        user: "src_reader".to_string(),
        server_name: HOST.to_string(),
        ca_sha256: "0".repeat(64),
        secret_ref: "secret://fixture/src".to_string(),
        tables: vec![("public".to_string(), "amounts".to_string())],
        connect_timeout_ms: 10000,
        statement_timeout_ms: 30000,
        lock_timeout_ms: 1000,
        max_rows: 10000,
        max_bytes: 1_048_576,
        batch_rows: 1000,
        snapshot_deadline_ms: 300000,
        generation,
        shard: "gold".to_string(),
        owner: "11111111-1111-4111-8111-111111111111".to_string(),
        target_host: HOST.to_string(),
        target_port: 9,
        target_database: "mirrordb".to_string(),
        target_admin_user: "fixture_admin".to_string(),
        target_runtime_user: "mirror_runtime".to_string(),
        target_reader_user: "mirror_reader".to_string(),
        target_server_name: HOST.to_string(),
        target_ca_sha256: "0".repeat(64),
    };
    let admitted = vec![TableShape {
        schema: "public".to_string(),
        name: "amounts".to_string(),
        columns: vec![ColumnShape {
            attnum: 1,
            name: "id".to_string(),
            oid: 20,
            typmod: -1,
            not_null: true,
        }],
        primary_key: vec!["id".to_string()],
    }];
    seal_plan(config, admitted, "src_reader", "srcdb", false, "start").unwrap_or_else(|err| panic!("{} {}", err.code, err.fact))
}

fn document(engine: &str, statement: u64, lock: u64, batch: u64, deadline: u64) -> String {
    format!(
        r#"{{"protocol":"oo.pipeline.local-pg-mirror/v1","engine":"{engine}","source_id":"fixa","host":"{HOST}","port":9,"database":"srcdb","user":"src_reader","server_name":"{HOST}","ca_sha256":"{ca}","secret_ref":"secret://fixture/src","tables":[{{"schema":"public","table":"amounts"}}],"connect_timeout_ms":10000,"statement_timeout_ms":{statement},"lock_timeout_ms":{lock},"max_rows":10000,"max_bytes":1048576,"batch_rows":{batch},"snapshot_deadline_ms":{deadline},"generation":1,"shard":"gold","owner":"11111111-1111-4111-8111-111111111111","target_host":"{HOST}","target_port":9,"target_database":"mirrordb","target_admin_user":"fixture_admin","target_runtime_user":"mirror_runtime","target_reader_user":"mirror_reader","target_server_name":"{HOST}","target_ca_sha256":"{ca}"}}"#,
        ca = "0".repeat(64),
    )
}

fn workdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("a318-contract-{name}"));
    fs::create_dir_all(&path).unwrap_or_else(|_| panic!("workdir"));
    path
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap_or_else(|| panic!("path"))
}

fn run(args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_local-pg-mirror");
    let output = Command::new(bin).args(args).output().unwrap_or_else(|_| panic!("spawn"));
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

fn assert_clean(stdout: &str, stderr: &str) {
    assert!(!stdout.contains(HOST), "stdout leaked host");
    assert!(!stderr.contains(HOST), "stderr leaked host");
    assert!(!stdout.to_ascii_lowercase().contains("password"), "stdout leaked secret word");
    assert!(!stderr.to_ascii_lowercase().contains("password"), "stderr leaked secret word");
}
