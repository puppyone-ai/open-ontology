#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use oo_local_llm::{
    dialogue_bearer_class, exact_model, https_once, is_sql_candidate, model_id, read_regular, reject_redirect_env,
    select_model, visible_ascii, Fault, Limits, CHAT_PATH, MODELS_PATH, REQUIRED_MODEL, SYNTHETIC_USER, SYSTEM_PROMPT,
};

fn main() -> ExitCode {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build();
    let Ok(runtime) = runtime else {
        eprintln!("oo-local-llm-models: io");
        return ExitCode::from(1);
    };
    match runtime.block_on(run()) {
        Ok(()) => {
            println!("oo-local-llm-models: ok");
            ExitCode::SUCCESS
        }
        Err(fault) => {
            eprintln!("oo-local-llm-models: {fault}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<(), Fault> {
    reject_redirect_env()?;
    let options = options()?;
    let ca = read_regular(&options.ca, 65536)?;
    if ca.windows(b"PRIVATE KEY".len()).any(|window| window == b"PRIVATE KEY") {
        return Err(Fault::config());
    }
    let bearer = match &options.bearer_file {
        Some(path) => Some(read_bearer(path)?),
        None => None,
    };
    let limits = Limits::standard();
    let reply = https_once(
        &options.origin,
        MODELS_PATH,
        &ca,
        bearer.as_deref(),
        None,
        limits,
    )
    .await?;
    let (selection, parse) = match select_model(&reply.body) {
        Ok(selection) => (selection, "ok"),
        Err(_) => (
            oo_local_llm::Selection {
                selected: None,
                candidate_count: 0,
                grammar_match_count: 0,
            },
            "fail",
        ),
    };
    let exact = if reply.status == 200 && parse == "ok" {
        exact_model(&reply.body, REQUIRED_MODEL).ok().flatten()
    } else {
        None
    };
    let bearer_len = bearer.as_ref().map(String::len);
    let class = bearer_len.map(dialogue_bearer_class).unwrap_or("absent");
    let document = models_document(reply.status, &selection, exact.as_deref(), bearer_len, class, parse);
    write_clean(&options.out, &document, bearer.as_deref())?;
    if let Some(chat_out) = &options.chat_out {
        let chat = if reply.status != 200 {
            "{\"protocol\":\"oo.local-llm-relay.chat-status/v1\",\"skipped\":\"models_status\"}\n".to_string()
        } else {
            chat_document(&options.origin, &ca, bearer.as_deref(), exact.as_deref(), limits).await?
        };
        write_clean(chat_out, &chat, bearer.as_deref())?;
    }
    Ok(())
}

fn models_document(
    status: u16,
    selection: &oo_local_llm::Selection,
    exact: Option<&str>,
    bearer_len: Option<usize>,
    class: &str,
    parse: &str,
) -> String {
    let selected = match exact {
        Some(id) => format!("\"{id}\""),
        None => "null".to_string(),
    };
    let available = if exact.is_some() { "true" } else { "false" };
    let bearer_bytes = bearer_len.map(|len| len.to_string()).unwrap_or_else(|| "null".to_string());
    let count = selection.candidate_count;
    let grammar = selection.grammar_match_count;
    format!(
        "{{\"protocol\":\"oo.local-llm-relay.models/v1\",\"http_status\":{status},\"required_model\":\"{REQUIRED_MODEL}\",\"model_available\":{available},\"selected_model_id\":{selected},\"candidate_count\":{count},\"grammar_match_count\":{grammar},\"bearer_bytes\":{bearer_bytes},\"dialogue_bearer\":\"{class}\",\"parse\":\"{parse}\"}}\n"
    )
}

async fn chat_document(
    origin: &str,
    ca: &[u8],
    bearer: Option<&str>,
    model: Option<&str>,
    limits: Limits,
) -> Result<String, Fault> {
    let Some(model) = model else {
        return Ok(
            "{\"protocol\":\"oo.local-llm-relay.chat-status/v1\",\"skipped\":\"model_unavailable\"}\n".to_string(),
        );
    };
    if model != REQUIRED_MODEL {
        return Ok(
            "{\"protocol\":\"oo.local-llm-relay.chat-status/v1\",\"skipped\":\"model_unavailable\"}\n".to_string(),
        );
    };
    if !model_id(model) {
        return Ok(
            "{\"protocol\":\"oo.local-llm-relay.chat-status/v1\",\"skipped\":\"model_id\"}\n".to_string(),
        );
    }
    let body = chat_body(model)?;
    if let Some(secret) = bearer
        && body.windows(secret.len()).any(|window| window == secret.as_bytes())
    {
        return Err(Fault::secret());
    }
    match https_once(origin, CHAT_PATH, ca, bearer, Some(&body), limits).await {
        Ok(reply) => {
            let candidate = is_sql_candidate(&reply.body);
            let status = reply.status;
            Ok(format!(
                "{{\"protocol\":\"oo.local-llm-relay.chat-status/v1\",\"http_status\":{status},\"candidate\":{candidate}}}\n"
            ))
        }
        Err(fault) => Ok(format!(
            "{{\"protocol\":\"oo.local-llm-relay.chat-status/v1\",\"http_status\":null,\"candidate\":false,\"error\":\"{}\"}}\n",
            fault.token()
        )),
    }
}

fn chat_body(model: &str) -> Result<Vec<u8>, Fault> {
    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "stream": false,
        "max_completion_tokens": 256,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": SYNTHETIC_USER}
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "oo_ontology_dialogue_sql_candidate_v1",
                "strict": true,
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["protocol", "sql"],
                    "properties": {
                        "protocol": {"type": "string"},
                        "sql": {"type": "string"}
                    }
                }
            }
        }
    });
    serde_json::to_vec(&body).map_err(|_| Fault::config())
}

fn write_clean(path: &std::path::Path, text: &str, bearer: Option<&str>) -> Result<(), Fault> {
    if let Some(secret) = bearer
        && !secret.is_empty()
        && text.contains(secret)
    {
        return Err(Fault::secret());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|_| Fault::io())?;
    }
    fs::write(path, text).map_err(|_| Fault::io())
}

fn read_bearer(path: &std::path::Path) -> Result<String, Fault> {
    let mut bytes = read_regular(path, oo_local_llm::BEARER_MAX + 1)?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if !visible_ascii(&bytes) {
        return Err(Fault::config());
    }
    String::from_utf8(bytes).map_err(|_| Fault::config())
}

struct Options {
    origin: String,
    ca: PathBuf,
    bearer_file: Option<PathBuf>,
    out: PathBuf,
    chat_out: Option<PathBuf>,
}

fn options() -> Result<Options, Fault> {
    let mut args = env::args().skip(1);
    let mut origin = None;
    let mut ca = None;
    let mut bearer_file = None;
    let mut out = None;
    let mut chat_out = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(Fault::config)?;
        match flag.as_str() {
            "--origin" if origin.is_none() => origin = Some(value),
            "--ca" if ca.is_none() => ca = Some(PathBuf::from(value)),
            "--bearer-file" if bearer_file.is_none() => bearer_file = Some(PathBuf::from(value)),
            "--out" if out.is_none() => out = Some(PathBuf::from(value)),
            "--chat-out" if chat_out.is_none() => chat_out = Some(PathBuf::from(value)),
            _ => return Err(Fault::config()),
        }
    }
    Ok(Options {
        origin: origin.ok_or_else(Fault::config)?,
        ca: ca.ok_or_else(Fault::config)?,
        bearer_file,
        out: out.ok_or_else(Fault::config)?,
        chat_out,
    })
}
