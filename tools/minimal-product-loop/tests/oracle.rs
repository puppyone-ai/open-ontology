#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use minimal_product_loop::{
    ask, close, doctor, load_config, map_stdout, prompt_for, ready_bins, CONNECTOR_PROTOCOL,
};

#[test]
fn connector_constant_matches_the_source_crate() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/oo-connectors/src/lib.rs");
    let text = fs::read_to_string(path).expect("connector source");
    let line = text
        .lines()
        .find(|line| line.contains("pub const PROTOCOL"))
        .expect("protocol constant");
    assert!(line.contains(&format!("\"{CONNECTOR_PROTOCOL}\"")));
}

#[test]
fn config_rejects_plaintext_and_mysql_without_a_connection() {
    let dir = scratch("config");
    let config = write_config(&dir, "mysql");
    let started = Instant::now();
    let error = doctor(&config).expect_err("mysql");
    assert!(started.elapsed().as_secs() < 2);
    assert_eq!(error.code, "unsupported");
    let mut patched = fs::read_to_string(&config).expect("read");
    patched = patched.replacen("\"type\": \"mysql\"", "\"type\": \"postgresql\"", 1);
    patched = patched.replacen("\"tls\": \"require\"", "\"tls\": \"require\", \"password\": \"secret\"", 1);
    let secret_path = dir.join("secret.json");
    fs::write(&secret_path, patched).expect("write");
    let error = load_config(&secret_path).expect_err("password");
    assert_eq!(error.code, "config.secret");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn doctor_accepts_postgresql_and_close_leaves_the_source() {
    let dir = scratch("close");
    let config = write_config(&dir, "postgresql");
    let report = doctor(&config).expect("doctor");
    assert_eq!(report.status, "config_ok");
    assert_eq!(report.connector, CONNECTOR_PROTOCOL);
    assert!(report.supported);
    let outcome = close(&config).expect("close");
    assert!(!outcome.pid_recorded);
    assert!(!outcome.pid_absent);
    assert!(!outcome.database_dropped);
    assert!(!outcome.database_observed);
    assert_eq!(outcome.mapping, "absent");
    assert!(!outcome.residue_absent);
    assert_eq!(outcome.status, "mapping_absent");
    let work = dir.join("work");
    fs::create_dir_all(&work).expect("work");
    fs::write(
        work.join("proxy-session-map.json"),
        "{\"protocol\":\"oo.minimal-product-loop.proxy-map/v1\",\"state\":\"active\",\"session_sha256\":\"abc\",\"development_local\":true}",
    )
    .expect("map");
    fs::write(work.join("proxy-marker"), "public-proxy-marker").expect("marker");
    let outcome = close(&config).expect("revoked");
    assert_eq!(outcome.mapping, "revoked");
    assert!(outcome.residue_absent);
    assert!(!work.join("proxy-session-map.json").exists());
    assert!(!work.join("proxy-marker").exists());
    assert_eq!(outcome.status, "database_unobserved");
    assert!(!outcome.database_dropped);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn ask_reports_pi_absent_before_a_query() {
    let dir = scratch("ask");
    let config = write_config(&dir, "postgresql");
    let work = dir.join("work");
    fs::create_dir_all(&work).expect("work");
    fs::write(work.join("state.json"), "{\"selfbuild\":\"generated_unreviewed\"}").expect("state");
    let error = ask(&config, "count-entry").expect_err("pi");
    assert_eq!(error.code, "pi.absent");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prompt_has_no_prepared_statement_or_gold_number() {
    let prompt = prompt_for(
        "How many rows are in shop.entry? Write the SQL yourself and run it with oo query.",
        Path::new("/usr/local/bin/oo"),
        Path::new("/knowledge"),
        Path::new("/secrets/token"),
        "http://127.0.0.1:9",
    );
    assert!(!prompt.contains("SELECT"));
    assert!(!prompt.contains("90000"));
    assert!(!prompt.contains("1234567890"));
    assert!(prompt.contains("/usr/local/bin/oo"));
    assert!(prompt.contains("http://127.0.0.1:9"));
    assert!(prompt.contains("unreviewed"));
    assert!(prompt.contains("public proxy marker"));
    assert!(!prompt.contains("/secrets/token-session"));
    let outcome = map_stdout("shop.entry", "", 6);
    assert_eq!(outcome.http_status, 400);
    assert!(outcome.query_id.starts_with("q_"));
}

#[test]
fn ready_refuses_a_demo_without_pi_or_query_pg() {
    let dir = scratch("ready");
    let config = write_config(&dir, "postgresql");
    let loaded = load_config(&config).expect("config");
    let error = ready_bins(&loaded).expect_err("missing");
    assert_eq!(error.code, "pi.absent");
    fs::write(dir.join("missing-pi"), b"pi").expect("pi");
    fs::write(dir.join("missing-node"), b"node").expect("node");
    let loaded = load_config(&config).expect("config");
    let error = ready_bins(&loaded).expect_err("query");
    assert_eq!(error.code, "query_pg.absent");
    fs::write(dir.join("missing-query-pg"), b"query-pg").expect("query");
    let loaded = load_config(&config).expect("config");
    let error = ready_bins(&loaded).expect_err("oo");
    assert_eq!(error.code, "oo.absent");
    let _ = fs::remove_dir_all(&dir);
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a405-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn write_config(dir: &Path, database_type: &str) -> PathBuf {
    fs::write(dir.join("password"), b"pppppppp").expect("password");
    fs::write(dir.join("ca.pem"), b"ca-bytes\n").expect("ca");
    fs::write(dir.join("bearer"), b"bbbbbbbb").expect("bearer");
    let config = dir.join("loop.json");
    let body = format!(
        "{{\n\
           \"protocol\": \"oo.minimal-product-loop.config/v1\",\n\
           \"mode\": \"operator-development\",\n\
           \"development_preview\": true,\n\
           \"publication\": \"not_published\",\n\
           \"llm_review\": \"unreviewed\",\n\
           \"runtime_acknowledgement\": \"local-process-not-a-sandbox-certificate\",\n\
           \"database\": {{\"type\": \"{database_type}\", \"host\": \"127.0.0.1\", \"port\": 9, \"name\": \"shop\", \"username\": \"reader\", \"tls\": \"require\", \"secret_ref\": \"secret://pg/a405/reader\", \"password_file\": \"{password}\", \"ca_file\": \"{ca}\"}},\n\
           \"tables\": [{{\"schema\": \"shop\", \"table\": \"party\"}}, {{\"schema\": \"shop\", \"table\": \"account\"}}, {{\"schema\": \"shop\", \"table\": \"entry\"}}],\n\
           \"llm\": {{\"model_id\": \"grok-4.7\", \"origin\": \"http://127.0.0.1:8317\", \"path\": \"/v1/chat/completions\", \"bearer_file\": \"{bearer}\", \"deadline_ms\": 30000}},\n\
           \"gateway\": {{\"oo\": \"{oo}\", \"query_pg\": \"{query}\", \"policy_file\": \"{policy}\", \"token_file\": \"{token}\"}},\n\
           \"pi\": {{\"executable\": \"{pi}\", \"executable_sha256\": \"{sha}\", \"runtime\": \"{node}\", \"runtime_sha256\": \"{sha}\", \"provider\": \"grok\", \"model\": \"grok-4.7\"}},\n\
           \"paths\": {{\"catalog_bin\": \"{catalog}\", \"knowledge\": \"{knowledge}\", \"work\": \"{work}\"}},\n\
           \"operator\": {{\"label\": \"a405\", \"at\": \"2026-10-08T00:00:00Z\"}}\n\
         }}\n",
        database_type = database_type,
        password = dir.join("password").display(),
        ca = dir.join("ca.pem").display(),
        bearer = dir.join("bearer").display(),
        oo = dir.join("missing-oo").display(),
        query = dir.join("missing-query-pg").display(),
        policy = dir.join("missing-policy.json").display(),
        token = dir.join("gateway-token").display(),
        pi = dir.join("missing-pi").display(),
        node = dir.join("missing-node").display(),
        sha = "0000000000000000000000000000000000000000000000000000000000000000",
        catalog = dir.join("missing-catalog").display(),
        knowledge = dir.join("knowledge").display(),
        work = dir.join("work").display(),
    );
    fs::write(&config, body).expect("config");
    config
}
