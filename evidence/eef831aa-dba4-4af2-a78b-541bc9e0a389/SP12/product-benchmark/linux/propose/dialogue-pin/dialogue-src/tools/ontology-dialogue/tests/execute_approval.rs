mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{
    absent, contains_bytes, one_line, pack_artifact, sha256_hex, write_executable, Files, Mock,
    Script, BEARER, POLICY, SQL,
};
use ontology_dialogue::{execute, propose, Fault};
use serde_json::{json, Value};

const TABLE: &str = "column headcount is visible to this role\n";
const FINGERPRINT: &str = "abc123def4567890";

struct Ready {
    files: Files,
    proposal_path: PathBuf,
    approval_path: PathBuf,
    pack_path: PathBuf,
    request_path: PathBuf,
    marker: PathBuf,
    script: PathBuf,
    audit: PathBuf,
    proposal: Value,
}

fn ready(script_body: &str) -> Ready {
    let files = Files::new();
    let question = files.question("How many visible headcount values are stored?");
    let (pack_bytes, _) = pack_artifact("oo_r_north", TABLE);
    let pack_path = files.dir.join("pack.json");
    fs::write(&pack_path, &pack_bytes).unwrap();
    let mock = Mock::spawn(Script::Sql(SQL.to_string()));
    let config = files.propose_config(&format!("https://localhost:{port}", port = mock.port), &question, &pack_path);
    let stdout = propose(&config).expect("proposal");
    let proposal: Value = serde_json::from_slice(one_line(&stdout)).unwrap();
    let proposal_path = files.dir.join("proposal.json");
    fs::write(&proposal_path, &stdout).unwrap();
    let approval = json!({
        "protocol": "oo.ontology-dialogue.approval/v1",
        "trust": "local_operator_declared",
        "authenticated": false,
        "proposal_sha256": proposal["proposal_sha256"],
        "sql_sha256": proposal["sql_sha256"],
        "tuple_sha256": proposal["tuple_sha256"],
        "pack_digest": proposal["pack_digest"]
    });
    let approval_path = files.dir.join("approval.json");
    fs::write(&approval_path, serde_json::to_vec(&approval).unwrap()).unwrap();
    let audit = files.dir.join("audit.jsonl");
    let marker = files.dir.join("marker");
    let script = files.dir.join("query-pg");
    write_executable(&script, script_body);
    let password = files.dir.join("password");
    fs::write(&password, b"pw-a300-canary-value\n").unwrap();
    let request = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {"host": "oo-a300-pg", "port": 5432, "database": "oo_a300", "user": "oo_a300_login"},
        "password_file": password.display().to_string(),
        "ca_file": files.dir.join("ca.pem").display().to_string(),
        "audit_file": audit.display().to_string(),
        "identity": {"project": "depot", "role": "oo_r_north", "policy_id": POLICY},
        "statement": {
            "source": "request",
            "sql": proposal["sql"],
            "max_rows": 20,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    });
    let request_path = files.dir.join("request.json");
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    Ready {
        files,
        proposal_path,
        approval_path,
        pack_path,
        request_path,
        marker,
        script,
        audit,
        proposal,
    }
}

fn execute_config(ready: &Ready, sha: &str) -> String {
    json!({
        "protocol": "oo.ontology-dialogue.execute-config/v1",
        "proposal_file": ready.proposal_path.display().to_string(),
        "approval_file": ready.approval_path.display().to_string(),
        "pack_file": ready.pack_path.display().to_string(),
        "request_file": ready.request_path.display().to_string(),
        "query_pg": {"path": ready.script.display().to_string(), "sha256": sha},
        "binding": {
            "session_id": "sess1",
            "turn_id": "turn1",
            "declares_data": true,
            "render_rows": true
        },
        "budget": {"deadline_ms": 5000, "max_stdout_bytes": 65536}
    })
    .to_string()
}

fn script_sha(path: &Path) -> String {
    sha256_hex(&fs::read(path).unwrap())
}

#[test]
fn stale_pack_sql_tamper_and_bad_binary_do_not_spawn() {
    let ready = ready(&common::marker_script(&common::scratch().join("nope")));
    // Recreate the script so the marker path is the one we assert.
    let script = common::marker_script(&ready.marker);
    write_executable(&ready.script, &script);
    let sha = script_sha(&ready.script);
    let config = execute_config(&ready, &sha);

    fs::write(&ready.approval_path, b"{}").unwrap();
    let err = execute(&config).expect_err("approval");
    assert_eq!(err, Fault::approval());
    absent(&ready.marker);

    let approval = json!({
        "protocol": "oo.ontology-dialogue.approval/v1",
        "trust": "local_operator_declared",
        "authenticated": false,
        "proposal_sha256": ready.proposal["proposal_sha256"],
        "sql_sha256": ready.proposal["sql_sha256"],
        "tuple_sha256": ready.proposal["tuple_sha256"],
        "pack_digest": ready.proposal["pack_digest"]
    });
    fs::write(&ready.approval_path, serde_json::to_vec(&approval).unwrap()).unwrap();
    let (other, _) = pack_artifact("oo_r_north", "column headcount changed after review\n");
    fs::write(&ready.pack_path, other).unwrap();
    let err = execute(&config).expect_err("stale");
    assert_eq!(err, Fault::approval());
    absent(&ready.marker);
    let missing = execute_config(&ready, &sha).replace(
        &ready.script.display().to_string(),
        &ready.files.dir.join("missing-query-pg").display().to_string(),
    );
    let err = execute(&missing).expect_err("stale before open");
    assert_eq!(err, Fault::approval());
    absent(&ready.marker);

    let (pack_bytes, _) = pack_artifact("oo_r_north", TABLE);
    fs::write(&ready.pack_path, pack_bytes).unwrap();
    let mut request: Value = serde_json::from_slice(&fs::read(&ready.request_path).unwrap()).unwrap();
    request["statement"]["sql"] = json!(format!("{SQL} "));
    fs::write(&ready.request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let err = execute(&config).expect_err("tamper");
    assert_eq!(err, Fault::approval());
    absent(&ready.marker);

    request["statement"]["sql"] = json!(SQL);
    fs::write(&ready.request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let bad = execute_config(&ready, &"0".repeat(64));
    let err = execute(&bad).expect_err("binary");
    assert_eq!(err, Fault::binary());
    absent(&ready.marker);

    let mut authenticated = approval;
    authenticated["authenticated"] = json!(true);
    fs::write(&ready.approval_path, serde_json::to_vec(&authenticated).unwrap()).unwrap();
    let err = execute(&config).expect_err("authenticated");
    assert_eq!(err, Fault::approval());
    absent(&ready.marker);
}

#[test]
fn guard_refusal_has_no_query_id() {
    let files_marker = common::scratch().join("marker");
    let ready = ready("#!/bin/sh\nexit 0\n");
    let script = format!(
        "#!/bin/sh\n/bin/touch {}\nprintf '%s\\n' 'query-pg: guard' >&2\nexit 6\n",
        shell(&ready.marker)
    );
    write_executable(&ready.script, &script);
    let config = execute_config(&ready, &script_sha(&ready.script));
    let finished = execute(&config).expect("spawned");
    assert_eq!(finished.exit, 6);
    assert!(ready.marker.exists());
    assert!(!files_marker.exists());
    let doc: Value = serde_json::from_slice(one_line(&finished.stdout)).unwrap();
    assert_eq!(doc["spawned"], true);
    assert_eq!(doc["child_exit"], 6);
    assert_eq!(doc["child_token"], "guard");
    assert_eq!(doc["query_receipt"], "absent");
    assert!(doc["query_id"].is_null());
    assert!(doc["citation"].is_null());
    assert!(doc["rendered"].is_null());
    assert!(!contains_bytes(&finished.stdout, b"pw-a300-canary-value"));
    assert!(!contains_bytes(&finished.stdout, BEARER.as_bytes()));
    let cli_path = ready.files.dir.join("execute.json");
    fs::write(&cli_path, &config).unwrap();
    let cli = common::run_cli(&["execute", "--config", cli_path.to_str().unwrap()]);
    assert_eq!(cli.status.code(), Some(6));
    assert_eq!(cli.stderr, b"ontology-dialogue: guard\n");
    assert_eq!(cli.stdout, finished.stdout);
}

#[test]
fn approved_child_receipt_uses_the_citations_module() {
    let ready = ready("#!/bin/sh\nexit 0\n");
    let audit_src = ready.files.dir.join("audit-src.jsonl");
    fs::write(&audit_src, audit_body()).unwrap();
    let script = format!(
        "#!/bin/sh\n/bin/touch {}\n/bin/cp {} {}\nprintf '%s\\n' {}\nprintf '%s\\n' {}\nexit 0\n",
        shell(&ready.marker),
        shell(&audit_src),
        shell(&ready.audit),
        shell_text(&row_line()),
        shell_text(&summary_line())
    );
    write_executable(&ready.script, &script);
    let config = execute_config(&ready, &script_sha(&ready.script));
    let finished = execute(&config).expect("finished");
    assert_eq!(finished.exit, 0, "{}", String::from_utf8_lossy(&finished.stdout));
    let doc: Value = serde_json::from_slice(one_line(&finished.stdout)).unwrap();
    assert_eq!(doc["authenticated_audit"], false);
    assert_eq!(doc["entailment"], "not_judged");
    assert_eq!(doc["warning_comprehension"], "not_judged");
    assert_eq!(doc["auto_execute"], false);
    assert_eq!(doc["query_receipt"], "clean");
    assert_eq!(doc["query_id"], format!("q_{FINGERPRINT}"));
    assert_eq!(doc["rows"][0][0]["kind"], "int");
    assert_eq!(doc["rows"][0][0]["value"], "1");
    let rendered = doc["rendered"].as_str().unwrap();
    assert!(rendered.contains("status succeeded"));
    assert!(rendered.contains("cell int 1"));
    assert!(rendered.contains(&format!("[q_{FINGERPRINT}]")));
    assert_eq!(doc["citation"]["citation_status"], "ok");
    assert_eq!(doc["citation"]["authenticated_audit"], false);
    assert_eq!(doc["citation"]["admitted"][0]["query_id"], format!("q_{FINGERPRINT}"));
    assert!(!contains_bytes(&finished.stdout, b"pw-a300-canary-value"));
    if let Ok(bin) = std::env::var("OO_A300_ANSWER_CITATIONS") {
        let request = json!({
            "version": "oo.answer-citations/v1",
            "trust": "local_operator_declared",
            "binding": {
                "session_id": "sess1",
                "turn_id": "turn1",
                "role": "oo_r_north",
                "project": "depot",
                "protocol": "oo.gateway.query-pg-audit/v1"
            },
            "receipts": [{
                "receipt_sha256": sha256_hex(audit_body().as_bytes()),
                "body": audit_body()
            }],
            "answer": {"declares_data": true, "text": rendered}
        });
        let mut child = Command::new(bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("citations");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&request).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        let cli_report = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
        let cli_value: Value = serde_json::from_slice(cli_report).unwrap();
        assert_eq!(cli_value, doc["citation"]);
    }
    let cli_path = ready.files.dir.join("execute.json");
    fs::write(&cli_path, &config).unwrap();
    let _ = fs::remove_file(&ready.audit);
    let _ = fs::remove_file(&ready.marker);
    let cli = common::run_cli(&["execute", "--config", cli_path.to_str().unwrap()]);
    assert_eq!(cli.status.code(), Some(0));
    assert!(cli.stderr.is_empty());
    assert_eq!(cli.stdout, finished.stdout);
}

fn audit_body() -> String {
    let hash = "ab".repeat(32);
    format!(
        "{{\"protocol\":\"oo.gateway.query-pg-audit/v1\",\"record\":\"admit\",\"original_sha256\":\"{hash}\",\"executed_sha256\":\"{hash}\",\"fingerprint_hex\":\"{FINGERPRINT}\",\"role\":\"oo_r_north\",\"project\":\"depot\",\"policy_id\":\"{POLICY}\",\"backend_pid\":7,\"backend_start\":\"1\"}}\n{{\"protocol\":\"oo.gateway.query-pg-audit/v1\",\"record\":\"complete\",\"status\":\"succeeded\",\"truncated\":false,\"returned_rows\":1,\"result_bytes\":8,\"rollback_confirmed\":true,\"socket_dropped\":false,\"stop\":null,\"sqlstate\":null,\"original_sha256\":\"{hash}\",\"executed_sha256\":\"{hash}\",\"fingerprint_hex\":\"{FINGERPRINT}\",\"role\":\"oo_r_north\",\"project\":\"depot\",\"policy_id\":\"{POLICY}\",\"backend_pid\":7,\"backend_start\":\"1\"}}\n"
    )
}

fn row_line() -> String {
    "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"1\"}]}".to_string()
}

fn summary_line() -> String {
    let hash = "ab".repeat(32);
    format!(
        "{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":false,\"returned_rows\":1,\"result_bytes\":8,\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":true,\"original_sha256\":\"{hash}\",\"executed_sha256\":\"{hash}\",\"stop\":null,\"sqlstate\":null}}"
    )
}

fn shell(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn shell_text(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
