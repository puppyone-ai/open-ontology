//! One model call, or a cache hit that does not open a socket.

use serde_json::{json, Value};

use oo_builder::{
    complete_description_candidates, prepare_description_request, DESCRIPTION_REQUEST_PROTOCOL,
    DESCRIPTION_RESPONSE_PROTOCOL,
};

use crate::cache::{read_validated, store_validated};
use crate::canonical::{canonical_json, canonical_sha256};
use crate::cards::approval_matches;
use crate::config::{amount, contains_secret, Config, Price};
use crate::error::PgBuilderError;
use crate::http;
use crate::{field_seam, prompt_sha256, PROMPT, PROMPT_VERSION};

pub struct GenerateIn<'a> {
    pub config: &'a Config,
    pub config_bytes: &'a [u8],
    pub token: &'a str,
    pub command_text: &'a str,
    pub cards: &'a Value,
    pub approval: &'a Value,
    pub cache_dir: &'a std::path::Path,
    pub transport: &'a mut dyn ChatTransport,
}

pub struct RawReply {
    pub http_status: u16,
    pub body: Vec<u8>,
    pub latency_ms: u128,
    pub refusal: Option<&'static str>,
}

pub trait ChatTransport {
    fn post(&mut self, body: &[u8]) -> Result<RawReply, PgBuilderError>;
}

pub struct LoopbackTransport {
    pub config: Config,
    pub token: String,
}

impl ChatTransport for LoopbackTransport {
    fn post(&mut self, body: &[u8]) -> Result<RawReply, PgBuilderError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| PgBuilderError::new("provider.runtime", "the loopback runtime could not start"))?;
        let reply = runtime.block_on(http::post_loopback(
            self.config.addr,
            &self.config.path,
            &self.token,
            body,
            self.config.deadline,
            self.config.max_response_bytes,
        ))?;
        Ok(RawReply {
            http_status: reply.http_status,
            body: reply.body,
            latency_ms: reply.latency_ms,
            refusal: reply.refusal,
        })
    }
}

pub struct ScriptedTransport {
    pub http_status: u16,
    pub content: String,
    pub finish_reason: String,
    pub usage: Option<(u64, u64, u64)>,
    pub calls: u32,
    pub refusal: Option<&'static str>,
}

impl ChatTransport for ScriptedTransport {
    fn post(&mut self, _body: &[u8]) -> Result<RawReply, PgBuilderError> {
        self.calls = self.calls.saturating_add(1);
        if let Some(refusal) = self.refusal {
            return Ok(RawReply {
                http_status: self.http_status,
                body: Vec::new(),
                latency_ms: 1,
                refusal: Some(refusal),
            });
        }
        let usage = self.usage.map(|(prompt, completion, total)| {
            json!({"prompt_tokens": prompt, "completion_tokens": completion, "total_tokens": total})
        });
        let envelope = json!({
            "choices": [{
                "finish_reason": self.finish_reason,
                "message": {"role": "assistant", "content": self.content}
            }],
            "usage": usage.unwrap_or(Value::Null)
        });
        Ok(RawReply {
            http_status: self.http_status,
            body: serde_json::to_vec(&envelope).unwrap_or_default(),
            latency_ms: 4,
            refusal: None,
        })
    }
}

pub fn generate(input: GenerateIn<'_>) -> Result<Value, PgBuilderError> {
    approval_matches(input.cards, input.approval)?;
    if contains_secret(input.config_bytes, input.token) || contains_secret(PROMPT.as_bytes(), input.token) {
        return Err(PgBuilderError::new("config.secret", "the bearer is present outside its file"));
    }
    let request_text = prepare_description_request(input.command_text).map_err(|_| {
        PgBuilderError::new("compiler.request", "the description request was refused")
    })?;
    let request: Value = serde_json::from_str(&request_text)
        .map_err(|_| PgBuilderError::new("compiler.request", "the description request was refused"))?;
    if request.get("protocol").and_then(Value::as_str) != Some(DESCRIPTION_REQUEST_PROTOCOL) {
        let input_sha = crate::sha256_hex(request_text.as_bytes());
        return Ok(failure_document(
            input,
            &input_sha,
            &request_text,
            "budget_failure",
            Some("compiler_request"),
            "not_opened",
        ));
    }
    if request.get("model_id").and_then(Value::as_str) != Some(input.config.model_id.as_str()) {
        return Err(PgBuilderError::new("compiler.model", "the prepared model is not the builder model"));
    }
    if request
        .pointer("/statements/samples")
        .and_then(Value::as_str)
        != Some("not_included")
    {
        return Err(PgBuilderError::new("compiler.samples", "the prepared request includes samples"));
    }
    let cards_hash = canonical_sha256(input.cards);
    let model_input = json!({
        "protocol": "oo.pg-builder.model-input/v1",
        "role": "data",
        "prompt_version": PROMPT_VERSION,
        "prompt_sha256": prompt_sha256(),
        "model_id": input.config.model_id,
        "sample_policy": "none",
        "compiler_request": request,
        "cards": input.cards.get("tables").cloned().unwrap_or(Value::Array(Vec::new()))
    });
    let model_input_text = canonical_json(&model_input);
    if contains_secret(model_input_text.as_bytes(), input.token) {
        return Err(PgBuilderError::new("config.secret", "the bearer is present outside its file"));
    }
    let input_sha = input_hash(input.config, input.cards, &cards_hash, &request_text)?;
    let evidence_hash = input.cards.get("evidence_hash").and_then(Value::as_str).unwrap_or("");
    if let Some(cached) = read_validated(input.cache_dir, &input_sha, evidence_hash)? {
        let mut hit = cached;
        if let Some(map) = hit.as_object_mut() {
            map.insert("cache".to_owned(), Value::String("hit".to_owned()));
            if let Some(transport) = map.get_mut("transport").and_then(Value::as_object_mut) {
                transport.insert("socket".to_owned(), Value::String("not_opened".to_owned()));
                transport.insert("attempt".to_owned(), Value::from(0));
            }
        }
        return Ok(hit);
    }
    let chat = chat_body(input.config, &model_input_text)?;
    let chat_bytes = serde_json::to_vec(&chat)
        .map_err(|_| PgBuilderError::new("budget.request_bytes", "the chat body could not be encoded"))?;
    if chat_bytes.len() > input.config.max_request_bytes {
        return Ok(failure_document(
            input,
            &input_sha,
            &request_text,
            "budget_failure",
            Some("request_bytes"),
            "not_opened",
        ));
    }
    let reply = input.transport.post(&chat_bytes)?;
    let parsed = parse_completion(input.config, input.token, &reply)?;
    finish_generation(input, &input_sha, &request_text, &model_input_text, parsed)
}

fn finish_generation(
    input: GenerateIn<'_>,
    input_sha: &str,
    request_text: &str,
    model_input_text: &str,
    parsed: Parsed,
) -> Result<Value, PgBuilderError> {
    let request: Value = serde_json::from_str(request_text)
        .map_err(|_| PgBuilderError::new("compiler.request", "the description request was refused"))?;
    let raw_hash = if parsed.secret_echo {
        Value::Null
    } else {
        Value::String(crate::sha256_hex(&parsed.raw))
    };
    if parsed.status != "complete" {
        let command = captured_command(input.command_text, parsed.status, None)?;
        return Ok(structural_draft(&input, input_sha, model_input_text, &parsed, raw_hash, command, "not_parsed"));
    }
    let outer: Result<Value, _> = serde_json::from_slice(&parsed.raw);
    let Ok(outer) = outer else {
        let command = captured_command(input.command_text, "complete", Some(&parsed.content))?;
        return Ok(structural_draft(&input, input_sha, model_input_text, &parsed, raw_hash, command, "not_parsed"));
    };
    if !is_s5(&outer) {
        let command = captured_command(input.command_text, "complete", Some(&parsed.content))?;
        return Ok(structural_draft(&input, input_sha, model_input_text, &parsed, raw_hash, command, "rejected"));
    }
    let table_response = outer.get("table_response").cloned().unwrap_or(Value::Null);
    let table_text = canonical_json(&table_response);
    let command = captured_command(input.command_text, "complete", Some(&table_text))?;
    let table_outcome_text = match complete_description_candidates(&command) {
        Ok(text) => text,
        Err(_) => {
            return Ok(document(
                &input,
                input_sha,
                model_input_text,
                Draft {
                    outcome: "structural_draft",
                    stored: "not_stored",
                    parsed: &parsed,
                    raw_hash,
                    fields: Value::Null,
                    table_outcome: Value::Null,
                    field_outcome: "not_parsed",
                    command,
                },
            ));
        }
    };
    let table_outcome: Value = serde_json::from_str(&table_outcome_text)
        .map_err(|_| PgBuilderError::new("compiler.complete", "the description compiler refused the captured body"))?;
    let fields = validate_fields(input.cards, outer.get("field_candidates").unwrap_or(&Value::Null))?;
    let table_ok = table_outcome.get("outcome").and_then(Value::as_str) == Some("candidates");
    let validated = table_ok && fields.0;
    let outcome = if validated { "validated" } else { "structural_draft" };
    let cache_dir = input.cache_dir;
    let mut doc = document(
        &input,
        input_sha,
        model_input_text,
        Draft {
            outcome,
            stored: if validated { "stored" } else { "not_stored" },
            parsed: &parsed,
            raw_hash,
            fields: fields.1,
            table_outcome,
            field_outcome: if fields.0 { "validated" } else { "rejected" },
            command,
        },
    );
    if validated && let Some(map) = doc.as_object_mut() {
        map.insert("evidence_hash".to_owned(), Value::String(evidence_of(&request)));
        map.insert("input_sha256".to_owned(), Value::String(input_sha.to_owned()));
    }
    if validated {
        store_validated(cache_dir, input_sha, &doc)?;
    }
    Ok(doc)
}

struct Parsed {
    status: &'static str,
    refusal: Option<&'static str>,
    finish_reason: Option<String>,
    usage: Option<(u64, u64, u64)>,
    http_status: u16,
    latency_ms: u128,
    raw: Vec<u8>,
    content: String,
    secret_echo: bool,
    budget_reason: Option<&'static str>,
}

fn parse_completion(config: &Config, token: &str, reply: &RawReply) -> Result<Parsed, PgBuilderError> {
    if let Some(refusal) = reply.refusal {
        let status = if refusal == "timeout" || refusal == "unavailable" || refusal == "redirect" {
            "provider_failure"
        } else if refusal == "response_bytes" || refusal == "partial" {
            "budget_failure"
        } else {
            "provider_failure"
        };
        return Ok(Parsed {
            status,
            refusal: Some(refusal),
            finish_reason: None,
            usage: None,
            http_status: reply.http_status,
            latency_ms: reply.latency_ms,
            raw: Vec::new(),
            content: String::new(),
            secret_echo: false,
            budget_reason: if status == "budget_failure" { Some(refusal) } else { None },
        });
    }
    if contains_secret(&reply.body, token) {
        return Ok(Parsed {
            status: "provider_failure",
            refusal: Some("secret_echo"),
            finish_reason: None,
            usage: None,
            http_status: reply.http_status,
            latency_ms: reply.latency_ms,
            raw: Vec::new(),
            content: String::new(),
            secret_echo: true,
            budget_reason: None,
        });
    }
    if reply.http_status != 200 {
        return Ok(Parsed {
            status: "provider_failure",
            refusal: Some("http_status"),
            finish_reason: None,
            usage: None,
            http_status: reply.http_status,
            latency_ms: reply.latency_ms,
            raw: reply.body.clone(),
            content: String::new(),
            secret_echo: false,
            budget_reason: None,
        });
    }
    let value: Value = match serde_json::from_slice(&reply.body) {
        Ok(value) => value,
        Err(_) => {
            return Ok(empty_parse("provider_failure", Some("malformed_envelope"), reply));
        }
    };
    let choices = value.get("choices").and_then(Value::as_array);
    let Some(choices) = choices else {
        return Ok(empty_parse("incomplete", None, reply));
    };
    if choices.len() != 1 {
        return Ok(empty_parse("provider_failure", Some("malformed_envelope"), reply));
    }
    let choice = &choices[0];
    let finish = choice.get("finish_reason").and_then(Value::as_str).map(str::to_owned);
    let content = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if contains_secret(content.as_bytes(), token) {
        return Ok(Parsed {
            status: "provider_failure",
            refusal: Some("secret_echo"),
            finish_reason: finish,
            usage: None,
            http_status: 200,
            latency_ms: reply.latency_ms,
            raw: Vec::new(),
            content: String::new(),
            secret_echo: true,
            budget_reason: None,
        });
    }
    let usage = match usage_of(&value) {
        UsageRead::Consistent(usage) => Some(usage),
        UsageRead::Absent => None,
        UsageRead::Bad => {
            return Ok(empty_parse("provider_failure", Some("malformed_envelope"), reply));
        }
    };
    if let Some((prompt, completion, total)) = usage
        && (prompt > config.max_prompt_tokens
            || completion > config.max_completion_tokens
            || total > config.max_total_tokens)
    {
            return Ok(Parsed {
                status: "budget_failure",
                refusal: None,
                finish_reason: finish,
                usage: Some((prompt, completion, total)),
                http_status: 200,
                latency_ms: reply.latency_ms,
                raw: content.as_bytes().to_vec(),
                content,
                secret_echo: false,
                budget_reason: Some("tokens"),
            });
    }
    let status = match finish.as_deref() {
        Some("stop") if !content.is_empty() && usage.is_some() => "complete",
        Some("length") => "truncated",
        _ => "incomplete",
    };
    Ok(Parsed {
        status,
        refusal: None,
        finish_reason: finish,
        usage,
        http_status: 200,
        latency_ms: reply.latency_ms,
        raw: content.as_bytes().to_vec(),
        content,
        secret_echo: false,
        budget_reason: None,
    })
}

enum UsageRead {
    Consistent((u64, u64, u64)),
    Absent,
    Bad,
}

fn usage_of(value: &Value) -> UsageRead {
    let Some(usage) = value.get("usage") else {
        return UsageRead::Absent;
    };
    if usage.is_null() {
        return UsageRead::Absent;
    }
    let Some(map) = usage.as_object() else {
        return UsageRead::Bad;
    };
    let Some(prompt) = map.get("prompt_tokens").and_then(Value::as_u64) else {
        return UsageRead::Bad;
    };
    let Some(completion) = map.get("completion_tokens").and_then(Value::as_u64) else {
        return UsageRead::Bad;
    };
    let Some(total) = map.get("total_tokens").and_then(Value::as_u64) else {
        return UsageRead::Bad;
    };
    if prompt.saturating_add(completion) == total {
        UsageRead::Consistent((prompt, completion, total))
    } else {
        UsageRead::Bad
    }
}

fn empty_parse(status: &'static str, refusal: Option<&'static str>, reply: &RawReply) -> Parsed {
    Parsed {
        status,
        refusal,
        finish_reason: None,
        usage: None,
        http_status: reply.http_status,
        latency_ms: reply.latency_ms,
        raw: reply.body.clone(),
        content: String::new(),
        secret_echo: false,
        budget_reason: None,
    }
}

fn is_s5(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    map.get("protocol").and_then(Value::as_str) == Some("oo.pg-builder.s5-response/v1")
        && map.get("complete") == Some(&Value::Bool(true))
        && map.get("table_response").is_some()
        && map.get("field_candidates").is_some()
        && map.len() == 4
}

fn validate_fields(cards: &Value, fields: &Value) -> Result<(bool, Value), PgBuilderError> {
    let Some(rows) = fields.as_array() else {
        return Ok((false, Value::Array(Vec::new())));
    };
    let mut expected = Vec::new();
    let no_tables = Vec::new();
    let tables = cards.get("tables").and_then(Value::as_array).unwrap_or(&no_tables);
    for table in tables {
        let dataset = table.get("dataset").and_then(Value::as_str).unwrap_or("");
        let card_sha = table.get("card_sha256").and_then(Value::as_str).unwrap_or("");
        let no_columns = Vec::new();
        let columns = table.get("columns").and_then(Value::as_array).unwrap_or(&no_columns);
        for column in columns {
            expected.push((
                dataset.to_owned(),
                column.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
                column.get("attnum").cloned().unwrap_or(Value::Null),
                card_sha.to_owned(),
            ));
        }
    }
    if rows.len() != expected.len() {
        return Ok((false, Value::Array(rows.clone())));
    }
    let mut clean = Vec::new();
    for row in rows {
        let Some(map) = row.as_object() else {
            return Ok((false, Value::Array(Vec::new())));
        };
        if map.len() != 9
            || map.keys().any(|key| {
                ![
                    "attnum",
                    "card_sha256",
                    "caveats",
                    "column",
                    "confidence",
                    "dataset",
                    "description",
                    "label",
                    "open_questions",
                ]
                .contains(&key.as_str())
            })
        {
            return Ok((false, Value::Array(Vec::new())));
        }
        let dataset = map.get("dataset").and_then(Value::as_str).unwrap_or("");
        let column = map.get("column").and_then(Value::as_str).unwrap_or("");
        let card = map.get("card_sha256").and_then(Value::as_str).unwrap_or("");
        let attnum = map.get("attnum").cloned().unwrap_or(Value::Null);
        if !expected.iter().any(|item| item.0 == dataset && item.1 == column && item.2 == attnum && item.3 == card)
        {
            return Ok((false, Value::Array(Vec::new())));
        }
        if !text_ok(map.get("label"), 256)
            || !text_ok(map.get("description"), 4000)
            || !confidence_ok(map.get("confidence").and_then(Value::as_str).unwrap_or(""))
            || !list_ok(map.get("caveats"), 8, 2000)
            || !list_ok(map.get("open_questions"), 8, 2000)
        {
            return Ok((false, Value::Array(Vec::new())));
        }
        clean.push(row.clone());
    }
    let mut seen = std::collections::BTreeSet::new();
    for row in &clean {
        let key = format!(
            "{}:{}",
            row.get("dataset").and_then(Value::as_str).unwrap_or(""),
            row.get("column").and_then(Value::as_str).unwrap_or("")
        );
        if !seen.insert(key) {
            return Ok((false, Value::Array(Vec::new())));
        }
    }
    if seen.len() != expected.len() {
        return Ok((false, Value::Array(Vec::new())));
    }
    clean.sort_by(|left, right| {
        (
            left.get("dataset").and_then(Value::as_str),
            left.get("attnum").and_then(Value::as_i64),
        )
            .cmp(&(
                right.get("dataset").and_then(Value::as_str),
                right.get("attnum").and_then(Value::as_i64),
            ))
    });
    Ok((true, Value::Array(clean)))
}

fn text_ok(value: Option<&Value>, max_scalars: usize) -> bool {
    let Some(text) = value.and_then(Value::as_str) else {
        return false;
    };
    let count = text.chars().count();
    if count == 0 || count > max_scalars {
        return false;
    }
    if text.chars().any(|ch| {
        let code = u32::from(ch);
        ch == '\u{0000}' || (code < 0x20 && ch != '\t' && ch != '\n') || ch == '\u{007f}'
    }) {
        return false;
    }
    !(text.contains('`')
        || text.contains("{{")
        || text.contains("}}")
        || text.contains("${")
        || text.contains("$(")
        || text.contains("%{"))
}

fn list_ok(value: Option<&Value>, max_items: usize, max_scalars: usize) -> bool {
    let Some(items) = value.and_then(Value::as_array) else {
        return false;
    };
    items.len() <= max_items && items.iter().all(|item| text_ok(Some(item), max_scalars))
}

fn confidence_ok(text: &str) -> bool {
    if text == "0" || text == "1" {
        return true;
    }
    let Some((whole, frac)) = text.split_once('.') else {
        return false;
    };
    if frac.is_empty() || frac.len() > 4 || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    (whole == "0") || (whole == "1" && frac.bytes().all(|byte| byte == b'0'))
}

fn chat_body(config: &Config, user: &str) -> Result<Value, PgBuilderError> {
    Ok(json!({
        "model": config.model_id,
        "temperature": 0,
        "stream": false,
        "max_completion_tokens": config.max_completion_tokens,
        "messages": [
            {"role": "system", "content": PROMPT},
            {"role": "user", "content": user}
        ],
        "response_format": {"type": "json_object"}
    }))
}

fn captured_command(command: &str, status: &str, body: Option<&str>) -> Result<String, PgBuilderError> {
    let mut value: Value = serde_json::from_str(command)
        .map_err(|_| PgBuilderError::new("compiler.request", "the description command is not JSON"))?;
    let map = value
        .as_object_mut()
        .ok_or_else(|| PgBuilderError::new("compiler.request", "the description command is not JSON"))?;
    if body.is_none() {
        map.insert(
            "captured_response".to_owned(),
            json!({"status": status}),
        );
    } else {
        let body = body.unwrap_or("");
        map.insert(
            "captured_response".to_owned(),
            json!({"status": status, "body": body, "byte_len": body.len()}),
        );
    }
    Ok(canonical_json(&value))
}

fn input_hash(config: &Config, cards: &Value, cards_hash: &str, request_text: &str) -> Result<String, PgBuilderError> {
    let request: Value = serde_json::from_str(request_text)
        .map_err(|_| PgBuilderError::new("compiler.request", "the description request was refused"))?;
    let key = json!({
        "protocol": "oo.pg-builder.cache-key/v1",
        "card_schema": crate::CARD_SCHEMA,
        "prompt_version": PROMPT_VERSION,
        "prompt_sha256": prompt_sha256(),
        "model_id": config.model_id,
        "sample_policy": "none",
        "adapter": "loopback_http_local_dev",
        "origin": config.origin,
        "path": config.path,
        "namespace": "pg-builder",
        "evidence_hash": cards.get("evidence_hash").cloned().unwrap_or(Value::Null),
        "schema_hash": cards.get("schema_hash").cloned().unwrap_or(Value::Null),
        "cards_sha256": cards_hash,
        "request_sha256": request.get("request_sha256").cloned().unwrap_or(Value::Null)
    });
    Ok(canonical_sha256(&key))
}

fn evidence_of(request: &Value) -> String {
    request
        .get("evidence_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

struct Draft<'a> {
    outcome: &'a str,
    stored: &'a str,
    parsed: &'a Parsed,
    raw_hash: Value,
    fields: Value,
    table_outcome: Value,
    field_outcome: &'a str,
    command: String,
}

fn structural_draft(
    input: &GenerateIn<'_>,
    input_sha: &str,
    model_input_text: &str,
    parsed: &Parsed,
    raw_hash: Value,
    command: String,
    field_outcome: &str,
) -> Value {
    let table = complete_description_candidates(&command).unwrap_or_else(|_| "{}".to_owned());
    document(
        input,
        input_sha,
        model_input_text,
        Draft {
            outcome: "structural_draft",
            stored: "not_stored",
            parsed,
            raw_hash,
            fields: Value::Null,
            table_outcome: serde_json::from_str(&table).unwrap_or(Value::Null),
            field_outcome,
            command,
        },
    )
}

fn document(input: &GenerateIn<'_>, input_sha: &str, model_input_text: &str, draft: Draft<'_>) -> Value {
    let parsed = draft.parsed;
    let usage = parsed.usage.map(|(prompt, completion, total)| {
        json!({"prompt_tokens": prompt, "completion_tokens": completion, "total_tokens": total})
    });
    let tokens = parsed.usage.map(|(_, _, total)| total).unwrap_or(0);
    let cost_amount = amount(&input.config.price, tokens).unwrap_or(Value::Null);
    let amount_value = if matches!(input.config.price, Price::Unknown) {
        Value::Null
    } else if parsed.usage.is_some() {
        cost_amount
    } else {
        Value::Null
    };
    let candidates = json!({
        "field": draft.fields,
        "table": draft.table_outcome.get("candidates").cloned().unwrap_or(Value::Array(Vec::new()))
    });
    json!({
        "protocol": "oo.pg-builder.generation/v1",
        "compiler": "oo.pg-builder/v1",
        "outcome": draft.outcome,
        "cache": "miss",
        "cache_store": draft.stored,
        "input_sha256": input_sha,
        "repair": "not_performed",
        "response_protocol_checked": DESCRIPTION_RESPONSE_PROTOCOL,
        "transport": {
            "adapter": "loopback_http_local_dev",
            "model_id": input.config.model_id,
            "endpoint_origin": input.config.origin,
            "endpoint_path": input.config.path,
            "namespace": "pg-builder",
            "request_input_sha256": crate::sha256_hex(model_input_text.as_bytes()),
            "response_sha256": draft.raw_hash,
            "usage": usage.unwrap_or(Value::Null),
            "latency_ms": parsed.latency_ms,
            "attempt": 1,
            "finish_reason": parsed.finish_reason.clone().map(Value::String).unwrap_or(Value::Null),
            "status": parsed.status,
            "http_status": if parsed.http_status == 0 { Value::Null } else { Value::from(parsed.http_status) },
            "refusal": parsed.refusal.map(|text| Value::String(text.to_owned())).unwrap_or(Value::Null),
            "budget_reason": parsed.budget_reason.map(|text| Value::String(text.to_owned())).unwrap_or(Value::Null),
            "socket": "opened",
            "declared_cost": {
                "status": match input.config.price {
                    Price::Unknown => "unknown",
                    Price::Declared { .. } => "declared_non_billing",
                },
                "billing_certificate": false,
                "invoice": false,
                "amount": amount_value,
                "currency": match &input.config.price {
                    Price::Unknown => Value::Null,
                    Price::Declared { .. } => Value::String("USD".to_owned()),
                },
                "per_total_token": match &input.config.price {
                    Price::Unknown => Value::Null,
                    Price::Declared { per_total_token, .. } => Value::String(per_total_token.clone()),
                }
            }
        },
        "candidates": candidates,
        "candidate_sha256": canonical_sha256(&candidates),
        "table_outcome": draft.table_outcome,
        "field_outcome": draft.field_outcome,
        "field_publication_seam": field_seam(),
        "complete_command": draft.command,
        "statements": statements()
    })
}

fn failure_document(
    input: GenerateIn<'_>,
    input_sha: &str,
    request_text: &str,
    status: &str,
    budget_reason: Option<&str>,
    socket: &str,
) -> Value {
    let parsed = Parsed {
        status: "budget_failure",
        refusal: None,
        finish_reason: None,
        usage: None,
        http_status: 0,
        latency_ms: 0,
        raw: Vec::new(),
        content: String::new(),
        secret_echo: false,
        budget_reason: None,
    };
    let mut doc = document(
        &input,
        input_sha,
        request_text,
        Draft {
            outcome: "structural_draft",
            stored: "not_stored",
            parsed: &parsed,
            raw_hash: Value::Null,
            fields: Value::Array(Vec::new()),
            table_outcome: Value::Null,
            field_outcome: "not_parsed",
            command: String::new(),
        },
    );
    if let Some(map) = doc.as_object_mut()
        && let Some(transport) = map.get_mut("transport").and_then(Value::as_object_mut)
    {
        transport.insert("status".to_owned(), Value::String(status.to_owned()));
        transport.insert("socket".to_owned(), Value::String(socket.to_owned()));
        transport.insert(
            "budget_reason".to_owned(),
            budget_reason.map(|item| Value::String(item.to_owned())).unwrap_or(Value::Null),
        );
    }
    doc
}

fn statements() -> Value {
    json!({
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "parent_auth": "required",
        "reviewer": "declared_only",
        "grants": "not_authorized",
        "publication": "unavailable",
        "pii_classification": "not_performed",
        "semantic_classification": "not_performed",
        "curated": "not_written",
        "effective": "not_written",
        "pack": "not_compiled",
        "samples": "not_included",
        "llm_decides_review": false,
        "billing_certificate": false,
        "repair": "not_performed"
    })
}

pub fn cache_hit_same(left: &Value, right: &Value) -> bool {
    left.get("candidate_sha256") == right.get("candidate_sha256")
        && left.get("candidates") == right.get("candidates")
}
