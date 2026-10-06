//! One HTTPS chat completion bound by the description-candidate compiler.
//!
//! This crate does not edit that compiler. It does not cache, retry, or call
//! a second endpoint.

#![forbid(unsafe_code)]

mod chat;
mod config;
mod error;
mod price;
mod transport;

use serde_json::Value;

use chat::{Usage, chat_body, request_view};
use config::Config;

pub use error::ProviderError;
use oo_builder::{
    DESCRIPTION_CANDIDATE_PROTOCOL, DESCRIPTION_REQUEST_PROTOCOL, DescriptionCandidateError,
    complete_description_candidates, prepare_description_request,
};
use transport::Reply;

const COMMAND_MAX: usize = 1_048_576;
const COMPILER: &str = "oo-builder/description-provider/v1";
const PROTOCOL: &str = "oo.builder.description-provider-outcome/v1";

struct Report {
    status: String,
    http_status: Option<u16>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    budget_reason: Option<String>,
    refusal: Option<String>,
}

/// Read the config file, then run one provider call for `command`.
pub fn run_from_config_file(command: &str, path: &str) -> Result<String, ProviderError> {
    let text = Config::read_file(path)?;
    run_description_provider(command, &text)
}

/// Run one provider call. `config_text` is the config object, not a path.
pub fn run_description_provider(command: &str, config_text: &str) -> Result<String, ProviderError> {
    if command.len() > COMMAND_MAX {
        return Err(ProviderError::new(
            "command.binding",
            "the command exceeds the byte budget",
        ));
    }
    let config = Config::parse(config_text)?;
    if contains_token(command, &config.token) {
        return Err(ProviderError::new(
            "config.secret",
            "a secret is present in the local documents",
        ));
    }
    let prepared = prepare_description_request(command).map_err(map_compiler)?;
    if contains_token(&prepared, &config.token) {
        return Err(ProviderError::new(
            "config.secret",
            "a secret is present in the local documents",
        ));
    }
    dispatch(command, &prepared, &config)
}

fn dispatch(command: &str, prepared: &str, config: &Config) -> Result<String, ProviderError> {
    let value: Value = serde_json::from_str(prepared).map_err(|_| {
        ProviderError::new("command.binding", "the prepared request is not JSON")
    })?;
    let protocol = value.get("protocol").and_then(Value::as_str).unwrap_or("");
    if protocol == DESCRIPTION_CANDIDATE_PROTOCOL {
        return wrap_candidate(value, config);
    }
    if protocol != DESCRIPTION_REQUEST_PROTOCOL {
        return Err(ProviderError::new(
            "command.binding",
            "the prepared document is not a request",
        ));
    }
    let view = match request_view(prepared) {
        Ok(view) => view,
        Err(err) if err.code.ends_with("/request.samples") => {
            return complete_local(command, "provider_failure", None, Some("samples"), config);
        }
        Err(err) => return Err(err),
    };
    if view.model_id != config.model_id {
        return complete_local(command, "provider_failure", None, Some("model"), config);
    }
    if view.tables > config.budget.max_tables {
        return complete_local(command, "budget_failure", Some("tables"), None, config);
    }
    let body = match chat_body(&config.model_id, prepared, &view.instructions, &config.budget) {
        Ok(body) => body,
        Err(err) if err.code.ends_with("/budget.request_bytes") => {
            return complete_local(command, "budget_failure", Some("request_bytes"), None, config);
        }
        Err(err) => return Err(err),
    };
    let body_text = std::str::from_utf8(&body).map_err(|_| {
        ProviderError::new("config.schema", "the chat body could not be encoded")
    })?;
    if contains_token(body_text, &config.token) {
        return Err(ProviderError::new(
            "config.secret",
            "a secret is present in the local documents",
        ));
    }
    let reply = runtime_post(config, &body)?;
    complete_reply(command, reply, config)
}

fn wrap_candidate(value: Value, config: &Config) -> Result<String, ProviderError> {
    if value.get("outcome").and_then(Value::as_str) != Some("budget_failure") {
        return Err(ProviderError::new(
            "command.binding",
            "the prepared document is not a request",
        ));
    }
    let reason = match value.get("budget_reason") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(ProviderError::new(
                "command.binding",
                "the prepared document is not a request",
            ));
        }
    };
    emit(
        Report {
            status: "budget_failure".to_owned(),
            http_status: None,
            finish_reason: None,
            usage: None,
            budget_reason: reason,
            refusal: None,
        },
        value,
        config,
    )
}

fn complete_local(
    command: &str,
    status: &str,
    reason: Option<&str>,
    refusal: Option<&str>,
    config: &Config,
) -> Result<String, ProviderError> {
    finish(
        command,
        serde_json::json!({ "status": status }),
        Report {
            status: status.to_owned(),
            http_status: None,
            finish_reason: None,
            usage: None,
            budget_reason: reason.map(str::to_owned),
            refusal: refusal.map(str::to_owned),
        },
        config,
    )
}

fn complete_reply(command: &str, reply: Reply, config: &Config) -> Result<String, ProviderError> {
    let status = reply
        .failure
        .as_ref()
        .map(|failure| failure.status)
        .unwrap_or("complete");
    let captured = if status == "complete" {
        let body = reply.content.clone().ok_or_else(|| {
            ProviderError::new("command.binding", "a complete response has no content")
        })?;
        let byte_len = u64::try_from(body.len()).map_err(|_| {
            ProviderError::new(
                "budget.response_bytes",
                "the response body exceeds the byte budget",
            )
        })?;
        serde_json::json!({
            "body": body,
            "byte_len": byte_len,
            "status": "complete",
        })
    } else {
        serde_json::json!({ "status": status })
    };
    finish(
        command,
        captured,
        Report {
            status: status.to_owned(),
            http_status: reply.http_status,
            finish_reason: reply.finish_reason,
            usage: reply.usage,
            budget_reason: reply
                .failure
                .as_ref()
                .and_then(|failure| failure.reason)
                .map(str::to_owned),
            refusal: reply
                .failure
                .as_ref()
                .and_then(|failure| failure.refusal)
                .map(str::to_owned),
        },
        config,
    )
}

fn finish(
    command: &str,
    captured: Value,
    report: Report,
    config: &Config,
) -> Result<String, ProviderError> {
    let bound = bind_captured(command, &captured)?;
    let document = complete_description_candidates(&bound).map_err(map_compiler)?;
    let candidate = serde_json::from_str(&document).map_err(|_| {
        ProviderError::new("command.binding", "the compiler document is not JSON")
    })?;
    emit(report, candidate, config)
}

fn bind_captured(command: &str, captured: &Value) -> Result<String, ProviderError> {
    let trimmed = command.trim_end();
    let Some(head) = trimmed.strip_suffix('}') else {
        return Err(ProviderError::new(
            "command.binding",
            "the command is not one JSON object",
        ));
    };
    let extra = serde_json::to_string(captured).map_err(|_| {
        ProviderError::new(
            "command.binding",
            "the captured response could not be encoded",
        )
    })?;
    let mut out = String::with_capacity(head.len().saturating_add(extra.len()).saturating_add(22));
    out.push_str(head);
    out.push_str(",\"captured_response\":");
    out.push_str(&extra);
    out.push('}');
    Ok(out)
}

fn runtime_post(config: &Config, body: &[u8]) -> Result<Reply, ProviderError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ProviderError::new("transport", "the local runtime could not start"))?;
    runtime.block_on(transport::post_chat(config, body))
}

fn emit(report: Report, candidate: Value, config: &Config) -> Result<String, ProviderError> {
    let text = outcome_string(&report, &candidate, config)?;
    if contains_token(&text, &config.token) {
        return Err(ProviderError::new(
            "config.secret",
            "a secret is present in the local documents",
        ));
    }
    Ok(text)
}

fn outcome_string(report: &Report, candidate: &Value, config: &Config) -> Result<String, ProviderError> {
    let usage = report.usage.as_ref().map(|usage| {
        serde_json::json!({
            "completion_tokens": usage.completion_tokens,
            "prompt_tokens": usage.prompt_tokens,
            "total_tokens": usage.total_tokens,
        })
    });
    let declared = match &report.usage {
        Some(usage) => {
            let amount = config.price.amount(usage.total_tokens)?;
            Some(serde_json::json!({
                "amount": amount,
                "billing_certificate": false,
                "currency": "USD",
                "per_total_token": config.price.lexeme(),
            }))
        }
        None => None,
    };
    let value = serde_json::json!({
        "candidate": candidate,
        "compiler": COMPILER,
        "protocol": PROTOCOL,
        "transport": {
            "billing_certificate": false,
            "budget_reason": report.budget_reason,
            "caller_residency_approval": "explicit",
            "declared_cost": declared,
            "finish_reason": report.finish_reason,
            "http_status": report.http_status,
            "refusal": report.refusal,
            "residency_certificate": false,
            "sample_policy": "none",
            "status": report.status,
            "usage": usage,
        }
    });
    serde_json::to_string(&value).map_err(|_| {
        ProviderError::new("config.schema", "the outcome could not be encoded")
    })
}

fn contains_token(text: &str, token: &str) -> bool {
    !token.is_empty() && text.as_bytes().windows(token.len()).any(|window| window == token.as_bytes())
}

fn map_compiler(err: DescriptionCandidateError) -> ProviderError {
    ProviderError::from_compiler(err.code, err.message)
}
