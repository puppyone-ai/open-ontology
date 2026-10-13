//! One model call per table batch, or a cache hit that does not open a socket.

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
        let mut last = None;
        for _ in 0..3 {
            let reply = self.post_once(body)?;
            let broken = reply.http_status == 0
                && matches!(reply.refusal, Some("malformed_envelope" | "unavailable" | "partial"));
            if !broken {
                return Ok(reply);
            }
            last = Some(reply);
        }
        Ok(last.unwrap_or(RawReply {
            http_status: 0,
            body: Vec::new(),
            latency_ms: 0,
            refusal: Some("malformed_envelope"),
        }))
    }
}

impl LoopbackTransport {
    fn post_once(&self, body: &[u8]) -> Result<RawReply, PgBuilderError> {
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
    pub bodies: Vec<Vec<u8>>,
}

impl ChatTransport for ScriptedTransport {
    fn post(&mut self, body: &[u8]) -> Result<RawReply, PgBuilderError> {
        self.bodies.push(body.to_vec());
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
        let requested_model = serde_json::from_slice::<Value>(body).ok()
            .and_then(|request| request.get("model").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();
        let envelope = json!({
            "id": "chatcmpl-scripted",
            "model": requested_model,
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

pub fn column_batch_ranges(column_count: usize) -> Vec<(usize, usize)> {
    const BATCH: usize = 8;
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < column_count {
        let end = (start + BATCH).min(column_count);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

pub fn generate(mut input: GenerateIn<'_>) -> Result<Value, PgBuilderError> {
    approval_matches(input.cards, input.approval)?;
    if contains_secret(input.config_bytes, input.token) || contains_secret(PROMPT.as_bytes(), input.token) {
        return Err(PgBuilderError::new("config.secret", "the bearer is present outside its file"));
    }
    let tables = input
        .cards
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("approval.schema", "the cards have no tables"))?
        .clone();
    if tables.is_empty() {
        return Err(PgBuilderError::new("approval.schema", "the cards have no tables"));
    }
    if tables.len() > input.config.max_tables {
        return Err(PgBuilderError::new("config.budget", "the card list is above the table budget"));
    }
    let mut docs = Vec::new();
    for table in &tables {
        docs.extend(generate_table(&mut input, table)?);
    }
    if docs.len() == 1 {
        return Ok(docs.remove(0));
    }
    Ok(aggregate(input.config, &docs))
}

fn generate_table(input: &mut GenerateIn<'_>, table: &Value) -> Result<Vec<Value>, PgBuilderError> {
    let dataset = table.get("dataset").and_then(Value::as_str).unwrap_or("");
    let column_count = table.get("columns").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let ranges = column_batch_ranges(column_count);
    if ranges.is_empty() {
        return Err(PgBuilderError::new("compiler.columns", "a selected table has no columns"));
    }
    let command_text = command_for_table(input.command_text, dataset)?;
    let request_text = prepare_description_request(&command_text).map_err(|_| {
        PgBuilderError::new("compiler.request", "the description request was refused")
    })?;
    let request: Value = serde_json::from_str(&request_text)
        .map_err(|_| PgBuilderError::new("compiler.request", "the description request was refused"))?;
    let batch_count = ranges.len();
    if request.get("protocol").and_then(Value::as_str) != Some(DESCRIPTION_REQUEST_PROTOCOL) {
        let input_sha = crate::sha256_hex(request_text.as_bytes());
        let scope = BatchScope { table, column_from: 0, column_to: column_count, batch_index: 0, batch_count, dataset };
        return Ok(vec![failure_document(
            input,
            &input_sha,
            &request_text,
            "budget_failure",
            Some("compiler_request"),
            "not_opened",
            &scope,
        )]);
    }
    if request.get("model_id").and_then(Value::as_str) != Some(input.config.model_id.as_str()) {
        return Err(PgBuilderError::new("compiler.model", "the prepared model is not the builder model"));
    }
    if request.pointer("/statements/samples").and_then(Value::as_str) != Some("not_included") {
        return Err(PgBuilderError::new("compiler.samples", "the prepared request includes samples"));
    }
    let mut docs = Vec::new();
    for (batch_index, (column_from, column_to)) in ranges.into_iter().enumerate() {
        let scope = BatchScope { table, column_from, column_to, batch_index, batch_count, dataset };
        docs.push(generate_batch(input, &command_text, &request_text, &request, &scope)?);
    }
    Ok(docs)
}

fn generate_batch(
    input: &mut GenerateIn<'_>,
    command_text: &str,
    request_text: &str,
    request: &Value,
    scope: &BatchScope<'_>,
) -> Result<Value, PgBuilderError> {
    let model_input = model_input(input.config, request, scope)?;
    let model_input_text = canonical_json(&model_input);
    if contains_secret(model_input_text.as_bytes(), input.token) {
        return Err(PgBuilderError::new("config.secret", "the bearer is present outside its file"));
    }
    let input_sha = input_hash(input.config, input.cards, request, scope)?;
    let evidence_hash = input.cards.get("evidence_hash").and_then(Value::as_str).unwrap_or("");
    if let Some(cached) = read_validated(input.cache_dir, &input_sha, evidence_hash)? {
        return Ok(mark_hit(cached));
    }
    let chat = chat_body(input.config, &model_input_text)?;
    let chat_bytes = serde_json::to_vec(&chat)
        .map_err(|_| PgBuilderError::new("budget.request_bytes", "the chat body could not be encoded"))?;
    if chat_bytes.len() > input.config.max_request_bytes {
        return Ok(failure_document(
            input,
            &input_sha,
            request_text,
            "budget_failure",
            Some("request_bytes"),
            "not_opened",
            scope,
        ));
    }
    let reply = input.transport.post(&chat_bytes)?;
    let parsed = parse_completion(input.config, input.token, &reply)?;
    finish_generation(input, &input_sha, command_text, &model_input_text, parsed, scope)
}

fn mark_hit(mut hit: Value) -> Value {
    if let Some(map) = hit.as_object_mut() {
        map.insert("cache".to_owned(), Value::String("hit".to_owned()));
        if let Some(transport) = map.get_mut("transport").and_then(Value::as_object_mut) {
            transport.insert("socket".to_owned(), Value::String("not_opened".to_owned()));
            transport.insert("attempt".to_owned(), Value::from(0));
        }
    }
    hit
}

struct BatchScope<'a> {
    table: &'a Value,
    column_from: usize,
    column_to: usize,
    batch_index: usize,
    batch_count: usize,
    dataset: &'a str,
}

fn command_for_table(command_text: &str, dataset: &str) -> Result<String, PgBuilderError> {
    let value: Value = serde_json::from_str(command_text).map_err(|_| {
        PgBuilderError::new("compiler.request", "the description command is not JSON")
    })?;
    let tables = value
        .pointer("/selection/tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("compiler.request", "the description command has no tables"))?;
    if tables.len() == 1 {
        let only = tables[0].get("dataset").and_then(Value::as_str).unwrap_or("");
        if only != dataset {
            return Err(PgBuilderError::new("compiler.request", "the description command does not name the table"));
        }
        return Ok(command_text.to_owned());
    }
    let kept: Vec<Value> = tables
        .iter()
        .filter(|table| table.get("dataset").and_then(Value::as_str) == Some(dataset))
        .cloned()
        .collect();
    if kept.len() != 1 {
        return Err(PgBuilderError::new("compiler.request", "the description command does not name the table"));
    }
    let mut narrowed = value;
    let selection = narrowed
        .pointer_mut("/selection/tables")
        .ok_or_else(|| PgBuilderError::new("compiler.request", "the description command has no tables"))?;
    *selection = Value::Array(kept);
    Ok(canonical_json(&narrowed))
}

fn model_input(config: &Config, request: &Value, scope: &BatchScope<'_>) -> Result<Value, PgBuilderError> {
    let columns = scope
        .table
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("compiler.columns", "a selected table has no columns"))?;
    if scope.column_to > columns.len() || scope.column_from >= scope.column_to {
        return Err(PgBuilderError::new("compiler.columns", "the column batch is outside the table"));
    }
    let mut minimal = Vec::new();
    for column in &columns[scope.column_from..scope.column_to] {
        minimal.push(json!({
            "attnum": column.get("attnum").cloned().unwrap_or(Value::Null),
            "correlation": column.get("correlation").cloned().unwrap_or(Value::Null),
            "n_distinct": column.get("n_distinct").cloned().unwrap_or(Value::Null),
            "name": column.get("name").cloned().unwrap_or(Value::Null),
            "null_frac": column.get("null_frac").cloned().unwrap_or(Value::Null),
            "type_name": column.get("type_name").cloned().unwrap_or(Value::Null),
            "untrusted_comment": column.get("untrusted_comment").cloned().unwrap_or(Value::Null)
        }));
    }
    Ok(json!({
        "protocol": "oo.pg-builder.model-input/v2",
        "role": "data",
        "prompt_version": PROMPT_VERSION,
        "prompt_sha256": prompt_sha256(),
        "model_id": config.model_id,
        "sample_policy": "none",
        "batch": {
            "column_from": scope.column_from,
            "column_to": scope.column_to,
            "count": scope.batch_count,
            "index": scope.batch_index
        },
        "echo": {
            "budget_sha256": request.get("budget_sha256").cloned().unwrap_or(Value::Null),
            "compiler_card_sha256": request.pointer("/boundary/data/tables/0/card_sha256").cloned().unwrap_or(Value::Null),
            "dataset": scope.dataset,
            "evidence_card_sha256": scope.table.get("card_sha256").cloned().unwrap_or(Value::Null),
            "evidence_hash": request.get("evidence_hash").cloned().unwrap_or(Value::Null),
            "model_id": request.get("model_id").cloned().unwrap_or(Value::Null),
            "prompt_sha256": request.get("prompt_sha256").cloned().unwrap_or(Value::Null),
            "prompt_version": request.get("prompt_version").cloned().unwrap_or(Value::Null),
            "protocol": DESCRIPTION_RESPONSE_PROTOCOL,
            "request_sha256": request.get("request_sha256").cloned().unwrap_or(Value::Null),
            "schema_hash": request.get("schema_hash").cloned().unwrap_or(Value::Null),
            "source_id": request.get("source_id").cloned().unwrap_or(Value::Null)
        },
        "card": {
            "columns": minimal,
            "comment_authority": scope.table.get("comment_authority").cloned().unwrap_or(Value::Null),
            "dataset": scope.dataset,
            "foreign_keys": scope.table.get("foreign_keys").cloned().unwrap_or(Value::Array(Vec::new())),
            "reltuples": scope.table.get("reltuples").cloned().unwrap_or(Value::Null),
            "untrusted_table_comment": scope.table.get("untrusted_table_comment").cloned().unwrap_or(Value::Null)
        }
    }))
}

fn aggregate(config: &Config, docs: &[Value]) -> Value {
    let all_validated = docs.iter().all(|doc| doc.get("outcome").and_then(Value::as_str) == Some("validated"));
    let hits = docs.iter().filter(|doc| doc.get("cache").and_then(Value::as_str) == Some("hit")).count();
    let stored = docs
        .iter()
        .filter(|doc| doc.get("cache_store").and_then(Value::as_str) == Some("stored"))
        .count();
    let cache = if hits == docs.len() {
        "hit"
    } else if hits == 0 {
        "miss"
    } else {
        "partial"
    };
    let cache_store = if stored == docs.len() {
        "stored"
    } else if stored == 0 {
        "not_stored"
    } else {
        "partial"
    };
    let any_opened = docs.iter().any(|doc| doc.pointer("/transport/socket").and_then(Value::as_str) == Some("opened"));
    let all_complete = docs.iter().all(|doc| doc.pointer("/transport/status").and_then(Value::as_str) == Some("complete"));
    let mut field_rows = Vec::new();
    let mut table_rows = Vec::new();
    for doc in docs {
        if let Some(rows) = doc.pointer("/candidates/field").and_then(Value::as_array) {
            field_rows.extend(rows.iter().cloned());
        }
        if let Some(rows) = doc.pointer("/candidates/table").and_then(Value::as_array) {
            table_rows.extend(rows.iter().cloned());
        }
    }
    let candidates = json!({"field": field_rows, "table": table_rows});
    let usage = usage_sum(docs);
    let tokens = usage.map(|(_, _, total)| total).unwrap_or(0);
    let amount_value = if usage.is_none() {
        Value::Null
    } else {
        amount(&config.price, tokens).unwrap_or(Value::Null)
    };
    let amount_value = if matches!(config.price, Price::Unknown) { Value::Null } else { amount_value };
    json!({
        "protocol": "oo.pg-builder.generation/v1",
        "compiler": "oo.pg-builder/v1",
        "outcome": if all_validated { "validated" } else { "structural_draft" },
        "cache": cache,
        "cache_store": cache_store,
        "call_count": docs.len(),
        "calls": docs,
        "repair": "not_performed",
        "complete_command": Value::Null,
        "candidates": candidates,
        "candidate_sha256": canonical_sha256(&candidates),
        "field_publication_seam": field_seam(),
        "transport": {
            "adapter": "loopback_http_local_dev",
            "model_id": config.model_id,
            "endpoint_origin": config.origin,
            "endpoint_path": config.path,
            "namespace": "pg-builder",
            "request_input_sha256": Value::Null,
            "response_sha256": Value::Null,
            "usage": usage.map(|(prompt, completion, total)| json!({
                "prompt_tokens": prompt,
                "completion_tokens": completion,
                "total_tokens": total
            })).unwrap_or(Value::Null),
            "latency_ms": latency_sum(docs),
            "attempt": if any_opened { 1 } else { 0 },
            "finish_reason": Value::Null,
            "status": if all_complete { "complete" } else { "incomplete" },
            "http_status": Value::Null,
            "refusal": Value::Null,
            "budget_reason": Value::Null,
            "socket": if any_opened { "opened" } else { "not_opened" },
            "declared_cost": {
                "status": match config.price {
                    Price::Unknown => "unknown",
                    Price::Declared { .. } => "declared_non_billing",
                },
                "billing_certificate": false,
                "invoice": false,
                "amount": amount_value,
                "currency": match &config.price {
                    Price::Unknown => Value::Null,
                    Price::Declared { .. } => Value::String("USD".to_owned()),
                },
                "per_total_token": match &config.price {
                    Price::Unknown => Value::Null,
                    Price::Declared { per_total_token, .. } => Value::String(per_total_token.clone()),
                }
            }
        },
        "statements": statements()
    })
}

fn usage_sum(docs: &[Value]) -> Option<(u64, u64, u64)> {
    let mut prompt = 0u64;
    let mut completion = 0u64;
    let mut total = 0u64;
    for doc in docs {
        let usage = doc.pointer("/transport/usage")?;
        if usage.is_null() {
            return None;
        }
        prompt = prompt.checked_add(usage.get("prompt_tokens")?.as_u64()?)?;
        completion = completion.checked_add(usage.get("completion_tokens")?.as_u64()?)?;
        total = total.checked_add(usage.get("total_tokens")?.as_u64()?)?;
    }
    Some((prompt, completion, total))
}

fn latency_sum(docs: &[Value]) -> u64 {
    docs.iter().fold(0u64, |sum, doc| {
        let latency = doc.pointer("/transport/latency_ms").and_then(Value::as_u64).unwrap_or(0);
        sum.saturating_add(latency)
    })
}

fn finish_generation(
    input: &GenerateIn<'_>,
    input_sha: &str,
    command_text: &str,
    model_input_text: &str,
    parsed: Parsed,
    scope: &BatchScope<'_>,
) -> Result<Value, PgBuilderError> {
    let raw_hash = if parsed.secret_echo {
        Value::Null
    } else {
        Value::String(crate::sha256_hex(&parsed.raw))
    };
    if parsed.status != "complete" {
        let command = if scope.batch_index == 0 {
            captured_command(command_text, parsed.status, None)?
        } else {
            String::new()
        };
        return Ok(structural_draft(input, exchange(input_sha, model_input_text, scope), &parsed, raw_hash, command, "not_parsed"));
    }
    let outer: Result<Value, _> = serde_json::from_slice(&parsed.raw);
    let Ok(outer) = outer else {
        let command = if scope.batch_index == 0 {
            captured_command(command_text, "complete", Some(&parsed.content))?
        } else {
            String::new()
        };
        return Ok(structural_draft(input, exchange(input_sha, model_input_text, scope), &parsed, raw_hash, command, "not_parsed"));
    };
    if scope.batch_index > 0 {
        return finish_field_batch(input, input_sha, model_input_text, parsed, raw_hash, &outer, scope);
    }
    if !is_s5(&outer) {
        let command = captured_command(command_text, "complete", Some(&parsed.content))?;
        return Ok(structural_draft(input, exchange(input_sha, model_input_text, scope), &parsed, raw_hash, command, "rejected"));
    }
    let table_response = outer.get("table_response").cloned().unwrap_or(Value::Null);
    let table_text = canonical_json(&table_response);
    let command = captured_command(command_text, "complete", Some(&table_text))?;
    let table_outcome_text = match complete_description_candidates(&command) {
        Ok(text) => text,
        Err(_) => {
            return Ok(document(
                input,
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
                    scope,
                },
            ));
        }
    };
    let table_outcome: Value = serde_json::from_str(&table_outcome_text)
        .map_err(|_| PgBuilderError::new("compiler.complete", "the description compiler refused the captured body"))?;
    let fields = validate_fields(scope, outer.get("field_candidates").unwrap_or(&Value::Null))?;
    let table_ok = table_outcome.get("outcome").and_then(Value::as_str) == Some("candidates");
    let validated = table_ok && fields.0;
    store_draft(
        input,
        exchange(input_sha, model_input_text, scope),
        Kept {
            parsed,
            raw_hash,
            fields_ok: fields.0,
            fields: fields.1,
            table_outcome,
            command,
            validated,
        },
    )
}

fn finish_field_batch(
    input: &GenerateIn<'_>,
    input_sha: &str,
    model_input_text: &str,
    parsed: Parsed,
    raw_hash: Value,
    outer: &Value,
    scope: &BatchScope<'_>,
) -> Result<Value, PgBuilderError> {
    if !is_field_batch(outer) {
        return Ok(structural_draft(
            input,
            exchange(input_sha, model_input_text, scope),
            &parsed,
            raw_hash,
            String::new(),
            "rejected",
        ));
    }
    let fields = validate_fields(scope, outer.get("field_candidates").unwrap_or(&Value::Null))?;
    store_draft(
        input,
        exchange(input_sha, model_input_text, scope),
        Kept {
            parsed,
            raw_hash,
            fields_ok: fields.0,
            fields: fields.1,
            table_outcome: Value::Null,
            command: String::new(),
            validated: fields.0,
        },
    )
}

struct Exchange<'a> {
    input_sha: &'a str,
    model_text: &'a str,
    scope: &'a BatchScope<'a>,
}

fn exchange<'a>(input_sha: &'a str, model_text: &'a str, scope: &'a BatchScope<'a>) -> Exchange<'a> {
    Exchange { input_sha, model_text, scope }
}

struct Kept {
    parsed: Parsed,
    raw_hash: Value,
    fields_ok: bool,
    fields: Value,
    table_outcome: Value,
    command: String,
    validated: bool,
}

fn store_draft(input: &GenerateIn<'_>, exchange: Exchange<'_>, kept: Kept) -> Result<Value, PgBuilderError> {
    let evidence_hash = input.cards.get("evidence_hash").and_then(Value::as_str).unwrap_or("");
    let mut doc = document(
        input,
        exchange.input_sha,
        exchange.model_text,
        Draft {
            outcome: if kept.validated { "validated" } else { "structural_draft" },
            stored: if kept.validated { "stored" } else { "not_stored" },
            parsed: &kept.parsed,
            raw_hash: kept.raw_hash,
            fields: kept.fields,
            table_outcome: kept.table_outcome,
            field_outcome: if kept.fields_ok { "validated" } else { "rejected" },
            command: kept.command,
            scope: exchange.scope,
        },
    );
    if kept.validated && let Some(map) = doc.as_object_mut() {
        map.insert("evidence_hash".to_owned(), Value::String(evidence_hash.to_owned()));
        map.insert("input_sha256".to_owned(), Value::String(exchange.input_sha.to_owned()));
    }
    if kept.validated {
        store_validated(input.cache_dir, exchange.input_sha, &doc)?;
    }
    Ok(doc)
}

pub(crate) struct Parsed {
    pub(crate) status: &'static str,
    pub(crate) refusal: Option<&'static str>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) usage: Option<(u64, u64, u64)>,
    pub(crate) http_status: u16,
    pub(crate) latency_ms: u128,
    pub(crate) raw: Vec<u8>,
    pub(crate) content: String,
    pub(crate) secret_echo: bool,
    pub(crate) budget_reason: Option<&'static str>,
}

pub(crate) fn parse_completion(config: &Config, token: &str, reply: &RawReply) -> Result<Parsed, PgBuilderError> {
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

fn is_field_batch(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    map.get("protocol").and_then(Value::as_str) == Some("oo.pg-builder.s5-field-batch/v1")
        && map.get("complete") == Some(&Value::Bool(true))
        && map.get("field_candidates").is_some()
        && map.len() == 3
}

fn validate_fields(scope: &BatchScope<'_>, fields: &Value) -> Result<(bool, Value), PgBuilderError> {
    let Some(rows) = fields.as_array() else {
        return Ok((false, Value::Array(Vec::new())));
    };
    let mut expected = Vec::new();
    let dataset = scope.dataset;
    let card_sha = scope.table.get("card_sha256").and_then(Value::as_str).unwrap_or("");
    let no_columns = Vec::new();
    let columns = scope.table.get("columns").and_then(Value::as_array).unwrap_or(&no_columns);
    if scope.column_to > columns.len() {
        return Ok((false, Value::Array(rows.clone())));
    }
    for column in &columns[scope.column_from..scope.column_to] {
        expected.push((
            dataset.to_owned(),
            column.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
            column.get("attnum").cloned().unwrap_or(Value::Null),
            card_sha.to_owned(),
        ));
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
    chat_request(config, PROMPT, user)
}

pub(crate) fn chat_request(config: &Config, system: &str, user: &str) -> Result<Value, PgBuilderError> {
    let mut body = json!({
        "model": config.model_id,
        "temperature": 0,
        "stream": false,
        "max_completion_tokens": config.max_completion_tokens,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "response_format": {"type": "json_object"}
    });
    if let Some(effort) = &config.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    Ok(body)
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

fn input_hash(config: &Config, cards: &Value, request: &Value, scope: &BatchScope<'_>) -> Result<String, PgBuilderError> {
    let key = json!({
        "protocol": "oo.pg-builder.cache-key/v2",
        "reasoning_effort": config.reasoning_effort,
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
        "dataset": scope.dataset,
        "card_sha256": scope.table.get("card_sha256").cloned().unwrap_or(Value::Null),
        "batch_index": scope.batch_index,
        "column_from": scope.column_from,
        "column_to": scope.column_to,
        "request_sha256": request.get("request_sha256").cloned().unwrap_or(Value::Null)
    });
    Ok(canonical_sha256(&key))
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
    scope: &'a BatchScope<'a>,
}

fn structural_draft(
    input: &GenerateIn<'_>,
    exchange: Exchange<'_>,
    parsed: &Parsed,
    raw_hash: Value,
    command: String,
    field_outcome: &str,
) -> Value {
    let table = if command.is_empty() {
        Value::Null
    } else {
        let text = complete_description_candidates(&command).unwrap_or_else(|_| "{}".to_owned());
        serde_json::from_str(&text).unwrap_or(Value::Null)
    };
    document(
        input,
        exchange.input_sha,
        exchange.model_text,
        Draft {
            outcome: "structural_draft",
            stored: "not_stored",
            parsed,
            raw_hash,
            fields: Value::Null,
            table_outcome: table,
            field_outcome,
            command,
            scope: exchange.scope,
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
        "dataset": draft.scope.dataset,
        "batch": {
            "index": draft.scope.batch_index,
            "count": draft.scope.batch_count,
            "column_from": draft.scope.column_from,
            "column_to": draft.scope.column_to
        },
        "candidates": candidates,
        "candidate_sha256": canonical_sha256(&candidates),
        "table_outcome": draft.table_outcome,
        "field_outcome": draft.field_outcome,
        "field_publication_seam": field_seam(),
        "complete_command": if draft.command.is_empty() { Value::Null } else { Value::String(draft.command) },
        "statements": statements()
    })
}

fn failure_document(
    input: &GenerateIn<'_>,
    input_sha: &str,
    request_text: &str,
    status: &str,
    budget_reason: Option<&str>,
    socket: &str,
    scope: &BatchScope<'_>,
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
        input,
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
            scope,
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
