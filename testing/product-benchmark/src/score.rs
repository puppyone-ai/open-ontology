//! Score producer files against the frozen questions. HTTP 200 is not a pass.

use std::fs;
use std::path::Path;

use crate::json::{self, Val};
use crate::sha256::sha256_hex;

pub const FREEZE_SHA256: &str = "f500dda158ea5220c190c92adfbbfa4687ccfb644b0a86961e3858c6f14fa268";

const WHOLE: [&str; 7] = [
    "public_auth",
    "session_close_api",
    "natural_language_harness",
    "runsc",
    "wal",
    "semantic_publication",
    "customer_uat",
];

#[derive(Clone, Debug)]
pub struct QuestionVerdict {
    pub id: String,
    pub verdict: &'static str,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub freeze_sha256: String,
    pub freeze_unchanged: bool,
    pub benchmark_loop_pass: bool,
    pub integration_passed: bool,
    pub parent_acceptance: bool,
    pub rows_match: bool,
    pub party: Option<u64>,
    pub account: Option<u64>,
    pub entry: Option<u64>,
    pub model_id: String,
    pub http_status: Option<u64>,
    pub request_input_sha256: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub usage_sum_ok: bool,
    pub cost_status: String,
    pub cost_amount_null: bool,
    pub cache_first: String,
    pub cache_hit: String,
    pub cache_socket: String,
    pub fresh_socket: String,
    pub http_200_alone_is_e2e_pass: bool,
    pub review_unauthenticated: bool,
    pub hidden_national_id: bool,
    pub roles_wide_narrow: bool,
    pub query_closed: bool,
    pub session_close_api: String,
    pub questions: Vec<QuestionVerdict>,
    pub whole_product: Vec<(String, String)>,
    pub failures: Vec<String>,
}

pub fn score(freeze_path: &Path, input: &Path) -> Report {
    let mut failures = Vec::new();
    let freeze_bytes = fs::read(freeze_path).unwrap_or_default();
    let freeze_sha256 = sha256_hex(&freeze_bytes);
    let freeze_unchanged = freeze_sha256 == FREEZE_SHA256;
    if !freeze_unchanged {
        failures.push("freeze bytes differ from the pre-edit smoke hash".to_string());
    }
    let freeze = json::parse(std::str::from_utf8(&freeze_bytes).unwrap_or(""))
        .ok()
        .filter(|_| freeze_unchanged);

    let mut questions = Vec::new();
    let rows = read_rows(input, &mut failures);
    let rows_match = rows == Some((2000, 8000, 90000));
    if !rows_match {
        failures.push("actual row counts are not party 2000, account 8000, entry 90000".to_string());
    }

    push_fixture(&mut questions, &mut failures, input, &freeze, "q-party-null", "query-wide-null");
    push_fixture(&mut questions, &mut failures, input, &freeze, "q-qty", "query-wide-qty");
    push_fixture(&mut questions, &mut failures, input, &freeze, "q-fk", "query-wide-fk");
    push_fixture(&mut questions, &mut failures, input, &freeze, "q-phone", "query-wide-phone");
    push_fixture(
        &mut questions,
        &mut failures,
        input,
        &freeze,
        "q-narrow-party",
        "query-narrow-party",
    );
    push_fixture(
        &mut questions,
        &mut failures,
        input,
        &freeze,
        "q-party-count",
        "query-party-count",
    );
    let entry_fixture = fixture_ok(input, &freeze, "q-entry-count", "query-wide-count");
    if !entry_fixture {
        failures.push("query-pg entry count does not match gold 90000".to_string());
    }

    let model = read_model(input, &mut failures);
    let e2e = read_e2e(input, &freeze, &mut questions, &mut failures);
    let wrong = read_wrong_model(input, &mut questions, &mut failures);
    let hidden = read_hidden(input, &mut questions, &mut failures);
    let citation = read_citation_false(input, &mut questions, &mut failures);
    let review = read_review(input, &mut failures);
    let query_closed = entry_fixture && summary_closed(&read_text(input, "query-wide-count.stdout"));
    if !query_closed {
        failures.push("query close is not end_audit_ok with rollback and a dropped socket".to_string());
    }
    let whole = read_whole(input, &mut failures);
    let session_close_api = whole
        .iter()
        .find(|(key, _)| key == "session_close_api")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| "missing".to_string());

    let http_alone_would_look_green = model.http_status == Some(200);
    if http_alone_would_look_green && !e2e {
        failures.push("model HTTP 200 is present and the end-to-end cells are not a pass".to_string());
    }

    let loop_pass = failures.is_empty()
        && freeze_unchanged
        && rows_match
        && entry_fixture
        && e2e
        && wrong
        && hidden
        && citation
        && model.ok
        && review.unauthenticated
        && review.hidden
        && review.roles
        && query_closed
        && whole.iter().all(|(_, value)| value == "notrun");

    Report {
        freeze_sha256,
        freeze_unchanged,
        benchmark_loop_pass: loop_pass,
        integration_passed: false,
        parent_acceptance: false,
        rows_match,
        party: rows.map(|row| row.0),
        account: rows.map(|row| row.1),
        entry: rows.map(|row| row.2),
        model_id: model.model_id,
        http_status: model.http_status,
        request_input_sha256: model.input_sha,
        prompt_tokens: model.prompt,
        completion_tokens: model.completion,
        total_tokens: model.total,
        usage_sum_ok: model.usage_sum_ok,
        cost_status: model.cost_status,
        cost_amount_null: model.cost_amount_null,
        cache_first: model.cache_first,
        cache_hit: model.cache_hit,
        cache_socket: model.cache_socket,
        fresh_socket: model.fresh_socket,
        http_200_alone_is_e2e_pass: false,
        review_unauthenticated: review.unauthenticated,
        hidden_national_id: review.hidden,
        roles_wide_narrow: review.roles,
        query_closed,
        session_close_api,
        questions,
        whole_product: whole,
        failures,
    }
}

pub fn report_json(report: &Report) -> String {
    let mut questions = Vec::new();
    for question in &report.questions {
        questions.push(Val::Object(vec![
            ("id".to_string(), Val::String(question.id.clone())),
            ("verdict".to_string(), Val::String(question.verdict.to_string())),
            ("reason".to_string(), Val::String(question.reason.clone())),
        ]));
    }
    let mut whole = Vec::new();
    for (key, value) in &report.whole_product {
        whole.push((key.clone(), Val::String(value.clone())));
    }
    let failures = report
        .failures
        .iter()
        .map(|item| Val::String(item.clone()))
        .collect();
    let value = Val::Object(vec![
        ("protocol".to_string(), Val::String("oo.product-benchmark.report/v1".to_string())),
        ("freeze_sha256".to_string(), Val::String(report.freeze_sha256.clone())),
        ("freeze_unchanged".to_string(), Val::Bool(report.freeze_unchanged)),
        ("benchmark_loop_pass".to_string(), Val::Bool(report.benchmark_loop_pass)),
        ("integration_passed".to_string(), Val::Bool(false)),
        ("parent_acceptance".to_string(), Val::Bool(false)),
        (
            "rows".to_string(),
            Val::Object(vec![
                ("party".to_string(), num_opt(report.party)),
                ("account".to_string(), num_opt(report.account)),
                ("entry".to_string(), num_opt(report.entry)),
                ("total".to_string(), num_opt(report.party.zip(report.account).zip(report.entry).map(|((p, a), e)| p + a + e))),
                ("match".to_string(), Val::Bool(report.rows_match)),
            ]),
        ),
        (
            "model".to_string(),
            Val::Object(vec![
                ("model_id".to_string(), Val::String(report.model_id.clone())),
                ("http_status".to_string(), num_opt(report.http_status)),
                ("request_input_sha256".to_string(), Val::String(report.request_input_sha256.clone())),
                ("prompt_tokens".to_string(), num_opt(report.prompt_tokens)),
                ("completion_tokens".to_string(), num_opt(report.completion_tokens)),
                ("total_tokens".to_string(), num_opt(report.total_tokens)),
                ("usage_sum_ok".to_string(), Val::Bool(report.usage_sum_ok)),
                ("cost_status".to_string(), Val::String(report.cost_status.clone())),
                ("cost_amount_null".to_string(), Val::Bool(report.cost_amount_null)),
                ("cache_first".to_string(), Val::String(report.cache_first.clone())),
                ("cache_hit".to_string(), Val::String(report.cache_hit.clone())),
                ("cache_socket".to_string(), Val::String(report.cache_socket.clone())),
                ("fresh_socket".to_string(), Val::String(report.fresh_socket.clone())),
                ("http_200_alone_is_e2e_pass".to_string(), Val::Bool(false)),
            ]),
        ),
        ("questions".to_string(), Val::Array(questions)),
        (
            "review".to_string(),
            Val::Object(vec![
                ("unauthenticated".to_string(), Val::Bool(report.review_unauthenticated)),
                ("hidden_national_id".to_string(), Val::Bool(report.hidden_national_id)),
                ("roles_wide_narrow".to_string(), Val::Bool(report.roles_wide_narrow)),
            ]),
        ),
        (
            "close".to_string(),
            Val::Object(vec![
                ("query_end_audit_ok".to_string(), Val::Bool(report.query_closed)),
                ("session_close_api".to_string(), Val::String(report.session_close_api.clone())),
            ]),
        ),
        ("whole_product".to_string(), Val::Object(whole)),
        ("failures".to_string(), Val::Array(failures)),
    ]);
    format!("{}\n", json::render(&value))
}

pub fn report_markdown(report: &Report) -> String {
    let mut text = String::new();
    text.push_str("# Product benchmark report\n\n");
    text.push_str(&format!(
        "Loop pass: {}. Integration passed: {}. Parent acceptance: {}.\n\n",
        report.benchmark_loop_pass, report.integration_passed, report.parent_acceptance
    ));
    text.push_str(&format!(
        "Freeze unchanged: {}. SHA-256 `{}`.\n\n",
        report.freeze_unchanged, report.freeze_sha256
    ));
    text.push_str(&format!(
        "Rows party {:?} account {:?} entry {:?}. Match: {}.\n\n",
        report.party, report.account, report.entry, report.rows_match
    ));
    text.push_str(&format!(
        "Model `{}` HTTP status {:?} input SHA-256 `{}`. Usage {:?} + {:?} = {:?} (sum ok {}). Cost status `{}`, amount null {}. Cache first `{}`, hit `{}`, cache socket `{}`. Fresh call socket `{}`.\n\n",
        report.model_id,
        report.http_status,
        report.request_input_sha256,
        report.prompt_tokens,
        report.completion_tokens,
        report.total_tokens,
        report.usage_sum_ok,
        report.cost_status,
        report.cost_amount_null,
        report.cache_first,
        report.cache_hit,
        report.cache_socket,
        report.fresh_socket
    ));
    text.push_str("HTTP 200 alone is an end-to-end pass: false.\n\n");
    text.push_str(&format!(
        "Review unauthenticated: {}. Hidden national_id: {}. Wide and narrow roles: {}. Query closed: {}. Session-close API: {}.\n\n",
        report.review_unauthenticated,
        report.hidden_national_id,
        report.roles_wide_narrow,
        report.query_closed,
        report.session_close_api
    ));
    text.push_str("## Questions\n\n");
    for question in &report.questions {
        text.push_str(&format!("- `{}` {}: {}\n", question.id, question.verdict, question.reason));
    }
    text.push_str("\n## Whole product\n\n");
    for (key, value) in &report.whole_product {
        text.push_str(&format!("- `{key}`: {value}\n"));
    }
    text.push_str("\n## Failures\n\n");
    if report.failures.is_empty() {
        text.push_str("None.\n");
    } else {
        for failure in &report.failures {
            text.push_str(&format!("- {failure}\n"));
        }
    }
    text
}

struct ModelRead {
    ok: bool,
    model_id: String,
    http_status: Option<u64>,
    input_sha: String,
    prompt: Option<u64>,
    completion: Option<u64>,
    total: Option<u64>,
    usage_sum_ok: bool,
    cost_status: String,
    cost_amount_null: bool,
    cache_first: String,
    cache_hit: String,
    cache_socket: String,
    fresh_socket: String,
}

struct ReviewRead {
    unauthenticated: bool,
    hidden: bool,
    roles: bool,
}

fn read_rows(input: &Path, failures: &mut Vec<String>) -> Option<(u64, u64, u64)> {
    let value = match read_json(input, "import-result.json") {
        Ok(value) => value,
        Err(err) => {
            failures.push(err);
            return None;
        }
    };
    let tables = value.get("tables").and_then(Val::as_array);
    let Some(tables) = tables else {
        failures.push("import-result.json has no tables".to_string());
        return None;
    };
    let mut party = None;
    let mut account = None;
    let mut entry = None;
    for table in tables {
        let name = table.get("name").and_then(Val::as_str).unwrap_or("");
        let rows = table.get("rows").and_then(Val::as_u64);
        match name {
            "party" => party = rows,
            "account" => account = rows,
            "entry" => entry = rows,
            _ => {}
        }
    }
    match (party, account, entry) {
        (Some(party), Some(account), Some(entry)) => Some((party, account, entry)),
        _ => {
            failures.push("import-result.json is missing a table row count".to_string());
            None
        }
    }
}

fn push_fixture(
    questions: &mut Vec<QuestionVerdict>,
    failures: &mut Vec<String>,
    input: &Path,
    freeze: &Option<Val>,
    id: &str,
    stem: &str,
) {
    let ok = fixture_ok(input, freeze, id, stem);
    if ok {
        questions.push(QuestionVerdict {
            id: id.to_string(),
            verdict: "pass",
            reason: "cells match the freeze and the summary closed".to_string(),
        });
    } else {
        failures.push(format!("{id} does not match frozen gold"));
        questions.push(QuestionVerdict {
            id: id.to_string(),
            verdict: "fail",
            reason: "cells, exit, or close record do not match the freeze".to_string(),
        });
    }
}

fn fixture_ok(input: &Path, freeze: &Option<Val>, id: &str, stem: &str) -> bool {
    let Some(gold) = gold_rows(freeze, id) else {
        return false;
    };
    let exit = read_exit(input, &format!("{stem}.exit"));
    let text = read_text(input, &format!("{stem}.stdout"));
    exit == Some(0) && summary_closed(&text) && rows_of(&text).as_ref() == Some(&gold)
}

fn read_e2e(
    input: &Path,
    freeze: &Option<Val>,
    questions: &mut Vec<QuestionVerdict>,
    failures: &mut Vec<String>,
) -> bool {
    let config = read_json(input, "dialogue-config.json").ok();
    let model = config
        .as_ref()
        .and_then(|value| value.get("endpoint"))
        .and_then(|value| value.get("model_id"))
        .and_then(Val::as_str)
        .unwrap_or("");
    let policy_id = config
        .as_ref()
        .and_then(|value| value.get("binding"))
        .and_then(|value| value.get("policy_id"))
        .and_then(Val::as_str)
        .unwrap_or("");
    let proposal_exit = read_exit(input, "dialogue-proposal.exit");
    let execution_exit = read_exit(input, "dialogue-execution.exit");
    let execution = read_json(input, "dialogue-execution.json").ok();
    let receipt = execution
        .as_ref()
        .and_then(|value| value.get("query_receipt"))
        .and_then(Val::as_str)
        .unwrap_or("");
    let rows = execution.as_ref().and_then(|value| value.get("rows")).cloned();
    let citation = execution.as_ref().and_then(|value| value.get("citation"));
    let citation_ok = citation_is_ok(citation);
    let gold = gold_rows(freeze, "q-entry-count");
    let cells_ok = match (&rows, gold.as_ref()) {
        (Some(rows), Some(gold)) => rows.as_array().is_some_and(|items| &cell_rows(items) == gold),
        _ => false,
    };
    let ok = model == "grok-4.7"
        && proposal_exit == Some(0)
        && execution_exit == Some(0)
        && receipt == "clean"
        && citation_ok
        && cells_ok;
    if ok {
        questions.push(QuestionVerdict {
            id: "q-entry-count".to_string(),
            verdict: "pass",
            reason: "grok-4.7 proposal, clean query, citation ok, cells 90000".to_string(),
        });
    } else {
        failures.push(
            "q-entry-count is not an end-to-end pass; HTTP status alone cannot pass it".to_string(),
        );
        questions.push(QuestionVerdict {
            id: "q-entry-count".to_string(),
            verdict: "fail",
            reason: format!(
                "model={model} policy_id={policy_id} proposal_exit={proposal_exit:?} execution_exit={execution_exit:?} receipt={receipt} citation_ok={citation_ok} cells_ok={cells_ok}"
            ),
        });
    }
    ok
}

fn read_wrong_model(input: &Path, questions: &mut Vec<QuestionVerdict>, failures: &mut Vec<String>) -> bool {
    let config = read_json(input, "wrong-model-config.json").ok();
    let model = config
        .as_ref()
        .and_then(|value| value.get("endpoint"))
        .and_then(|value| value.get("model_id"))
        .and_then(Val::as_str)
        .unwrap_or("");
    let exit = read_exit(input, "wrong-model.exit");
    let held = model == "grok-4.3" && exit.is_some() && exit != Some(0);
    if held {
        questions.push(QuestionVerdict {
            id: "q-wrong-model".to_string(),
            verdict: "pass",
            reason: "grok-4.3 was refused, matching the frozen must_fail expectation".to_string(),
        });
    } else {
        failures.push("wrong model was not refused".to_string());
        questions.push(QuestionVerdict {
            id: "q-wrong-model".to_string(),
            verdict: "fail",
            reason: format!("model={model} exit={exit:?}; frozen refusal did not hold"),
        });
    }
    held
}

fn read_hidden(input: &Path, questions: &mut Vec<QuestionVerdict>, failures: &mut Vec<String>) -> bool {
    let exit = read_exit(input, "query-wide-hidden.exit");
    let stdout = read_text(input, "query-wide-hidden.stdout");
    let held = exit == Some(6) && stdout.trim().is_empty();
    if held {
        questions.push(QuestionVerdict {
            id: "q-hidden".to_string(),
            verdict: "pass",
            reason: "national_id query exited 6 with empty stdout, matching the frozen must_fail expectation".to_string(),
        });
    } else {
        failures.push("query rejection did not match the frozen exit 6".to_string());
        questions.push(QuestionVerdict {
            id: "q-hidden".to_string(),
            verdict: "fail",
            reason: format!("exit={exit:?}; frozen query rejection did not hold"),
        });
    }
    held
}

fn read_citation_false(input: &Path, questions: &mut Vec<QuestionVerdict>, failures: &mut Vec<String>) -> bool {
    let exit = read_exit(input, "citation-invalid.exit");
    let report = read_json(input, "citation-invalid.json").ok();
    let status = report
        .as_ref()
        .and_then(|value| value.get("citation_status"))
        .and_then(Val::as_str)
        .unwrap_or("");
    let held = exit.is_some() && exit != Some(0) && status == "invalid";
    if held {
        questions.push(QuestionVerdict {
            id: "q-citation-false".to_string(),
            verdict: "pass",
            reason: "citation_status invalid matches the frozen must_fail expectation".to_string(),
        });
    } else {
        failures.push("citation false did not match the frozen invalid report".to_string());
        questions.push(QuestionVerdict {
            id: "q-citation-false".to_string(),
            verdict: "fail",
            reason: format!("status={status} exit={exit:?}; frozen citation false did not hold"),
        });
    }
    held
}

fn read_model(input: &Path, failures: &mut Vec<String>) -> ModelRead {
    let mut model = ModelRead {
        ok: false,
        model_id: String::new(),
        http_status: None,
        input_sha: String::new(),
        prompt: None,
        completion: None,
        total: None,
        usage_sum_ok: false,
        cost_status: String::new(),
        cost_amount_null: false,
        cache_first: String::new(),
        cache_hit: String::new(),
        cache_socket: String::new(),
        fresh_socket: "absent".to_string(),
    };
    let first = match read_json(input, "builder-first.json") {
        Ok(value) => value,
        Err(err) => {
            failures.push(err);
            return model;
        }
    };
    let hit = match read_json(input, "builder-hit.json") {
        Ok(value) => value,
        Err(err) => {
            failures.push(err);
            return model;
        }
    };
    model.cache_first = cache_label(&first);
    model.cache_hit = cache_label(&hit);
    if let Some(call) = opened_call(&first) {
        let transport = call.get("transport");
        model.model_id = transport
            .and_then(|value| value.get("model_id"))
            .and_then(Val::as_str)
            .unwrap_or("")
            .to_string();
        model.http_status = transport.and_then(|value| value.get("http_status")).and_then(Val::as_u64);
        model.input_sha = transport
            .and_then(|value| value.get("request_input_sha256"))
            .and_then(Val::as_str)
            .unwrap_or("")
            .to_string();
        let usage = transport.and_then(|value| value.get("usage"));
        model.prompt = usage.and_then(|value| value.get("prompt_tokens")).and_then(Val::as_u64);
        model.completion = usage
            .and_then(|value| value.get("completion_tokens"))
            .and_then(Val::as_u64);
        model.total = usage.and_then(|value| value.get("total_tokens")).and_then(Val::as_u64);
        model.usage_sum_ok = matches!(
            (model.prompt, model.completion, model.total),
            (Some(prompt), Some(completion), Some(total)) if prompt.checked_add(completion) == Some(total)
        );
        let cost = transport.and_then(|value| value.get("declared_cost"));
        model.cost_status = cost
            .and_then(|value| value.get("status"))
            .and_then(Val::as_str)
            .unwrap_or("")
            .to_string();
        model.cost_amount_null = cost.and_then(|value| value.get("amount")).is_some_and(|value| matches!(value, Val::Null));
        model.fresh_socket = "opened".to_string();
        let invoice_false = cost.and_then(|value| value.get("invoice")).and_then(Val::as_bool) == Some(false);
        let sha_ok = model.input_sha.len() == 64 && model.input_sha.chars().all(|ch| ch.is_ascii_hexdigit());
        if !(model.model_id == "grok-4.7"
            && model.http_status == Some(200)
            && sha_ok
            && model.usage_sum_ok
            && model.cost_status == "unknown"
            && model.cost_amount_null
            && invoice_false
            && model.cache_first == "miss")
        {
            failures.push("opened model call is missing grok-4.7, usage, cost, input sha, or a miss".to_string());
        }
    } else {
        failures.push("builder first document has no opened call".to_string());
    }
    let socket_ok = hit_socket_closed(&hit);
    model.cache_socket = if socket_ok { "not_opened".to_string() } else { "opened".to_string() };
    if model.cache_hit != "hit" || !socket_ok {
        failures.push("cache hit did not keep the socket closed".to_string());
    }
    model.ok = model.model_id == "grok-4.7"
        && model.http_status == Some(200)
        && model.input_sha.len() == 64
        && model.usage_sum_ok
        && model.cost_status == "unknown"
        && model.cost_amount_null
        && model.cache_first == "miss"
        && model.cache_hit == "hit"
        && model.fresh_socket == "opened"
        && socket_ok;
    model
}

fn read_review(input: &Path, failures: &mut Vec<String>) -> ReviewRead {
    let mut review = ReviewRead {
        unauthenticated: false,
        hidden: false,
        roles: false,
    };
    let value = match read_json(input, "apply-outcome.json") {
        Ok(value) => value,
        Err(err) => {
            failures.push(err);
            return review;
        }
    };
    let claims = value.get("non_claims");
    review.unauthenticated = claims
        .and_then(|item| item.get("authentication"))
        .and_then(Val::as_str)
        == Some("not_claimed")
        && claims.and_then(|item| item.get("parent_acceptance")).and_then(Val::as_bool) == Some(false);
    review.hidden = value
        .get("hidden")
        .and_then(Val::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("dataset").and_then(Val::as_str) == Some("shop.party")
                    && row.get("column").and_then(Val::as_str) == Some("national_id")
            })
        });
    let mut wide = false;
    let mut narrow = false;
    if let Some(rows) = value.get("provision").and_then(Val::as_array) {
        for row in rows {
            match row.get("slug").and_then(Val::as_str) {
                Some("wide") => wide = true,
                Some("narrow") => narrow = true,
                _ => {}
            }
        }
    }
    review.roles = wide && narrow;
    if !review.unauthenticated || !review.hidden || !review.roles {
        failures.push("review, hidden column, or wide/narrow role record is incomplete".to_string());
    }
    review
}

fn read_whole(input: &Path, failures: &mut Vec<String>) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let value = match read_json(input, "whole-product.json") {
        Ok(value) => value,
        Err(err) => {
            failures.push(err);
            for key in WHOLE {
                rows.push((key.to_string(), "missing".to_string()));
            }
            return rows;
        }
    };
    for key in WHOLE {
        let status = value.get(key).and_then(Val::as_str).unwrap_or("missing").to_string();
        if status != "notrun" {
            failures.push(format!("{key} is {status}; a missing whole-product API stays notrun"));
        }
        rows.push((key.to_string(), status));
    }
    rows
}

fn citation_is_ok(citation: Option<&Val>) -> bool {
    let Some(citation) = citation else {
        return false;
    };
    if citation.get("citation_status").and_then(Val::as_str) != Some("ok") {
        return false;
    }
    if citation.get("missing_citation").and_then(Val::as_bool) == Some(true) {
        return false;
    }
    if citation.get("ok").and_then(Val::as_bool) == Some(false) {
        return false;
    }
    true
}

fn gold_rows(freeze: &Option<Val>, id: &str) -> Option<Vec<Vec<Val>>> {
    let questions = freeze.as_ref()?.get("questions")?.as_array()?;
    let question = questions.iter().find(|item| item.get("id").and_then(Val::as_str) == Some(id))?;
    let rows = question.get("gold_rows")?.as_array()?;
    Some(cell_rows(rows))
}

fn cell_rows(rows: &[Val]) -> Vec<Vec<Val>> {
    rows.iter()
        .map(|row| row.as_array().unwrap_or(&[]).to_vec())
        .collect()
}

fn rows_of(text: &str) -> Option<Vec<Vec<Val>>> {
    let docs = json::parse_lines(text).ok()?;
    let mut rows = Vec::new();
    for doc in docs {
        if doc.get("record").and_then(Val::as_str) == Some("row") {
            let cells = doc.get("cells").and_then(Val::as_array).unwrap_or(&[]);
            rows.push(cells.to_vec());
        }
    }
    Some(rows)
}

fn summary_closed(text: &str) -> bool {
    let Ok(docs) = json::parse_lines(text) else {
        return false;
    };
    docs.iter().any(|doc| {
        doc.get("record").and_then(Val::as_str) == Some("summary")
            && doc.get("status").and_then(Val::as_str) == Some("succeeded")
            && doc.get("end_audit_ok").and_then(Val::as_bool) == Some(true)
            && doc.get("rollback_confirmed").and_then(Val::as_bool) == Some(true)
            && doc.get("socket_dropped").and_then(Val::as_bool) == Some(false)
            && doc.get("truncated").and_then(Val::as_bool) == Some(false)
    })
}

fn opened_call(doc: &Val) -> Option<&Val> {
    if let Some(calls) = doc.get("calls").and_then(Val::as_array) {
        return calls.iter().find(|call| {
            call.get("transport").and_then(|value| value.get("socket")).and_then(Val::as_str) == Some("opened")
        });
    }
    let socket = doc.get("transport").and_then(|value| value.get("socket")).and_then(Val::as_str);
    if socket == Some("opened") {
        Some(doc)
    } else {
        None
    }
}

fn cache_label(doc: &Val) -> String {
    if let Some(label) = doc.get("cache").and_then(Val::as_str) {
        return label.to_string();
    }
    String::new()
}

fn hit_socket_closed(doc: &Val) -> bool {
    if doc.get("cache").and_then(Val::as_str) != Some("hit") {
        return false;
    }
    if let Some(calls) = doc.get("calls").and_then(Val::as_array) {
        return !calls.is_empty()
            && calls.iter().all(|call| {
                call.get("cache").and_then(Val::as_str) == Some("hit")
                    && call
                        .get("transport")
                        .and_then(|value| value.get("socket"))
                        .and_then(Val::as_str)
                        == Some("not_opened")
            });
    }
    doc.get("transport").and_then(|value| value.get("socket")).and_then(Val::as_str) == Some("not_opened")
}

fn read_json(input: &Path, name: &str) -> Result<Val, String> {
    let text = fs::read_to_string(input.join(name)).map_err(|_| format!("{name} is missing"))?;
    json::parse(&text).map_err(|err| format!("{name} {err}"))
}

fn read_text(input: &Path, name: &str) -> String {
    fs::read_to_string(input.join(name)).unwrap_or_default()
}

fn read_exit(input: &Path, name: &str) -> Option<i32> {
    fs::read_to_string(input.join(name))
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

fn num_opt(value: Option<u64>) -> Val {
    match value {
        Some(number) => Val::Number(number.to_string()),
        None => Val::Null,
    }
}
