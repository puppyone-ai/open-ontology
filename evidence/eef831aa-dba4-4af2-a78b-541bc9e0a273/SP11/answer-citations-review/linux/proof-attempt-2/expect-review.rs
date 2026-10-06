#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use answer_citations::sha256_hex;
use serde_json::{json, Value};

const FIXTURE: &[u8] = include_bytes!("fixtures/desk-explicit.audit.jsonl");
const FIXTURE_SHA: &str = "8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783";
const PROTO: &str = "oo.gateway.query-pg-audit/v1";
const QUERY: &str = "q_836041cccc8287ad";
const SQL_ORIGINAL: &str = "24db5e087ebb8f4bd2fdac6711e8df4c72221d5051025a4ecf3f7bb0357f58f5";
const SQL_EXECUTED: &str = "3fc98cd0c3c26446104b49aed4f12abe111458ef1333499e4237082ffc51d386";
const POLICY_FIXTURE: &str = "a198a198a198a198a198a198a198a198a198a198a198a198a198a198a198a198";
const BACKEND_START: &str = "20261006010140151115";
const SECRET: &str = "SECRET_a273_ANSWER";
const CELL: &str = "SECRET_a273_CELL";

#[derive(Clone, Copy)]
struct Bind {
    session: &'static str,
    turn: &'static str,
    role: &'static str,
    project: &'static str,
    protocol: &'static str,
}

#[derive(Clone)]
struct Spec {
    fingerprint: String,
    role: String,
    project: String,
    status: &'static str,
    truncated: bool,
    rollback: bool,
    dropped: bool,
    stop: Option<&'static str>,
    sqlstate: Option<&'static str>,
    rows: u32,
    bytes: u64,
    pid: i64,
    start: String,
    original: String,
    executed: String,
    policy: String,
}

struct Out {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn x4(tag: &str) -> String {
    assert_eq!(tag.len(), 4);
    tag.repeat(16)
}

fn clean(fingerprint: &str) -> Spec {
    Spec {
        fingerprint: fingerprint.to_owned(),
        role: "oo_r_rev".to_owned(),
        project: "cite273".to_owned(),
        status: "succeeded",
        truncated: false,
        rollback: true,
        dropped: false,
        stop: None,
        sqlstate: None,
        rows: 273_273,
        bytes: 273_273_273,
        pid: 273_273,
        start: "202610060273273".to_owned(),
        original: x4("a273"),
        executed: x4("b273"),
        policy: x4("c273"),
    }
}

fn local() -> Bind {
    Bind {
        session: "sess273",
        turn: "turn273",
        role: "oo_r_rev",
        project: "cite273",
        protocol: PROTO,
    }
}

fn pair_lines(spec: &Spec) -> Vec<Value> {
    let admit = json!({
        "protocol": PROTO,
        "record": "admit",
        "original_sha256": &spec.original,
        "executed_sha256": &spec.executed,
        "fingerprint_hex": &spec.fingerprint,
        "role": &spec.role,
        "project": &spec.project,
        "policy_id": &spec.policy,
        "backend_pid": spec.pid,
        "backend_start": &spec.start,
    });
    let complete = json!({
        "protocol": PROTO,
        "record": "complete",
        "status": spec.status,
        "truncated": spec.truncated,
        "returned_rows": spec.rows,
        "result_bytes": spec.bytes,
        "rollback_confirmed": spec.rollback,
        "socket_dropped": spec.dropped,
        "stop": spec.stop,
        "sqlstate": spec.sqlstate,
        "original_sha256": &spec.original,
        "executed_sha256": &spec.executed,
        "fingerprint_hex": &spec.fingerprint,
        "role": &spec.role,
        "project": &spec.project,
        "policy_id": &spec.policy,
        "backend_pid": spec.pid,
        "backend_start": &spec.start,
    });
    vec![admit, complete]
}

fn encode(lines: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend(serde_json::to_vec(line).expect("line"));
        out.push(b'\n');
    }
    out
}

fn request(bind: Bind, bodies: &[Vec<u8>], declares: bool, text: &str) -> Vec<u8> {
    let receipts: Vec<Value> = bodies
        .iter()
        .map(|body| {
            json!({
                "receipt_sha256": sha256_hex(body),
                "body": std::str::from_utf8(body).expect("body utf8"),
            })
        })
        .collect();
    serde_json::to_vec(&json!({
        "version": "oo.answer-citations/v1",
        "trust": "local_operator_declared",
        "binding": {
            "session_id": bind.session,
            "turn_id": bind.turn,
            "role": bind.role,
            "project": bind.project,
            "protocol": bind.protocol,
        },
        "receipts": receipts,
        "answer": {"declares_data": declares, "text": text}
    }))
    .expect("request")
}

fn binary() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_answer_citations")
        .filter(|path| !path.is_empty())
    {
        return std::path::PathBuf::from(path);
    }
    let mut path = std::env::current_exe().expect("current_exe");
    if path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        == Some("deps")
    {
        path.pop();
        path.pop();
    } else {
        path.pop();
    }
    path.push("answer-citations");
    path
}

fn run(args: &[&str], input: &[u8]) -> Out {
    let mut child = Command::new(binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("PGPASSWORD")
        .env_remove("DATABASE_URL")
        .env_remove("PGHOST")
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(input).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait");
    Out {
        code: output.status.code().expect("exit code"),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn assert_error(out: &Out, code: i32, token: &str) {
    assert_eq!(out.code, code);
    assert!(out.stdout.is_empty());
    assert_eq!(
        out.stderr,
        format!("answer-citations: {token}\n").as_bytes()
    );
}

fn parse(out: &Out) -> Value {
    assert!(out.stdout.ends_with(b"\n"));
    assert_eq!(out.stdout.iter().filter(|byte| **byte == b'\n').count(), 1);
    serde_json::from_slice(&out.stdout).expect("report json")
}

fn assert_frame(value: &Value, bind: Bind) {
    assert_eq!(value["version"], "oo.answer-citations/v1");
    assert_eq!(value["trust"], "local_operator_declared");
    assert_eq!(value["authenticated_audit"], false);
    assert_eq!(value["entailment"], "not_judged");
    assert_eq!(value["warning_comprehension"], "not_judged");
    assert_eq!(value["session_id"], bind.session);
    assert_eq!(value["turn_id"], bind.turn);
    assert_eq!(value["role"], bind.role);
    assert_eq!(value["project"], bind.project);
    assert_eq!(value["protocol"], PROTO);
    assert!(value.get("principal").is_none());
    assert!(value.get("query_id").is_none());
    assert!(value.get("returned_rows").is_none());
    assert!(value.get("result_bytes").is_none());
    assert!(value.get("policy_id").is_none());
    assert!(value.get("answer").is_none());
}

fn assert_sealed(stdout: &[u8], stderr: &[u8], tokens: &[&str]) {
    let out = std::str::from_utf8(stdout).expect("stdout utf8");
    let err = std::str::from_utf8(stderr).expect("stderr utf8");
    for token in tokens {
        assert!(!out.contains(token), "stdout leaked {token}");
        assert!(!err.contains(token), "stderr leaked {token}");
    }
}

fn assert_redacted_keys(raw: &str) {
    for key in [
        "\"returned_rows\"",
        "\"result_bytes\"",
        "\"policy_id\"",
        "\"backend_pid\"",
        "\"backend_start\"",
        "\"original_sha256\"",
        "\"executed_sha256\"",
        "\"sqlstate\"",
        "\"fingerprint_hex\"",
        "\"principal\"",
        "\"session\"",
    ] {
        assert!(!raw.contains(key), "report key {key}");
    }
}

fn top_keys(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut keys = Vec::new();
    let mut depth = 0_i32;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                index += 1;
            }
            b'"' if depth == 1 => {
                index += 1;
                let start = index;
                while index < bytes.len() && bytes[index] != b'"' {
                    index += 1;
                }
                let key = std::str::from_utf8(&bytes[start..index]).expect("key");
                index += 1;
                while index < bytes.len() && bytes[index] == b' ' {
                    index += 1;
                }
                if bytes.get(index) == Some(&b':') {
                    keys.push(key);
                }
            }
            _ => index += 1,
        }
    }
    keys
}

fn leaks(spec: &Spec) -> Vec<String> {
    vec![
        spec.original.clone(),
        spec.executed.clone(),
        spec.policy.clone(),
        spec.start.clone(),
        spec.rows.to_string(),
        spec.bytes.to_string(),
        spec.pid.to_string(),
        SECRET.to_owned(),
        CELL.to_owned(),
        "SELECT".to_owned(),
    ]
}

fn qid(spec: &Spec) -> String {
    format!("q_{}", spec.fingerprint)
}

#[test]
fn fixture_exact_hash_and_cli() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(FIXTURE.len(), 1041);
    assert_eq!(sha256_hex(FIXTURE), FIXTURE_SHA);
    let bind = Bind {
        session: "sess273",
        turn: "turn273",
        role: "oo_r_desk",
        project: "tickets",
        protocol: PROTO,
    };
    let text = format!("华东回款 128.45万元 [{QUERY}] {SECRET} {CELL} SELECT hidden");
    let out = run(&[], &request(bind, &[FIXTURE.to_vec()], true, &text));
    assert_eq!(out.code, 0);
    assert!(out.stderr.is_empty());
    let value = parse(&out);
    assert_frame(&value, bind);
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["declares_data"], true);
    assert_eq!(value["missing_citation"], false);
    assert_eq!(value["receipts"][0]["receipt_sha256"], FIXTURE_SHA);
    assert_eq!(value["receipts"][0]["status"], "bound");
    assert_eq!(value["receipts"][0]["ordinal"], 0);
    assert_eq!(value["admitted"][0]["query_id"], QUERY);
    assert_eq!(value["admitted"][0]["receipt_ordinal"], 0);
    assert_eq!(value["admitted"][0]["pair_ordinal"], 0);
    assert_eq!(value["admitted"][0]["status"], "succeeded");
    assert_eq!(value["admitted"][0]["truncated"], false);
    assert_eq!(value["citations"][0]["query_id"], QUERY);
    assert_eq!(value["citations"][0]["ordinal"], 0);
    assert_eq!(value["citations"][0]["classification"], "admitted");
    assert!(value["warnings"].as_array().expect("warnings").is_empty());
    assert!(value["disclosures"].as_array().expect("disclosures").is_empty());
    let raw = std::str::from_utf8(&out.stdout).expect("raw");
    assert_eq!(
        top_keys(raw),
        [
            "version",
            "trust",
            "authenticated_audit",
            "entailment",
            "warning_comprehension",
            "session_id",
            "turn_id",
            "role",
            "project",
            "protocol",
            "citation_status",
            "declares_data",
            "missing_citation",
            "receipts",
            "admitted",
            "citations",
            "warnings",
            "disclosures",
        ]
    );
    assert!(raw.contains(
        "{\"query_id\":\"q_836041cccc8287ad\",\"receipt_ordinal\":0,\"pair_ordinal\":0,\"status\":\"succeeded\",\"truncated\":false}"
    ));
    assert_redacted_keys(raw);
    assert_sealed(
        &out.stdout,
        &out.stderr,
        &[
            SECRET,
            CELL,
            "128.45",
            "华东",
            "SELECT",
            SQL_ORIGINAL,
            SQL_EXECUTED,
            POLICY_FIXTURE,
            BACKEND_START,
            "principal",
        ],
    );
}

#[test]
fn fresh_scope_uppercase_and_fingerprint_bounds() {
    let spec = clean("a273a273a273");
    let body = encode(&pair_lines(&spec));
    let bind = local();
    let text = format!("[{}] {SECRET} {CELL} SELECT", qid(&spec));
    let out = run(&[], &request(bind, std::slice::from_ref(&body), true, &text));
    assert_eq!(out.code, 0, "in-scope clean pair");
    assert!(out.stderr.is_empty());
    let value = parse(&out);
    assert_frame(&value, bind);
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["admitted"][0]["query_id"], qid(&spec));
    assert_eq!(value["admitted"][0]["truncated"], false);
    assert_eq!(value["citations"][0]["classification"], "admitted");
    let raw = std::str::from_utf8(&out.stdout).expect("raw");
    assert_redacted_keys(raw);
    let banned = leaks(&spec);
    let banned_refs: Vec<&str> = banned.iter().map(String::as_str).collect();
    assert_sealed(&out.stdout, &out.stderr, &banned_refs);

    let mut cross = bind;
    cross.role = "oo_r_other";
    let crossed = run(&[], &request(cross, std::slice::from_ref(&body), true, &text));
    assert_eq!(crossed.code, 1);
    assert_eq!(crossed.stderr, b"answer-citations: citations\n");
    let cross_value = parse(&crossed);
    assert_frame(&cross_value, cross);
    assert_eq!(cross_value["citation_status"], "invalid");
    assert_eq!(cross_value["citations"][0]["classification"], "crossdeclaredscope");
    assert!(cross_value["admitted"].as_array().expect("admitted").is_empty());
    assert_sealed(&crossed.stdout, &crossed.stderr, &["oo_r_rev"]);

    let mut other_project = bind;
    other_project.project = "other273";
    let projected = run(&[], &request(other_project, std::slice::from_ref(&body), true, &text));
    assert_eq!(projected.code, 1);
    let projected_value = parse(&projected);
    assert_frame(&projected_value, other_project);
    assert_eq!(
        projected_value["citations"][0]["classification"],
        "crossdeclaredscope"
    );
    assert!(projected_value["admitted"].as_array().expect("admitted").is_empty());
    assert_sealed(&projected.stdout, &projected.stderr, &["cite273"]);

    let upper = format!("[q_A273A273A273] {SECRET}");
    let upper_out = run(&[], &request(bind, &[body], true, &upper));
    assert_eq!(upper_out.code, 1);
    let upper_value = parse(&upper_out);
    assert_eq!(upper_value["admitted"][0]["query_id"], "q_a273a273a273");
    assert_eq!(upper_value["citations"][0]["query_id"], "q_A273A273A273");
    assert_eq!(upper_value["citations"][0]["classification"], "unknown");
    assert_eq!(upper_value["missing_citation"], true);
    assert_eq!(upper_value["admitted"][0]["query_id"], "q_a273a273a273");
    assert_eq!(upper_value["admitted"].as_array().expect("admitted").len(), 1);

    let short = clean("a273a");
    let short_body = encode(&pair_lines(&short));
    let short_out = run(
        &[],
        &request(bind, &[short_body], true, &format!("{SECRET} [q_a273a]")),
    );
    assert_eq!(short_out.code, 1);
    let short_value = parse(&short_out);
    assert_eq!(short_value["missing_citation"], true);
    assert!(short_value["citations"].as_array().expect("citations").is_empty());
    assert!(short_value["admitted"].as_array().expect("admitted").is_empty());

    let wide = "ab27".repeat(10);
    assert_eq!(wide.len(), 40);
    let wide_spec = clean(&wide);
    let wide_body = encode(&pair_lines(&wide_spec));
    let wide_text = format!("[{}] {SECRET}", qid(&wide_spec));
    let wide_out = run(&[], &request(bind, &[wide_body], true, &wide_text));
    assert_eq!(wide_out.code, 0);
    let wide_value = parse(&wide_out);
    assert_eq!(wide_value["admitted"][0]["query_id"], qid(&wide_spec));
    assert_eq!(wide_value["citations"][0]["classification"], "admitted");

    let narrow = clean("a273a2");
    let narrow_body = encode(&pair_lines(&narrow));
    let narrow_out = run(
        &[],
        &request(bind, &[narrow_body], true, &format!("[{}] {SECRET}", qid(&narrow))),
    );
    assert_eq!(narrow_out.code, 0);
    let narrow_value = parse(&narrow_out);
    assert_eq!(narrow_value["admitted"][0]["query_id"], "q_a273a2");

    let too_wide = "c".repeat(41);
    let too_spec = clean(&too_wide);
    let too_body = encode(&pair_lines(&too_spec));
    let too_out = run(&[], &request(bind, &[too_body], true, SECRET));
    assert_eq!(too_out.code, 1);
    let too_value = parse(&too_out);
    assert_eq!(too_value["missing_citation"], true);
    assert!(too_value["admitted"].as_array().expect("admitted").is_empty());
    assert_sealed(&too_out.stdout, &too_out.stderr, &[too_wide.as_str(), SECRET]);
}

#[test]
fn mismatched_identity_does_not_admit() {
    let spec = clean("a273a273a273");
    let bind = local();
    let mut lines = pair_lines(&spec);
    lines[1]["fingerprint_hex"] = json!("c273c273c273");
    let body = encode(&lines);
    let text = format!("[q_a273a273a273] [q_c273c273c273] {SECRET} SELECT");
    let out = run(&[], &request(bind, &[body], true, &text));
    assert_eq!(out.code, 1);
    let value = parse(&out);
    assert_frame(&value, bind);
    assert!(value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(value["citations"][0]["query_id"], "q_a273a273a273");
    assert_eq!(value["citations"][0]["classification"], "incomplete");
    assert_eq!(value["citations"][1]["query_id"], "q_c273c273c273");
    assert_eq!(value["citations"][1]["classification"], "unknown");
    assert_eq!(value["missing_citation"], true);
    let banned = leaks(&spec);
    let banned_refs: Vec<&str> = banned.iter().map(String::as_str).collect();
    assert_sealed(&out.stdout, &out.stderr, &banned_refs);

    let other_policy = x4("d273");
    let mut policy_lines = pair_lines(&spec);
    policy_lines[1]["policy_id"] = json!(&other_policy);
    let policy_out = run(
        &[],
        &request(bind, &[encode(&policy_lines)], true, "[q_a273a273a273]"),
    );
    assert_eq!(policy_out.code, 1);
    let policy_value = parse(&policy_out);
    assert!(policy_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(policy_value["citations"][0]["classification"], "incomplete");
    assert_sealed(&policy_out.stdout, &policy_out.stderr, &[other_policy.as_str()]);

    let mut pid_lines = pair_lines(&spec);
    pid_lines[1]["backend_pid"] = json!(273_274);
    let pid_out = run(
        &[],
        &request(bind, &[encode(&pid_lines)], true, "[q_a273a273a273]"),
    );
    assert_eq!(pid_out.code, 1);
    let pid_value = parse(&pid_out);
    assert!(pid_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(pid_value["citations"][0]["classification"], "incomplete");
    assert_sealed(&pid_out.stdout, &pid_out.stderr, &["273274"]);

    let mut role_lines = pair_lines(&spec);
    role_lines[1]["role"] = json!("oo_r_foreign");
    let role_out = run(
        &[],
        &request(bind, &[encode(&role_lines)], true, "[q_a273a273a273]"),
    );
    assert_eq!(role_out.code, 1);
    let role_value = parse(&role_out);
    assert_frame(&role_value, bind);
    assert!(role_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(role_value["citations"][0]["classification"], "incomplete");
    assert_sealed(&role_out.stdout, &role_out.stderr, &["oo_r_foreign"]);
}

#[test]
fn refused_pair_and_foreign_success_do_not_admit() {
    let mut refused = clean("a273a273a273");
    refused.status = "failed";
    refused.stop = Some("database");
    refused.sqlstate = Some("42501");
    refused.rollback = true;
    let bind = local();
    let body = encode(&pair_lines(&refused));
    let text = format!("[q_a273a273a273] {SECRET} SELECT");
    let out = run(&[], &request(bind, &[body], true, &text));
    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, b"answer-citations: citations\n");
    let value = parse(&out);
    assert_frame(&value, bind);
    assert!(value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(value["citations"][0]["classification"], "rejected");
    assert_eq!(value["warnings"][0]["code"], "rejected");
    assert_sealed(
        &out.stdout,
        &out.stderr,
        &[SECRET, "SELECT", "42501", "database", refused.original.as_str()],
    );

    let mut partial = clean("a273a273a273");
    partial.status = "partial";
    let partial_out = run(
        &[],
        &request(bind, &[encode(&pair_lines(&partial))], true, "[q_a273a273a273]"),
    );
    assert_eq!(partial_out.code, 1);
    let partial_value = parse(&partial_out);
    assert!(partial_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(partial_value["citations"][0]["classification"], "rejected");

    let mut dirty = clean("a273a273a273");
    dirty.sqlstate = Some("42501");
    let dirty_out = run(
        &[],
        &request(bind, &[encode(&pair_lines(&dirty))], true, "[q_a273a273a273]"),
    );
    assert_eq!(dirty_out.code, 1);
    let dirty_value = parse(&dirty_out);
    assert!(dirty_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(dirty_value["citations"][0]["classification"], "rejected");
    assert_sealed(&dirty_out.stdout, &dirty_out.stderr, &["42501"]);

    let mut foreign = clean("a273a273a273");
    foreign.role = "oo_r_foreign".to_owned();
    let mut both = pair_lines(&refused);
    both.extend(pair_lines(&foreign));
    let both_out = run(
        &[],
        &request(bind, &[encode(&both)], true, "[q_a273a273a273]"),
    );
    assert_eq!(both_out.code, 1);
    let both_value = parse(&both_out);
    assert!(both_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(both_value["citations"][0]["classification"], "rejected");
    assert_sealed(&both_out.stdout, &both_out.stderr, &["oo_r_foreign", "42501"]);
}

#[test]
fn truncation_disclosure_and_conflict() {
    let mut truncated = clean("a273a273a273");
    truncated.truncated = true;
    let bind = local();
    let text = format!("shown [q_a273a273a273] {SECRET}");
    let out = run(&[], &request(bind, &[encode(&pair_lines(&truncated))], true, &text));
    assert_eq!(out.code, 0);
    assert!(out.stderr.is_empty());
    let value = parse(&out);
    assert_frame(&value, bind);
    assert_eq!(value["citation_status"], "ok");
    assert_eq!(value["admitted"][0]["truncated"], true);
    assert_eq!(value["citations"][0]["classification"], "admitted");
    assert_eq!(value["warnings"][0]["code"], "querytruncated");
    assert_eq!(value["disclosures"][0]["code"], "querytruncated");
    assert_eq!(value["disclosures"][0]["required"], true);
    assert_eq!(value["disclosures"][0]["comprehended"], "not_judged");
    assert_sealed(&out.stdout, &out.stderr, &[SECRET, "shown"]);

    let mut lines = pair_lines(&clean("a273a273a273"));
    let mut again = clean("a273a273a273");
    again.truncated = true;
    lines.extend(pair_lines(&again));
    let conflict = run(&[], &request(bind, &[encode(&lines)], true, "[q_a273a273a273]"));
    assert_eq!(conflict.code, 1);
    let conflict_value = parse(&conflict);
    assert!(conflict_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(conflict_value["citations"][0]["classification"], "rejected");
    let codes: Vec<&str> = conflict_value["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|item| item["code"].as_str().expect("code"))
        .collect();
    assert_eq!(codes, ["duplicate", "rejected"]);
    assert!(conflict_value["disclosures"].as_array().expect("disclosures").is_empty());
}

#[test]
fn missing_citation_and_missing_complete() {
    let spec = clean("a273a273a273");
    let body = encode(&pair_lines(&spec));
    let bind = local();
    let missing = run(&[], &request(bind, std::slice::from_ref(&body), true, SECRET));
    assert_eq!(missing.code, 1);
    assert_eq!(missing.stderr, b"answer-citations: citations\n");
    let missing_value = parse(&missing);
    assert_frame(&missing_value, bind);
    assert_eq!(missing_value["citation_status"], "invalid");
    assert_eq!(missing_value["missing_citation"], true);
    assert_eq!(missing_value["declares_data"], true);
    assert!(missing_value["citations"].as_array().expect("citations").is_empty());
    assert_eq!(missing_value["admitted"][0]["query_id"], qid(&spec));
    assert_sealed(&missing.stdout, &missing.stderr, &[SECRET]);

    let empty = run(&[], &request(bind, std::slice::from_ref(&body), false, ""));
    assert_eq!(empty.code, 0);
    assert!(empty.stderr.is_empty());
    let empty_value = parse(&empty);
    assert_eq!(empty_value["citation_status"], "ok");
    assert_eq!(empty_value["missing_citation"], false);
    assert_eq!(empty_value["declares_data"], false);
    assert!(empty_value["citations"].as_array().expect("citations").is_empty());
    assert_eq!(empty_value["admitted"][0]["query_id"], qid(&spec));

    let admit_only = encode(&pair_lines(&spec)[..1]);
    let incomplete = run(
        &[],
        &request(bind, &[admit_only], true, "[q_a273a273a273]"),
    );
    assert_eq!(incomplete.code, 1);
    let incomplete_value = parse(&incomplete);
    assert!(incomplete_value["admitted"].as_array().expect("admitted").is_empty());
    assert_eq!(incomplete_value["citations"][0]["classification"], "incomplete");
    assert_eq!(incomplete_value["missing_citation"], true);

    let unknown = run(
        &[],
        &request(bind, &[body], false, "[q_0123456789ab]"),
    );
    assert_eq!(unknown.code, 1);
    let unknown_value = parse(&unknown);
    assert_eq!(unknown_value["citations"][0]["classification"], "unknown");
    assert_eq!(unknown_value["admitted"][0]["query_id"], qid(&spec));
    assert_eq!(unknown_value["missing_citation"], false);
}

#[test]
fn multiple_queries_and_repeated_fingerprint() {
    let first = clean("111111111111");
    let second = clean("222222222222");
    let mut lines = pair_lines(&first);
    lines.extend(pair_lines(&second));
    let bind = local();
    let text = format!(
        "[{second}] [{first}] [{second}] {SECRET}",
        first = qid(&first),
        second = qid(&second)
    );
    let out = run(&[], &request(bind, &[encode(&lines)], true, &text));
    assert_eq!(out.code, 0);
    assert!(out.stderr.is_empty());
    let value = parse(&out);
    assert_frame(&value, bind);
    assert_eq!(value["citations"][0]["query_id"], qid(&second));
    assert_eq!(value["citations"][0]["ordinal"], 0);
    assert_eq!(value["citations"][1]["query_id"], qid(&first));
    assert_eq!(value["citations"][1]["ordinal"], 1);
    assert_eq!(value["citations"].as_array().expect("citations").len(), 2);
    assert_eq!(value["admitted"][0]["query_id"], qid(&first));
    assert_eq!(value["admitted"][0]["pair_ordinal"], 0);
    assert_eq!(value["admitted"][1]["query_id"], qid(&second));
    assert_eq!(value["admitted"][1]["pair_ordinal"], 1);
    assert!(value["warnings"].as_array().expect("warnings").is_empty());
    assert_sealed(&out.stdout, &out.stderr, &[SECRET, first.original.as_str()]);

    let repeated = encode(&pair_lines(&first));
    let dup = run(
        &[],
        &request(
            bind,
            &[repeated.clone(), repeated],
            true,
            "[q_111111111111]",
        ),
    );
    assert_eq!(dup.code, 0);
    let dup_value = parse(&dup);
    assert_eq!(dup_value["admitted"].as_array().expect("admitted").len(), 1);
    assert_eq!(dup_value["admitted"][0]["receipt_ordinal"], 0);
    assert_eq!(dup_value["admitted"][0]["pair_ordinal"], 0);
    assert_eq!(dup_value["warnings"][0]["code"], "duplicate");
    assert_eq!(dup_value["citations"][0]["classification"], "admitted");
    assert_eq!(dup_value["receipts"].as_array().expect("receipts").len(), 2);

    let mut foreign = first.clone();
    foreign.role = "oo_r_foreign".to_owned();
    let scoped = run(
        &[],
        &request(
            bind,
            &[encode(&pair_lines(&foreign)), encode(&pair_lines(&first))],
            true,
            "[q_111111111111]",
        ),
    );
    assert_eq!(scoped.code, 0);
    let scoped_value = parse(&scoped);
    assert_eq!(scoped_value["admitted"][0]["receipt_ordinal"], 1);
    assert_eq!(scoped_value["admitted"][0]["pair_ordinal"], 1);
    assert_eq!(scoped_value["warnings"][0]["code"], "duplicate");
    assert_sealed(&scoped.stdout, &scoped.stderr, &["oo_r_foreign"]);

    let mut failed = first.clone();
    failed.status = "failed";
    failed.stop = Some("database");
    failed.sqlstate = Some("42501");
    let later = run(
        &[],
        &request(
            bind,
            &[encode(&pair_lines(&failed)), encode(&pair_lines(&first))],
            true,
            "[q_111111111111]",
        ),
    );
    assert_eq!(later.code, 0);
    let later_value = parse(&later);
    assert_eq!(later_value["admitted"][0]["receipt_ordinal"], 1);
    assert_eq!(later_value["admitted"][0]["pair_ordinal"], 1);
    assert_eq!(later_value["admitted"][0]["status"], "succeeded");
    assert_eq!(later_value["warnings"][0]["code"], "duplicate");
    assert_sealed(&later.stdout, &later.stderr, &["42501"]);
}

#[test]
fn strict_json_sqlstate_and_budget() {
    let spec = clean("a273a273a273");
    let body = encode(&pair_lines(&spec));
    let bind = local();
    let good = request(bind, &[body], true, &format!("[q_a273a273a273] {SECRET}"));

    let mut flipped = serde_json::from_slice::<Value>(&good).expect("good");
    let mut hash = sha256_hex(&encode(&pair_lines(&spec)));
    let replacement = if hash.ends_with('3') { "4" } else { "3" };
    hash.replace_range(63..64, replacement);
    flipped["receipts"][0]["receipt_sha256"] = json!(hash);
    let hash_out = run(&[], &serde_json::to_vec(&flipped).expect("flip"));
    assert_error(&hash_out, 4, "hash");
    assert_sealed(&hash_out.stdout, &hash_out.stderr, &[SECRET, "a273a273a273"]);

    let duplicate = br#"{"version":"oo.answer-citations/v1","version":"oo.answer-citations/v1","trust":"local_operator_declared","binding":{"session_id":"sess273","turn_id":"turn273","role":"oo_r_rev","project":"cite273","protocol":"oo.gateway.query-pg-audit/v1"},"receipts":[],"answer":{"declares_data":false,"text":""}}"#;
    assert_error(&run(&[], duplicate), 3, "request");

    let mut password = serde_json::from_slice::<Value>(&request(bind, &[], false, "")).expect("base");
    password["password"] = json!("SECRET_a273_PASSWORD");
    let password_out = run(&[], &serde_json::to_vec(&password).expect("password"));
    assert_error(&password_out, 3, "request");
    assert_sealed(&password_out.stdout, &password_out.stderr, &["SECRET_a273_PASSWORD"]);

    let mut trust = serde_json::from_slice::<Value>(&request(bind, &[], false, "")).expect("base");
    trust["trust"] = json!("authenticated");
    assert_error(&run(&[], &serde_json::to_vec(&trust).expect("trust")), 4, "binding");

    let mut protocol = local();
    protocol.protocol = "oo.gateway.query-pg-audit/v0";
    assert_error(&run(&[], &request(protocol, &[], false, "")), 4, "binding");

    let mut session_lines = pair_lines(&spec);
    session_lines[0]["session"] = json!("sess273");
    session_lines[1]["session"] = json!("sess273");
    assert_error(
        &run(&[], &request(bind, &[encode(&session_lines)], true, "[q_a273a273a273]")),
        5,
        "receipt",
    );

    let mut query_lines = pair_lines(&spec);
    query_lines[0]["query_id"] = json!("q_a273a273a273");
    query_lines[1]["query_id"] = json!("q_a273a273a273");
    let injected = run(
        &[],
        &request(bind, &[encode(&query_lines)], true, "[q_a273a273a273]"),
    );
    assert_error(&injected, 5, "receipt");
    assert_sealed(&injected.stdout, &injected.stderr, &["q_a273a273a273", SECRET]);

    let dup_line = r#"{"protocol":"oo.gateway.query-pg-audit/v1","record":"admit","record":"admit"}"#;
    let dup_body = format!("{dup_line}\n");
    assert_error(
        &run(&[], &request(bind, &[dup_body.into_bytes()], false, "")),
        5,
        "receipt",
    );

    let mut short_state = clean("a273a273a273");
    short_state.sqlstate = Some("4250");
    assert_error(
        &run(&[], &request(bind, &[encode(&pair_lines(&short_state))], true, "[q_a273a273a273]")),
        5,
        "receipt",
    );
    let mut lower_state = clean("a273a273a273");
    lower_state.sqlstate = Some("42p01");
    assert_error(
        &run(&[], &request(bind, &[encode(&pair_lines(&lower_state))], true, "[q_a273a273a273]")),
        5,
        "receipt",
    );

    let mut text = String::new();
    for index in 0..33 {
        text.push_str(&format!("[q_{index:06x}]"));
    }
    assert_error(&run(&[], &request(bind, &[], false, &text)), 3, "budget");

    assert_error(&run(&["--nope"], b""), 2, "usage");
    assert_error(&run(&["--help", "extra"], b""), 2, "usage");
    assert_error(&run(&[], b""), 3, "request");
}

#[test]
fn help_states_local_operator_scope() {
    let out = run(&["--help"], b"");
    assert_eq!(out.code, 0);
    assert!(out.stderr.is_empty());
    let help = String::from_utf8(out.stdout).expect("help");
    assert!(help.contains("local_operator_declared"));
    assert!(help.contains("no session, principal"));
    assert!(help.contains("does not judge"));
    assert!(help.contains("local query-pg receipts"));
    assert!(!help.contains(SECRET));
    assert!(!help.contains("authenticated_audit\":true"));
}
