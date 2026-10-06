//! Strict parse of one paired trial document.

use std::collections::BTreeSet;

use oo_core::parse_json_value;
use serde_json::{Map, Value};

use crate::DOCUMENT_MAX_BYTES;

pub(crate) const STDIN_CAP: u64 = DOCUMENT_MAX_BYTES as u64 + 1;

const MAX_QUESTIONS: usize = 256;
const MAX_TRIALS: usize = 8;
const MAX_RESAMPLES: u64 = 10_000;
const MAX_VARIANT_ID: usize = 64;
const MAX_QUESTION_ID: usize = 128;
const MAX_TUPLE_BYTES: usize = 128;
const MAX_PAYLOAD: usize = 65_536;
const PROTOCOL: &str = "oo.benchmark.trial-statistics/v1";
const ALGORITHM: &str = "oo.benchmark.paired-question-bootstrap/v1";

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
            Some(pointer) => format!("trial-statistics: {} {pointer}\n", self.token),
            None => format!("trial-statistics: {}\n", self.token),
        }
    }

    pub fn token(&self) -> &'static str {
        self.token
    }

    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Pass,
    Fail,
    NeedsReannotation,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub id: String,
    pub trials: Vec<Status>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Variant {
    pub id: String,
    pub questions: Vec<Question>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub seed: u64,
    pub seed_text: String,
    pub resamples: u32,
    pub trial_count: u32,
    pub variants: [Variant; 2],
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
        &["algorithm", "resamples", "seed", "variants", "version"],
        "/",
    )?;
    let version = req_str(root, "version", "/")?;
    if version != PROTOCOL {
        return Err(ConfigError::at("schema", "/version"));
    }
    let algorithm = req_str(root, "algorithm", "/")?;
    if algorithm != ALGORITHM {
        return Err(ConfigError::at("schema", "/algorithm"));
    }
    let seed_text = req_str(root, "seed", "/")?.to_owned();
    let seed = match spell_integer(&seed_text) {
        Spell::Value(value) => value,
        Spell::Negative | Spell::Overflow | Spell::NotInteger => {
            return Err(ConfigError::at("schema", "/seed"));
        }
    };
    let resamples = parse_resamples(req(root, "resamples", "/")?)?;
    let mut payload = seed_text.len();
    let items = array(req(root, "variants", "/")?, "/variants")?;
    if items.len() != 2 {
        return Err(ConfigError::at("schema", "/variants"));
    }
    let mut expected_k: Option<usize> = None;
    let left = parse_variant(&items[0], "/variants/0", &mut expected_k, &mut payload)?;
    let right = parse_variant(&items[1], "/variants/1", &mut expected_k, &mut payload)?;
    if left.variant.id == right.variant.id {
        return Err(ConfigError::at("schema", "/variants/1/id"));
    }
    align_variants(&left, &right)?;
    if payload > MAX_PAYLOAD {
        return Err(ConfigError::at("budget", "/"));
    }
    let trial_count = u32::try_from(expected_k.expect("k was set")).expect("trial count");
    Ok(Parsed {
        seed,
        seed_text,
        resamples,
        trial_count,
        variants: [left.variant, right.variant],
    })
}

struct ParsedVariant {
    variant: Variant,
    tuple: [String; 10],
}

fn parse_variant(
    value: &Value,
    pointer: &str,
    expected_k: &mut Option<usize>,
    payload: &mut usize,
) -> Result<ParsedVariant, ConfigError> {
    let map = object(value, pointer)?;
    check_keys(map, &["id", "questions", "tuple"], pointer)?;
    let id = req_str(map, "id", pointer)?.to_owned();
    ident(&id, MAX_VARIANT_ID, &child(pointer, "id"))?;
    *payload += id.len();
    let tuple = parse_tuple(req(map, "tuple", pointer)?, pointer, payload)?;
    let questions = parse_questions(
        req(map, "questions", pointer)?,
        &child(pointer, "questions"),
        expected_k,
        payload,
    )?;
    Ok(ParsedVariant {
        variant: Variant { id, questions },
        tuple,
    })
}

fn parse_tuple(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<[String; 10], ConfigError> {
    let at = child(pointer, "tuple");
    let map = object(value, &at)?;
    check_keys(map, &TUPLE_ORDER, &at)?;
    let mut values = Vec::with_capacity(TUPLE_ORDER.len());
    for key in TUPLE_ORDER {
        let text = req_str(map, key, &at)?.to_owned();
        bounded_label(&text, MAX_TUPLE_BYTES, &child(&at, key))?;
        *payload += text.len();
        values.push(text);
    }
    Ok(values.try_into().expect("ten tuple fields"))
}

fn align_variants(left: &ParsedVariant, right: &ParsedVariant) -> Result<(), ConfigError> {
    for (index, key) in TUPLE_ORDER.iter().enumerate() {
        if left.tuple[index] != right.tuple[index] {
            return Err(ConfigError::at("tuple", format!("/variants/1/tuple/{key}")));
        }
    }
    if left.variant.questions.len() != right.variant.questions.len() {
        return Err(ConfigError::at("schema", "/variants/1/questions"));
    }
    for (index, (left_question, right_question)) in left
        .variant
        .questions
        .iter()
        .zip(right.variant.questions.iter())
        .enumerate()
    {
        if left_question.id != right_question.id {
            return Err(ConfigError::at(
                "schema",
                format!("/variants/1/questions/{index}/id"),
            ));
        }
    }
    Ok(())
}

fn parse_questions(
    value: &Value,
    pointer: &str,
    expected_k: &mut Option<usize>,
    payload: &mut usize,
) -> Result<Vec<Question>, ConfigError> {
    let items = array(value, pointer)?;
    if items.is_empty() || items.len() > MAX_QUESTIONS {
        return Err(ConfigError::at("budget", pointer));
    }
    let mut seen = BTreeSet::new();
    let mut questions = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{pointer}/{index}");
        let map = object(item, &at)?;
        check_keys(map, &["id", "trials"], &at)?;
        let id = req_str(map, "id", &at)?.to_owned();
        ident(&id, MAX_QUESTION_ID, &child(&at, "id"))?;
        if !seen.insert(id.clone()) {
            return Err(ConfigError::at("schema", child(&at, "id")));
        }
        *payload += id.len();
        let trials = parse_trials(
            req(map, "trials", &at)?,
            &child(&at, "trials"),
            expected_k,
            payload,
        )?;
        questions.push(Question { id, trials });
    }
    Ok(questions)
}

fn parse_trials(
    value: &Value,
    pointer: &str,
    expected_k: &mut Option<usize>,
    payload: &mut usize,
) -> Result<Vec<Status>, ConfigError> {
    let items = array(value, pointer)?;
    if items.is_empty() || items.len() > MAX_TRIALS {
        return Err(ConfigError::at("budget", pointer));
    }
    if let Some(k) = *expected_k {
        if items.len() != k {
            return Err(ConfigError::at("trials", pointer));
        }
    } else {
        *expected_k = Some(items.len());
    }
    let mut trials = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{pointer}/{index}");
        let map = object(item, &at)?;
        check_keys(map, &["index", "status"], &at)?;
        match_index(req(map, "index", &at)?, index, &child(&at, "index"))?;
        let status = match req_str(map, "status", &at)? {
            "pass" => Status::Pass,
            "fail" => Status::Fail,
            "needs_reannotation" => Status::NeedsReannotation,
            _ => return Err(ConfigError::at("schema", child(&at, "status"))),
        };
        *payload += 1;
        trials.push(status);
    }
    Ok(trials)
}

fn match_index(value: &Value, position: usize, pointer: &str) -> Result<(), ConfigError> {
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", pointer));
    };
    let text = number.to_string();
    let expected = u64::try_from(position).expect("position");
    match spell_integer(&text) {
        Spell::Value(index) if index == expected => Ok(()),
        Spell::NotInteger => Err(ConfigError::at("schema", pointer)),
        Spell::Value(_) | Spell::Negative | Spell::Overflow => {
            Err(ConfigError::at("trials", pointer))
        }
    }
}

fn parse_resamples(value: &Value) -> Result<u32, ConfigError> {
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", "/resamples"));
    };
    match spell_integer(&number.to_string()) {
        Spell::Value(count) if (1..=MAX_RESAMPLES).contains(&count) => {
            Ok(u32::try_from(count).expect("resamples"))
        }
        Spell::Value(_) | Spell::Negative | Spell::Overflow => {
            Err(ConfigError::at("budget", "/resamples"))
        }
        Spell::NotInteger => Err(ConfigError::at("schema", "/resamples")),
    }
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

fn bounded_label(text: &str, max: usize, pointer: &str) -> Result<(), ConfigError> {
    let len = text.len();
    if len < 1 || len > max {
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

fn child(pointer: &str, key: &str) -> String {
    if pointer == "/" {
        format!("/{key}")
    } else {
        format!("{pointer}/{key}")
    }
}
