//! Private PostgreSQL and local HTTPS fixture for the trusted-operator command.
//!
//! The test prepares the database and the role grants. `ontology-dialogue` does not.
//! Passwords stay in files. This file does not print them.

mod common;

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use common::{
    contains_bytes, one_line, pack_artifact, sha256_hex, write_executable, Files, Mock, Script,
    BEARER, POLICY, SQL,
};
use ontology_dialogue::{execute, propose, Fault};
use serde_json::{json, Value};
use rustls::pki_types::pem::PemObject;
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST_DEFAULT: &str = "oo-a300-pg";
const DATABASE: &str = "oo_a300";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a300_login";
const OWNER: &str = "a3000000-0000-4000-8000-000000000300";
const SHARD: &str = "shard_a300";
const SOURCE: &str = "depot";
const TYPE_DIGEST: &str = "b300b300b300b300b300b300b300b300b300b300b300b300b300b300b300b300";
const INPUT_DIGEST: &str = "c300c300c300c300c300c300c300c300c300c300c300c300c300c300c300c300";
const SCHEMA_DIGEST: &str = "d300d300d300d300d300d300d300d300d300d300d300d300d300d300d300d300";
const NAMESPACE: &str = "depot";
const TABLE: &str = "counter";
const GENERATION: i64 = 3;
const ADVISORY_LOCK: i64 = 64_042_026_300;
const QUESTION: &str = "How many visible headcount values are stored?";
const NORTH_TEXT: &str = "column headcount is visible to this role\n";
const SOUTH_TEXT: &str = "column secret_note is visible to this role\n";
const WRITE_SQL: &str = "INSERT INTO depot.counter (headcount) VALUES (1);";
const COUNT_SQL: &str = "SELECT count(*) FROM depot.counter";
const NOTES: [&str; 3] = ["south-only-note-1", "south-only-note-2", "south-only-note-3"];

#[test]
fn live_trusted_operator_dialogue() {
    if env::var("OO_A300_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A300_LIVE is unset; the private postgres dialogue was not run");
        return;
    }
    let ctx = LiveCtx::load();
    record_onboarding(&ctx);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(prepare_database(&ctx));
    drop(runtime);
    install_roles(&ctx);
    prove_dialogue(&ctx);
    ctx.fact("live_done", "1");
}

struct LiveCtx {
    host: String,
    ca: PathBuf,
    admin_password_file: PathBuf,
    login_password_file: PathBuf,
    admin_password: String,
    login_password: String,
    query_pg: PathBuf,
    query_sha: String,
    install_bin: PathBuf,
    provision_bin: PathBuf,
    onboarding_bin: PathBuf,
    oracle_path: PathBuf,
    bans: Vec<String>,
    lines: std::sync::Mutex<Vec<String>>,
}

impl LiveCtx {
    fn load() -> Self {
        let admin_password_file = env_path("OO_A300_ADMIN_PASSWORD_FILE");
        let login_password_file = env_path("OO_A300_LOGIN_PASSWORD_FILE");
        let admin_password = read_secret(&admin_password_file);
        let login_password = read_secret(&login_password_file);
        assert!(
            login_password.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "login password is not hex"
        );
        assert!(!admin_password.is_empty() && !login_password.is_empty());
        let query_pg = env_path("OO_A300_QUERY_PG");
        let query_bytes = fs::read(&query_pg).expect("read query-pg");
        let bans = vec![
            admin_password.clone(),
            login_password.clone(),
            BEARER.to_string(),
            NOTES[0].to_string(),
            NOTES[1].to_string(),
            NOTES[2].to_string(),
        ];
        let oracle_path = env::var("OO_A300_ORACLE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/evidence/live/oracle.txt"));
        Self {
            host: env::var("OO_A300_PG_HOST").unwrap_or_else(|_| HOST_DEFAULT.to_string()),
            ca: env_path("OO_A300_PG_CA"),
            admin_password_file,
            login_password_file,
            admin_password,
            login_password,
            query_sha: sha256_hex(&query_bytes),
            query_pg,
            install_bin: env_path("OO_A300_INSTALL_COLUMN_ACL"),
            provision_bin: env_path("OO_A300_PROVISION_MAPPING_READER"),
            onboarding_bin: env_path("OO_A300_PG_ONBOARDING"),
            oracle_path,
            bans,
            lines: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn fact(&self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        self.scan(key, value.as_bytes());
        let mut lines = self.lines.lock().expect("oracle");
        lines.push(format!("{key}={value}"));
        if let Some(parent) = self.oracle_path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(&self.oracle_path, lines.join("\n") + "\n").expect("oracle");
    }

    fn scan(&self, label: &str, bytes: &[u8]) {
        for ban in &self.bans {
            if contains_bytes(bytes, ban.as_bytes()) {
                panic!("{label} contained a banned token");
            }
        }
    }

    fn save(&self, name: &str, bytes: &[u8]) {
        self.scan(name, bytes);
        let path = self
            .oracle_path
            .parent()
            .expect("oracle parent")
            .join(name);
        fs::write(path, bytes).expect("save");
    }
}

fn env_path(key: &str) -> PathBuf {
    PathBuf::from(env::var(key).unwrap_or_else(|_| panic!("{key} is unset")))
}

fn read_secret(path: &Path) -> String {
    let bytes = fs::read(path).expect("secret file");
    let mut text = String::from_utf8(bytes).expect("secret utf8");
    if text.ends_with("\r\n") {
        text.truncate(text.len() - 2);
    } else if text.ends_with('\n') {
        text.pop();
    }
    assert!(!text.contains('\0'));
    text
}

fn record_onboarding(ctx: &LiveCtx) {
    let output = Command::new(&ctx.onboarding_bin)
        .arg("--help")
        .env_remove("PGPASSWORD")
        .output()
        .expect("pg-onboarding");
    ctx.scan("pg-onboarding-stdout", &output.stdout);
    ctx.scan("pg-onboarding-stderr", &output.stderr);
    ctx.save("pg-onboarding-help.stdout", &output.stdout);
    ctx.save("pg-onboarding-help.stderr", &output.stderr);
    ctx.fact(
        "pg_onboarding_help_exit",
        &output.status.code().unwrap_or(127).to_string(),
    );
}

fn install_roles(ctx: &LiveCtx) {
    let north = tool_ok(
        ctx,
        "install-north",
        &ctx.install_bin,
        &install_doc(ctx, "north", &["headcount"]),
    );
    assert!(north.windows(b"oo_r_north".len()).any(|window| window == b"oo_r_north"));
    let south = tool_ok(
        ctx,
        "install-south",
        &ctx.install_bin,
        &install_doc(ctx, "south", &["secret_note"]),
    );
    assert!(south.windows(b"oo_r_south".len()).any(|window| window == b"oo_r_south"));
    let provision = tool_ok(
        ctx,
        "provision-north",
        &ctx.provision_bin,
        &provision_doc(ctx),
    );
    assert!(provision.windows(b"oo_r_north".len()).any(|window| window == b"oo_r_north"));
    ctx.fact("install_north", "0");
    ctx.fact("install_south", "0");
    ctx.fact("provision_north", "0");
}

fn install_doc(ctx: &LiveCtx, role: &str, columns: &[&str]) -> Vec<u8> {
    let doc = json!({
        "protocol": "oo.gateway.install-column-acl-cli/v1",
        "policies": [acl_with_filter(role, columns)],
        "endpoint": endpoint(ctx, ADMIN),
        "password_file": ctx.admin_password_file.display().to_string(),
        "ca_file": ctx.ca.display().to_string()
    });
    serde_json::to_vec(&doc).unwrap()
}

fn provision_doc(ctx: &LiveCtx) -> Vec<u8> {
    let doc = json!({
        "protocol": "oo.gateway.provision-mapping-reader-cli/v1",
        "policies": [acl_with_filter("north", &["headcount"])],
        "login": LOGIN,
        "endpoint": endpoint(ctx, ADMIN),
        "password_file": ctx.admin_password_file.display().to_string(),
        "ca_file": ctx.ca.display().to_string()
    });
    serde_json::to_vec(&doc).unwrap()
}

fn acl_with_filter(role: &str, columns: &[&str]) -> Value {
    let mut value = acl(role, columns);
    value["row_filter"] = Value::Null;
    value
}

fn acl(role: &str, columns: &[&str]) -> Value {
    json!({
        "project": SOURCE,
        "config_digest": POLICY,
        "generation": GENERATION,
        "role": role,
        "login": LOGIN,
        "table": {
            "namespace": NAMESPACE,
            "name": TABLE,
            "columns": columns.iter().copied().map(|name| json!({
                "name": name,
                "status": "confirmed",
                "semantic_type": "value",
                "pii": {"class": "none"}
            })).collect::<Vec<_>>()
        }
    })
}

fn endpoint(ctx: &LiveCtx, user: &str) -> Value {
    json!({
        "host": ctx.host,
        "port": 5432,
        "database": DATABASE,
        "user": user
    })
}

fn tool_ok(ctx: &LiveCtx, name: &str, bin: &Path, stdin_bytes: &[u8]) -> Vec<u8> {
    ctx.scan(name, stdin_bytes);
    let mut child = Command::new(bin);
    child
        .env_remove("PGPASSWORD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().unwrap_or_else(|_| panic!("{name} spawn"));
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_bytes)
        .expect("stdin write");
    let output = child.wait_with_output().expect("wait");
    if !output.stdout.is_empty() {
        ctx.save(&format!("{name}.stdout"), &output.stdout);
    }
    if !output.stderr.is_empty() {
        ctx.save(&format!("{name}.stderr"), &output.stderr);
    }
    assert!(
        output.status.success(),
        "{name} exit {}",
        output.status.code().unwrap_or(127)
    );
    output.stdout
}

fn prove_dialogue(ctx: &LiveCtx) {
    prove_propose_cli_matches_api(ctx);
    let north = propose_case(ctx, "oo_r_north", NORTH_TEXT, QUESTION, SQL);
    ctx.scan("north-model", &north.model_body);
    ctx.save("north-model-body.json", &north.model_body);
    assert!(north.model_body.windows(QUESTION.len()).any(|window| window == QUESTION.as_bytes()));
    assert!(!contains_bytes(&north.model_body, b"secret_note"));
    assert!(contains_bytes(&north.model_body, b"\"temperature\":0"));
    let request = query_request(ctx, "oo_r_north", SQL, &["headcount"], 20, &north.dir.join("audit-north.jsonl"));
    let finished = run_execute(ctx, &north, &request);
    assert_eq!(finished.exit, 0, "north library exit");
    let doc = parse_execution(&finished.stdout);
    assert_eq!(doc["query_receipt"], "clean");
    assert_eq!(doc["authenticated_audit"], false);
    assert_eq!(doc["entailment"], "not_judged");
    assert_eq!(doc["spawned"], true);
    assert_eq!(doc["rows"].as_array().expect("rows").len(), 3);
    assert_eq!(doc["rows"][0][0]["value"], "1");
    assert_eq!(doc["rows"][1][0]["value"], "2");
    assert_eq!(doc["rows"][2][0]["value"], "3");
    let query_id = doc["query_id"].as_str().expect("query id");
    assert!(query_id.starts_with("q_"));
    assert_eq!(doc["citation"]["admitted"][0]["query_id"], query_id);
    assert_eq!(doc["citation"]["authenticated_audit"], false);
    assert_eq!(doc["citation"]["entailment"], "not_judged");
    ctx.scan("north-execution", &finished.stdout);
    ctx.save("north-execution.json", &finished.stdout);
    ctx.fact("north_exit", "0");
    ctx.fact("north_rows", "3");
    ctx.fact("north_query_id", query_id);
    compare_cli_execute(ctx, &north, "oo_r_north", SQL, &["headcount"], 20);
    prove_truncation(ctx, &north);
    prove_stale_and_tamper(ctx, &north);
    prove_write(ctx);
    prove_south(ctx);
    prove_http_failure(ctx);
    prove_count(ctx);
}

fn prove_propose_cli_matches_api(ctx: &LiveCtx) {
    let files = Files::new();
    let question = files.question(QUESTION);
    let (pack_bytes, _) = pack_artifact("oo_r_north", NORTH_TEXT);
    let pack = files.dir.join("pack.json");
    fs::write(&pack, &pack_bytes).unwrap();
    let api_mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let api_config = files.propose_config_role(
        &format!("https://localhost:{port}", port = api_mock.port),
        &question,
        &pack,
        "oo_r_north",
    );
    let api = propose(&api_config).expect("api propose");
    let cli_mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let cli_config = files.propose_config_role(
        &format!("https://localhost:{port}", port = cli_mock.port),
        &question,
        &pack,
        "oo_r_north",
    );
    let config_path = files.dir.join("propose.json");
    fs::write(&config_path, &cli_config).unwrap();
    let cli = common::run_cli(&["propose", "--config", config_path.to_str().unwrap()]);
    assert_eq!(cli.status.code(), Some(0));
    assert!(cli.stderr.is_empty());
    assert_eq!(cli.stdout, api);
    ctx.scan("propose-cli", &cli.stdout);
    ctx.fact("cli_propose_equal", "true");
}

struct Proposed {
    dir: PathBuf,
    proposal_path: PathBuf,
    approval_path: PathBuf,
    pack_path: PathBuf,
    pack_bytes: Vec<u8>,
    model_body: Vec<u8>,
}

fn propose_case(ctx: &LiveCtx, role: &str, table_text: &str, question_text: &str, sql: &str) -> Proposed {
    let files = Files::new();
    let question = files.question(question_text);
    let (pack_bytes, _) = pack_artifact(role, table_text);
    let pack_path = files.dir.join("pack.json");
    fs::write(&pack_path, &pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Sql(sql.to_string()));
    let config = files.propose_config_role(
        &format!("https://localhost:{port}", port = mock.port),
        &question,
        &pack_path,
        role,
    );
    let stdout = propose(&config).unwrap_or_else(|err| panic!("propose {role} {token}", token = err.token));
    ctx.scan("proposal", &stdout);
    let value: Value = serde_json::from_slice(one_line(&stdout)).unwrap();
    assert_eq!(value["sql"], sql);
    assert_eq!(value["model_sql"], "untrusted");
    assert_eq!(value["auto_execute"], false);
    assert_eq!(value["authenticated"], false);
    let proposal_path = files.dir.join("proposal.json");
    fs::write(&proposal_path, &stdout).unwrap();
    let approval = json!({
        "protocol": "oo.ontology-dialogue.approval/v1",
        "trust": "local_operator_declared",
        "authenticated": false,
        "proposal_sha256": value["proposal_sha256"],
        "sql_sha256": value["sql_sha256"],
        "tuple_sha256": value["tuple_sha256"],
        "pack_digest": value["pack_digest"]
    });
    let approval_path = files.dir.join("approval.json");
    fs::write(&approval_path, serde_json::to_vec(&approval).unwrap()).unwrap();
    let model_body = mock.hits().into_iter().next().expect("hit").body;
    Proposed {
        dir: files.dir,
        proposal_path,
        approval_path,
        pack_path,
        pack_bytes,
        model_body,
    }
}

fn query_request(
    ctx: &LiveCtx,
    role: &str,
    sql: &str,
    columns: &[&str],
    max_rows: u64,
    audit: &Path,
) -> PathBuf {
    let slug = role.strip_prefix("oo_r_").expect("role");
    let doc = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": endpoint(ctx, LOGIN),
        "password_file": ctx.login_password_file.display().to_string(),
        "ca_file": ctx.ca.display().to_string(),
        "audit_file": audit.display().to_string(),
        "identity": {"project": SOURCE, "role": role, "policy_id": POLICY},
        "policies": [acl(slug, columns)],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": "request",
            "sql": sql,
            "max_rows": max_rows,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    });
    let bytes = serde_json::to_vec(&doc).unwrap();
    ctx.scan("query-request", &bytes);
    let path = audit.with_extension("request.json");
    fs::write(&path, bytes).unwrap();
    path
}

fn run_execute(ctx: &LiveCtx, proposed: &Proposed, request: &Path) -> ontology_dialogue::Finished {
    let config = execute_text(ctx, proposed, request);
    execute(&config).unwrap_or_else(|err| panic!("execute {token}", token = err.token))
}

fn execute_text(ctx: &LiveCtx, proposed: &Proposed, request: &Path) -> String {
    json!({
        "protocol": "oo.ontology-dialogue.execute-config/v1",
        "proposal_file": proposed.proposal_path.display().to_string(),
        "approval_file": proposed.approval_path.display().to_string(),
        "pack_file": proposed.pack_path.display().to_string(),
        "request_file": request.display().to_string(),
        "query_pg": {"path": ctx.query_pg.display().to_string(), "sha256": ctx.query_sha},
        "binding": {
            "session_id": "sess-a300",
            "turn_id": "turn-a300",
            "declares_data": true,
            "render_rows": true
        },
        "budget": {"deadline_ms": 30000, "max_stdout_bytes": 1048576}
    })
    .to_string()
}

fn parse_execution(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes.trim_ascii_end()).expect("execution json")
}

fn compare_cli_execute(
    ctx: &LiveCtx,
    proposed: &Proposed,
    role: &str,
    sql: &str,
    columns: &[&str],
    max_rows: u64,
) {
    let audit = proposed.dir.join("audit-north-cli.jsonl");
    let request = query_request(ctx, role, sql, columns, max_rows, &audit);
    let config = execute_text(ctx, proposed, &request);
    let config_path = proposed.dir.join("execute-cli.json");
    fs::write(&config_path, &config).unwrap();
    let cli = common::run_cli(&["execute", "--config", config_path.to_str().unwrap()]);
    assert_eq!(cli.status.code(), Some(0));
    assert!(cli.stderr.is_empty());
    ctx.scan("cli-execute", &cli.stdout);
    let doc = parse_execution(&cli.stdout);
    assert_eq!(doc["query_receipt"], "clean");
    assert_eq!(doc["rows"].as_array().expect("rows").len(), 3);
    assert_eq!(doc["authenticated_audit"], false);
    let query_id = doc["query_id"].as_str().expect("cli query id");
    assert_eq!(doc["citation"]["admitted"][0]["query_id"], query_id);
    ctx.fact("cli_execute_exit", "0");
}

fn prove_truncation(ctx: &LiveCtx, proposed: &Proposed) {
    let audit = proposed.dir.join("audit-trunc.jsonl");
    let request = query_request(ctx, "oo_r_north", SQL, &["headcount"], 1, &audit);
    let finished = run_execute(ctx, proposed, &request);
    assert_eq!(finished.exit, 0, "truncation exit");
    ctx.scan("truncation", &finished.stdout);
    let doc = parse_execution(&finished.stdout);
    assert_eq!(doc["query_receipt"], "truncated");
    assert_eq!(doc["summary"]["truncated"], true);
    assert_eq!(doc["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(doc["rows"][0][0]["value"], "1");
    assert_eq!(doc["citation"]["disclosures"][0]["code"], "querytruncated");
    assert_eq!(doc["citation"]["disclosures"][0]["comprehended"], "not_judged");
    ctx.fact("truncated_exit", "0");
    ctx.fact("truncated_disclosure", "querytruncated");
}

fn prove_stale_and_tamper(ctx: &LiveCtx, proposed: &Proposed) {
    let audit = proposed.dir.join("audit-stale.jsonl");
    let request = query_request(ctx, "oo_r_north", SQL, &["headcount"], 20, &audit);
    let (stale, _) = pack_artifact("oo_r_north", "column headcount changed after review\n");
    fs::write(&proposed.pack_path, &stale).unwrap();
    let err = execute(&execute_text(ctx, proposed, &request)).expect_err("stale");
    assert_eq!(err, Fault::approval());
    assert!(!audit.exists(), "stale approval spawned query-pg");
    ctx.fact("stale_pack", "approval");
    fs::write(&proposed.pack_path, &proposed.pack_bytes).unwrap();

    let tamper_audit = proposed.dir.join("audit-tamper.jsonl");
    let tamper_request = query_request(ctx, "oo_r_north", SQL, &["headcount"], 20, &tamper_audit);
    let mut tampered: Value = serde_json::from_slice(&fs::read(&tamper_request).unwrap()).unwrap();
    tampered["statement"]["sql"] = json!(format!("{SQL} "));
    let tampered_bytes = serde_json::to_vec(&tampered).unwrap();
    ctx.scan("tamper-request", &tampered_bytes);
    fs::write(&tamper_request, tampered_bytes).unwrap();
    let err = execute(&execute_text(ctx, proposed, &tamper_request)).expect_err("tamper");
    assert_eq!(err, Fault::approval());
    assert!(!tamper_audit.exists(), "tampered sql spawned query-pg");
    ctx.fact("sql_tamper", "approval");
}

fn prove_write(ctx: &LiveCtx) {
    let proposed = propose_case(ctx, "oo_r_north", NORTH_TEXT, QUESTION, WRITE_SQL);
    ctx.scan("write-model", &proposed.model_body);
    let audit = proposed.dir.join("audit-write.jsonl");
    let request = query_request(ctx, "oo_r_north", WRITE_SQL, &["headcount"], 20, &audit);
    let finished = run_execute(ctx, &proposed, &request);
    assert_eq!(finished.exit, 6);
    ctx.scan("write-execution", &finished.stdout);
    let doc = parse_execution(&finished.stdout);
    assert_eq!(doc["spawned"], true);
    assert_eq!(doc["child_exit"], 6);
    assert_eq!(doc["child_token"], "guard");
    assert_eq!(doc["query_receipt"], "absent");
    assert!(doc["query_id"].is_null());
    assert!(doc["citation"].is_null());
    assert!(!audit.exists());
    ctx.fact("write_exit", "6");
    ctx.fact("write_receipt", "absent");
}

fn prove_south(ctx: &LiveCtx) {
    let question = "List the stored notes.";
    let proposed = propose_case(ctx, "oo_r_south", SOUTH_TEXT, question, SQL);
    ctx.scan("south-model", &proposed.model_body);
    assert!(!contains_bytes(&proposed.model_body, b"headcount"));
    let audit = proposed.dir.join("audit-south.jsonl");
    let request = query_request(ctx, "oo_r_south", SQL, &["secret_note"], 20, &audit);
    let finished = run_execute(ctx, &proposed, &request);
    assert_eq!(finished.exit, 6);
    ctx.scan("south-execution", &finished.stdout);
    let doc = parse_execution(&finished.stdout);
    assert_eq!(doc["query_receipt"], "absent");
    assert!(doc["query_id"].is_null());
    assert!(!audit.exists());
    ctx.fact("south_exit", "6");
    ctx.fact("south_receipt", "absent");
}

fn prove_http_failure(ctx: &LiveCtx) {
    let files = Files::new();
    let question = files.question(QUESTION);
    let (pack_bytes, _) = pack_artifact("oo_r_north", NORTH_TEXT);
    let pack = files.dir.join("pack.json");
    fs::write(&pack, &pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Status(500));
    let config = files.propose_config_role(
        &format!("https://localhost:{port}", port = mock.port),
        &question,
        &pack,
        "oo_r_north",
    );
    let err = propose(&config).expect_err("http");
    assert_eq!(err, Fault::provider());
    assert_eq!(mock.hits().len(), 1);
    let marker = files.dir.join("marker");
    let script = files.dir.join("query-pg");
    write_executable(&script, &common::marker_script(&marker));
    assert!(!marker.exists());
    ctx.fact("http_failure", "provider");
    ctx.fact("http_execute_called", "false");
}

fn prove_count(ctx: &LiveCtx) {
    let proposed = propose_case(ctx, "oo_r_north", NORTH_TEXT, QUESTION, COUNT_SQL);
    ctx.scan("count-model", &proposed.model_body);
    let audit = proposed.dir.join("audit-count.jsonl");
    let request = query_request(ctx, "oo_r_north", COUNT_SQL, &["headcount"], 20, &audit);
    let outcome = execute(&execute_text(ctx, &proposed, &request));
    match outcome {
        Ok(finished) => classify_count(ctx, finished.exit, &finished.stdout, audit.exists()),
        Err(err) => panic!("count left the child unspawned: {token}", token = err.token),
    }
}

fn classify_count(ctx: &LiveCtx, exit: u8, stdout: &[u8], audit_exists: bool) {
    ctx.scan("count-execution", stdout);
    let doc = parse_execution(stdout);
    ctx.fact("count_exit", &exit.to_string());
    let receipt = doc["query_receipt"].as_str().unwrap_or("missing");
    ctx.fact("count_receipt", receipt);
    match exit {
        0 => {
            assert_eq!(receipt, "clean");
            assert_eq!(doc["rows"][0][0]["value"], "3");
            assert!(audit_exists);
        }
        6 => {
            assert_eq!(receipt, "absent");
            assert!(doc["query_id"].is_null());
            assert!(!audit_exists);
        }
        7 => {
            assert_eq!(receipt, "failed");
            assert!(doc["query_id"].is_null());
        }
        _ => panic!("count exit {exit}"),
    }
}

async fn prepare_database(ctx: &LiveCtx) {
    let ca = fs::read(&ctx.ca).expect("ca");
    reset_database(ctx, &ca).await;
    let mut client = connect_db(ctx, DATABASE, ADMIN, &ctx.admin_password, &ca).await;
    apply_frozen_schema(&mut client).await;
    create_login(&client, &ctx.login_password).await;
    let shadow = prepare_shadow(&mut client).await;
    analyze_shadow(&client, &shadow.table).await;
}

async fn connect_db(ctx: &LiveCtx, database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let certificate = rustls::pki_types::CertificateDer::from_pem_slice(ca).expect("ca pem");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).expect("ca root");
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tls = tokio_postgres_rustls::MakeRustlsConnect::new(config);
    let mut last_state = String::new();
    for _ in 0..20 {
        let mut pg = tokio_postgres::Config::new();
        pg.user(user);
        pg.password(password);
        pg.host(&ctx.host);
        pg.port(5432);
        pg.dbname(database);
        pg.ssl_mode(tokio_postgres::config::SslMode::Require);
        pg.application_name("oo-ontology-dialogue-fixture");
        match pg.connect(tls.clone()).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last_state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect failed sqlstate={last_state}");
}

async fn reset_database(ctx: &LiveCtx, ca: &[u8]) {
    let maintenance = connect_db(ctx, "postgres", ADMIN, &ctx.admin_password, ca).await;
    exec_hidden(
        &maintenance,
        "terminate",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    exec_hidden(
        &maintenance,
        "drop-database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    exec_hidden(
        &maintenance,
        "drop-roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a300_login') THEN\n\
             DROP ROLE oo_a300_login;\n\
           END IF;\n\
           FOR role_name IN\n\
             SELECT rolname FROM pg_catalog.pg_roles\n\
             WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
           LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    exec_hidden(
        &maintenance,
        "create-database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn exec_hidden(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| fail_db(name, &err));
}

async fn exec_tx(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| fail_db(name, &err));
}

fn fail_db(name: &str, err: &tokio_postgres::Error) -> ! {
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    panic!("{name} sqlstate={state}");
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_tx(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_tx(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_tx(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_tx(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_tx(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");
    let tx = begin_locked(client).await;
    exec_tx(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_tx(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_tx(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_tx(
        &tx,
        "marker-import",
        &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = migration_dir().join(name);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("read {name}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} utf8"));
    (text, sha256_hex(&bytes))
}

fn migration_dir() -> PathBuf {
    env::var("OO_A300_MIGRATIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/oo-pipeline/migrations")
        })
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert_eq!(checksum.len(), 64);
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}"
    )
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_tx(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_tx(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_tx(
        &tx,
        "advisory-lock",
        &format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})"),
    )
    .await;
    tx
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    exec_hidden(
        client,
        "create-login",
        &format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ),
    )
    .await;
}

struct Shadow {
    table: String,
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .unwrap_or_else(|err| fail_db("adopt", &err))
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .unwrap_or_else(|err| fail_db("bind", &err));
    let columns = ["headcount", "secret_note"];
    register(client, "namespace", "n", NAMESPACE, "", "").await;
    register(client, "table", "t", NAMESPACE, TABLE, "").await;
    for column in columns {
        register(client, "column", "c", NAMESPACE, TABLE, column).await;
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let headcount = lookup(client, "column", NAMESPACE, TABLE, "headcount").await;
    let secret_note = lookup(client, "column", NAMESPACE, TABLE, "secret_note").await;
    let oids = vec![type_oid(client, "int4").await, type_oid(client, "text").await];
    assert!(oids.iter().all(|oid| *oid > 0));
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &GENERATION,
        &POLICY,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &open_params,
        )
        .await
        .unwrap_or_else(|err| fail_db("open-run", &err))
        .get(0);
    let idents: Vec<String> = columns.into_iter().map(str::to_string).collect();
    let typmods = vec![-1_i32; 2];
    let required = vec![true, true];
    let index = 0_i32;
    let prepare_params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_id,
        &index,
        &SOURCE,
        &POLICY,
        &GENERATION,
        &NAMESPACE,
        &TABLE,
        &idents,
        &oids,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &prepare_params,
        )
        .await
        .unwrap_or_else(|err| fail_db("prepare", &err))
        .get(0);
    assert_eq!(prepared, table);
    insert_rows(client, &table, &headcount, &secret_note).await;
    Shadow { table }
}

async fn type_oid(client: &Client, name: &str) -> i64 {
    let oid: i32 = client
        .query_one(
            "SELECT type_row.oid::int4 \
             FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
            &[&name],
        )
        .await
        .unwrap_or_else(|err| fail_db("type-oid", &err))
        .get(0);
    i64::from(oid)
}

async fn register(
    client: &Client,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &POLICY,
        &GENERATION,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .unwrap_or_else(|err| fail_db("digest", &err))
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &POLICY,
        &GENERATION,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .unwrap_or_else(|err| fail_db("register", &err));
}

async fn lookup(
    client: &Client,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &POLICY,
        &GENERATION,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .unwrap_or_else(|err| fail_db("lookup", &err))
        .get(0)
}

async fn insert_rows(client: &mut Client, table: &str, headcount: &str, secret_note: &str) {
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {headcount}, {secret_note}) \
         VALUES (($1::text)::numeric, $2, $3)",
        table = quote_physical(table, "pt_"),
        headcount = quote_physical(headcount, "pc_"),
        secret_note = quote_physical(secret_note, "pc_"),
    );
    let tx = client.transaction().await.expect("insert begin");
    for (index, note) in NOTES.into_iter().enumerate() {
        let pos = (index as i64 + 1).to_string();
        let headcount = (index as i32) + 1;
        tx.execute(&sql, &[&pos, &headcount, &note])
            .await
            .unwrap_or_else(|err| fail_db("insert", &err));
    }
    tx.commit().await.expect("insert commit");
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}

async fn analyze_shadow(client: &Client, table: &str) {
    exec_hidden(
        client,
        "analyze",
        &format!("ANALYZE \"_oo\".{}", quote_physical(table, "pt_")),
    )
    .await;
}
