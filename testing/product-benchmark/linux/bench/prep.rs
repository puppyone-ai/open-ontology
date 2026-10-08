//! Prepare producer inputs from real chain files. No model calls.

#[allow(dead_code)]
#[path = "/crate/src/json.rs"]
mod json;
#[allow(dead_code)]
#[path = "/crate/src/sha256.rs"]
mod sha256;

use std::env;
use std::fs;
use std::process::ExitCode;

use json::Val;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("pack") if args.len() == 3 => pack(&args[1], &args[2]),
        Some("propose-config") if args.len() == 8 => {
            propose_config(&args[1], &args[2], &args[3], &args[4], &args[5], &args[6], &args[7])
        }
        Some("approval") if args.len() == 3 => approval(&args[1], &args[2]),
        Some("request") if args.len() == 6 => request(&args[1], &args[2], &args[3], &args[4], &args[5]),
        Some("cite") if args.len() == 5 => cite(&args[1], &args[2], &args[3], &args[4]),
        Some("execute-config") if args.len() == 8 => execute_config(
            &args[1], &args[2], &args[3], &args[4], &args[5], &args[6], &args[7],
        ),
        Some("whole") if args.len() == 2 => whole(&args[1]),
        _ => Err("usage"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(token) => {
            eprintln!("prep: {token}");
            ExitCode::from(1)
        }
    }
}

fn pack(bundle_path: &str, out_path: &str) -> Result<(), &'static str> {
    let bundle = read_json(bundle_path)?;
    let pack = bundle.get("pack").cloned().ok_or("pack")?;
    write_json(out_path, &pack)
}

fn propose_config(
    pack_path: &str,
    question_path: &str,
    ca_path: &str,
    bearer_path: &str,
    model: &str,
    origin: &str,
    out_path: &str,
) -> Result<(), &'static str> {
    let bound = binding(pack_path)?;
    let value = Val::Object(vec![
        ("protocol".to_string(), strv("oo.ontology-dialogue.propose-config/v1")),
        ("question_file".to_string(), strv(question_path)),
        ("pack_file".to_string(), strv(pack_path)),
        (
            "binding".to_string(),
            Val::Object(vec![
                ("role".to_string(), strv(&bound.role)),
                ("project".to_string(), strv(&bound.project)),
                ("policy_id".to_string(), strv(&bound.policy_id)),
                ("policy_version".to_string(), strv(&bound.policy_version)),
                ("knowledge_id".to_string(), strv(&bound.knowledge_id)),
                ("knowledge_version".to_string(), strv(&bound.knowledge_version)),
                ("as_of".to_string(), strv("2026-10-07T00:00:00Z")),
            ]),
        ),
        (
            "endpoint".to_string(),
            Val::Object(vec![
                ("origin".to_string(), strv(origin)),
                ("path".to_string(), strv("/v1/chat/completions")),
                ("model_id".to_string(), strv(model)),
            ]),
        ),
        (
            "trust".to_string(),
            Val::Object(vec![
                ("ca_file".to_string(), strv(ca_path)),
                ("residency_approved".to_string(), Val::Bool(true)),
            ]),
        ),
        (
            "secret".to_string(),
            Val::Object(vec![("bearer_file".to_string(), strv(bearer_path))]),
        ),
        (
            "budget".to_string(),
            Val::Object(vec![
                ("max_question_bytes".to_string(), num("8192")),
                ("max_context_bytes".to_string(), num("65536")),
                ("max_request_bytes".to_string(), num("65536")),
                ("max_response_bytes".to_string(), num("16384")),
                ("max_response_chunks".to_string(), num("32")),
                ("deadline_ms".to_string(), num("30000")),
                ("max_prompt_tokens".to_string(), num("100000")),
                ("max_completion_tokens".to_string(), num("4096")),
                ("max_total_tokens".to_string(), num("100000")),
                ("max_sql_bytes".to_string(), num("65536")),
            ]),
        ),
    ]);
    write_json(out_path, &value)
}

fn approval(proposal_path: &str, out_path: &str) -> Result<(), &'static str> {
    let proposal = read_json(proposal_path)?;
    let value = Val::Object(vec![
        ("protocol".to_string(), strv("oo.ontology-dialogue.approval/v1")),
        ("trust".to_string(), strv("local_operator_declared")),
        ("authenticated".to_string(), Val::Bool(false)),
        ("proposal_sha256".to_string(), cloned(&proposal, "proposal_sha256")?),
        ("sql_sha256".to_string(), cloned(&proposal, "sql_sha256")?),
        ("tuple_sha256".to_string(), cloned(&proposal, "tuple_sha256")?),
        ("pack_digest".to_string(), cloned(&proposal, "pack_digest")?),
    ]);
    write_json(out_path, &value)
}

fn request(
    template_path: &str,
    proposal_path: &str,
    sql: &str,
    audit_path: &str,
    out_path: &str,
) -> Result<(), &'static str> {
    let mut doc = read_json(template_path)?;
    let sql_text = if proposal_path == "-" {
        sql.to_string()
    } else {
        let proposal = read_json(proposal_path)?;
        set_identity(&mut doc, &proposal)?;
        proposal
            .get("sql")
            .and_then(Val::as_str)
            .ok_or("sql")?
            .to_string()
    };
    let statement = object_mut(&mut doc, "statement")?;
    set(statement, "sql", strv(&sql_text));
    set(&mut doc, "audit_file", strv(audit_path));
    write_json(out_path, &doc)
}

fn cite(audit_path: &str, role: &str, project: &str, out_path: &str) -> Result<(), &'static str> {
    let body = fs::read(audit_path).map_err(|_| "audit")?;
    let text = String::from_utf8(body.clone()).map_err(|_| "audit")?;
    let digest = sha256::sha256_hex(&body);
    let value = Val::Object(vec![
        ("version".to_string(), strv("oo.answer-citations/v1")),
        ("trust".to_string(), strv("local_operator_declared")),
        (
            "binding".to_string(),
            Val::Object(vec![
                ("session_id".to_string(), strv("a389s1")),
                ("turn_id".to_string(), strv("a389t1")),
                ("role".to_string(), strv(role)),
                ("project".to_string(), strv(project)),
                ("protocol".to_string(), strv("oo.gateway.query-pg-audit/v1")),
            ]),
        ),
        (
            "receipts".to_string(),
            Val::Array(vec![Val::Object(vec![
                ("receipt_sha256".to_string(), strv(&digest)),
                ("body".to_string(), strv(&text)),
            ])]),
        ),
        (
            "answer".to_string(),
            Val::Object(vec![
                ("declares_data".to_string(), Val::Bool(true)),
                (
                    "text".to_string(),
                    strv("The fixture query returned rows and this answer does not cite them."),
                ),
            ]),
        ),
    ]);
    write_json(out_path, &value)
}

fn execute_config(
    proposal: &str,
    approval: &str,
    pack: &str,
    request: &str,
    query_pg: &str,
    sha: &str,
    out_path: &str,
) -> Result<(), &'static str> {
    let value = Val::Object(vec![
        ("protocol".to_string(), strv("oo.ontology-dialogue.execute-config/v1")),
        ("proposal_file".to_string(), strv(proposal)),
        ("approval_file".to_string(), strv(approval)),
        ("pack_file".to_string(), strv(pack)),
        ("request_file".to_string(), strv(request)),
        (
            "query_pg".to_string(),
            Val::Object(vec![
                ("path".to_string(), strv(query_pg)),
                ("sha256".to_string(), strv(sha)),
            ]),
        ),
        (
            "binding".to_string(),
            Val::Object(vec![
                ("session_id".to_string(), strv("a389s1")),
                ("turn_id".to_string(), strv("a389t1")),
                ("declares_data".to_string(), Val::Bool(true)),
                ("render_rows".to_string(), Val::Bool(true)),
            ]),
        ),
        (
            "budget".to_string(),
            Val::Object(vec![
                ("deadline_ms".to_string(), num("30000")),
                ("max_stdout_bytes".to_string(), num("1048576")),
            ]),
        ),
    ]);
    write_json(out_path, &value)
}

fn whole(out_path: &str) -> Result<(), &'static str> {
    let keys = [
        "public_auth",
        "session_close_api",
        "natural_language_harness",
        "runsc",
        "wal",
        "semantic_publication",
        "customer_uat",
    ];
    let fields = keys
        .into_iter()
        .map(|key| (key.to_string(), strv("notrun")))
        .collect();
    write_json(out_path, &Val::Object(fields))
}

struct Bound {
    role: String,
    project: String,
    policy_id: String,
    policy_version: String,
    knowledge_id: String,
    knowledge_version: String,
}

fn binding(pack_path: &str) -> Result<Bound, &'static str> {
    let pack = read_json(pack_path)?;
    let files = pack.get("files").and_then(Val::as_array).ok_or("pack")?;
    let manifest = files
        .iter()
        .find(|file| file.get("path").and_then(Val::as_str) == Some("manifest.json"))
        .ok_or("pack")?;
    let text = manifest.get("text").and_then(Val::as_str).ok_or("pack")?;
    let doc = json::parse(text).map_err(|_| "pack")?;
    let policy = doc.get("policy").ok_or("pack")?;
    let knowledge = doc.get("knowledge").ok_or("pack")?;
    Ok(Bound {
        role: owned(doc.get("role"))?,
        project: owned(doc.get("project"))?,
        policy_id: owned(policy.get("id"))?,
        policy_version: owned(policy.get("version"))?,
        knowledge_id: owned(knowledge.get("id"))?,
        knowledge_version: owned(knowledge.get("version"))?,
    })
}

fn set_identity(doc: &mut Val, proposal: &Val) -> Result<(), &'static str> {
    let identity = object_mut(doc, "identity")?;
    set(identity, "role", cloned(proposal, "role")?);
    set(identity, "project", cloned(proposal, "project")?);
    set(identity, "policy_id", cloned(proposal, "policy_id")?);
    Ok(())
}

fn object_mut<'a>(value: &'a mut Val, key: &str) -> Result<&'a mut Val, &'static str> {
    let Val::Object(fields) = value else {
        return Err("json");
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == key)
        .map(|(_, child)| child)
        .ok_or("json")
}

fn set(value: &mut Val, key: &str, child: Val) {
    let Val::Object(fields) = value else {
        return;
    };
    if let Some(slot) = fields.iter_mut().find(|(name, _)| name == key) {
        slot.1 = child;
    } else {
        fields.push((key.to_string(), child));
    }
}

fn cloned(value: &Val, key: &str) -> Result<Val, &'static str> {
    value.get(key).cloned().ok_or("json")
}

fn owned(value: Option<&Val>) -> Result<String, &'static str> {
    value.and_then(Val::as_str).map(str::to_string).ok_or("pack")
}

fn strv(text: &str) -> Val {
    Val::String(text.to_string())
}

fn num(text: &str) -> Val {
    Val::Number(text.to_string())
}

fn read_json(path: &str) -> Result<Val, &'static str> {
    let text = fs::read_to_string(path).map_err(|_| "io")?;
    json::parse(&text).map_err(|_| "json")
}

fn write_json(path: &str, value: &Val) -> Result<(), &'static str> {
    fs::write(path, format!("{}\n", json::render(value))).map_err(|_| "io")
}
