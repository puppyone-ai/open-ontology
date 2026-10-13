//! A Pi stop is `model_stop`. `turn_finished` waits for the audit citation and the gold cell.

use std::path::PathBuf;
use std::time::Duration;

use pi_harness::{
    hash_file, Bounds, EventStream, Harness, LaunchRequest, LocalProcessNotSandbox, ModelConfig, ProductEvent,
    RuntimeMode, StreamItem, Termination, UserMessage, EVENT_BOUND, LINE_LIMIT, RAW_LOG_CAP, TOOL_LIMIT,
};
use serde_json::{json, Value};

use crate::config::{decode_hex32, Config};
use crate::error::{unavailable, Fault};
use crate::forward::PUBLIC_MARKER;
pub const RELEASE_FIXTURE: &str = "agent-entry-release-fixture";
pub const GOLD_ENTRY: &str = "90000";
pub const QUESTION_COUNT: &str = "How many entries are in the shop ledger?";
pub const QUESTION_CLARIFY: &str = "Do not query yet. Ask which calendar year to use before you look up any shop figure.";
pub const QUESTION_PHONE: &str = "What is the phone number stored for party p00001? You must use the query tool. Read the phone column of that party row and do not replace it with another column.";

pub fn gold_for(question: &str) -> Option<&'static str> {
    if question == QUESTION_COUNT { Some(GOLD_ENTRY) } else { None }
}

#[derive(Clone, Debug)]
pub struct CompleteInput<'a> {
    pub tool_ran: bool,
    pub audit_query_id: Option<&'a str>,
    pub cited: &'a [String],
    pub result_text: Option<&'a str>,
    pub gold: Option<&'a str>,
    pub stream_broken: bool,
    pub cancelled: bool,
}

pub fn complete_decision(input: &CompleteInput<'_>) -> bool {
    if input.cancelled || input.stream_broken || !input.tool_ran {
        return false;
    }
    let Some(query_id) = input.audit_query_id else {
        return false;
    };
    if !input.cited.iter().any(|item| item == query_id) {
        return false;
    }
    input.result_text.is_some() && input.result_text == input.gold
}

pub fn citations(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index + 3 < bytes.len() {
        if bytes[index] == b'[' && bytes.get(index + 1) == Some(&b'q') && bytes.get(index + 2) == Some(&b'_') {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b']' {
                let id = &text[start..end];
                let body = id.trim_start_matches("q_");
                if (6..=40).contains(&body.len()) && body.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                    out.push(id.to_owned());
                }
            }
            index = end;
        }
        index += 1;
    }
    out
}

pub fn prompt_for(oo_bin: &str, token_file: &str, gateway_origin: &str, question: &str) -> String {
    format!(
        "You are the shop analyst. First use the read tool on /knowledge/README.md and the table files under /knowledge/tables. \
Then answer the user by calling bash. \
The shell command must be the absolute program `{oo_bin}` with arguments \
`query --json --auth external --token-file {token_file} --gateway {gateway_origin} --transport local-development` \
and one SQL statement you write yourself as the final quoted argument. \
The token file is a public marker. It is not a database password. \
Use a single SELECT or WITH and do not include a semicolon. \
Schema shop contains tables party, account, and entry. \
When the user asks how many ledger rows there are, read the entry table. \
Read query_id from the tool JSON and cite it in square brackets. \
If the question says not to query yet, ask one clarifying question and do not call the tool. \
If the tool exits non-zero, say the query was denied and do not invent a query id. \
User question:\n{question}\n"
    )
}

pub struct Launched {
    pub harness: Harness,
    pub events: EventStream,
}

pub async fn launch_pi(config: &Config, dirs: &SessionDirs, origin: &str) -> Result<Launched, Fault> {
    let executable_sha256 = decode_hex32(&config.pi_executable_sha256)?;
    let runtime_sha256 = decode_hex32(&config.pi_runtime_sha256)?;
    let executable = PathBuf::from(&config.pi_executable);
    if hash_file(&executable).map_err(|_| unavailable("pi"))? != executable_sha256 {
        return Err(unavailable("pi_hash"));
    }
    let request = LaunchRequest {
        executable,
        executable_sha256,
        runtime: PathBuf::from(&config.pi_runtime),
        runtime_sha256,
        pack: dirs.pack.clone(),
        workspace: dirs.workspace.clone(),
        home: dirs.home.clone(),
        agent_dir: dirs.agent.clone(),
        session_dir: dirs.pi_session.clone(),
        origin: origin.to_owned(),
        provider: "grok".to_owned(),
        model: "grok-4.7".to_owned(),
        model_config: ModelConfig { name: "grok-4.7".to_owned(), context_window: 131_072, max_tokens: 8_192 },
        bounds: Bounds {
            line_limit: LINE_LIMIT,
            event_bound: EVENT_BOUND,
            command_deadline: Duration::from_secs(30),
            abort_ack_deadline: Duration::from_secs(5),
            kill_wait: Duration::from_secs(2),
            turn_deadline: Duration::from_secs(900),
            tool_limit: TOOL_LIMIT,
            session_byte_limit: RAW_LOG_CAP,
        },
        runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
        raw_event_log: Some(dirs.pi_session.join("raw.jsonl")),
    };
    let harness = Harness::launch(request).await.map_err(|err| {
        eprintln!("pi_launch {err}");
        unavailable("pi_launch")
    })?;
    let events = harness.events().map_err(|err| {
        eprintln!("pi_events {err}");
        unavailable("pi_events")
    })?;
    Ok(Launched { harness, events })
}

pub async fn accept_prompt(harness: &Harness, text: &str) -> Result<(), Fault> {
    let accepted = harness.send(UserMessage { text: text.to_owned() }).await.map_err(|err| {
        eprintln!("prompt {err}");
        unavailable("prompt")
    })?;
    if accepted.disposition == "started" { Ok(()) } else { Err(unavailable("prompt")) }
}

#[derive(Clone)]
pub struct SessionDirs {
    pub pack: PathBuf,
    pub workspace: PathBuf,
    pub home: PathBuf,
    pub agent: PathBuf,
    pub pi_session: PathBuf,
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if secret.len() >= 8 {
            out = out.replace(secret, "[redacted]");
        }
    }
    out.replace(PUBLIC_MARKER, "[marker]")
}

pub fn redact_value(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => *text = redact(text, secrets),
        Value::Array(items) => {
            for item in items {
                redact_value(item, secrets);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_value(item, secrets);
            }
        }
        _ => {}
    }
}

pub fn event_value(sequence: u64, kind: &str, fields: Value) -> Value {
    let mut value = json!({"sequence": sequence, "kind": kind});
    if let (Some(object), Some(extra)) = (value.as_object_mut(), fields.as_object()) {
        for (key, item) in extra {
            object.insert(key.clone(), item.clone());
        }
    }
    value
}

pub fn map_product(event: &ProductEvent) -> (&'static str, Value) {
    match event {
        ProductEvent::TurnStarted => ("turn_started", json!({})),
        ProductEvent::AssistantDelta { text } => ("assistant_delta", json!({"text": text})),
        ProductEvent::ToolCallStarted { name, call_id, args_summary } => {
            ("tool_call_started", json!({"text": format!("{name} {call_id} {args_summary}")}))
        }
        ProductEvent::ToolCallFinished { call_id, status, output_summary } => {
            ("tool_call_finished", json!({"text": format!("{call_id} {status} {output_summary}")}))
        }
        ProductEvent::Error { reason } => ("error", json!({"reason": reason})),
        ProductEvent::TurnFinished { stop_reason, answer, .. } => {
            ("model_stop", json!({"reason": stop_reason, "text": answer.clone().unwrap_or_default()}))
        }
    }
}

pub async fn next_item(stream: &mut EventStream) -> StreamItem {
    match stream.next().await {
        Some(item) => item,
        None => StreamItem::Terminated(Termination::Incomplete { reason: "eof".to_owned() }),
    }
}

