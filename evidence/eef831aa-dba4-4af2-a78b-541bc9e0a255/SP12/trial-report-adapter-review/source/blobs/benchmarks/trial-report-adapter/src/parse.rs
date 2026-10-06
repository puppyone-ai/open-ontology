//! Strict parse of two SQL runner reports and the explicit pairing labels.

use std::collections::BTreeSet;

use oo_core::parse_json_value;
use serde_json::{Map, Value};

use crate::DOCUMENT_MAX_BYTES;

pub(crate) const STDIN_CAP: u64 = DOCUMENT_MAX_BYTES as u64 + 1;

const PROTOCOL: &str = "oo.benchmark.trial-report-adapter/v1";
const REPORT_PROTOCOL: &str = "oo.benchmark.sql-runner-report/v1";
const STATS_ALGORITHM: &str = "oo.benchmark.paired-question-bootstrap/v1";
const LANE: &str = "trusted_sql";
const MODEL_IN: &str = "none";
const HARNESS: &str = "sql-runner";
const SCORER: &str = "oo.benchmark.sql-runner/v1";
const MAX_QUESTIONS: usize = 32;
const MAX_TRIALS: usize = 8;
const MAX_RESAMPLES: u64 = 10_000;
const MAX_VARIANT_ID: usize = 64;
const MAX_CASE_ID: usize = 64;
const MAX_TUPLE_BYTES: usize = 128;

const TUPLE_ORDER: [&str; 10] = [
    "project",
    "snapshot",
    "as_of",
    "dataset",
    "suite",
    "model",
    "harness",
    "role",
    "budget",
    "scorer_version",
];

const REPORT_KEYS: [&str; 15] = [
    "executable_sha256",
    "fail_count",
    "lane",
    "metadata",
    "mode",
    "not_run_count",
    "pass_count",
    "passed",
    "question_count",
    "reannotation_count",
    "timeout_count",
    "trial_count",
    "trials",
    "tuple",
    "version",
];

const TRIAL_KEYS: [&str; 12] = [
    "adapter_exit",
    "backend_cleanup",
    "case_id",
    "comparator_exit",
    "comparator_verdict",
    "end_audit_ok",
    "index",
    "partial",
    "query_exit",
    "reason",
    "status",
    "truncated",
];

const HASH_KEYS: [&str; 3] = ["adapter", "comparator", "query_pg"];

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError {
    token: &'static str,
    pointer: Option<String>,
}

impl ConfigError {
    pub(crate) fn bare(token: &'static str) -> Self {
        Self {
            token,
            pointer: None,
        }
    }

    pub(crate) fn at(token: &'static str, pointer: impl Into<String>) -> Self {
        Self {
            token,
            pointer: Some(pointer.into()),
        }
    }

    pub fn stderr_line(&self) -> String {
        match &self.pointer {
            Some(pointer) => format!("trial-report-adapter: {} {pointer}\n", self.token),
            None => format!("trial-report-adapter: {}\n", self.token),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Score {
    Pass,
    Fail,
    NeedsReannotation,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub id: String,
    pub trials: Vec<Score>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Variant {
    pub id: String,
    pub questions: Vec<Question>,
    pub tuple: [String; 10],
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub seed_text: String,
    pub resamples: u32,
    pub variants: [Variant; 2],
}

struct RawReport {
    questions: Vec<Question>,
    tuple: [String; 10],
}

struct Tally {
    pass: u64,
    fail: u64,
    timeout: u64,
    reannotation: u64,
    not_run: u64,
}

enum Spell {
    Value(u64),
    Negative,
    Overflow,
    NotInteger,
}

pub(crate) fn parse_input(input: &str) -> Result<Parsed, ConfigError> {
    if input.len() > DOCUMENT_MAX_BYTES {
        return Err(ConfigError::at("budget", "/"));
    }
    let value = parse_json_value(input, PROTOCOL).map_err(|_| ConfigError::bare("json"))?;
    let root = object(&value, "/")?;
    check_keys(
        root,
        &[
            "algorithm",
            "labels",
            "reports",
            "resamples",
            "seed",
            "version",
        ],
        "/",
    )?;
    let version = req_str(root, "version", "/")?;
    if version != PROTOCOL {
        return Err(ConfigError::at("schema", "/version"));
    }
    let algorithm = req_str(root, "algorithm", "/")?;
    if algorithm != STATS_ALGORITHM {
        return Err(ConfigError::at("schema", "/algorithm"));
    }
    let seed_text = req_str(root, "seed", "/")?.to_owned();
    match spell_integer(&seed_text) {
        Spell::Value(_) => {}
        Spell::Negative | Spell::Overflow | Spell::NotInteger => {
            return Err(ConfigError::at("schema", "/seed"));
        }
    }
    let resamples = parse_resamples(req(root, "resamples", "/")?)?;
    let (budget, scorer, ids) = parse_labels(req(root, "labels", "/")?)?;
    let reports = array(req(root, "reports", "/")?, "/reports")?;
    if reports.len() != 2 {
        return Err(ConfigError::at("schema", "/reports"));
    }
    let left = parse_report(&reports[0], "/reports/0")?;
    let right = parse_report(&reports[1], "/reports/1")?;
    align_reports(&left, &right)?;
    if left.tuple[8] != budget {
        return Err(ConfigError::at("tuple", "/labels/budget"));
    }
    if left.tuple[9] != scorer {
        return Err(ConfigError::at("tuple", "/labels/scorer_version"));
    }
    let variants = [
        variant(ids[0].clone(), left),
        variant(ids[1].clone(), right),
    ];
    Ok(Parsed {
        seed_text,
        resamples,
        variants,
    })
}

fn variant(id: String, report: RawReport) -> Variant {
    let mut tuple = report.tuple;
    tuple[5] = LANE.to_owned();
    Variant {
        id,
        questions: report.questions,
        tuple,
    }
}

fn parse_labels(value: &Value) -> Result<(String, String, [String; 2]), ConfigError> {
    let map = object(value, "/labels")?;
    check_keys(map, &["budget", "scorer_version", "variants"], "/labels")?;
    let budget = req_str(map, "budget", "/labels")?.to_owned();
    bounded_label(&budget, "/labels/budget")?;
    let scorer = req_str(map, "scorer_version", "/labels")?.to_owned();
    bounded_label(&scorer, "/labels/scorer_version")?;
    let items = array(req(map, "variants", "/labels")?, "/labels/variants")?;
    if items.len() != 2 {
        return Err(ConfigError::at("schema", "/labels/variants"));
    }
    let left = variant_id(&items[0], "/labels/variants/0")?;
    let right = variant_id(&items[1], "/labels/variants/1")?;
    if left == right {
        return Err(ConfigError::at("schema", "/labels/variants/1/id"));
    }
    Ok((budget, scorer, [left, right]))
}

fn variant_id(value: &Value, pointer: &str) -> Result<String, ConfigError> {
    let map = object(value, pointer)?;
    check_keys(map, &["id"], pointer)?;
    let id = req_str(map, "id", pointer)?.to_owned();
    ident(&id, MAX_VARIANT_ID, &child(pointer, "id"))?;
    Ok(id)
}

fn parse_report(value: &Value, pointer: &str) -> Result<RawReport, ConfigError> {
    let map = object(value, pointer)?;
    check_keys(map, &REPORT_KEYS, pointer)?;
    let version = req_str(map, "version", pointer)?;
    if version != REPORT_PROTOCOL {
        return Err(ConfigError::at("schema", child(pointer, "version")));
    }
    let lane = req_str(map, "lane", pointer)?;
    if lane != LANE {
        return Err(ConfigError::at("lane", child(pointer, "lane")));
    }
    let metadata = req_str(map, "metadata", pointer)?;
    if metadata != "declared" {
        return Err(ConfigError::at("schema", child(pointer, "metadata")));
    }
    let mode = req_str(map, "mode", pointer)?;
    if mode != "synthetic" && mode != "frozen_eval" {
        return Err(ConfigError::at("schema", child(pointer, "mode")));
    }
    check_hashes(req(map, "executable_sha256", pointer)?, pointer)?;
    let tuple = parse_tuple(req(map, "tuple", pointer)?, pointer)?;
    let (questions, tally) = parse_trials(req(map, "trials", pointer)?, pointer)?;
    check_counts(map, pointer, &questions, &tally)?;
    Ok(RawReport { questions, tuple })
}

fn check_hashes(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    let at = child(pointer, "executable_sha256");
    let map = object(value, &at)?;
    check_keys(map, &HASH_KEYS, &at)?;
    for key in HASH_KEYS {
        let text = req_str(map, key, &at)?;
        if !hex64(text) {
            return Err(ConfigError::at("schema", child(&at, key)));
        }
    }
    Ok(())
}

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn parse_tuple(value: &Value, pointer: &str) -> Result<[String; 10], ConfigError> {
    let at = child(pointer, "tuple");
    let map = object(value, &at)?;
    check_keys(map, &TUPLE_ORDER, &at)?;
    let mut values = Vec::with_capacity(TUPLE_ORDER.len());
    for key in TUPLE_ORDER {
        let text = req_str(map, key, &at)?.to_owned();
        bounded_label(&text, &child(&at, key))?;
        if text == "unknown" || text == "production" {
            return Err(ConfigError::at("tuple", child(&at, key)));
        }
        values.push(text);
    }
    let tuple: [String; 10] = values.try_into().expect("ten tuple fields");
    if tuple[5] != MODEL_IN {
        return Err(ConfigError::at("tuple", child(&at, "model")));
    }
    if tuple[6] != HARNESS {
        return Err(ConfigError::at("tuple", child(&at, "harness")));
    }
    if tuple[9] != SCORER {
        return Err(ConfigError::at("tuple", child(&at, "scorer_version")));
    }
    Ok(tuple)
}

fn parse_trials(value: &Value, pointer: &str) -> Result<(Vec<Question>, Tally), ConfigError> {
    let at = child(pointer, "trials");
    let items = array(value, &at)?;
    if items.is_empty() {
        return Err(ConfigError::at("budget", &at));
    }
    let mut questions = Vec::new();
    let mut seen = BTreeSet::new();
    let mut tally = Tally {
        pass: 0,
        fail: 0,
        timeout: 0,
        reannotation: 0,
        not_run: 0,
    };
    let mut index = 0usize;
    while index < items.len() {
        let case_id = case_id_at(&items[index], &at, index)?;
        if !seen.insert(case_id.clone()) {
            return Err(ConfigError::at(
                "schema",
                child(&trial_at(&at, index), "case_id"),
            ));
        }
        let mut scores = Vec::new();
        while index < items.len() && case_id_at(&items[index], &at, index)? == case_id {
            let score = parse_trial(&items[index], &at, index, scores.len(), &mut tally)?;
            scores.push(score);
            index += 1;
        }
        if scores.is_empty() || scores.len() > MAX_TRIALS {
            return Err(ConfigError::at("budget", &at));
        }
        questions.push(Question {
            id: case_id,
            trials: scores,
        });
    }
    if questions.len() > MAX_QUESTIONS {
        return Err(ConfigError::at("budget", &at));
    }
    let width = questions[0].trials.len();
    for question in &questions {
        if question.trials.len() != width {
            return Err(ConfigError::at("trials", &at));
        }
    }
    Ok((questions, tally))
}

fn case_id_at(value: &Value, trials: &str, index: usize) -> Result<String, ConfigError> {
    let at = trial_at(trials, index);
    let map = object(value, &at)?;
    check_keys(map, &TRIAL_KEYS, &at)?;
    let id = req_str(map, "case_id", &at)?.to_owned();
    ident(&id, MAX_CASE_ID, &child(&at, "case_id"))?;
    Ok(id)
}

fn parse_trial(
    value: &Value,
    trials: &str,
    index: usize,
    position: usize,
    tally: &mut Tally,
) -> Result<Score, ConfigError> {
    let at = trial_at(trials, index);
    let map = object(value, &at)?;
    match_index(req(map, "index", &at)?, position, &child(&at, "index"))?;
    let status = req_str(map, "status", &at)?;
    let reason = req_str(map, "reason", &at)?;
    let verdict = req_str(map, "comparator_verdict", &at)?;
    optional_i32(req(map, "query_exit", &at)?, &child(&at, "query_exit"))?;
    optional_i32(req(map, "adapter_exit", &at)?, &child(&at, "adapter_exit"))?;
    optional_i32(
        req(map, "comparator_exit", &at)?,
        &child(&at, "comparator_exit"),
    )?;
    optional_bool(req(map, "truncated", &at)?, &child(&at, "truncated"))?;
    optional_bool(req(map, "end_audit_ok", &at)?, &child(&at, "end_audit_ok"))?;
    optional_bool(req(map, "partial", &at)?, &child(&at, "partial"))?;
    let score = map_score(status, reason, verdict)
        .map_err(|field| ConfigError::at("schema", child(&at, field)))?;
    check_cleanup(
        req(map, "backend_cleanup", &at)?,
        status,
        &child(&at, "backend_cleanup"),
    )?;
    add_tally(tally, status);
    Ok(score)
}

const ABSENT_FAIL: [&str; 8] = [
    "adapter_refused",
    "budget",
    "comparator_refused",
    "no_end",
    "partial",
    "protocol",
    "query",
    "refused",
];

fn map_score(status: &str, reason: &str, verdict: &str) -> Result<Score, &'static str> {
    let score = match (status, reason) {
        ("pass", "match") => Score::Pass,
        ("fail", "mismatch") | ("fail", "truncated") => Score::Fail,
        ("fail", reason) if ABSENT_FAIL.contains(&reason) => Score::Fail,
        ("timeout", "timeout") | ("not_run", "not_run") => Score::Fail,
        ("needs_reannotation", "needs_reannotation") => Score::NeedsReannotation,
        _ => return Err("status"),
    };
    let verdict_ok = match (status, reason) {
        ("pass", _) => verdict == "match",
        ("fail", "mismatch") => verdict == "mismatch",
        ("fail", "truncated") => verdict == "match" || verdict == "mismatch",
        ("needs_reannotation", _) => matches!(verdict, "match" | "mismatch" | "absent"),
        _ => verdict == "absent",
    };
    if verdict_ok {
        Ok(score)
    } else {
        Err("comparator_verdict")
    }
}

fn add_tally(tally: &mut Tally, status: &str) {
    match status {
        "pass" => tally.pass += 1,
        "fail" => tally.fail += 1,
        "timeout" => tally.timeout += 1,
        "not_run" => tally.not_run += 1,
        "needs_reannotation" => tally.reannotation += 1,
        _ => {}
    }
}

fn check_cleanup(value: &Value, status: &str, pointer: &str) -> Result<(), ConfigError> {
    let ok = if status == "not_run" {
        value.is_null()
    } else {
        matches!(value, Value::String(text) if text == "unknown")
    };
    if ok {
        Ok(())
    } else {
        Err(ConfigError::at("schema", pointer))
    }
}

fn check_counts(
    map: &Map<String, Value>,
    pointer: &str,
    questions: &[Question],
    tally: &Tally,
) -> Result<(), ConfigError> {
    let questions_len = u64::try_from(questions.len()).expect("question count");
    let trials = tally
        .pass
        .saturating_add(tally.fail)
        .saturating_add(tally.timeout)
        .saturating_add(tally.reannotation)
        .saturating_add(tally.not_run);
    expect_u64(map, "question_count", questions_len, pointer)?;
    expect_u64(map, "trial_count", trials, pointer)?;
    expect_u64(map, "pass_count", tally.pass, pointer)?;
    expect_u64(map, "fail_count", tally.fail, pointer)?;
    expect_u64(map, "timeout_count", tally.timeout, pointer)?;
    expect_u64(map, "not_run_count", tally.not_run, pointer)?;
    expect_u64(map, "reannotation_count", tally.reannotation, pointer)?;
    let passed = req_bool(map, "passed", pointer)?;
    if passed != (trials > 0 && tally.pass == trials) {
        return Err(ConfigError::at("schema", child(pointer, "passed")));
    }
    Ok(())
}

fn align_reports(left: &RawReport, right: &RawReport) -> Result<(), ConfigError> {
    for (index, key) in TUPLE_ORDER.iter().enumerate() {
        if left.tuple[index] != right.tuple[index] {
            return Err(ConfigError::at("tuple", format!("/reports/1/tuple/{key}")));
        }
    }
    if left.questions.len() != right.questions.len() {
        return Err(ConfigError::at("schema", "/reports/1/trials"));
    }
    if left.questions.first().map(|question| question.trials.len())
        != right
            .questions
            .first()
            .map(|question| question.trials.len())
    {
        return Err(ConfigError::at("trials", "/reports/1/trials"));
    }
    for (index, (left_question, right_question)) in left
        .questions
        .iter()
        .zip(right.questions.iter())
        .enumerate()
    {
        if left_question.id != right_question.id {
            let width = left.questions[0].trials.len();
            let trial = index.checked_mul(width).expect("trial index");
            return Err(ConfigError::at(
                "schema",
                format!("/reports/1/trials/{trial}/case_id"),
            ));
        }
        if left_question.trials.len() != right_question.trials.len() {
            return Err(ConfigError::at("trials", "/reports/1/trials"));
        }
    }
    Ok(())
}

fn expect_u64(
    map: &Map<String, Value>,
    key: &str,
    want: u64,
    pointer: &str,
) -> Result<(), ConfigError> {
    if req_u64(map, key, pointer)? != want {
        return Err(ConfigError::at("schema", child(pointer, key)));
    }
    Ok(())
}

fn req_u64(map: &Map<String, Value>, key: &str, pointer: &str) -> Result<u64, ConfigError> {
    let at = child(pointer, key);
    match spell_number(req(map, key, pointer)?, &at)? {
        Spell::Value(value) => Ok(value),
        Spell::Negative | Spell::Overflow | Spell::NotInteger => Err(ConfigError::at("schema", at)),
    }
}

fn req_bool(map: &Map<String, Value>, key: &str, pointer: &str) -> Result<bool, ConfigError> {
    match req(map, key, pointer)? {
        Value::Bool(flag) => Ok(*flag),
        _ => Err(ConfigError::at("schema", child(pointer, key))),
    }
}

fn match_index(value: &Value, position: usize, pointer: &str) -> Result<(), ConfigError> {
    let expected = u64::try_from(position).expect("position");
    match spell_number(value, pointer)? {
        Spell::Value(index) if index == expected => Ok(()),
        Spell::NotInteger => Err(ConfigError::at("schema", pointer)),
        Spell::Value(_) | Spell::Negative | Spell::Overflow => {
            Err(ConfigError::at("trials", pointer))
        }
    }
}

fn optional_i32(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    if value.is_null() {
        return Ok(());
    }
    match spell_number(value, pointer)? {
        Spell::Value(_) | Spell::Negative => Ok(()),
        Spell::NotInteger | Spell::Overflow => Err(ConfigError::at("schema", pointer)),
    }
}

fn optional_bool(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn parse_resamples(value: &Value) -> Result<u32, ConfigError> {
    match spell_number(value, "/resamples")? {
        Spell::Value(count) if (1..=MAX_RESAMPLES).contains(&count) => {
            Ok(u32::try_from(count).expect("resamples"))
        }
        Spell::Value(_) | Spell::Negative | Spell::Overflow => {
            Err(ConfigError::at("budget", "/resamples"))
        }
        Spell::NotInteger => Err(ConfigError::at("schema", "/resamples")),
    }
}

fn spell_number(value: &Value, pointer: &str) -> Result<Spell, ConfigError> {
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", pointer));
    };
    Ok(spell_integer(&number.to_string()))
}

fn spell_integer(text: &str) -> Spell {
    if text.is_empty() {
        return Spell::NotInteger;
    }
    let bytes = text.as_bytes();
    if bytes[0] == b'-' {
        let rest = &text[1..];
        if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
            return Spell::NotInteger;
        }
        if rest.len() > 1 && rest.starts_with('0') {
            return Spell::NotInteger;
        }
        return Spell::Negative;
    }
    if bytes.contains(&b'.')
        || bytes.contains(&b'e')
        || bytes.contains(&b'E')
        || bytes.contains(&b'+')
        || !bytes.iter().all(|byte| byte.is_ascii_digit())
    {
        return Spell::NotInteger;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return Spell::NotInteger;
    }
    match parse_u64(text) {
        Some(value) => Spell::Value(value),
        None => Spell::Overflow,
    }
}

fn parse_u64(text: &str) -> Option<u64> {
    let mut value = 0u64;
    for byte in text.bytes() {
        let digit = u64::from(byte - b'0');
        if value > (u64::MAX - digit) / 10 {
            return None;
        }
        value = value * 10 + digit;
    }
    Some(value)
}

fn ident(text: &str, max: usize, pointer: &str) -> Result<(), ConfigError> {
    let len = text.len();
    if len < 1 || len > max {
        return Err(ConfigError::at("budget", pointer));
    }
    let bytes = text.as_bytes();
    let body_ok = bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'));
    if bytes[0].is_ascii_alphabetic() && body_ok {
        Ok(())
    } else {
        Err(ConfigError::at("schema", pointer))
    }
}

fn bounded_label(text: &str, pointer: &str) -> Result<(), ConfigError> {
    let len = text.len();
    if !(1..=MAX_TUPLE_BYTES).contains(&len) {
        return Err(ConfigError::at("budget", pointer));
    }
    if text.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(())
}

fn object<'a>(value: &'a Value, pointer: &str) -> Result<&'a Map<String, Value>, ConfigError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, ConfigError> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

fn req<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a Value, ConfigError> {
    map.get(key)
        .ok_or_else(|| ConfigError::at("schema", child(pointer, key)))
}

fn req_str<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<&'a str, ConfigError> {
    match req(map, key, pointer)? {
        Value::String(text) => Ok(text),
        _ => Err(ConfigError::at("schema", child(pointer, key))),
    }
}

fn check_keys(
    map: &Map<String, Value>,
    allowed: &[&str],
    pointer: &str,
) -> Result<(), ConfigError> {
    let mut unknown: Vec<&str> = map
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .map(String::as_str)
        .collect();
    unknown.sort_unstable();
    if let Some(key) = unknown.first() {
        return Err(ConfigError::at("schema", child(pointer, key)));
    }
    Ok(())
}

fn trial_at(trials: &str, index: usize) -> String {
    format!("{trials}/{index}")
}

fn child(pointer: &str, key: &str) -> String {
    if pointer == "/" {
        format!("/{key}")
    } else {
        format!("{pointer}/{key}")
    }
}
