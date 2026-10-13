//! One question. Pi reads the draft pack, writes SQL, and the `oo` CLI calls query-pg.
//!
//! A missing runtime, binary, or policy returns before any statement is executed.
//! Gold cells do not choose the query or the success bit. The model answer has to
//! cite a query id this session actually returned.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use pi_harness::{
    Bounds, DevelopmentUidBoundary, EventStream, Harness, LaunchRequest, LocalProcessNotSandbox, ModelConfig,
    ProductEvent, RuntimeMode, StreamItem, UidBoundaryReceipt, UserMessage, EVENT_BOUND, LINE_LIMIT, RAW_LOG_CAP,
    TOOL_LIMIT,
};
use serde_json::{json, Value};
use std::os::unix::fs::{chown, OpenOptionsExt, PermissionsExt};

use crate::close::identity;
use crate::config::{Config, ConfigError};
use crate::doctor::doctor;
use crate::forward::{self, PUBLIC_MARKER};
use crate::gateway::CapturedQuery;
use crate::gold::{bulk_questions, frozen_questions, question_text_for};
use crate::proxy;
use crate::state::{merge_state, read_state};

const QUESTION_MAX_CHARS: usize = 2_000;
const QUESTION_MAX_BYTES: usize = 8_192;

#[derive(Clone, Debug)]
pub struct AskOutcome {
    pub status: String,
    pub question_id: String,
    pub matched: bool,
    pub query_id: String,
    pub tool_events: u32,
    pub answer: String,
    pub cited: bool,
    pub rows: Vec<Vec<Value>>,
    pub uid_boundary: Value,
}

struct ResolvedQuestion {
    record_id: String,
    text: String,
}

struct PiDrive {
    events: Vec<Value>,
    followup_prompt: Option<String>,
    uid_boundary: Value,
}

struct PiTree {
    root: PathBuf,
    home: PathBuf,
    agent: PathBuf,
    session: PathBuf,
    workspace: PathBuf,
    pack: PathBuf,
    marker: PathBuf,
}

struct Judgement {
    status: &'static str,
    matched: bool,
    cited: bool,
    query_id: String,
    rows: Vec<Vec<Value>>,
    answer: String,
}

pub fn prompt_for(question: &str, oo: &Path, knowledge: &Path, token_file: &Path, origin: &str) -> String {
    format!(
        "Read the unreviewed draft files under {knowledge} before answering.\n\
         Read {knowledge}/README.md before you run a statement.\n\
         Use the read tool for the draft files and the bash tool to run the program.\n\
         The folder is an operator development preview. It is not published.\n\
         Run one statement you write yourself with this absolute program: {oo}\n\
         Command name: query\n\
         Flags: --json --gateway {origin} --transport local-development --auth external --token-file {token_file} -f <a file you create in the workspace>\n\
         The token file is a public proxy marker. It is not a database password and it is not a gateway session token.\n\
         The statement must be your own. This prompt does not contain a statement or an answer.\n\
         The gateway admits schema-qualified names. It rejects aliases, natural joins, and subqueries.\n\
         The gateway admits count star, sum of one numeric column, and min and max of one int8 column. It rejects casts and every other function.\n\
         The gateway JSON includes query_id. Your final answer must cite that query_id and the returned values.\n\
         Do not invent a query_id.\n\
         Question: {question}\n",
        knowledge = knowledge.display(),
        oo = oo.display(),
        token_file = token_file.display(),
        origin = origin,
        question = question,
    )
}

/// `None` stays admitted so a config with no binaries still reports `pi.absent`
/// before a process starts. A spawn requires `draft_ready` separately.
pub(crate) fn ask_admitted(status: Option<&str>, selfbuild: Option<&str>) -> bool {
    selfbuild == Some("generated_unreviewed") && matches!(status, Some("draft_ready") | None)
}

fn spawn_admitted(status: Option<&str>) -> bool {
    status == Some("draft_ready")
}

pub fn ask(path: &Path, question_input: &str) -> Result<AskOutcome, ConfigError> {
    doctor(path)?;
    let config = crate::config::load_config(path)?;
    let state = read_state(&config).map_err(|_| ConfigError::new("state.absent", "the loop state is absent"))?;
    let status = state.get("status").and_then(Value::as_str);
    let selfbuild = state.get("selfbuild").and_then(Value::as_str);
    let question_set = state.get("question_set").and_then(Value::as_str).unwrap_or("shop");
    let resolved = resolve_question(question_set, question_input)?;
    if !ask_admitted(status, selfbuild) {
        return Err(ConfigError::new("ask.refused", "self-build is not an open unreviewed generation"));
    }
    if !config.pi_executable.is_file() || !config.pi_runtime.is_file() {
        return Err(ConfigError::new("pi.absent", "the pi runtime is absent"));
    }
    if !config.query_pg.is_file() {
        return Err(ConfigError::new("query_pg.absent", "query-pg is absent"));
    }
    if !config.oo_bin.is_file() {
        return Err(ConfigError::new("oo.absent", "the oo CLI is absent"));
    }
    let policy_path = {
        let dev = config.work.join("dev-policy.json");
        if dev.is_file() { dev } else { config.policy_file.clone() }
    };
    if !policy_path.is_file() {
        return Err(ConfigError::new("policy.absent", "the gateway policy is absent"));
    }
    if !config.knowledge.is_dir() {
        return Err(ConfigError::new("pack.absent", "the knowledge folder is absent"));
    }
    for item in [
        &config.work,
        &config.knowledge,
        &config.oo_bin,
        &config.query_pg,
        &config.pi_executable,
        &config.pi_runtime,
        &policy_path,
        &config.gateway_token_file,
    ] {
        if !item.is_absolute() {
            return Err(ConfigError::new("path", "a configured path is not absolute"));
        }
    }
    let sealed = crate::bind::load_sealed(path)?;
    let (manifest_sha256, target_config_sha256) = crate::bind::sealed_binding_hashes(path)?;
    fs::write(config.work.join("query-binding.json"), serde_json::to_vec_pretty(&json!({
        "mapping_generation": sealed.generation,
        "target_adopt_generation": sealed.target_adopt_generation,
        "import_hash": sealed.import_hash,
        "manifest_sha256": manifest_sha256,
        "target_config_sha256": target_config_sha256,
        "snapshot_id": sealed.snapshot_id
    })).map_err(|_| ConfigError::new("audit.binding", "the import binding could not be encoded"))?).map_err(|_| ConfigError::new("audit.binding", "the import binding could not be written"))?;
    let executable_sha256 = parse_sha256(&config.pi_executable_sha256)?;
    let runtime_sha256 = parse_sha256(&config.pi_runtime_sha256)?;
    let policy: Value = serde_json::from_slice(&fs::read(&policy_path).map_err(|_| ConfigError::new("policy.absent", "the gateway policy is absent"))?)
        .map_err(|_| ConfigError::new("policy", "the gateway policy is not JSON"))?;
    if !policy.is_object() {
        return Err(ConfigError::new("policy", "the gateway policy is not one object"));
    }
    if config.gateway_token_file.starts_with(&config.knowledge) || config.work.starts_with(&config.knowledge) {
        return Err(ConfigError::new("path", "a pi directory overlaps the knowledge pack"));
    }
    if !spawn_admitted(status) {
        return Err(ConfigError::new("ask.refused", "ask runs only while the loop state is draft_ready"));
    }
    let bearer = pg_builder::read_bearer(&config.bearer_file).map_err(|_| ConfigError::new("config.secret", "the bearer file was refused"))?;
    let session = random_token()?;
    crate::close::write_active_map(&config.work, &crate::source::hex(session.as_bytes()))?;
    let opened = open_session(OpenSession {
        config: &config,
        resolved: &resolved,
        policy,
        bearer,
        session,
        executable_sha256,
        runtime_sha256,
    });
    if opened.is_err() {
        abandon_map(&config.work);
    }
    opened
}

struct OpenSession<'a> {
    config: &'a Config,
    resolved: &'a ResolvedQuestion,
    policy: Value,
    bearer: String,
    session: String,
    executable_sha256: [u8; 32],
    runtime_sha256: [u8; 32],
}

fn open_session(open: OpenSession<'_>) -> Result<AskOutcome, ConfigError> {
    let OpenSession {
        config,
        resolved,
        mut policy,
        bearer,
        session,
        executable_sha256,
        runtime_sha256,
    } = open;
    let boundary = DevelopmentUidBoundary::checked(config.pi_uid(), config.pi_gid()).map_err(|error| map_pi(error.kind))?;
    let tree = prepare_pi_tree(config, boundary)?;
    shield_secrets(config, &mut policy)?;
    let pki = proxy::mint_pki(&config.work.join("pki")).map_err(|_| ConfigError::new("pki.absent", "the development PKI could not be created"))?;
    let server = proxy::server_config(&pki).map_err(|_| ConfigError::new("pki.absent", "the development PKI could not be created"))?;
    let client = proxy::client_config(&pki).map_err(|_| ConfigError::new("pki.absent", "the development PKI could not be created"))?;
    privatize_work(&config.work)?;
    write_token(&config.work.join("proxy-marker"), PUBLIC_MARKER)?;
    let gateway_listener = TcpListener::bind("127.0.0.1:0").map_err(|_| ConfigError::new("gateway", "the local gateway could not bind"))?;
    let proxy_listener = TcpListener::bind("127.0.0.1:0").map_err(|_| ConfigError::new("gateway", "the local proxy could not bind"))?;
    let forward_listener = TcpListener::bind("127.0.0.1:0").map_err(|_| ConfigError::new("forward", "the local forwarder could not bind"))?;
    let gateway_addr = gateway_listener.local_addr().map_err(|_| ConfigError::new("gateway", "the local gateway could not bind"))?;
    let proxy_port = proxy_listener.local_addr().map_err(|_| ConfigError::new("gateway", "the local proxy could not bind"))?.port();
    let forward_port = forward_listener.local_addr().map_err(|_| ConfigError::new("forward", "the local forwarder could not bind"))?.port();
    let origin = format!("http://127.0.0.1:{proxy_port}");
    // Pi's OpenAI client posts `{baseUrl}/chat/completions`. The harness contract
    // requires the `/v1` suffix. The query proxy origin stays unsuffixed.
    let forward_origin = format!("http://127.0.0.1:{forward_port}/v1");
    let prompt = prompt_for(&resolved.text, &config.oo_bin, &tree.pack, &tree.marker, &origin);
    if prompt_leaks(&prompt) {
        return Err(ConfigError::new("ask.prompt", "the prompt contains a prepared answer"));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let captured = Arc::new(Mutex::new(Vec::<CapturedQuery>::new()));
    let gateway_thread = {
        let stop = Arc::clone(&stop);
        let captured = Arc::clone(&captured);
        let binary = config.query_pg.clone();
        let request_dir = config.work.join("queries");
        let session = session.clone();
        thread::spawn(move || {
            proxy::serve_gateway(proxy::GatewayServe {
                listener: gateway_listener,
                config: server,
                binary,
                policy,
                session,
                request_dir,
                captured,
                stop,
            })
        })
    };
    let proxy_thread = {
        let stop = Arc::clone(&stop);
        let record = config.work.join("proxy-record.jsonl");
        thread::spawn(move || proxy::serve_proxy(proxy_listener, gateway_addr, client, session, record, stop))
    };
    let forward_thread = {
        let stop = Arc::clone(&stop);
        let bearer = bearer.clone();
        thread::spawn(move || forward::serve_ports(forward_listener, forward::MODEL_SIDECAR_PORT, forward::MODEL_PORT, bearer, stop))
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| ConfigError::new("runtime", "the async runtime could not start"))?;
    let driven = runtime.block_on(drive_pi(
        PiLaunch {
            config,
            prompt: &prompt,
            bearer: &bearer,
            executable_sha256,
            runtime_sha256,
            tree: &tree,
            forward_origin: &forward_origin,
            boundary,
        },
        &captured,
    ));
    stop.store(true, Ordering::Relaxed);
    let _ = gateway_thread.join();
    let _ = proxy_thread.join();
    let _ = forward_thread.join();
    let captured = captured.lock().map(|guard| guard.clone()).unwrap_or_default();
    finish_ask(config, resolved, &prompt, &bearer, driven, &captured)
}

struct PiLaunch<'a> {
    config: &'a Config,
    prompt: &'a str,
    bearer: &'a str,
    executable_sha256: [u8; 32],
    runtime_sha256: [u8; 32],
    tree: &'a PiTree,
    forward_origin: &'a str,
    boundary: DevelopmentUidBoundary,
}

async fn drive_pi(launch: PiLaunch<'_>, captured: &Mutex<Vec<CapturedQuery>>) -> Result<PiDrive, ConfigError> {
    let PiLaunch { config, prompt, bearer, executable_sha256, runtime_sha256, tree, forward_origin, boundary } = launch;
    let request = LaunchRequest {
        executable: config.pi_executable.clone(),
        executable_sha256,
        runtime: config.pi_runtime.clone(),
        runtime_sha256,
        pack: tree.pack.clone(),
        workspace: tree.workspace.clone(),
        home: tree.home.clone(),
        agent_dir: tree.agent.clone(),
        session_dir: tree.session.clone(),
        origin: forward_origin.to_owned(),
        provider: config.pi_provider.clone(),
        model: config.pi_model.clone(),
        model_config: ModelConfig {
            name: config.pi_model.clone(),
            context_window: 131_072,
            max_tokens: 8_192,
        },
        bounds: Bounds {
            line_limit: LINE_LIMIT,
            event_bound: EVENT_BOUND,
            command_deadline: Duration::from_secs(20),
            abort_ack_deadline: Duration::from_secs(5),
            kill_wait: Duration::from_secs(5),
            turn_deadline: Duration::from_millis(config.deadline_ms),
            tool_limit: TOOL_LIMIT,
            session_byte_limit: RAW_LOG_CAP.min(1_048_576),
        },
        runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
        raw_event_log: Some(config.work.join("pi-raw.log")),
    };
    let harness = match Harness::launch_with_uid_boundary(request, boundary).await {
        Ok(harness) => harness,
        Err(error) => {
            // The public error stays `pi.launch`. Keep the harness category and a
            // short detail beside the work tree so the next refusal is diagnosable.
            let detail: String = error.detail.chars().take(400).collect();
            let _ = fs::write(config.work.join("pi-launch-error.txt"), format!("{}\n{detail}\n", error.category()));
            return Err(map_pi(error.kind));
        }
    };
    let receipt = harness
        .uid_boundary_receipt()
        .cloned()
        .ok_or_else(|| ConfigError::new("uid.boundary", "the development uid boundary is unavailable"))?;
    fs::set_permissions(tree.agent.join("models.json"), fs::Permissions::from_mode(0o644))
        .map_err(|_| ConfigError::new("uid.boundary", "the pi directory could not be prepared"))?;
    if let Some(proc_id) = identity(harness.child_pid()) {
        let _ = merge_state(
            config,
            &json!({
                "pi_pid": harness.child_pid(),
                "pi_comm": proc_id.comm,
                "pi_start": proc_id.start,
                "pi_uid": config.pi_uid(),
                "pi_gid": config.pi_gid()
            }),
        );
    }
    let mut events = Vec::new();
    let mut stream = harness.events().map_err(|error| map_pi(error.kind))?;
    harness.send(UserMessage { text: prompt.to_owned() }).await.map_err(|error| map_pi(error.kind))?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(config.deadline_ms);
    let finished = collect_turn(&mut stream, &mut events, deadline, bearer).await;
    let mut followup_prompt = None;
    if finished {
        let snap = captured.lock().map(|guard| guard.clone()).unwrap_or_default();
        if followup_wanted(&events, &snap) && tokio::time::Instant::now() + Duration::from_secs(5) < deadline {
            let text = followup_text(&snap);
            if !text.is_empty() && !prompt_leaks(&text) {
                followup_prompt = Some(text.clone());
                if harness.send(UserMessage { text }).await.is_ok() {
                    let _ = collect_turn(&mut stream, &mut events, deadline, bearer).await;
                } else {
                    followup_prompt = None;
                }
            }
        }
    }
    let _ = harness.abort().await;
    let guest_absent = identity(harness.child_pid()).is_none();
    let uid_boundary = receipt_value(&receipt, guest_absent);
    if guest_absent {
        let _ = merge_state(
            config,
            &json!({"pi_pid": null, "pi_comm": null, "pi_start": null, "pi_uid": null, "pi_gid": null, "pid_absent": true}),
        );
    }
    write_readiness(config, tree, &uid_boundary, guest_absent)?;
    if !guest_absent {
        return Err(ConfigError::new("pi.reap", "the pi process is still present after abort"));
    }
    Ok(PiDrive { events, followup_prompt, uid_boundary })
}

async fn collect_turn(stream: &mut EventStream, events: &mut Vec<Value>, deadline: tokio::time::Instant, bearer: &str) -> bool {
    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(StreamItem::Event(sequenced)) => {
                        let finished = matches!(sequenced.event, ProductEvent::TurnFinished { .. });
                        events.push(event_value(sequenced.sequence, &sequenced.event, bearer));
                        if finished {
                            return true;
                        }
                    }
                    Some(StreamItem::Terminated(_)) | None => return false,
                }
            }
            _ = tokio::time::sleep_until(deadline) => return false,
        }
    }
}

fn finish_ask(
    config: &Config,
    resolved: &ResolvedQuestion,
    prompt: &str,
    bearer: &str,
    driven: Result<PiDrive, ConfigError>,
    captured: &[CapturedQuery],
) -> Result<AskOutcome, ConfigError> {
    let driven = driven?;
    let judged = judge_turn(&driven.events, captured);
    let tool_events = driven.events.iter().filter(|event| event.get("kind").and_then(Value::as_str) == Some("tool_call_started")).count() as u32;
    let queries: Vec<Value> = captured
        .iter()
        .map(|query| {
            json!({
                "query_id": query.query_id,
                "http_status": query.http_status,
                "exit": query.exit,
                "rows": query.rows,
                "database": config.database,
                "sql_sha256": crate::source::hex(query.sql.as_bytes()),
                "sql": scrub(&query.sql, bearer, 2_000)
            })
        })
        .collect();
    let cited_query = captured.iter().find(|query| judged.cited && query.query_id == judged.query_id).map(|query| {
        json!({
            "query_id": query.query_id,
            "http_status": query.http_status,
            "exit": query.exit,
            "rows": query.rows,
            "database": config.database
        })
    });
    let record = json!({
        "protocol": crate::PROTOCOL,
        "question_id": resolved.record_id,
        "question": scrub(&resolved.text, bearer, QUESTION_MAX_CHARS),
        "status": judged.status,
        "matched": judged.matched,
        "cited": judged.cited,
        "query_id": judged.query_id,
        "answer": judged.answer,
        "rows": judged.rows,
        "tool_events": tool_events,
        "events": driven.events,
        "queries": queries,
        "cited_query": cited_query,
        "followup_prompt": driven.followup_prompt.as_deref().map(|text| scrub(text, bearer, 2_000)),
        "prompt_sha256": crate::source::hex(prompt.as_bytes()),
        "prompt": scrub(prompt, bearer, 8_000),
        "publication": "not_published",
        "llm_review": "unreviewed",
        "uid_boundary": driven.uid_boundary.clone(),
        "parent_acceptance": false,
        "goal_acceptance": false
    });
    persist_ask(config, &resolved.record_id, judged.status, &record)?;
    Ok(AskOutcome {
        status: judged.status.to_owned(),
        question_id: resolved.record_id.clone(),
        matched: judged.matched,
        query_id: judged.query_id,
        tool_events,
        answer: judged.answer,
        cited: judged.cited,
        rows: judged.rows,
        uid_boundary: driven.uid_boundary,
    })
}

fn persist_ask(config: &Config, record_id: &str, status: &str, record: &Value) -> Result<(), ConfigError> {
    let dir = config.work.join("asks");
    fs::create_dir_all(&dir).map_err(|_| ConfigError::new("state", "the ask record could not be written"))?;
    let path = ask_record_path(&dir, record_id)?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|_| ConfigError::new("state", "the ask record could not be written"))?;
    fs::write(&path, &bytes).map_err(|_| ConfigError::new("state", "the ask record could not be written"))?;
    if merge_state(config, &json!({"ask_status": status})).is_err() {
        let _ = fs::remove_file(&path);
        return Err(ConfigError::new("state", "the ask record could not be written"));
    }
    Ok(())
}

fn abandon_map(work: &Path) {
    let _ = fs::remove_file(work.join("proxy-session-map.json"));
}

fn resolve_question(set: &str, input: &str) -> Result<ResolvedQuestion, ConfigError> {
    if input.is_empty() || input.chars().all(char::is_whitespace) {
        return Err(ConfigError::new("ask.question", "the question is empty"));
    }
    if input.chars().count() > QUESTION_MAX_CHARS || input.len() > QUESTION_MAX_BYTES {
        return Err(ConfigError::new("ask.question", "the question is longer than the bound"));
    }
    if input.chars().any(|ch| ch == '\0' || (ch.is_control() && ch != '\n' && ch != '\t' && ch != '\r')) {
        return Err(ConfigError::new("ask.question", "the question contains a control character"));
    }
    if id_shape(input) {
        let Some(text) = question_text_for(set, input) else {
            return Err(ConfigError::new("ask.question", "the question id is not in the frozen set"));
        };
        return Ok(ResolvedQuestion { record_id: input.to_owned(), text: text.to_owned() });
    }
    if let Some(id) = fixture_id_for_text(set, input) {
        return Ok(ResolvedQuestion { record_id: id.to_owned(), text: input.to_owned() });
    }
    let digest = crate::source::hex(input.as_bytes());
    Ok(ResolvedQuestion { record_id: format!("nl-{}", &digest[..32]), text: input.to_owned() })
}

fn id_shape(input: &str) -> bool {
    let bytes = input.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn fixture_id_for_text(set: &str, input: &str) -> Option<&'static str> {
    let list = match set {
        "shop" => frozen_questions(),
        "bulk" => bulk_questions(),
        _ => return None,
    };
    list.iter().find(|item| item.text == input).map(|item| item.id)
}

fn ask_record_path(dir: &Path, record_id: &str) -> Result<PathBuf, ConfigError> {
    if !record_id_ok(record_id) {
        return Err(ConfigError::new("ask.path", "the question record id is not a safe file name"));
    }
    let path = dir.join(format!("{record_id}.json"));
    if path.parent() != Some(dir) {
        return Err(ConfigError::new("ask.path", "the question record id is not a safe file name"));
    }
    Ok(path)
}

fn record_id_ok(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 80
        && bytes.first().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && !id.contains("..")
}

fn prompt_leaks(text: &str) -> bool {
    text.contains("SELECT") || text.contains("90000") || text.contains("1234567890")
}

fn followup_wanted(events: &[Value], captured: &[CapturedQuery]) -> bool {
    judge_turn(events, captured).status == "citation.absent"
}

fn followup_text(captured: &[CapturedQuery]) -> String {
    let ids = successful_queries(captured);
    if ids.is_empty() {
        return String::new();
    }
    let listed = ids.iter().take(8).map(|query| query.query_id.as_str()).collect::<Vec<_>>().join(" ");
    format!(
        "Your reply did not cite a gateway query_id from this session.\n\
         Observed successful query_id values: {listed}.\n\
         Cite the query_id of the result you used, together with the values you already received.\n\
         Do not invent an id. Do not write a new statement.\n"
    )
}

fn judge_turn(events: &[Value], captured: &[CapturedQuery]) -> Judgement {
    let answer = last_answer(events);
    let turn = events.iter().any(|event| event.get("kind").and_then(Value::as_str) == Some("turn_finished"));
    let read_at = tool_at(events, "read");
    let bash_at = tool_at(events, "bash");
    let successful = successful_queries(captured);
    if !turn {
        return unfinished("turn.absent", answer);
    }
    if read_at.is_none() || bash_at.is_none() {
        let status = if tool_count(events) == 0 { "tools.absent" } else { "tools.incomplete" };
        return unfinished(status, answer);
    }
    if successful.is_empty() {
        return unfinished("query.absent", answer);
    }
    let cited = cited_ids(&answer, &successful);
    if cited.len() != 1 {
        let status = if cited.is_empty() { "citation.absent" } else { "citation.ambiguous" };
        return unfinished(status, answer);
    }
    let Some(query) = successful.into_iter().find(|query| query.query_id == cited[0]) else {
        return unfinished("citation.absent", answer);
    };
    Judgement {
        status: "matched",
        matched: true,
        cited: true,
        query_id: query.query_id.clone(),
        rows: query.rows.clone(),
        answer,
    }
}

fn unfinished(status: &'static str, answer: String) -> Judgement {
    Judgement { status, matched: false, cited: false, query_id: String::new(), rows: Vec::new(), answer }
}

fn successful_queries(captured: &[CapturedQuery]) -> Vec<&CapturedQuery> {
    captured.iter().filter(|query| query.http_status == 200 && query.exit == 0 && query_id_shape(&query.query_id)).collect()
}

fn cited_ids(answer: &str, successful: &[&CapturedQuery]) -> Vec<String> {
    query_ids_in(answer).into_iter().filter(|id| successful.iter().any(|query| query.query_id == *id)).collect()
}

fn query_ids_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'q' && bytes[index + 1] == b'_' {
            let mut end = index + 2;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() { end += 1; }
            let id = &text[index..end];
            if query_id_shape(id) && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric()) && (end == bytes.len() || (!bytes[end].is_ascii_alphanumeric() && bytes[end] != b'_')) {
                let id = id.to_ascii_lowercase();
                if !out.contains(&id) { out.push(id); }
            }
            index = end;
        } else { index += 1; }
    }
    out
}

fn query_id_shape(id: &str) -> bool {
    let bytes = id.as_bytes();
    matches!(bytes.len(), 10 | 34) && bytes[0] == b'q' && bytes[1] == b'_' && bytes[2..].iter().all(|byte| byte.is_ascii_hexdigit())
}

fn tool_at(events: &[Value], name: &str) -> Option<usize> {
    events.iter().position(|event| {
        event.get("kind").and_then(Value::as_str) == Some("tool_call_started") && event.get("name").and_then(Value::as_str) == Some(name)
    })
}

fn tool_count(events: &[Value]) -> usize {
    events.iter().filter(|event| event.get("kind").and_then(Value::as_str) == Some("tool_call_started")).count()
}

fn last_answer(events: &[Value]) -> String {
    events.iter().rev().find_map(|event| {
        if event.get("kind").and_then(Value::as_str) == Some("turn_finished") {
            event.get("answer").and_then(Value::as_str).map(str::to_owned)
        } else {
            None
        }
    }).unwrap_or_default()
}

fn event_value(sequence: u64, event: &ProductEvent, bearer: &str) -> Value {
    match event {
        ProductEvent::TurnStarted => json!({"sequence": sequence, "kind": "turn_started"}),
        ProductEvent::AssistantDelta { text } => json!({"sequence": sequence, "kind": "assistant_delta", "chars": text.chars().count()}),
        ProductEvent::ToolCallStarted { name, call_id, args_summary } => json!({
            "sequence": sequence,
            "kind": "tool_call_started",
            "name": scrub(name, bearer, 128),
            "call_id": scrub(call_id, bearer, 128),
            "args_summary": scrub(args_summary, bearer, 1_024)
        }),
        ProductEvent::ToolCallFinished { call_id, status, output_summary } => json!({
            "sequence": sequence,
            "kind": "tool_call_finished",
            "call_id": scrub(call_id, bearer, 128),
            "status": scrub(status, bearer, 128),
            "output_summary": scrub(output_summary, bearer, 1_024)
        }),
        ProductEvent::Error { reason } => json!({"sequence": sequence, "kind": "error", "reason": scrub(reason, bearer, 500)}),
        ProductEvent::TurnFinished { stop_reason, answer, usage } => json!({
            "sequence": sequence,
            "kind": "turn_finished",
            "stop_reason": scrub(stop_reason, bearer, 128),
            "answer": scrub(answer.as_deref().unwrap_or(""), bearer, 8_000),
            "answer_chars": answer.as_ref().map(|text| text.chars().count()).unwrap_or(0),
            "usage": usage.as_ref().map(|item| json!({"input": item.input, "output": item.output, "total": item.total}))
        }),
    }
}

fn scrub(text: &str, bearer: &str, cap: usize) -> String {
    let replaced = if bearer.is_empty() { text.to_owned() } else { text.replace(bearer, "[redacted]") };
    if replaced.chars().count() <= cap {
        replaced
    } else {
        replaced.chars().take(cap).collect()
    }
}

fn map_pi(kind: pi_harness::ErrorKind) -> ConfigError {
    match kind {
        pi_harness::ErrorKind::Hash => ConfigError::new("pi.hash", "the pi digest does not match"),
        pi_harness::ErrorKind::Path => ConfigError::new("path", "a pi path was refused"),
        pi_harness::ErrorKind::Unsupported => ConfigError::new("pi.unsupported", "production runsc is not a launch certificate"),
        pi_harness::ErrorKind::UidBoundaryUnavailable | pi_harness::ErrorKind::UidBoundaryUnverified => {
            ConfigError::new("uid.boundary", "the development uid boundary is unavailable")
        }
        _ => ConfigError::new("pi.launch", "the pi process did not start"),
    }
}

fn parse_sha256(text: &str) -> Result<[u8; 32], ConfigError> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ConfigError::new("pi.hash", "the pi digest is not 64 hex characters"));
    }
    let mut out = [0u8; 32];
    for index in 0..32 {
        out[index] = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| ConfigError::new("pi.hash", "the pi digest is not 64 hex characters"))?;
    }
    Ok(out)
}

fn random_token() -> Result<String, ConfigError> {
    let mut bytes = [0u8; 16];
    let mut file = fs::File::open("/dev/urandom").map_err(|_| ConfigError::new("token", "the token source is absent"))?;
    file.read_exact(&mut bytes).map_err(|_| ConfigError::new("token", "the token source is absent"))?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(out)
}

fn write_token(path: &Path, token: &str) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| ConfigError::new("token", "the token directory could not be created"))?;
    }
    if path.exists() {
        fs::remove_file(path).map_err(|_| ConfigError::new("token", "the token file could not be written"))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(path).map_err(|_| ConfigError::new("token", "the token file could not be written"))?;
    file.write_all(token.as_bytes()).map_err(|_| ConfigError::new("token", "the token file could not be written"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| ConfigError::new("token", "the token file mode could not be set"))?;
    Ok(())
}

fn separate(pack: &Path, path: &Path) -> Result<(), ConfigError> {
    if path.starts_with(pack) || pack.starts_with(path) {
        return Err(ConfigError::new("path", "a pi directory overlaps the knowledge pack"));
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), ConfigError> {
    fs::create_dir_all(to).map_err(|_| ConfigError::new("pack", "the workspace copy could not be created"))?;
    let entries = fs::read_dir(from).map_err(|_| ConfigError::new("pack", "the knowledge folder could not be read"))?;
    for entry in entries {
        let entry = entry.map_err(|_| ConfigError::new("pack", "the knowledge folder could not be read"))?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| ConfigError::new("pack", "the knowledge folder could not be read"))?;
        if meta.file_type().is_symlink() {
            return Err(ConfigError::new("pack", "the knowledge folder contains a symlink"));
        }
        let dest = to.join(entry.file_name());
        if meta.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if meta.is_file() {
            fs::copy(entry.path(), dest).map_err(|_| ConfigError::new("pack", "the workspace copy could not be created"))?;
        }
    }
    Ok(())
}

fn pi_root(work: &Path) -> Result<PathBuf, ConfigError> {
    if !work.is_absolute() {
        return Err(ConfigError::new("path", "a configured path is not absolute"));
    }
    let Some(parent) = work.parent() else {
        return Err(ConfigError::new("path", "the pi directory could not be created"));
    };
    if parent.as_os_str().is_empty() {
        return Err(ConfigError::new("path", "the pi directory could not be created"));
    }
    let Some(file_name) = work.file_name() else {
        return Err(ConfigError::new("path", "the pi directory could not be created"));
    };
    if file_name.is_empty() || file_name == "." || file_name == ".." {
        return Err(ConfigError::new("path", "the pi directory could not be created"));
    }
    let mut name = file_name.to_os_string();
    name.push("-pi");
    let root = parent.join(name);
    if root == work || root.starts_with(work) || work.starts_with(&root) {
        return Err(ConfigError::new("path", "the pi directory overlaps the private work tree"));
    }
    Ok(root)
}

fn prepare_pi_tree(config: &Config, boundary: DevelopmentUidBoundary) -> Result<PiTree, ConfigError> {
    let root = pi_root(&config.work)?;
    separate(&config.knowledge, &root)?;
    separate(&config.work, &root)?;
    if let Err(error) = discard_pi_root(&root) {
        let _ = fs::write(config.work.join("pi-tree-error.txt"), format!("{error}\n"));
        return Err(ConfigError::new("path", "the pi directory could not be created"));
    }
    let _ = fs::remove_file(config.work.join("pi-tree-error.txt"));
    fs::create_dir(&root).map_err(|_| ConfigError::new("path", "the pi directory could not be created"))?;
    own_broker(&root, config.pi_gid(), 0o750)?;
    let home = root.join("home");
    let agent = root.join("agent");
    let session = root.join("session");
    let workspace = root.join("work");
    let pack = root.join("pack");
    for dir in [&home, &agent, &session, &workspace] {
        separate(&config.knowledge, dir)?;
        fs::create_dir(dir).map_err(|_| ConfigError::new("path", "a pi directory could not be created"))?;
        // 0707, not 0700. assign_directory gives these directories to the guest.
        // The broker has no CAP_DAC_OVERRIDE, and the harness still creates
        // models.json as uid 0. Other-write is the only way that create succeeds.
        // The parent stays 0750, so a third uid cannot traverse here.
        fs::set_permissions(dir, fs::Permissions::from_mode(0o707))
            .map_err(|_| ConfigError::new("uid.boundary", "the pi directory could not be prepared"))?;
        boundary.assign_directory(dir).map_err(|error| map_pi(error.kind))?;
    }
    copy_tree(&config.knowledge, &pack)?;
    seal_tree(&pack, config.pi_gid(), 0o550, 0o440)?;
    let marker = root.join("proxy-marker");
    write_mode(&marker, PUBLIC_MARKER.as_bytes(), 0o600, "token", "the token file could not be written")?;
    chown(&marker, Some(config.pi_uid()), Some(config.pi_gid())).map_err(|_| ConfigError::new("uid.boundary", "the public proxy marker could not be assigned"))?;
    Ok(PiTree { root, home, agent, session, workspace, pack, marker })
}

fn discard_pi_root(root: &Path) -> Result<(), String> {
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err("pi root is not a directory".to_owned()),
        Ok(_) => {
            // The broker has no CAP_DAC_OVERRIDE. A guest-owned 0700 directory
            // and a sealed 0550 pack are not removable until uid 0 owns them
            // and the directory mode includes owner write. CAP_CHOWN is present.
            reclaim_owned(root).map_err(|error| format!("reclaim {error}"))?;
            fs::remove_dir_all(root).map_err(|error| format!("remove {error}"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("stat {error}")),
    }
}

fn reclaim_owned(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "symlink"));
    }
    chown(path, Some(0), Some(0))?;
    let mode = if meta.is_dir() { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            reclaim_owned(&entry?.path())?;
        }
    }
    Ok(())
}

fn own_broker(path: &Path, gid: u32, mode: u32) -> Result<(), ConfigError> {
    chown(path, Some(0), Some(gid)).map_err(|_| ConfigError::new("uid.boundary", "the pi directory could not be prepared"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|_| ConfigError::new("uid.boundary", "the pi directory could not be prepared"))
}

fn seal_tree(path: &Path, gid: u32, dir_mode: u32, file_mode: u32) -> Result<(), ConfigError> {
    let meta = fs::symlink_metadata(path).map_err(|_| ConfigError::new("pack", "the workspace copy could not be created"))?;
    if meta.file_type().is_symlink() {
        return Err(ConfigError::new("pack", "the knowledge folder contains a symlink"));
    }
    if meta.is_dir() {
        let entries = fs::read_dir(path).map_err(|_| ConfigError::new("pack", "the workspace copy could not be created"))?;
        for entry in entries {
            let entry = entry.map_err(|_| ConfigError::new("pack", "the workspace copy could not be created"))?;
            seal_tree(&entry.path(), gid, dir_mode, file_mode)?;
        }
        own_broker(path, gid, dir_mode)?;
    } else if meta.is_file() {
        own_broker(path, gid, file_mode)?;
    }
    Ok(())
}

fn shield_secrets(config: &Config, policy: &mut Value) -> Result<(), ConfigError> {
    let shield = config.work.join("shield");
    let root = pi_root(&config.work)?;
    if shield.starts_with(&root) || root.starts_with(&shield) {
        return Err(ConfigError::new("path", "the pi directory overlaps the private work tree"));
    }
    fs::create_dir_all(&shield).map_err(|_| ConfigError::new("config.secret", "a shielded secret could not be copied"))?;
    fs::set_permissions(&shield, fs::Permissions::from_mode(0o700))
        .map_err(|_| ConfigError::new("config.secret", "a shielded secret could not be copied"))?;
    let password = shield.join("password");
    let ca = shield.join("ca.crt");
    copy_secret(&config.password_file, &password)?;
    copy_secret(&config.ca_file, &ca)?;
    copy_secret(&config.bearer_file, &shield.join("bearer"))?;
    shield_policy(policy, &password, &ca)
}

fn shield_policy(policy: &mut Value, password: &Path, ca: &Path) -> Result<(), ConfigError> {
    let object = policy.as_object_mut().ok_or_else(|| ConfigError::new("policy", "the gateway policy is not one object"))?;
    object.insert("password_file".to_owned(), Value::String(password.display().to_string()));
    object.insert("ca_file".to_owned(), Value::String(ca.display().to_string()));
    Ok(())
}

fn copy_secret(from: &Path, to: &Path) -> Result<(), ConfigError> {
    let meta = fs::symlink_metadata(from).map_err(|_| ConfigError::new("config.secret", "a shielded secret could not be copied"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ConfigError::new("config.secret", "a shielded secret could not be copied"));
    }
    let bytes = fs::read(from).map_err(|_| ConfigError::new("config.secret", "a shielded secret could not be copied"))?;
    write_mode(to, &bytes, 0o600, "config.secret", "a shielded secret could not be copied")
}

fn privatize_work(work: &Path) -> Result<(), ConfigError> {
    let meta = fs::symlink_metadata(work).map_err(|_| ConfigError::new("path", "the work directory could not be protected"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(ConfigError::new("path", "the work directory could not be protected"));
    }
    fs::set_permissions(work, fs::Permissions::from_mode(0o700))
        .map_err(|_| ConfigError::new("path", "the work directory could not be protected"))?;
    let pki = work.join("pki");
    if pki.exists() {
        tighten(&pki, 0o700, 0o600)?;
    }
    Ok(())
}

fn tighten(path: &Path, dir_mode: u32, file_mode: u32) -> Result<(), ConfigError> {
    let meta = fs::symlink_metadata(path).map_err(|_| ConfigError::new("path", "a private directory could not be protected"))?;
    if meta.file_type().is_symlink() {
        return Err(ConfigError::new("path", "a private directory could not be protected"));
    }
    if meta.is_dir() {
        fs::set_permissions(path, fs::Permissions::from_mode(dir_mode))
            .map_err(|_| ConfigError::new("path", "a private directory could not be protected"))?;
        let entries = fs::read_dir(path).map_err(|_| ConfigError::new("path", "a private directory could not be protected"))?;
        for entry in entries {
            let entry = entry.map_err(|_| ConfigError::new("path", "a private directory could not be protected"))?;
            tighten(&entry.path(), dir_mode, file_mode)?;
        }
    } else if meta.is_file() {
        fs::set_permissions(path, fs::Permissions::from_mode(file_mode))
            .map_err(|_| ConfigError::new("path", "a private directory could not be protected"))?;
    }
    Ok(())
}

fn receipt_value(receipt: &UidBoundaryReceipt, guest_absent: bool) -> Value {
    json!({
        "parent_uids": receipt.parent_uids,
        "parent_gids": receipt.parent_gids,
        "parent_groups": receipt.parent_groups,
        "parent_no_new_privs": receipt.parent_no_new_privs,
        "parent_cap_inh": receipt.parent_cap_inh.to_string(),
        "parent_cap_prm": receipt.parent_cap_prm.to_string(),
        "parent_cap_eff": receipt.parent_cap_eff.to_string(),
        "parent_cap_bnd": receipt.parent_cap_bnd.to_string(),
        "parent_cap_amb": receipt.parent_cap_amb.to_string(),
        "child_uids": receipt.child_uids,
        "child_gids": receipt.child_gids,
        "child_groups": receipt.child_groups,
        "child_no_new_privs": receipt.child_no_new_privs,
        "child_cap_inh": receipt.child_cap_inh.to_string(),
        "child_cap_prm": receipt.child_cap_prm.to_string(),
        "child_cap_eff": receipt.child_cap_eff.to_string(),
        "child_cap_bnd": receipt.child_cap_bnd.to_string(),
        "child_cap_amb": receipt.child_cap_amb.to_string(),
        "guest_absent_after_abort": guest_absent
    })
}

fn write_readiness(config: &Config, tree: &PiTree, uid_boundary: &Value, guest_absent: bool) -> Result<(), ConfigError> {
    let body = json!({
        "protocol": "oo.minimal-product-loop.uid-readiness/v1",
        "pi_uid": config.pi_uid(),
        "pi_gid": config.pi_gid(),
        "pi_root": tree.root.display().to_string(),
        "home": tree.home.display().to_string(),
        "agent": tree.agent.display().to_string(),
        "session": tree.session.display().to_string(),
        "workspace": tree.workspace.display().to_string(),
        "pack": tree.pack.display().to_string(),
        "marker": tree.marker.display().to_string(),
        "modes": {
            "pi_root": "0750",
            "home": "0707",
            "agent": "0707",
            "session": "0707",
            "workspace": "0707",
            "pack_dir": "0550",
            "pack_file": "0440",
            "marker": "0600",
            "work": "0700",
            "shield": "0700",
            "pki": "0600"
        },
        "guest_absent_after_abort": guest_absent,
        "uid_boundary": uid_boundary
    });
    let bytes = serde_json::to_vec_pretty(&body).map_err(|_| ConfigError::new("state", "the uid readiness record could not be written"))?;
    write_mode(&config.work.join("uid-readiness.json"), &bytes, 0o600, "state", "the uid readiness record could not be written")
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32, code: &'static str, message: &'static str) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| ConfigError::new(code, message))?;
    }
    if path.symlink_metadata().is_ok() {
        let meta = fs::symlink_metadata(path).map_err(|_| ConfigError::new(code, message))?;
        if meta.file_type().is_symlink() {
            return Err(ConfigError::new(code, message));
        }
        fs::remove_file(path).map_err(|_| ConfigError::new(code, message))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut file = options.open(path).map_err(|_| ConfigError::new(code, message))?;
    file.write_all(bytes).map_err(|_| ConfigError::new(code, message))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|_| ConfigError::new(code, message))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ask, ask_admitted, ask_record_path, followup_text, judge_turn, pi_root, prompt_for, prompt_leaks, resolve_question,
        shield_policy, spawn_admitted,
    };
    use crate::gateway::CapturedQuery;
    use crate::gold::{expected_cells, question_text_for};
    use serde_json::{json, Value};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn random_execution_ids_are_cited_whole_and_reject_prefixes() {
        let id = "q_0123456789abcdef0123456789abcdef";
        assert_eq!(super::query_ids_in(&format!("Result [{id}].")), vec![id.to_owned()]);
        assert!(super::query_ids_in(&format!("x{id} {id}0 {id}g {id}_suffix")).is_empty());
    }

    #[test]
    fn blocked_or_structural_draft_is_not_askable() {
        assert!(!ask_admitted(Some("blocked"), Some("structural_draft")));
        assert!(!ask_admitted(Some("blocked"), Some("generated_unreviewed")));
        assert!(!ask_admitted(Some("draft_ready"), Some("structural_draft")));
        assert!(!ask_admitted(Some("draft_ready"), Some("catalog.absent")));
        assert!(ask_admitted(Some("draft_ready"), Some("generated_unreviewed")));
        assert!(!ask_admitted(Some("released"), Some("generated_unreviewed")));
        assert!(!ask_admitted(Some("closed"), Some("generated_unreviewed")));
        assert!(!ask_admitted(Some("failure"), Some("generated_unreviewed")));
        assert!(!ask_admitted(Some("failed"), Some("generated_unreviewed")));
        assert!(spawn_admitted(Some("draft_ready")));
        assert!(!spawn_admitted(Some("released")));
        assert!(!spawn_admitted(None));
    }

    #[test]
    fn arbitrary_text_benchmark_aliases_and_paths_stay_inside_the_ask_dir() {
        let natural = "How many rows does the sealed public.batch_c table contain? Write the SQL yourself and run it with oo query.";
        let first = resolve_question("bulk", natural).expect("natural");
        let second = resolve_question("bulk", natural).expect("stable");
        assert_eq!(first.record_id, second.record_id);
        assert!(first.record_id.starts_with("nl-"));
        assert!(!first.record_id.contains('/') && !first.record_id.contains('.'));
        let path = ask_record_path(Path::new("/work/asks"), &first.record_id).expect("path");
        assert_eq!(path.parent(), Some(Path::new("/work/asks")));
        assert!(ask_record_path(Path::new("/work/asks"), "../etc/passwd").is_err());
        assert!(ask_record_path(Path::new("/work/asks"), "..").is_err());
        assert!(ask_record_path(Path::new("/work/asks"), "nl-../x").is_err());
        let frozen = question_text_for("bulk", "count-batch-c").expect("frozen");
        assert_eq!(resolve_question("bulk", frozen).expect("alias").record_id, "count-batch-c");
        assert_eq!(resolve_question("bulk", "batch-a-amount").expect("id").record_id, "batch-a-amount");
        let legacy = resolve_question("bulk", "decimal-row").expect("legacy");
        assert_eq!(legacy.record_id, "decimal-row");
        assert_eq!(legacy.text, question_text_for("bulk", "batch-a-amount").unwrap());
        assert!(resolve_question("bulk", "binary-row").expect("binary").text.contains("blob"));
        assert!(expected_cells("binary-row").is_none());
        assert!(resolve_question("bulk", "not-a-question").is_err());
        assert!(resolve_question("bulk", "count-entry").is_err());
        assert!(resolve_question("shop", "count-batch-c").is_err());
        assert!(resolve_question("configured", "今天 public.batch_c 有多少行？").is_ok());
        assert!(resolve_question("bulk", " ").is_err());
        assert!(resolve_question("bulk", &"a".repeat(2_001)).is_err());
        let prompt = prompt_for(frozen, Path::new("/usr/local/bin/oo"), Path::new("/knowledge"), Path::new("/secrets/token"), "http://127.0.0.1:9");
        assert!(!prompt_leaks(&prompt));
        assert!(prompt.contains("query_id"));
        assert!(prompt.contains("read tool"));
        assert!(prompt.contains("bash tool"));
        assert!(prompt.contains("public proxy marker"));
        assert!(!prompt.contains("SELECT"));
        let root = pi_root(Path::new("/target/a416-work")).expect("sibling");
        assert_eq!(root, Path::new("/target/a416-work-pi"));
        assert!(!root.starts_with(Path::new("/target/a416-work")));
        assert!(!Path::new("/target/a416-work").starts_with(&root));
        assert!(pi_root(Path::new("/")).is_err());
        assert!(pi_root(Path::new("relative-work")).is_err());
        let mut policy = json!({"password_file": "/secrets/reader.pass", "ca_file": "/secrets/ca.crt", "endpoint": {"database": "mirrordb"}});
        shield_policy(&mut policy, Path::new("/work/shield/password"), Path::new("/work/shield/ca.crt")).expect("policy");
        assert_eq!(policy["password_file"], json!("/work/shield/password"));
        assert_eq!(policy["ca_file"], json!("/work/shield/ca.crt"));
        assert_eq!(policy["endpoint"]["database"], json!("mirrordb"));
        assert!(!prompt.contains("shield"));
        assert!(!prompt.contains("reader.pass"));
    }

    #[test]
    fn citation_binds_the_named_query_not_the_first_scalar() {
        let events = vec![tool("read"), tool("bash"), turn("public.batch_c has 90000 rows. query_id q_bbbbbbbb.")];
        let captured = vec![
            query("q_aaaaaaaa", 200, 0, "2000"),
            query("q_bbbbbbbb", 200, 0, "90000"),
        ];
        let judged = judge_turn(&events, &captured);
        assert_eq!(judged.status, "matched");
        assert!(judged.cited);
        assert_eq!(judged.query_id, "q_bbbbbbbb");
        assert_eq!(judged.rows[0][0], json!("90000"));
        let missed = judge_turn(&[tool("read"), tool("bash"), turn("The overlap is 2000.")], &captured);
        assert_eq!(missed.status, "citation.absent");
        assert!(!missed.matched);
        assert!(missed.query_id.is_empty());
        let fixed = judge_turn(&[tool("read"), tool("bash"), turn("query_id q_fixed")], &captured);
        assert_ne!(fixed.status, "matched");
        assert!(fixed.query_id.is_empty());
        let absent = judge_turn(&[tool("read"), tool("bash")], &captured);
        assert_eq!(absent.status, "turn.absent");
        let no_tools = judge_turn(&[turn("query_id q_bbbbbbbb")], &captured);
        assert_eq!(no_tools.status, "tools.absent");
        let no_read = judge_turn(&[tool("bash"), turn("query_id q_bbbbbbbb")], &captured);
        assert_eq!(no_read.status, "tools.incomplete");
        let no_query = judge_turn(&events, &[query("q_cccccccc", 400, 6, "1")]);
        assert_eq!(no_query.status, "query.absent");
        let both = judge_turn(&[tool("read"), tool("bash"), turn("q_aaaaaaaa and q_bbbbbbbb")], &captured);
        assert_eq!(both.status, "citation.ambiguous");
        let follow = followup_text(&captured);
        assert!(follow.contains("q_aaaaaaaa"));
        assert!(follow.contains("q_bbbbbbbb"));
        assert!(!prompt_leaks(&follow));
        assert!(!follow.contains("2000"));
        assert!(!follow.contains("90000"));
    }

    #[test]
    fn released_closed_and_failure_refuse_before_a_model() {
        for status in ["released", "closed", "failure"] {
            let dir = scratch(status);
            let config = write_config(&dir);
            let work = dir.join("work");
            fs::create_dir_all(&work).expect("work");
            fs::write(work.join("state.json"), format!(r#"{{"status":"{status}","selfbuild":"generated_unreviewed"}}"#)).expect("state");
            let error = ask(&config, "count-entry").expect_err(status);
            assert_eq!(error.code, "ask.refused", "{status}");
            assert!(!work.join("proxy-session-map.json").exists(), "{status}");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn draft_ready_accepts_natural_text_and_stops_before_pi() {
        let dir = scratch("draft");
        let config = write_config(&dir);
        let work = dir.join("work");
        fs::create_dir_all(&work).expect("work");
        fs::write(work.join("state.json"), r#"{"status":"draft_ready","selfbuild":"generated_unreviewed","question_set":"bulk"}"#).expect("state");
        let error = ask(&config, "How many rows are in the sealed batch c table?").expect_err("pi");
        assert_eq!(error.code, "pi.absent");
        assert!(!work.join("proxy-session-map.json").exists());
        let unknown = ask(&config, "not-a-question").expect_err("id");
        assert_eq!(unknown.code, "ask.question");
        let _ = fs::remove_dir_all(&dir);
    }

    fn tool(name: &str) -> Value {
        json!({"kind": "tool_call_started", "name": name, "args_summary": "pack"})
    }

    fn turn(answer: &str) -> Value {
        json!({"kind": "turn_finished", "answer": answer})
    }

    fn query(id: &str, http_status: u16, exit: i32, cell: &str) -> CapturedQuery {
        CapturedQuery {
            sql: "statement-bytes".to_owned(),
            query_id: id.to_owned(),
            http_status,
            exit,
            rows: vec![vec![json!(cell)]],
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oo-a416-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn write_config(dir: &Path) -> PathBuf {
        fs::write(dir.join("password"), b"pppppppp").expect("password");
        fs::write(dir.join("ca.pem"), b"ca-bytes\n").expect("ca");
        fs::write(dir.join("bearer"), b"bbbbbbbb").expect("bearer");
        let config = dir.join("loop.json");
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let body = format!(
            "{{\n\
               \"protocol\": \"oo.minimal-product-loop.config/v1\",\n\
               \"mode\": \"operator-development\",\n\
               \"development_preview\": true,\n\
               \"publication\": \"not_published\",\n\
               \"llm_review\": \"unreviewed\",\n\
               \"runtime_acknowledgement\": \"local-process-not-a-sandbox-certificate\",\n\
               \"database\": {{\"type\": \"postgresql\", \"host\": \"127.0.0.1\", \"port\": 9, \"name\": \"mirrordb\", \"username\": \"reader\", \"tls\": \"require\", \"secret_ref\": \"secret://pg/a416/reader\", \"password_file\": \"{password}\", \"ca_file\": \"{ca}\"}},\n\
               \"tables\": [{{\"schema\": \"public\", \"table\": \"batch_c\"}}],\n\
               \"llm\": {{\"model_id\": \"grok-4.7\", \"origin\": \"http://127.0.0.1:8317\", \"path\": \"/v1/chat/completions\", \"bearer_file\": \"{bearer}\", \"deadline_ms\": 30000}},\n\
               \"gateway\": {{\"oo\": \"{oo}\", \"query_pg\": \"{query}\", \"policy_file\": \"{policy}\", \"token_file\": \"{token}\"}},\n\
               \"pi\": {{\"executable\": \"{pi}\", \"executable_sha256\": \"{sha}\", \"runtime\": \"{node}\", \"runtime_sha256\": \"{sha}\", \"provider\": \"grok\", \"model\": \"grok-4.7\"}},\n\
               \"paths\": {{\"catalog_bin\": \"{catalog}\", \"knowledge\": \"{knowledge}\", \"work\": \"{work}\"}},\n\
               \"operator\": {{\"label\": \"a416\", \"at\": \"2026-10-09T00:00:00Z\"}}\n\
             }}\n",
            password = dir.join("password").display(),
            ca = dir.join("ca.pem").display(),
            bearer = dir.join("bearer").display(),
            oo = dir.join("missing-oo").display(),
            query = dir.join("missing-query-pg").display(),
            policy = dir.join("policy.json").display(),
            token = dir.join("token").display(),
            pi = dir.join("missing-pi").display(),
            node = dir.join("missing-node").display(),
            catalog = dir.join("missing-catalog").display(),
            knowledge = dir.join("knowledge").display(),
            work = dir.join("work").display(),
        );
        fs::write(&config, body).expect("config");
        config
    }
}
