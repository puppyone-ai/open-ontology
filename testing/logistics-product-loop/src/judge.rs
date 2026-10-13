//! Closed-card judge. A missing measurement stays pending. A draft preview is
//! not an approved or published S9/S10. HTTP 200 and a first scalar do not pass.

use crate::gold::{cells, questions};
use crate::json::{object, stringify, Value};

const MODEL_ACTORS: &[&str] = &["model", "llm", "grok", "grok-4.7", "pi", "builder", "self"];

pub struct Verdict {
    pub document: Value,
    pub exit_code: i32,
}

pub fn judge(observation: Option<&Value>) -> Verdict {
    let Some(observation) = observation else {
        return pending("observation absent");
    };
    if observation.get("protocol").and_then(Value::as_str) != Some("oo.logistics-product-loop.observation/v1") {
        return fail_protocol();
    }
    let live = observation.get("provenance").and_then(Value::as_str) == Some("live");
    let copy_a = check("MP-01a", mp01a(observation));
    let copy_b = check("MP-01b", mp01b(observation));
    let copy_c = check("MP-01c", mp01c(observation));
    let copy_d = check("MP-01d", mp01d(observation));
    let copy_parent = and_pass(&["MP-01a", "MP-01b", "MP-01c", "MP-01d"], &[copy_a.clone(), copy_b.clone(), copy_c.clone(), copy_d.clone()]);
    let checks = vec![
        copy_a,
        copy_b,
        copy_c,
        copy_d,
        check("MP-01", copy_parent),
        check("MP-02", mp02(observation)),
        check("MP-03", mp03(observation)),
        check("MP-04", mp04(observation)),
        check("MP-05", mp05(observation)),
        check("MP-06", mp06(observation)),
    ];
    let stages = stages(observation);
    let logic_pass = checks.iter().all(|item| item.1 == "pass") && stages_honest(&stages);
    let short_goal = if !logic_pass {
        if checks.iter().any(|item| item.1 == "fail") || stages.iter().any(|item| item.1 == "fake_approval") {
            "fail"
        } else {
            "pending"
        }
    } else if live {
        "pass"
    } else {
        "pending"
    };
    let exit_code = if short_goal == "pass" {
        0
    } else if short_goal == "fail" {
        1
    } else {
        4
    };
    let document = object(vec![
        ("protocol", Value::String("oo.logistics-product-loop.verdict/v1".to_owned())),
        ("short_goal", Value::String(short_goal.to_owned())),
        ("logic_pass", Value::Bool(logic_pass)),
        ("provenance_live", Value::Bool(live)),
        ("parent_plan_acceptance", Value::Bool(false)),
        ("runsc_certified", Value::Bool(false)),
        ("arbitrary_database", Value::Bool(false)),
        ("formal_s9", Value::String(stage_status(&stages, "S9"))),
        ("formal_s10", Value::String(stage_status(&stages, "S10"))),
        ("checks", Value::Array(checks.into_iter().map(|(id, status, detail)| named(id, status, &detail)).collect())),
        ("stages", Value::Array(stages.into_iter().map(|(id, status, detail)| named(id, status, &detail)).collect())),
    ]);
    Verdict { document, exit_code }
}

pub fn verdict_text(verdict: &Verdict) -> String {
    stringify(&verdict.document)
}

fn pending(reason: &str) -> Verdict {
    let ids = ["MP-01a", "MP-01b", "MP-01c", "MP-01d", "MP-01", "MP-02", "MP-03", "MP-04", "MP-05", "MP-06"];
    let stages = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10"];
    let document = object(vec![
        ("protocol", Value::String("oo.logistics-product-loop.verdict/v1".to_owned())),
        ("short_goal", Value::String("pending".to_owned())),
        ("logic_pass", Value::Bool(false)),
        ("provenance_live", Value::Bool(false)),
        ("parent_plan_acceptance", Value::Bool(false)),
        ("runsc_certified", Value::Bool(false)),
        ("arbitrary_database", Value::Bool(false)),
        ("formal_s9", Value::String("notdone".to_owned())),
        ("formal_s10", Value::String("notdone".to_owned())),
        ("reason", Value::String(reason.to_owned())),
        ("checks", Value::Array(ids.into_iter().map(|id| named(id, "pending", reason)).collect())),
        ("stages", Value::Array(stages.into_iter().map(|id| named(id, "notdone", reason)).collect())),
    ]);
    Verdict { document, exit_code: 4 }
}

fn fail_protocol() -> Verdict {
    let mut verdict = pending("observation protocol mismatch");
    if let Value::Object(pairs) = &mut verdict.document
        && let Some(slot) = pairs.iter_mut().find(|(key, _)| key == "short_goal")
    {
        slot.1 = Value::String("fail".to_owned());
    }
    verdict.exit_code = 1;
    verdict
}

fn check(id: &'static str, result: (&'static str, String)) -> (&'static str, &'static str, String) {
    (id, result.0, result.1)
}

fn and_pass(ids: &[&str], checks: &[(&str, &str, String)]) -> (&'static str, String) {
    if ids.iter().all(|id| checks.iter().any(|item| item.0 == *id && item.1 == "pass")) {
        ("pass", "child cards passed".to_owned())
    } else if ids.iter().any(|id| checks.iter().any(|item| item.0 == *id && item.1 == "fail")) {
        ("fail", "a child card failed".to_owned())
    } else {
        ("pending", "a child card is pending".to_owned())
    }
}

fn mp01a(observation: &Value) -> (&'static str, String) {
    let identities = observation.get("identities");
    let run = observation.get("run");
    let source = text(identities, "source_system_identifier");
    let target = text(identities, "target_system_identifier");
    let source_db = text(identities, "source_database");
    let target_db = text(identities, "target_database");
    if source.is_empty() || target.is_empty() || source_db.is_empty() || target_db.is_empty() {
        return ("pending", "identities were not measured".to_owned());
    }
    if source == target || source_db == target_db {
        return ("fail", "source and target identities are not distinct".to_owned());
    }
    if text(run, "source_tx") != "repeatable_read_read_only" || number(run, "source_peak") != Some(1) || !flag(run, "copy_performed") {
        return ("fail", "the copy was not one repeatable-read read-only source connection".to_owned());
    }
    if text(run, "import_hash").len() != 64 || text(run, "snapshot_id").is_empty() {
        return ("fail", "the sealed snapshot is absent".to_owned());
    }
    ("pass", "distinct source and target, one read-only snapshot connection".to_owned())
}

fn mp01b(observation: &Value) -> (&'static str, String) {
    let Some(tables) = observation.get("typed").and_then(|value| value.get("tables")).and_then(Value::as_array) else {
        return ("pending", "typed reconciliation was not measured".to_owned());
    };
    let expected = [("public", "hubs", 2000_i64), ("public", "shipments", 8000), ("public", "tracking_events", 90000)];
    if tables.len() != expected.len() {
        return ("fail", "typed table count is not three".to_owned());
    }
    for (table, (schema, name, rows)) in tables.iter().zip(expected) {
        if text(Some(table), "schema") != schema || text(Some(table), "name") != name {
            return ("fail", "typed table identity does not match the logistics fixture".to_owned());
        }
        if number(Some(table), "rows") != Some(rows) || number(Some(table), "distinct_keys") != Some(rows) || !flag(Some(table), "hash_equal") {
            return ("fail", "row count, key coverage, or canonical hash did not match".to_owned());
        }
    }
    let typed = observation.get("typed");
    if !(flag(typed, "null_preserved") && flag(typed, "int8_bounds") && flag(typed, "numeric_exact") && flag(typed, "timestamp_edges") && flag(typed, "bytea_edges")) {
        return ("fail", "a typed edge was not preserved".to_owned());
    }
    if flag(observation.get("run"), "target_was_preloaded") {
        return ("fail", "the target was preloaded instead of copied".to_owned());
    }
    ("pass", "100000 typed rows reconciled".to_owned())
}

fn mp01c(observation: &Value) -> (&'static str, String) {
    let fault = observation.get("copy_fault");
    if fault.is_none() {
        return ("pending", "copy fault, cancel, drift, and replay were not measured".to_owned());
    }
    if number(fault, "source_writes").is_none()
        || ["partial_visible", "snapshot_splice", "duplicate_batch", "pseudo_complete"].iter().any(|key| measured_flag(fault, key).is_none())
    {
        return ("pending", "copy fault evidence is incomplete".to_owned());
    }
    if number(fault, "source_writes") != Some(0) || flag(fault, "partial_visible") || flag(fault, "snapshot_splice") || flag(fault, "duplicate_batch") || flag(fault, "pseudo_complete") {
        return ("fail", "the copy fault left a false complete or a source write".to_owned());
    }
    ("pass", "copy faults stayed incomplete and did not write the source".to_owned())
}

fn mp01d(observation: &Value) -> (&'static str, String) {
    let run = observation.get("run");
    let unreachable = measured_flag(run, "source_unreachable_after_seal");
    let access = number(run, "source_access_after_seal");
    let withheld = measured_flag(run, "source_secret_withheld");
    if unreachable.is_none() || access.is_none() || withheld.is_none() {
        return ("pending", "source block after seal was not measured".to_owned());
    }
    if unreachable != Some(true) || access != Some(0) || withheld != Some(true) {
        return ("fail", "source remained reachable, was accessed, or its secret was present".to_owned());
    }
    let generation = questions_share_generation(observation);
    if generation.is_empty() || generation != text(run, "mapping_generation") {
        return ("fail", "query audits do not bind the sealed local generation".to_owned());
    }
    let target_db = text(observation.get("identities"), "target_database");
    if !questions().iter().all(|question| audit_database(observation, question.id) == target_db) {
        return ("fail", "a query audit does not name the local target".to_owned());
    }
    ("pass", "sealed local generation, zero source access".to_owned())
}

fn mp02(observation: &Value) -> (&'static str, String) {
    let model = observation.get("model");
    let pack = observation.get("pack");
    if model.is_none() || pack.is_none() {
        return ("pending", "model and pack were not measured".to_owned());
    }
    if number(model, "fresh_requests").unwrap_or(0) < 1 {
        return ("fail", "the first build did not record a fresh model request".to_owned());
    }
    if number(model, "cache_second_build_requests") != Some(0) || !flag(model, "cache_hit") {
        return ("fail", "the second build did not reuse the cache".to_owned());
    }
    if !flag(model, "changed_input_invalidates") {
        return ("fail", "a changed input did not invalidate the cache".to_owned());
    }
    if text(model, "outcome") != "validated" || text(model, "model_id") != "grok-4.7" {
        return ("fail", "the generation is not a validated grok-4.7 draft".to_owned());
    }
    if !flag(pack, "unreviewed") || !flag(pack, "not_published") || !flag(pack, "native_parse") {
        return ("fail", "the pack is not an unreviewed native draft".to_owned());
    }
    match number(model, "failure_exit") {
        Some(0) => return ("fail", "a model failure was reported as success".to_owned()),
        Some(_) if flag(model, "failure_ready") || flag(model, "failure_published") => {
            return ("fail", "a model failure was reported as ready or published".to_owned());
        }
        Some(_) => {}
        None => return ("pending", "model failure was not measured".to_owned()),
    }
    ("pass", "fresh build, cache reuse, unreviewed native draft".to_owned())
}

fn mp03(observation: &Value) -> (&'static str, String) {
    if observation.get("questions").and_then(Value::as_array).is_none() {
        return ("pending", "Pi tool events were not measured".to_owned());
    }
    for question in questions() {
        let Some(row) = question_row(observation, question.id) else {
            return ("pending", format!("{} was not asked", question.id));
        };
        if !flag(Some(row), "read_before_sql") || !flag(Some(row), "bash_wrote_sql") || flag(Some(row), "sql_prewritten") {
            return ("fail", format!("{id} did not read the pack and write its own SQL", id = question.id));
        }
        if text(Some(row), "gateway") != "query-pg" || !flag(Some(row), "proxy_mtls") || flag(Some(row), "direct_select") {
            return ("fail", format!("{id} did not use the proxy and native gateway", id = question.id));
        }
    }
    ("pass", "Pi read the pack, wrote SQL, and reached query-pg through mTLS".to_owned())
}

fn mp04(observation: &Value) -> (&'static str, String) {
    if observation.get("questions").and_then(Value::as_array).is_none() {
        return ("pending", "answers were not measured".to_owned());
    }
    for question in questions() {
        let Some(row) = question_row(observation, question.id) else {
            return ("fail", format!("{} is missing", question.id));
        };
        let query_id = text(Some(row), "query_id");
        if query_id.is_empty() || !flag(Some(row), "cited") || text(Some(row), "cited_query_id") != query_id {
            return ("fail", format!("{id} does not cite its successful query id", id = question.id));
        }
        if number(Some(row), "audit_http") != Some(200) {
            return ("fail", format!("{id} audit is not HTTP 200", id = question.id));
        }
        let expected = cells(question.id).unwrap_or_default();
        let got = row_cells(row);
        if got != expected {
            return ("fail", format!("{id} cells do not match the independent gold", id = question.id));
        }
    }
    ("pass", "six answers match the independent typed gold".to_owned())
}

fn mp05(observation: &Value) -> (&'static str, String) {
    let fault = observation.get("faults");
    if fault.is_none() {
        return ("pending", "negative cases were not measured".to_owned());
    }
    for key in ["bad_connection_exit", "model_failure_exit", "cancel_exit", "released_ask_exit"] {
        if number(fault, key).is_none() {
            return ("pending", format!("{key} was not measured"));
        }
    }
    for key in [
        "bad_connection_ready",
        "model_failure_ready",
        "write_rejected",
        "source_unchanged_after_write",
        "processes_absent",
        "proxy_absent",
        "data_persistent",
    ] {
        if measured_flag(fault, key).is_none() {
            return ("pending", format!("{key} was not measured"));
        }
    }
    if number(fault, "bad_connection_exit") == Some(0) || measured_flag(fault, "bad_connection_ready") == Some(true) {
        return ("fail", "a bad connection was treated as ready".to_owned());
    }
    if number(fault, "model_failure_exit") == Some(0) || measured_flag(fault, "model_failure_ready") == Some(true) {
        return ("fail", "a model failure was treated as ready".to_owned());
    }
    if measured_flag(fault, "write_rejected") != Some(true) || measured_flag(fault, "source_unchanged_after_write") != Some(true) {
        return ("fail", "a SQL write was not rejected or the source changed".to_owned());
    }
    if number(fault, "cancel_exit") == Some(0) || number(fault, "released_ask_exit") == Some(0) {
        return ("fail", "cancel or a released ask was accepted".to_owned());
    }
    if measured_flag(fault, "processes_absent") != Some(true)
        || measured_flag(fault, "proxy_absent") != Some(true)
        || measured_flag(fault, "data_persistent") != Some(true)
    {
        return ("fail", "close did not release processes and mappings while keeping the database".to_owned());
    }
    ("pass", "negative cases failed closed and the local copy stayed".to_owned())
}

fn mp06(observation: &Value) -> (&'static str, String) {
    let repro = observation.get("repro");
    if !flag(repro, "secret_free_template") || !flag(repro, "command_documented") || !flag(repro, "clean_start") {
        return ("pending", "the clean reproducible run was not measured".to_owned());
    }
    if !flag(repro, "second_run_cache_only") {
        return ("fail", "the repeat run issued another model request".to_owned());
    }
    ("pass", "secret-free template and a clean cached repeat".to_owned())
}

fn stages(observation: &Value) -> Vec<(&'static str, &'static str, String)> {
    let mut out = Vec::new();
    for id in ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10"] {
        out.push(stage(observation, id));
    }
    out
}

fn stage(observation: &Value, id: &'static str) -> (&'static str, &'static str, String) {
    let item = observation.get("stages").and_then(|value| value.get(id));
    if flag(item, "approved") || flag(item, "published") {
        let actor = text(item, "operator_actor");
        let human = !actor.is_empty() && !MODEL_ACTORS.contains(&actor.as_str()) && flag(item, "decision_recorded");
        if !human {
            return (id, "fake_approval", "an approval or publication has no separate operator decision".to_owned());
        }
        return (id, "fake_approval", "this card has no human operator and cannot record S9 or S10 as approved".to_owned());
    }
    if item.is_none() {
        return (id, "notdone", "stage was not measured".to_owned());
    }
    if flag(item, "executed") {
        return (id, "observed", "stage artifact exists and is not an approval".to_owned());
    }
    (id, "notdone", "stage was not executed".to_owned())
}

fn stages_honest(stages: &[(&str, &str, String)]) -> bool {
    stages.iter().all(|item| item.1 != "fake_approval")
        && stage_status(stages, "S9") != "approved"
        && stage_status(stages, "S10") != "published"
}

fn stage_status(stages: &[(&str, &str, String)], id: &str) -> String {
    stages.iter().find(|item| item.0 == id).map(|item| item.1.to_owned()).unwrap_or_else(|| "notdone".to_owned())
}

fn named(id: &str, status: &str, detail: &str) -> Value {
    Value::Object(vec![
        ("id".to_owned(), Value::String(id.to_owned())),
        ("status".to_owned(), Value::String(status.to_owned())),
        ("detail".to_owned(), Value::String(detail.to_owned())),
    ])
}

fn question_row<'a>(observation: &'a Value, id: &str) -> Option<&'a Value> {
    observation.get("questions").and_then(Value::as_array).and_then(|rows| rows.iter().find(|row| text(Some(row), "id") == id))
}

fn row_cells(row: &Value) -> Vec<Vec<String>> {
    row.get("cells")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| row.as_array().map(|cells| cells.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default()
}

fn questions_share_generation(observation: &Value) -> String {
    let mut seen = String::new();
    for question in questions() {
        let generation = question_row(observation, question.id).map(|row| text(Some(row), "audit_generation")).unwrap_or_default();
        if generation.is_empty() {
            return String::new();
        }
        if seen.is_empty() {
            seen = generation;
        } else if seen != generation {
            return String::new();
        }
    }
    seen
}

fn audit_database(observation: &Value, id: &str) -> String {
    question_row(observation, id).map(|row| text(Some(row), "audit_database")).unwrap_or_default()
}

fn text(value: Option<&Value>, key: &str) -> String {
    value.and_then(|item| item.get(key)).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn flag(value: Option<&Value>, key: &str) -> bool {
    measured_flag(value, key) == Some(true)
}

fn measured_flag(value: Option<&Value>, key: &str) -> Option<bool> {
    value.and_then(|item| item.get(key)).and_then(Value::as_bool)
}

fn number(value: Option<&Value>, key: &str) -> Option<i64> {
    value.and_then(|item| item.get(key)).and_then(Value::as_i64)
}
