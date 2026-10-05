//! Trial report. Object keys are alphabetical because the map is a `BTreeMap`.

use serde_json::{Map, Number, Value};

use crate::parse::{Annotation, Loaded};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Pass,
    Fail,
    Timeout,
    NeedsReannotation,
    NotRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    Match,
    Mismatch,
    Truncated,
    Refused,
    NoEnd,
    Partial,
    Timeout,
    Query,
    AdapterRefused,
    ComparatorRefused,
    Budget,
    Protocol,
    NotRun,
    NeedsReannotation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Match,
    Mismatch,
    Absent,
}

#[derive(Debug)]
pub(crate) struct TrialOut {
    pub case_id: String,
    pub index: u32,
    pub status: Status,
    pub reason: Reason,
    pub query_exit: Option<i32>,
    pub adapter_exit: Option<i32>,
    pub comparator_exit: Option<i32>,
    pub truncated: Option<bool>,
    pub end_audit_ok: Option<bool>,
    pub partial: Option<bool>,
    pub backend_cleanup: Option<&'static str>,
    pub verdict: Verdict,
}

pub(crate) struct Rendered {
    pub passed: bool,
    pub bytes: Vec<u8>,
}

pub(crate) fn blank(case_id: &str, index: u32) -> TrialOut {
    TrialOut {
        case_id: case_id.to_owned(),
        index,
        status: Status::NotRun,
        reason: Reason::NotRun,
        query_exit: None,
        adapter_exit: None,
        comparator_exit: None,
        truncated: None,
        end_audit_ok: None,
        partial: None,
        backend_cleanup: None,
        verdict: Verdict::Absent,
    }
}

pub(crate) fn apply_annotation(trial: &mut TrialOut, annotation: Annotation) {
    if annotation == Annotation::NeedsReannotation && trial.status != Status::NotRun {
        trial.status = Status::NeedsReannotation;
        trial.reason = Reason::NeedsReannotation;
    }
}

pub(crate) fn render(loaded: &Loaded, trials: &[TrialOut]) -> Rendered {
    let mut pass_count = 0u64;
    let mut fail_count = 0u64;
    let mut timeout_count = 0u64;
    let mut reannotation_count = 0u64;
    let mut not_run_count = 0u64;
    for trial in trials {
        match trial.status {
            Status::Pass => pass_count += 1,
            Status::Fail => fail_count += 1,
            Status::Timeout => timeout_count += 1,
            Status::NeedsReannotation => reannotation_count += 1,
            Status::NotRun => not_run_count += 1,
        }
    }
    let trial_count = u64::try_from(trials.len()).expect("trial count fits");
    let passed = trial_count > 0 && pass_count == trial_count;
    let mut root = Map::new();
    root.insert(
        "executable_sha256".to_owned(),
        Value::Object({
            let mut hashes = Map::new();
            hashes.insert(
                "adapter".to_owned(),
                Value::String(loaded.executables.adapter_hash.clone()),
            );
            hashes.insert(
                "comparator".to_owned(),
                Value::String(loaded.executables.comparator_hash.clone()),
            );
            hashes.insert(
                "query_pg".to_owned(),
                Value::String(loaded.executables.query_hash.clone()),
            );
            hashes
        }),
    );
    root.insert("fail_count".to_owned(), json_u64(fail_count));
    root.insert("lane".to_owned(), Value::String("trusted_sql".to_owned()));
    root.insert("metadata".to_owned(), Value::String("declared".to_owned()));
    root.insert("mode".to_owned(), Value::String(loaded.mode.clone()));
    root.insert("not_run_count".to_owned(), json_u64(not_run_count));
    root.insert("pass_count".to_owned(), json_u64(pass_count));
    root.insert("passed".to_owned(), Value::Bool(passed));
    root.insert(
        "question_count".to_owned(),
        json_u64(u64::try_from(loaded.cases.len()).expect("case count")),
    );
    root.insert(
        "reannotation_count".to_owned(),
        json_u64(reannotation_count),
    );
    root.insert("timeout_count".to_owned(), json_u64(timeout_count));
    root.insert("trial_count".to_owned(), json_u64(trial_count));
    root.insert(
        "trials".to_owned(),
        Value::Array(trials.iter().map(trial_value).collect()),
    );
    root.insert("tuple".to_owned(), tuple_value(loaded));
    root.insert(
        "version".to_owned(),
        Value::String("oo.benchmark.sql-runner-report/v1".to_owned()),
    );
    let mut bytes = serde_json::to_vec(&Value::Object(root)).expect("report json");
    bytes.push(b'\n');
    Rendered { passed, bytes }
}

fn trial_value(trial: &TrialOut) -> Value {
    let mut map = Map::new();
    map.insert("adapter_exit".to_owned(), opt_i32(trial.adapter_exit));
    map.insert(
        "backend_cleanup".to_owned(),
        match trial.backend_cleanup {
            Some(text) => Value::String(text.to_owned()),
            None => Value::Null,
        },
    );
    map.insert("case_id".to_owned(), Value::String(trial.case_id.clone()));
    map.insert("comparator_exit".to_owned(), opt_i32(trial.comparator_exit));
    map.insert(
        "comparator_verdict".to_owned(),
        Value::String(verdict_name(trial.verdict).to_owned()),
    );
    map.insert("end_audit_ok".to_owned(), opt_bool(trial.end_audit_ok));
    map.insert("index".to_owned(), json_u64(u64::from(trial.index)));
    map.insert("partial".to_owned(), opt_bool(trial.partial));
    map.insert("query_exit".to_owned(), opt_i32(trial.query_exit));
    map.insert(
        "reason".to_owned(),
        Value::String(reason_name(trial.reason).to_owned()),
    );
    map.insert(
        "status".to_owned(),
        Value::String(status_name(trial.status).to_owned()),
    );
    map.insert("truncated".to_owned(), opt_bool(trial.truncated));
    Value::Object(map)
}

fn tuple_value(loaded: &Loaded) -> Value {
    let tuple = &loaded.tuple;
    let mut map = Map::new();
    map.insert("as_of".to_owned(), Value::String(tuple.as_of.clone()));
    map.insert("budget".to_owned(), Value::String(tuple.budget.clone()));
    map.insert("dataset".to_owned(), Value::String(tuple.dataset.clone()));
    map.insert("harness".to_owned(), Value::String(tuple.harness.clone()));
    map.insert("model".to_owned(), Value::String(tuple.model.clone()));
    map.insert("project".to_owned(), Value::String(tuple.project.clone()));
    map.insert("role".to_owned(), Value::String(tuple.role.clone()));
    map.insert(
        "scorer_version".to_owned(),
        Value::String(tuple.scorer_version.clone()),
    );
    map.insert("snapshot".to_owned(), Value::String(tuple.snapshot.clone()));
    map.insert("suite".to_owned(), Value::String(tuple.suite.clone()));
    Value::Object(map)
}

fn json_u64(value: u64) -> Value {
    Value::Number(Number::from(value))
}

fn opt_i32(value: Option<i32>) -> Value {
    match value {
        Some(code) => Value::Number(Number::from(code)),
        None => Value::Null,
    }
}

fn opt_bool(value: Option<bool>) -> Value {
    match value {
        Some(flag) => Value::Bool(flag),
        None => Value::Null,
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::Timeout => "timeout",
        Status::NeedsReannotation => "needs_reannotation",
        Status::NotRun => "not_run",
    }
}

fn reason_name(reason: Reason) -> &'static str {
    match reason {
        Reason::Match => "match",
        Reason::Mismatch => "mismatch",
        Reason::Truncated => "truncated",
        Reason::Refused => "refused",
        Reason::NoEnd => "no_end",
        Reason::Partial => "partial",
        Reason::Timeout => "timeout",
        Reason::Query => "query",
        Reason::AdapterRefused => "adapter_refused",
        Reason::ComparatorRefused => "comparator_refused",
        Reason::Budget => "budget",
        Reason::Protocol => "protocol",
        Reason::NotRun => "not_run",
        Reason::NeedsReannotation => "needs_reannotation",
    }
}

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Match => "match",
        Verdict::Mismatch => "mismatch",
        Verdict::Absent => "absent",
    }
}

pub(crate) fn status_for(reason: Reason) -> Status {
    match reason {
        Reason::Match => Status::Pass,
        Reason::Timeout => Status::Timeout,
        Reason::NotRun => Status::NotRun,
        Reason::NeedsReannotation => Status::NeedsReannotation,
        Reason::Mismatch
        | Reason::Truncated
        | Reason::Refused
        | Reason::NoEnd
        | Reason::Partial
        | Reason::Query
        | Reason::AdapterRefused
        | Reason::ComparatorRefused
        | Reason::Budget
        | Reason::Protocol => Status::Fail,
    }
}
