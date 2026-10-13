use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::error::Fault;
use crate::grammar::{caller_label, hex64, in_range, knowledge_id_ok};
use crate::json::{self, JsonError};
use crate::pack;
use crate::propose::{proposal_preimage_sha, tuple_line, HashParts};
use crate::read::{self, ReadFail, AUDIT_MAX, BINARY_MAX, DOC_MAX, PACK_MAX};
use crate::sha::{push_string, sha256_hex};

const EXECUTE_PROTOCOL: &str = "oo.ontology-dialogue.execute-config/v1";
const APPROVAL_PROTOCOL: &str = "oo.ontology-dialogue.approval/v1";
const PROPOSAL_PROTOCOL: &str = "oo.ontology-dialogue.proposal/v1";
const REQUEST_VERSION: &str = "oo.gateway.query-pg/v1";
const AUDIT_PROTOCOL: &str = "oo.gateway.query-pg-audit/v1";
const CHILD_TOKENS: [&str; 8] = [
    "usage", "request", "tls", "policy", "guard", "audit", "query", "io",
];

#[derive(Debug)]
pub struct Finished {
    pub exit: u8,
    pub stdout: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteConfig {
    protocol: String,
    proposal_file: String,
    approval_file: String,
    pack_file: String,
    request_file: String,
    query_pg: QueryPg,
    binding: ExecBinding,
    budget: ExecBudget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryPg {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecBinding {
    session_id: String,
    turn_id: String,
    declares_data: bool,
    render_rows: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecBudget {
    deadline_ms: u64,
    max_stdout_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalDoc {
    protocol: String,
    trust: String,
    authenticated: bool,
    model_sql: String,
    auto_execute: bool,
    question_sha256: String,
    role: String,
    pack_digest: String,
    manifest_sha256: String,
    project: String,
    policy_id: String,
    policy_version: String,
    knowledge_id: String,
    knowledge_version: String,
    as_of: String,
    tuple_sha256: String,
    sql: String,
    sql_sha256: String,
    proposal_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalDoc {
    protocol: String,
    trust: String,
    authenticated: bool,
    proposal_sha256: String,
    sql_sha256: String,
    tuple_sha256: String,
    pack_digest: String,
}

struct Prepared {
    proposal: ProposalDoc,
    request_path: String,
    audit_path: String,
    binary: String,
    session_id: String,
    turn_id: String,
    declares_data: bool,
    render_rows: bool,
    deadline_ms: u64,
    max_stdout: usize,
    sql_sha256: String,
    proposal_sha256: String,
}

fn proposal_declared(proposal: &ProposalDoc) -> bool {
    proposal.protocol == PROPOSAL_PROTOCOL
        && proposal.trust == "local_operator_declared"
        && !proposal.authenticated
        && proposal.model_sql == "untrusted"
        && !proposal.auto_execute
        && hex64(&proposal.policy_id)
        && knowledge_id_ok(&proposal.knowledge_id)
}

fn approval_declared(approval: &ApprovalDoc) -> bool {
    approval.protocol == APPROVAL_PROTOCOL
        && approval.trust == "local_operator_declared"
        && !approval.authenticated
}

fn pack_matches_proposal(pack: &pack::BoundPack, proposal: &ProposalDoc) -> bool {
    pack.role == proposal.role
        && pack.project == proposal.project
        && pack.policy_id == proposal.policy_id
        && pack.policy_version == proposal.policy_version
        && pack.knowledge_id == proposal.knowledge_id
        && pack.knowledge_version == proposal.knowledge_version
        && pack.manifest_sha256 == proposal.manifest_sha256
        && pack.pack_digest == proposal.pack_digest
}

fn approval_hashes_match(
    proposal: &ProposalDoc,
    approval: &ApprovalDoc,
    pack_digest: &str,
    sql_sha256: &str,
    tuple_sha256: &str,
    proposal_sha256: &str,
) -> bool {
    proposal.sql_sha256 == sql_sha256
        && proposal.tuple_sha256 == tuple_sha256
        && proposal.proposal_sha256 == proposal_sha256
        && proposal.pack_digest == pack_digest
        && approval.sql_sha256 == sql_sha256
        && approval.tuple_sha256 == tuple_sha256
        && approval.proposal_sha256 == proposal_sha256
        && approval.pack_digest == pack_digest
}

pub fn execute(config_text: &str) -> Result<Finished, Fault> {
    let prepared = prepare(config_text)?;
    let child = spawn_child(&prepared)?;
    Ok(finish(&prepared, child))
}

fn prepare(config_text: &str) -> Result<Prepared, Fault> {
    if config_text.len() > DOC_MAX {
        return Err(Fault::config());
    }
    let config = parse_config(config_text)?;
    let proposal = read_doc(&config.proposal_file, DOC_MAX, Fault::approval)?;
    let approval = read_doc(&config.approval_file, DOC_MAX, Fault::approval)?;
    let pack_bytes = read_doc_bytes(&config.pack_file, PACK_MAX, Fault::approval)?;
    let request_bytes = read_doc_bytes(&config.request_file, DOC_MAX, Fault::approval)?;
    let proposal: ProposalDoc = parse_strict(&proposal, Fault::approval)?;
    let approval: ApprovalDoc = parse_strict(&approval, Fault::approval)?;
    let pack = match pack::load_pack(&pack_bytes) {
        Ok(pack) => pack,
        Err(fault) if fault.token == "budget" => return Err(fault),
        Err(_) => return Err(Fault::approval()),
    };
    if !proposal_declared(&proposal) || !approval_declared(&approval) {
        return Err(Fault::approval());
    }
    if !pack_matches_proposal(&pack, &proposal) {
        return Err(Fault::approval());
    }
    let sql_sha256 = sha256_hex(proposal.sql.as_bytes());
    let parts = HashParts {
        role: &proposal.role,
        project: &proposal.project,
        policy_id: &proposal.policy_id,
        policy_version: &proposal.policy_version,
        knowledge_id: &proposal.knowledge_id,
        knowledge_version: &proposal.knowledge_version,
        as_of: &proposal.as_of,
        pack_digest: &pack.pack_digest,
        manifest_sha256: &pack.manifest_sha256,
        question_sha256: &proposal.question_sha256,
        sql_sha256: &sql_sha256,
    };
    let tuple_sha256 = sha256_hex(tuple_line(&parts).as_bytes());
    let proposal_sha256 = proposal_preimage_sha(&parts, &tuple_sha256);
    if !approval_hashes_match(
        &proposal,
        &approval,
        &pack.pack_digest,
        &sql_sha256,
        &tuple_sha256,
        &proposal_sha256,
    ) {
        return Err(Fault::approval());
    }
    let request = parse_request(&request_bytes)?;
    if request.sql != proposal.sql
        || request.role != proposal.role
        || request.project != proposal.project
        || request.policy_id != proposal.policy_id
    {
        return Err(Fault::approval());
    }
    let binary_bytes = read_doc_bytes(&config.query_pg.path, BINARY_MAX, Fault::binary)?;
    if sha256_hex(&binary_bytes) != config.query_pg.sha256 {
        return Err(Fault::binary());
    }
    Ok(Prepared {
        proposal,
        request_path: config.request_file,
        audit_path: request.audit_file,
        binary: config.query_pg.path,
        session_id: config.binding.session_id,
        turn_id: config.binding.turn_id,
        declares_data: config.binding.declares_data,
        render_rows: config.binding.render_rows,
        deadline_ms: config.budget.deadline_ms,
        max_stdout: usize::try_from(config.budget.max_stdout_bytes).map_err(|_| Fault::budget())?,
        sql_sha256,
        proposal_sha256,
    })
}

struct RequestView {
    sql: String,
    role: String,
    project: String,
    policy_id: String,
    audit_file: String,
}

fn parse_request(bytes: &[u8]) -> Result<RequestView, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::approval())?;
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::approval()),
    };
    if value.get("version").and_then(Value::as_str) != Some(REQUEST_VERSION) {
        return Err(Fault::approval());
    }
    let password = value.get("password_file").and_then(Value::as_str).ok_or_else(Fault::approval)?;
    if password.is_empty() {
        return Err(Fault::approval());
    }
    let audit_file = value
        .get("audit_file")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(Fault::approval)?
        .to_string();
    let identity = value.get("identity").ok_or_else(Fault::approval)?;
    let role = identity.get("role").and_then(Value::as_str).ok_or_else(Fault::approval)?;
    let project = identity.get("project").and_then(Value::as_str).ok_or_else(Fault::approval)?;
    let policy_id = identity.get("policy_id").and_then(Value::as_str).ok_or_else(Fault::approval)?;
    let statement = value.get("statement").ok_or_else(Fault::approval)?;
    if statement.get("source").and_then(Value::as_str) != Some("request") {
        return Err(Fault::approval());
    }
    let sql = statement.get("sql").and_then(Value::as_str).ok_or_else(Fault::approval)?;
    Ok(RequestView {
        sql: sql.to_string(),
        role: role.to_string(),
        project: project.to_string(),
        policy_id: policy_id.to_string(),
        audit_file,
    })
}

struct ChildOut {
    code: Option<i32>,
    killed: bool,
    stdout: Vec<u8>,
    stdout_overflow: bool,
    stderr: Vec<u8>,
}

fn spawn_child(prepared: &Prepared) -> Result<ChildOut, Fault> {
    let mut command = Command::new(&prepared.binary);
    command
        .arg("--request")
        .arg(&prepared.request_path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir("/tmp");
    let mut child = command.spawn().map_err(|_| Fault::io())?;
    let stdout = child.stdout.take().ok_or_else(Fault::io)?;
    let stderr = child.stderr.take().ok_or_else(Fault::io)?;
    let cap = prepared.max_stdout;
    let out_thread = thread::spawn(move || drain(stdout, cap));
    let err_thread = thread::spawn(move || drain(stderr, 1024));
    let started = Instant::now();
    let deadline = Duration::from_millis(prepared.deadline_ms);
    let mut killed = false;
    let mut code = None;
    loop {
        if started.elapsed() >= deadline {
            let _ = child.kill();
            killed = true;
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                code = status.code();
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                killed = true;
                break;
            }
        }
    }
    let _ = child.wait();
    let (stdout, stdout_overflow) = out_thread.join().unwrap_or((Vec::new(), true));
    let (stderr, _) = err_thread.join().unwrap_or((Vec::new(), true));
    Ok(ChildOut {
        code: if killed { None } else { code },
        killed,
        stdout,
        stdout_overflow,
        stderr,
    })
}

fn drain(mut pipe: impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut buf = [0_u8; 8192];
    let mut overflow = false;
    loop {
        let read = match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                overflow = true;
                break;
            }
        };
        if kept.len() >= cap {
            overflow = true;
            continue;
        }
        let room = cap - kept.len();
        if read > room {
            kept.extend_from_slice(&buf[..room]);
            overflow = true;
        } else {
            kept.extend_from_slice(&buf[..read]);
        }
    }
    (kept, overflow)
}

fn finish(prepared: &Prepared, child: ChildOut) -> Finished {
    let child_token = child_token(&child.stderr);
    let parsed = if child.stdout_overflow {
        None
    } else {
        parse_stdout(&child.stdout)
    };
    let audit = read_audit(&prepared.audit_path);
    let mut rendered = None;
    let mut citation_bytes = None;
    let mut citation_value = None;
    let mut validate_fault = false;
    if let AuditRead::Body(body) = &audit {
        let text = render_answer(prepared, parsed.as_ref(), body);
        match cite(prepared, body, &text) {
            Ok((bytes, value)) => {
                rendered = Some(text);
                citation_bytes = Some(bytes);
                citation_value = Some(value);
            }
            Err(()) => validate_fault = true,
        }
    }
    let receipt = classify(prepared, &child, parsed.as_ref(), &audit, citation_value.as_ref());
    let query_id = citation_value.as_ref().and_then(one_admitted_id);
    let exit = decide_exit(&child, &receipt, citation_value.as_ref(), validate_fault);
    let stdout = execution_json(&ExecutionDoc {
        child: &child,
        child_token: child_token.as_deref(),
        receipt: &receipt,
        query_id: query_id.as_deref(),
        sql_sha256: &prepared.sql_sha256,
        proposal_sha256: &prepared.proposal_sha256,
        parsed: parsed.as_ref(),
        render_rows: prepared.render_rows,
        rendered: rendered.as_deref(),
        citation: citation_bytes.as_deref(),
    });
    Finished { exit, stdout }
}

enum AuditRead {
    Absent,
    Unusable,
    Body(String),
}

fn read_audit(path: &str) -> AuditRead {
    match read::read_regular(Path::new(path), AUDIT_MAX) {
        Err(ReadFail::Missing) => AuditRead::Absent,
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) if !text.is_empty() => AuditRead::Body(text),
            _ => AuditRead::Unusable,
        },
        Err(_) => AuditRead::Unusable,
    }
}

struct ParsedStdout {
    summary_raw: String,
    status: String,
    truncated: bool,
    end_audit_ok: bool,
    rows: Vec<Vec<CellOut>>,
}

#[derive(Clone)]
struct CellOut {
    kind: String,
    value: Option<String>,
    bool_value: Option<bool>,
}

fn parse_stdout(bytes: &[u8]) -> Option<ParsedStdout> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() || lines.iter().any(|line| line.is_empty()) {
        return None;
    }
    let summary_raw = lines.pop()?.to_string();
    let summary = json::parse_json(&summary_raw).ok()?;
    if summary.get("record").and_then(Value::as_str) != Some("summary") {
        return None;
    }
    let status = summary.get("status").and_then(Value::as_str)?.to_string();
    let truncated = summary.get("truncated").and_then(Value::as_bool)?;
    let end_audit_ok = summary.get("end_audit_ok").and_then(Value::as_bool)?;
    let mut rows = Vec::new();
    for line in lines {
        let row = json::parse_json(line).ok()?;
        if row.get("record").and_then(Value::as_str) != Some("row") {
            return None;
        }
        let cells = row.get("cells").and_then(Value::as_array)?;
        let mut parsed = Vec::new();
        for cell in cells {
            parsed.push(parse_cell(cell)?);
        }
        rows.push(parsed);
    }
    Some(ParsedStdout {
        summary_raw,
        status,
        truncated,
        end_audit_ok,
        rows,
    })
}

fn parse_cell(value: &Value) -> Option<CellOut> {
    let kind = value.get("kind").and_then(Value::as_str)?.to_string();
    match kind.as_str() {
        "null" => Some(CellOut {
            kind,
            value: None,
            bool_value: None,
        }),
        "bool" => Some(CellOut {
            kind,
            value: None,
            bool_value: Some(value.get("value")?.as_bool()?),
        }),
        "int" | "text" => Some(CellOut {
            kind,
            value: Some(value.get("value")?.as_str()?.to_string()),
            bool_value: None,
        }),
        _ => None,
    }
}

fn render_answer(prepared: &Prepared, parsed: Option<&ParsedStdout>, audit: &str) -> String {
    let mut lines = Vec::new();
    if let Some(parsed) = parsed {
        lines.push(format!("status {status}", status = parsed.status));
        lines.push(format!("truncated {truncated}", truncated = parsed.truncated));
        if prepared.render_rows {
            for row in &parsed.rows {
                for cell in row {
                    lines.push(render_cell(cell));
                }
            }
        }
    }
    if let Some(fingerprint) = admit_fingerprint(audit) {
        lines.push(format!("[q_{fingerprint}]"));
    }
    lines.join("\n")
}

fn render_cell(cell: &CellOut) -> String {
    match cell.kind.as_str() {
        "null" => "cell null".to_string(),
        "bool" => format!("cell bool {}", cell.bool_value.unwrap_or(false)),
        "int" | "text" => {
            let kind = cell.kind.as_str();
            let value = cell.value.as_deref().unwrap_or("");
            format!("cell {kind} {value}")
        }
        _ => format!("cell {kind}", kind = cell.kind),
    }
}

fn admit_fingerprint(audit: &str) -> Option<String> {
    for line in audit.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = json::parse_json(line) else {
            continue;
        };
        if value.get("record").and_then(Value::as_str) != Some("admit") {
            continue;
        }
        let fingerprint = value.get("fingerprint_hex").and_then(Value::as_str)?;
        if (6..=40).contains(&fingerprint.len()) && hex64_loose(fingerprint) {
            return Some(fingerprint.to_string());
        }
    }
    None
}

fn hex64_loose(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn cite(prepared: &Prepared, audit: &str, answer: &str) -> Result<(Vec<u8>, Value), ()> {
    let document = serde_json::json!({
        "version": "oo.answer-citations/v1",
        "trust": "local_operator_declared",
        "binding": {
            "session_id": prepared.session_id,
            "turn_id": prepared.turn_id,
            "role": prepared.proposal.role,
            "project": prepared.proposal.project,
            "protocol": AUDIT_PROTOCOL
        },
        "receipts": [{
            "receipt_sha256": sha256_hex(audit.as_bytes()),
            "body": audit
        }],
        "answer": {
            "declares_data": prepared.declares_data,
            "text": answer
        }
    });
    let bytes = serde_json::to_vec(&document).map_err(|_| ())?;
    let outcome = answer_citations::validate(&bytes).map_err(|_| ())?;
    let report = outcome.json().map_err(|_| ())?;
    let value = serde_json::from_slice(&report).map_err(|_| ())?;
    Ok((report, value))
}

fn classify(
    _prepared: &Prepared,
    child: &ChildOut,
    parsed: Option<&ParsedStdout>,
    audit: &AuditRead,
    citation: Option<&Value>,
) -> String {
    match audit {
        AuditRead::Absent => return "absent".to_string(),
        AuditRead::Unusable => return "unknown".to_string(),
        AuditRead::Body(_) => {}
    }
    if child.stdout_overflow || parsed.is_none() {
        return "unknown".to_string();
    }
    let parsed = parsed.expect("parsed");
    if citation_class(citation, "incomplete") {
        return "incomplete".to_string();
    }
    let failed_child = matches!(child.code, Some(code) if code != 0);
    if parsed.status == "failed" || (failed_child && child.code != Some(6) && !child.killed) {
        return "failed".to_string();
    }
    let citation_ok = citation.and_then(|value| value.get("citation_status")).and_then(Value::as_str) == Some("ok");
    if child.code == Some(0)
        && parsed.status == "succeeded"
        && !parsed.truncated
        && parsed.end_audit_ok
        && citation_ok
    {
        return "clean".to_string();
    }
    if child.code == Some(0)
        && parsed.status == "succeeded"
        && parsed.truncated
        && parsed.end_audit_ok
        && citation_ok
        && disclosure(citation, "querytruncated")
    {
        return "truncated".to_string();
    }
    "unknown".to_string()
}

fn citation_class(citation: Option<&Value>, class: &str) -> bool {
    citation
        .and_then(|value| value.get("citations"))
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("classification").and_then(Value::as_str) == Some(class))
        })
}

fn disclosure(citation: Option<&Value>, code: &str) -> bool {
    citation
        .and_then(|value| value.get("disclosures"))
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("code").and_then(Value::as_str) == Some(code))
        })
}

fn one_admitted_id(citation: &Value) -> Option<String> {
    if citation.get("citation_status").and_then(Value::as_str) != Some("ok") {
        return None;
    }
    let admitted = citation.get("admitted").and_then(Value::as_array)?;
    if admitted.len() != 1 {
        return None;
    }
    admitted[0].get("query_id").and_then(Value::as_str).map(str::to_string)
}

fn decide_exit(child: &ChildOut, receipt: &str, citation: Option<&Value>, validate_fault: bool) -> u8 {
    if child.killed {
        return 7;
    }
    if child.code == Some(6) {
        return 6;
    }
    if receipt == "clean" || receipt == "truncated" {
        return 0;
    }
    let succeeded = citation.is_some();
    let status = citation.and_then(|value| value.get("citation_status")).and_then(Value::as_str);
    if child.code == Some(0) && (validate_fault || (succeeded && status == Some("invalid") && receipt != "failed")) {
        return 8;
    }
    if validate_fault && child.code == Some(0) {
        return 8;
    }
    7
}

fn child_token(stderr: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stderr).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let token = text.strip_prefix("query-pg: ")?;
    if CHILD_TOKENS.contains(&token) && !token.contains('\n') {
        Some(token.to_string())
    } else {
        None
    }
}

struct ExecutionDoc<'a> {
    child: &'a ChildOut,
    child_token: Option<&'a str>,
    receipt: &'a str,
    query_id: Option<&'a str>,
    sql_sha256: &'a str,
    proposal_sha256: &'a str,
    parsed: Option<&'a ParsedStdout>,
    render_rows: bool,
    rendered: Option<&'a str>,
    citation: Option<&'a [u8]>,
}

fn execution_json(doc: &ExecutionDoc<'_>) -> Vec<u8> {
    let child = doc.child;
    let child_token = doc.child_token;
    let receipt = doc.receipt;
    let query_id = doc.query_id;
    let sql_sha256 = doc.sql_sha256;
    let proposal_sha256 = doc.proposal_sha256;
    let parsed = doc.parsed;
    let render_rows = doc.render_rows;
    let rendered = doc.rendered;
    let citation = doc.citation;
    let mut buf = Vec::new();
    buf.push(b'{');
    field_str(&mut buf, true, "protocol", "oo.ontology-dialogue.execution/v1");
    field_str(&mut buf, false, "trust", "local_operator_declared");
    field_raw(&mut buf, "authenticated_audit", b"false");
    field_str(&mut buf, false, "entailment", "not_judged");
    field_str(&mut buf, false, "warning_comprehension", "not_judged");
    field_raw(&mut buf, "auto_execute", b"false");
    field_raw(&mut buf, "spawned", b"true");
    match child.code {
        Some(code) if (0..=255).contains(&code) && !child.killed => {
            field_raw(&mut buf, "child_exit", code.to_string().as_bytes());
        }
        _ => field_raw(&mut buf, "child_exit", b"null"),
    }
    match child_token {
        Some(token) => field_str(&mut buf, false, "child_token", token),
        None => field_raw(&mut buf, "child_token", b"null"),
    }
    field_str(&mut buf, false, "query_receipt", receipt);
    match query_id {
        Some(id) => field_str(&mut buf, false, "query_id", id),
        None => field_raw(&mut buf, "query_id", b"null"),
    }
    field_str(&mut buf, false, "sql_sha256", sql_sha256);
    field_str(&mut buf, false, "proposal_sha256", proposal_sha256);
    match parsed {
        Some(parsed) => field_raw(&mut buf, "summary", parsed.summary_raw.as_bytes()),
        None => field_raw(&mut buf, "summary", b"null"),
    }
    if render_rows {
        if let Some(parsed) = parsed {
            field_raw(&mut buf, "rows", &rows_json(&parsed.rows));
        } else {
            field_raw(&mut buf, "rows", b"null");
        }
    } else {
        field_raw(&mut buf, "rows", b"null");
    }
    match rendered {
        Some(text) => field_str(&mut buf, false, "rendered", text),
        None => field_raw(&mut buf, "rendered", b"null"),
    }
    match citation {
        Some(bytes) => field_raw(&mut buf, "citation", bytes),
        None => field_raw(&mut buf, "citation", b"null"),
    }
    buf.push(b'}');
    buf.push(b'\n');
    buf
}

fn rows_json(rows: &[Vec<CellOut>]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(b'[');
    for (row_index, row) in rows.iter().enumerate() {
        if row_index > 0 {
            buf.push(b',');
        }
        buf.push(b'[');
        for (cell_index, cell) in row.iter().enumerate() {
            if cell_index > 0 {
                buf.push(b',');
            }
            buf.push(b'{');
            field_str(&mut buf, true, "kind", &cell.kind);
            match cell.kind.as_str() {
                "bool" => {
                    let raw = if cell.bool_value == Some(true) { b"true".as_slice() } else { b"false".as_slice() };
                    field_raw(&mut buf, "value", raw);
                }
                "int" | "text" => field_str(&mut buf, false, "value", cell.value.as_deref().unwrap_or("")),
                _ => {}
            }
            buf.push(b'}');
        }
        buf.push(b']');
    }
    buf.push(b']');
    buf
}

fn field_str(buf: &mut Vec<u8>, first: bool, key: &str, value: &str) {
    if !first {
        buf.push(b',');
    }
    let mut tmp = String::new();
    push_string(&mut tmp, key);
    buf.extend(tmp.as_bytes());
    buf.push(b':');
    tmp.clear();
    push_string(&mut tmp, value);
    buf.extend(tmp.as_bytes());
}

fn field_raw(buf: &mut Vec<u8>, key: &str, raw: &[u8]) {
    buf.push(b',');
    let mut tmp = String::new();
    push_string(&mut tmp, key);
    buf.extend(tmp.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(raw);
}

fn parse_config(text: &str) -> Result<ExecuteConfig, Fault> {
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::config()),
    };
    if json::contains_null(&value) {
        return Err(Fault::config());
    }
    let config: ExecuteConfig = serde_json::from_value(value).map_err(|_| Fault::config())?;
    if config.protocol != EXECUTE_PROTOCOL
        || !caller_label(&config.binding.session_id)
        || !caller_label(&config.binding.turn_id)
        || !hex64(&config.query_pg.sha256)
        || config.query_pg.path.is_empty()
        || !in_range(config.budget.deadline_ms, 1, 30_000)
        || !in_range(config.budget.max_stdout_bytes, 1, 1_048_576)
    {
        return Err(Fault::config());
    }
    Ok(config)
}

fn parse_strict<T: for<'de> Deserialize<'de>>(bytes: &[u8], fault: fn() -> Fault) -> Result<T, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| fault())?;
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(fault()),
    };
    if json::contains_null(&value) {
        return Err(fault());
    }
    serde_json::from_value(value).map_err(|_| fault())
}

fn read_doc(path: &str, max: usize, fault: fn() -> Fault) -> Result<Vec<u8>, Fault> {
    read_doc_bytes(path, max, fault)
}

fn read_doc_bytes(path: &str, max: usize, fault: fn() -> Fault) -> Result<Vec<u8>, Fault> {
    if path.is_empty() {
        return Err(fault());
    }
    match read::read_regular(Path::new(path), max) {
        Ok(bytes) => Ok(bytes),
        Err(ReadFail::Io) => Err(Fault::io()),
        Err(_) => Err(fault()),
    }
}
