//! Strict parse of one public question bank.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::decimal::ExactDecimal;
use oo_core::limits::{
    CANONICAL_MAX_COEFFICIENT_DIGITS, CANONICAL_MAX_FIELD_NAME_BYTES, QUERY_CELL_MAX_CHARS,
};
use oo_core::parse_json_value;
use oo_core::rfc3339::Rfc3339Timestamp;
use serde_json::{Map, Number, Value};

use crate::DOCUMENT_MAX_BYTES;

pub(crate) const STDIN_CAP: u64 = DOCUMENT_MAX_BYTES as u64 + 1;

const MAX_QUESTIONS: usize = 64;
const MAX_CATEGORIES: usize = 4;
const MAX_METRICS: usize = 8;
const MAX_CLAIMS: usize = 8;
const MAX_CLAIM_VALUES: usize = 4;
const MAX_INTERPRETATIONS: usize = 4;
const MAX_TOTALS: usize = 8;
const MAX_NOTES: usize = 8;
const MAX_ROWS: usize = 32;
const MAX_COLUMNS: usize = 16;
const MAX_PAYLOAD: usize = 16_384;
const MAX_QUESTION_SCALARS: usize = 500;
const MAX_GOLD_SQL_BYTES: usize = 2_000;
const MAX_STATEMENT_SCALARS: usize = 200;
const MAX_ID_BYTES: usize = 64;
const PROTOCOL: &str = "oo.benchmark.question-bank/v1";

const CATEGORIES: [&str; 17] = [
    "ambiguity",
    "code",
    "data_quality",
    "entity_ambiguity",
    "entity_join",
    "injection",
    "large_result",
    "metric",
    "multi_table",
    "multi_turn",
    "not_found",
    "permission",
    "single_table",
    "time_anchor",
    "time_relative",
    "top_k",
    "undetermined_code",
];

const QUESTION_KEYS: [&str; 16] = [
    "accepted_interpretations",
    "annotations",
    "as_of",
    "asker",
    "category",
    "control_totals",
    "difficulty",
    "expected",
    "id",
    "provenance",
    "question",
    "required_metrics",
    "review",
    "role_group",
    "split",
    "version",
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
            Some(pointer) => format!("question-bank: {} {pointer}\n", self.token),
            None => format!("question-bank: {}\n", self.token),
        }
    }

    pub fn token(&self) -> &'static str {
        self.token
    }

    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValueColumn {
    pub name: String,
    pub tolerance: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Expected {
    pub kind: String,
    pub gold_sql: Option<String>,
    pub ordered: Option<bool>,
    pub key_columns: Option<Vec<String>>,
    pub value_columns: Option<Vec<ValueColumn>>,
    pub unresolved: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Interpretation {
    pub id: String,
    pub statement: String,
    pub selected: bool,
    pub gold_sql: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Total {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Note {
    pub label: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub id: String,
    pub question: String,
    pub role: String,
    pub claims: Vec<(String, Vec<String>)>,
    pub category: Vec<String>,
    pub difficulty: String,
    pub expected: Expected,
    pub required_metrics: Vec<String>,
    pub as_of: String,
    pub interpretations: Vec<Interpretation>,
    pub split: String,
    pub version: String,
    pub role_group: String,
    pub review_status: String,
    pub dual_annotation: String,
    pub control_totals: Vec<Total>,
    pub annotations: Vec<Note>,
    pub typed_gold: Option<Value>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Bank {
    pub bank_version: String,
    pub dataset_id: String,
    pub scale: String,
    pub suite_id: String,
    pub track: String,
    pub questions: Vec<Question>,
}

enum Defect {
    Ambiguity,
    MissingGold,
    Tolerance,
    DualDisagree,
}

impl Defect {
    fn label(self) -> &'static str {
        match self {
            Self::Ambiguity => "ambiguity",
            Self::MissingGold => "missing_gold",
            Self::Tolerance => "tolerance_not_exact",
            Self::DualDisagree => "dual_disagree",
        }
    }
}

pub(crate) fn parse_input(input: &str) -> Result<Bank, ConfigError> {
    if input.len() > DOCUMENT_MAX_BYTES {
        return Err(ConfigError::at("budget", "/"));
    }
    let value = parse_json_value(input, PROTOCOL).map_err(|_| ConfigError::bare("json"))?;
    let root = object(&value, "/")?;
    check_keys(
        root,
        &["bank_version", "dataset", "questions", "suite", "version"],
        "/",
    )?;
    let version = req_str(root, "version", "/")?;
    if version != PROTOCOL {
        return Err(ConfigError::at("schema", "/version"));
    }
    let bank_version = req_str(root, "bank_version", "/")?.to_owned();
    if !bank_version_ok(&bank_version) {
        return Err(ConfigError::at("schema", "/bank_version"));
    }
    let dataset = parse_dataset(req(root, "dataset", "/")?)?;
    let suite = parse_suite(req(root, "suite", "/")?)?;
    let mut payload = 0;
    let questions = parse_questions(req(root, "questions", "/")?, &bank_version, &mut payload)?;
    if payload > MAX_PAYLOAD {
        return Err(ConfigError::at("budget", "/"));
    }
    check_role_groups(&questions)?;
    Ok(Bank {
        bank_version,
        dataset_id: dataset.0,
        scale: dataset.1,
        suite_id: suite.0,
        track: suite.1,
        questions,
    })
}

fn parse_dataset(value: &Value) -> Result<(String, String), ConfigError> {
    let map = object(value, "/dataset")?;
    check_keys(map, &["distribution", "id", "scale"], "/dataset")?;
    let id = ident_id(req_str(map, "id", "/dataset")?, "/dataset/id")?;
    if map.get("distribution").and_then(Value::as_str) != Some("public_synthetic") {
        return Err(ConfigError::at("schema", "/dataset/distribution"));
    }
    let scale = req_str(map, "scale", "/dataset")?;
    if !matches!(scale, "xs" | "s" | "m" | "l") {
        return Err(ConfigError::at("schema", "/dataset/scale"));
    }
    Ok((id, scale.to_owned()))
}

fn parse_suite(value: &Value) -> Result<(String, String), ConfigError> {
    let map = object(value, "/suite")?;
    check_keys(map, &["id", "track"], "/suite")?;
    let id = ident_id(req_str(map, "id", "/suite")?, "/suite/id")?;
    let track = req_str(map, "track", "/suite")?;
    if !matches!(track, "A" | "B" | "C" | "D" | "E" | "F") {
        return Err(ConfigError::at("schema", "/suite/track"));
    }
    Ok((id, track.to_owned()))
}

fn parse_questions(
    value: &Value,
    bank_version: &str,
    payload: &mut usize,
) -> Result<Vec<Question>, ConfigError> {
    let items = array(value, "/questions")?;
    if items.is_empty() {
        return Err(ConfigError::at("schema", "/questions"));
    }
    if items.len() > MAX_QUESTIONS {
        return Err(ConfigError::at("budget", "/questions"));
    }
    let mut questions = Vec::with_capacity(items.len());
    let mut ids = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        let pointer = format!("/questions/{index}");
        let question = parse_question(item, &pointer, bank_version, payload)?;
        if !ids.insert(question.id.clone()) {
            return Err(ConfigError::at("schema", format!("{pointer}/id")));
        }
        questions.push(question);
    }
    Ok(questions)
}

fn parse_question(
    value: &Value,
    pointer: &str,
    bank_version: &str,
    payload: &mut usize,
) -> Result<Question, ConfigError> {
    let map = object(value, pointer)?;
    check_keys(map, &QUESTION_KEYS, pointer)?;
    let id = ident_id(req_str(map, "id", pointer)?, &child(pointer, "id"))?;
    let question = bounded_scalars(
        req_str(map, "question", pointer)?,
        1,
        MAX_QUESTION_SCALARS,
        &child(pointer, "question"),
    )?;
    *payload += question.len();
    let (role, claims) = parse_asker(req(map, "asker", pointer)?, pointer, payload)?;
    let category = parse_categories(req(map, "category", pointer)?, pointer)?;
    let difficulty = req_str(map, "difficulty", pointer)?;
    if !matches!(difficulty, "easy" | "medium" | "hard") {
        return Err(ConfigError::at("schema", child(pointer, "difficulty")));
    }
    let required_metrics = parse_metrics(req(map, "required_metrics", pointer)?, pointer)?;
    let as_of = req_str(map, "as_of", pointer)?.to_owned();
    if !canonical_as_of(&as_of) {
        return Err(ConfigError::at("as_of", child(pointer, "as_of")));
    }
    parse_provenance(req(map, "provenance", pointer)?, pointer)?;
    let split = req_str(map, "split", pointer)?;
    if split != "public" {
        return Err(ConfigError::at("schema", child(pointer, "split")));
    }
    let version = req_str(map, "version", pointer)?;
    if version != bank_version {
        return Err(ConfigError::at("version", child(pointer, "version")));
    }
    let role_group = ident_id(
        req_str(map, "role_group", pointer)?,
        &child(pointer, "role_group"),
    )?;
    let (review_status, dual_annotation) = parse_review(req(map, "review", pointer)?, pointer)?;
    let interpretations = parse_interpretations(
        req(map, "accepted_interpretations", pointer)?,
        pointer,
        payload,
    )?;
    let control_totals = parse_totals(req(map, "control_totals", pointer)?, pointer, payload)?;
    let annotations = parse_notes(req(map, "annotations", pointer)?, pointer, payload)?;
    let (expected, typed_gold) = parse_expected(
        req(map, "expected", pointer)?,
        pointer,
        &review_status,
        &dual_annotation,
        &interpretations,
        payload,
    )?;
    Ok(Question {
        id,
        question,
        role,
        claims,
        category,
        difficulty: difficulty.to_owned(),
        expected,
        required_metrics,
        as_of,
        interpretations,
        split: split.to_owned(),
        version: version.to_owned(),
        role_group,
        review_status,
        dual_annotation,
        control_totals,
        annotations,
        typed_gold,
    })
}

type ClaimList = Vec<(String, Vec<String>)>;

fn parse_asker(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<(String, ClaimList), ConfigError> {
    let at = child(pointer, "asker");
    let map = object(value, &at)?;
    check_keys(map, &["claims", "role"], &at)?;
    let role = ident_role(req_str(map, "role", &at)?, &child(&at, "role"))?;
    let claims_at = child(&at, "claims");
    let claims_map = object(req(map, "claims", &at)?, &claims_at)?;
    if claims_map.len() > MAX_CLAIMS {
        return Err(ConfigError::at("budget", claims_at));
    }
    let mut claims = Vec::with_capacity(claims_map.len());
    for (key, raw) in claims_map {
        if !role_spelling(key) {
            return Err(ConfigError::at("schema", child(&claims_at, key)));
        }
        let values = array(raw, &child(&claims_at, key))?;
        if values.is_empty() || values.len() > MAX_CLAIM_VALUES {
            return Err(ConfigError::at("schema", child(&claims_at, key)));
        }
        let mut texts = Vec::with_capacity(values.len());
        for (index, item) in values.iter().enumerate() {
            let text = match item {
                Value::String(text) => text,
                _ => {
                    return Err(ConfigError::at(
                        "schema",
                        format!("{}/{index}", child(&claims_at, key)),
                    ));
                }
            };
            let owned = bounded_bytes(text, 1, MAX_ID_BYTES, &child(&claims_at, key))?;
            if owned.contains('/') {
                return Err(ConfigError::at("schema", child(&claims_at, key)));
            }
            *payload += owned.len();
            texts.push(owned);
        }
        claims.push((key.clone(), texts));
    }
    Ok((role, claims))
}

fn parse_categories(value: &Value, pointer: &str) -> Result<Vec<String>, ConfigError> {
    let at = child(pointer, "category");
    let items = array(value, &at)?;
    if items.is_empty() || items.len() > MAX_CATEGORIES {
        return Err(ConfigError::at("schema", at));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let text = match item {
            Value::String(text) => text.as_str(),
            _ => return Err(ConfigError::at("schema", format!("{at}/{index}"))),
        };
        if !CATEGORIES.contains(&text) || !seen.insert(text.to_owned()) {
            return Err(ConfigError::at("schema", format!("{at}/{index}")));
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

fn parse_metrics(value: &Value, pointer: &str) -> Result<Vec<String>, ConfigError> {
    let at = child(pointer, "required_metrics");
    let items = array(value, &at)?;
    if items.len() > MAX_METRICS {
        return Err(ConfigError::at("budget", at));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let text = match item {
            Value::String(text) => text.as_str(),
            _ => return Err(ConfigError::at("schema", format!("{at}/{index}"))),
        };
        if !role_spelling(text) || text.len() > MAX_ID_BYTES || !seen.insert(text.to_owned()) {
            return Err(ConfigError::at("schema", format!("{at}/{index}")));
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

fn parse_provenance(value: &Value, pointer: &str) -> Result<(), ConfigError> {
    let at = child(pointer, "provenance");
    let map = object(value, &at)?;
    check_keys(map, &["annotator", "origin", "reviewed_by"], &at)?;
    if req_str(map, "origin", &at)? != "public_synthetic_fixture" {
        return Err(ConfigError::at("schema", child(&at, "origin")));
    }
    if req_str(map, "annotator", &at)? != "unassigned" {
        return Err(ConfigError::at("schema", child(&at, "annotator")));
    }
    if req_str(map, "reviewed_by", &at)? != "unassigned" {
        return Err(ConfigError::at("schema", child(&at, "reviewed_by")));
    }
    Ok(())
}

fn parse_review(value: &Value, pointer: &str) -> Result<(String, String), ConfigError> {
    let at = child(pointer, "review");
    let map = object(value, &at)?;
    check_keys(map, &["attestation", "dual_annotation", "status"], &at)?;
    if req_str(map, "attestation", &at)? != "label_only" {
        return Err(ConfigError::at("schema", child(&at, "attestation")));
    }
    let status = req_str(map, "status", &at)?;
    if !matches!(status, "fully_approved" | "needs_reannotation") {
        return Err(ConfigError::at("schema", child(&at, "status")));
    }
    let dual = req_str(map, "dual_annotation", &at)?;
    if !matches!(
        dual,
        "not_declared" | "declared_single" | "declared_dual_agree" | "declared_dual_disagree"
    ) {
        return Err(ConfigError::at("schema", child(&at, "dual_annotation")));
    }
    Ok((status.to_owned(), dual.to_owned()))
}

fn parse_interpretations(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<Vec<Interpretation>, ConfigError> {
    let at = child(pointer, "accepted_interpretations");
    let items = array(value, &at)?;
    if items.len() > MAX_INTERPRETATIONS {
        return Err(ConfigError::at("budget", at));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let row_at = format!("{at}/{index}");
        let map = object(item, &row_at)?;
        let selected = req_bool(map, "selected", &row_at)?;
        let allowed: &[&str] = if selected {
            &["gold_sql", "id", "selected", "statement"]
        } else {
            &["id", "selected", "statement"]
        };
        check_keys(map, allowed, &row_at)?;
        let id = ident_id(req_str(map, "id", &row_at)?, &child(&row_at, "id"))?;
        if !seen.insert(id.clone()) {
            return Err(ConfigError::at("schema", child(&row_at, "id")));
        }
        let statement = bounded_scalars(
            req_str(map, "statement", &row_at)?,
            1,
            MAX_STATEMENT_SCALARS,
            &child(&row_at, "statement"),
        )?;
        *payload += statement.len();
        let gold_sql = if selected {
            let sql = gold_sql_text(
                req_str(map, "gold_sql", &row_at)?,
                &child(&row_at, "gold_sql"),
            )?;
            *payload += sql.len();
            Some(sql)
        } else {
            None
        };
        out.push(Interpretation {
            id,
            statement,
            selected,
            gold_sql,
        });
    }
    Ok(out)
}

fn parse_totals(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<Vec<Total>, ConfigError> {
    let at = child(pointer, "control_totals");
    let items = array(value, &at)?;
    if items.len() > MAX_TOTALS {
        return Err(ConfigError::at("budget", at));
    }
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let row_at = format!("{at}/{index}");
        let map = object(item, &row_at)?;
        check_keys(map, &["name", "value"], &row_at)?;
        let name = req_str(map, "name", &row_at)?;
        if !role_spelling(name) || name.len() > MAX_ID_BYTES {
            return Err(ConfigError::at("schema", child(&row_at, "name")));
        }
        let text = bounded_scalars(
            req_str(map, "value", &row_at)?,
            1,
            MAX_STATEMENT_SCALARS,
            &child(&row_at, "value"),
        )?;
        *payload += text.len();
        out.push(Total {
            name: name.to_owned(),
            value: text,
        });
    }
    Ok(out)
}

fn parse_notes(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<Vec<Note>, ConfigError> {
    let at = child(pointer, "annotations");
    let items = array(value, &at)?;
    if items.len() > MAX_NOTES {
        return Err(ConfigError::at("budget", at));
    }
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let row_at = format!("{at}/{index}");
        let map = object(item, &row_at)?;
        check_keys(map, &["label", "text"], &row_at)?;
        let label = req_str(map, "label", &row_at)?;
        if !label_spelling(label) {
            return Err(ConfigError::at("schema", child(&row_at, "label")));
        }
        let text = bounded_scalars(
            req_str(map, "text", &row_at)?,
            1,
            MAX_STATEMENT_SCALARS,
            &child(&row_at, "text"),
        )?;
        *payload += text.len();
        out.push(Note {
            label: label.to_owned(),
            text,
        });
    }
    Ok(out)
}

fn parse_expected(
    value: &Value,
    pointer: &str,
    review_status: &str,
    dual_annotation: &str,
    interpretations: &[Interpretation],
    payload: &mut usize,
) -> Result<(Expected, Option<Value>), ConfigError> {
    let at = child(pointer, "expected");
    let map = object(value, &at)?;
    check_keys(
        map,
        &[
            "gold_sql",
            "key_columns",
            "kind",
            "ordered",
            "typed_gold",
            "unresolved",
            "value_columns",
        ],
        &at,
    )?;
    let kind = req_str(map, "kind", &at)?.to_owned();
    if !matches!(
        kind.as_str(),
        "table" | "scalar" | "top_k" | "not_found" | "refuse_permission" | "clarify"
    ) {
        return Err(ConfigError::at("schema", child(&at, "kind")));
    }
    if kind == "clarify" {
        if interpretations.is_empty() {
            return Err(ConfigError::at(
                "schema",
                child(pointer, "accepted_interpretations"),
            ));
        }
    } else if !interpretations.is_empty() {
        return Err(ConfigError::at(
            "schema",
            child(pointer, "accepted_interpretations"),
        ));
    }
    reject_forbidden(map, &kind, &at)?;
    if review_status == "fully_approved" && map.contains_key("unresolved") {
        return Err(ConfigError::at("schema", child(&at, "unresolved")));
    }
    let gold_sql = optional_gold_sql(map, &at, payload)?;
    let ordered = optional_bool(map, "ordered", &at)?;
    let key_columns = optional_names(map, "key_columns", &at)?;
    let value_columns = optional_value_columns(map, &at)?;
    let typed_gold = optional_typed_gold(map, &at, payload)?;
    let relative_at = first_relative(value_columns.as_deref(), &at);
    let gold = GoldView {
        kind: &kind,
        gold_sql: gold_sql.as_deref(),
        ordered,
        key_columns: key_columns.as_ref(),
        value_columns: value_columns.as_ref(),
        typed_gold: typed_gold.as_ref(),
        interpretations,
        dual: dual_annotation,
        relative: relative_at.is_some(),
        pointer: &at,
    };
    gold.shape()?;
    let defect = gold.defect();
    match review_status {
        "fully_approved" => {
            if let Some(defect) = defect {
                return Err(match defect {
                    Defect::Tolerance => ConfigError::at(
                        "tolerance",
                        relative_at.unwrap_or_else(|| child(&at, "value_columns")),
                    ),
                    _ => ConfigError::at("review", child(pointer, "review/status")),
                });
            }
        }
        "needs_reannotation" => {
            let declared = req_str(map, "unresolved", &at)?;
            let reason = defect.map(Defect::label).unwrap_or("declared_unresolved");
            if declared != reason {
                return Err(ConfigError::at("review", child(&at, "unresolved")));
            }
        }
        _ => return Err(ConfigError::at("schema", child(pointer, "review/status"))),
    }
    let unresolved = if review_status == "needs_reannotation" {
        Some(req_str(map, "unresolved", &at)?.to_owned())
    } else {
        None
    };
    Ok((
        Expected {
            kind,
            gold_sql,
            ordered,
            key_columns,
            value_columns,
            unresolved,
        },
        typed_gold,
    ))
}

fn reject_forbidden(
    map: &Map<String, Value>,
    kind: &str,
    pointer: &str,
) -> Result<(), ConfigError> {
    let forbidden: &[&str] = match kind {
        "not_found" => &["key_columns", "ordered", "value_columns"],
        "refuse_permission" => &["key_columns", "ordered", "typed_gold", "value_columns"],
        _ => &[],
    };
    for key in forbidden {
        if map.contains_key(*key) {
            return Err(ConfigError::at("schema", child(pointer, key)));
        }
    }
    Ok(())
}

fn optional_gold_sql(
    map: &Map<String, Value>,
    pointer: &str,
    payload: &mut usize,
) -> Result<Option<String>, ConfigError> {
    match map.get("gold_sql") {
        None => Ok(None),
        Some(_) => {
            let sql = gold_sql_text(
                req_str(map, "gold_sql", pointer)?,
                &child(pointer, "gold_sql"),
            )?;
            *payload += sql.len();
            Ok(Some(sql))
        }
    }
}

fn optional_bool(
    map: &Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<Option<bool>, ConfigError> {
    match map.get(key) {
        None => Ok(None),
        Some(_) => Ok(Some(req_bool(map, key, pointer)?)),
    }
}

fn optional_names(
    map: &Map<String, Value>,
    key: &str,
    pointer: &str,
) -> Result<Option<Vec<String>>, ConfigError> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => {
            let at = child(pointer, key);
            let items = array(value, &at)?;
            let mut seen = BTreeSet::new();
            let mut names = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let text = match item {
                    Value::String(text) => text.as_str(),
                    _ => return Err(ConfigError::at("schema", format!("{at}/{index}"))),
                };
                column_name(text, &format!("{at}/{index}"))?;
                if !seen.insert(text.to_owned()) {
                    return Err(ConfigError::at("schema", format!("{at}/{index}")));
                }
                names.push(text.to_owned());
            }
            Ok(Some(names))
        }
    }
}

fn optional_value_columns(
    map: &Map<String, Value>,
    pointer: &str,
) -> Result<Option<Vec<ValueColumn>>, ConfigError> {
    match map.get("value_columns") {
        None => Ok(None),
        Some(value) => {
            let at = child(pointer, "value_columns");
            let items = array(value, &at)?;
            let mut columns = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let row_at = format!("{at}/{index}");
                let row = object(item, &row_at)?;
                check_keys(row, &["name", "tolerance"], &row_at)?;
                let name = column_name(req_str(row, "name", &row_at)?, &child(&row_at, "name"))?;
                let tolerance = req_str(row, "tolerance", &row_at)?;
                if !matches!(tolerance, "exact" | "relative") {
                    return Err(ConfigError::at("schema", child(&row_at, "tolerance")));
                }
                columns.push(ValueColumn {
                    name,
                    tolerance: tolerance.to_owned(),
                });
            }
            Ok(Some(columns))
        }
    }
}

fn optional_typed_gold(
    map: &Map<String, Value>,
    pointer: &str,
    payload: &mut usize,
) -> Result<Option<Value>, ConfigError> {
    match map.get("typed_gold") {
        None => Ok(None),
        Some(value) => Ok(Some(parse_typed_gold(
            value,
            &child(pointer, "typed_gold"),
            payload,
        )?)),
    }
}

fn parse_typed_gold(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<Value, ConfigError> {
    let map = object(value, pointer)?;
    let kind = req_str(map, "kind", pointer)?;
    match kind {
        "not_found" => {
            check_keys(map, &["completion", "kind", "warnings"], pointer)?;
            require_complete(map, pointer)?;
            require_empty_warnings(map, pointer)?;
            Ok(not_found_gold())
        }
        "rows" => {
            check_keys(
                map,
                &["columns", "completion", "kind", "rows", "warnings"],
                pointer,
            )?;
            require_complete(map, pointer)?;
            require_empty_warnings(map, pointer)?;
            let columns = parse_columns(req(map, "columns", pointer)?, pointer, payload)?;
            let rows = parse_rows(req(map, "rows", pointer)?, &columns, pointer, payload)?;
            Ok(rows_gold(&columns, &rows))
        }
        _ => Err(ConfigError::at("schema", child(pointer, "kind"))),
    }
}

fn require_complete(map: &Map<String, Value>, pointer: &str) -> Result<(), ConfigError> {
    if req_str(map, "completion", pointer)? != "complete" {
        return Err(ConfigError::at("schema", child(pointer, "completion")));
    }
    Ok(())
}

fn require_empty_warnings(map: &Map<String, Value>, pointer: &str) -> Result<(), ConfigError> {
    let warnings = array(req(map, "warnings", pointer)?, &child(pointer, "warnings"))?;
    if warnings.is_empty() {
        Ok(())
    } else {
        Err(ConfigError::at("schema", child(pointer, "warnings")))
    }
}

struct Column {
    name: String,
    ty: &'static str,
}

fn parse_columns(
    value: &Value,
    pointer: &str,
    payload: &mut usize,
) -> Result<Vec<Column>, ConfigError> {
    let at = child(pointer, "columns");
    let items = array(value, &at)?;
    if items.is_empty() || items.len() > MAX_COLUMNS {
        return Err(ConfigError::at("budget", at));
    }
    let mut columns: Vec<Column> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let row_at = format!("{at}/{index}");
        let map = object(item, &row_at)?;
        check_keys(map, &["name", "type"], &row_at)?;
        let name = column_name(req_str(map, "name", &row_at)?, &child(&row_at, "name"))?;
        if columns.iter().any(|column| column.name == name) {
            return Err(ConfigError::at("schema", child(&row_at, "name")));
        }
        let ty = match req_str(map, "type", &row_at)? {
            "bool" => "bool",
            "int64" => "int64",
            "decimal" => "decimal",
            "text" => "text",
            _ => return Err(ConfigError::at("schema", child(&row_at, "type"))),
        };
        *payload += name.len();
        columns.push(Column { name, ty });
    }
    Ok(columns)
}

fn parse_rows(
    value: &Value,
    columns: &[Column],
    pointer: &str,
    payload: &mut usize,
) -> Result<Vec<Vec<Value>>, ConfigError> {
    let at = child(pointer, "rows");
    let items = array(value, &at)?;
    if items.len() > MAX_ROWS {
        return Err(ConfigError::at("budget", at));
    }
    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let row_at = format!("{at}/{index}");
        let cells = array(item, &row_at)?;
        if cells.len() != columns.len() {
            return Err(ConfigError::at("schema", row_at));
        }
        let mut row = Vec::with_capacity(cells.len());
        for (column_index, (cell, column)) in cells.iter().zip(columns).enumerate() {
            let cell_at = format!("{row_at}/{column_index}");
            row.push(parse_cell(cell, column.ty, &cell_at, payload)?);
            *payload += 1;
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_cell(
    value: &Value,
    ty: &str,
    pointer: &str,
    payload: &mut usize,
) -> Result<Value, ConfigError> {
    let map = object(value, pointer)?;
    match req_str(map, "presence", pointer)? {
        "null" | "missing" | "unavailable" => {
            check_keys(map, &["presence"], pointer)?;
            let mut out = Map::new();
            out.insert(
                "presence".to_owned(),
                Value::String(req_str(map, "presence", pointer)?.to_owned()),
            );
            Ok(Value::Object(out))
        }
        "value" => parse_value_cell(map, ty, pointer, payload),
        _ => Err(ConfigError::at("schema", child(pointer, "presence"))),
    }
}

fn parse_value_cell(
    map: &Map<String, Value>,
    ty: &str,
    pointer: &str,
    payload: &mut usize,
) -> Result<Value, ConfigError> {
    match ty {
        "bool" => {
            check_keys(map, &["presence", "value"], pointer)?;
            let mut out = Map::new();
            out.insert("presence".to_owned(), Value::String("value".to_owned()));
            out.insert(
                "value".to_owned(),
                Value::Bool(req_bool(map, "value", pointer)?),
            );
            Ok(Value::Object(out))
        }
        "int64" => {
            check_keys(map, &["presence", "value"], pointer)?;
            let text = req_str(map, "value", pointer)?;
            let canonical = canonical_i64(text)
                .map_err(|_| ConfigError::at("int64", child(pointer, "value")))?;
            *payload += canonical.len();
            let mut out = Map::new();
            out.insert("presence".to_owned(), Value::String("value".to_owned()));
            out.insert("value".to_owned(), Value::String(canonical));
            Ok(Value::Object(out))
        }
        "text" => {
            check_keys(map, &["presence", "value"], pointer)?;
            let text = req_str(map, "value", pointer)?;
            if text.chars().count() > QUERY_CELL_MAX_CHARS {
                return Err(ConfigError::at("budget", child(pointer, "value")));
            }
            if bad_control(text) {
                return Err(ConfigError::at("schema", child(pointer, "value")));
            }
            *payload += text.len();
            let mut out = Map::new();
            out.insert("presence".to_owned(), Value::String("value".to_owned()));
            out.insert("value".to_owned(), Value::String(text.to_owned()));
            Ok(Value::Object(out))
        }
        "decimal" => {
            check_keys(
                map,
                &["coefficient", "negative", "presence", "scale"],
                pointer,
            )?;
            let negative = req_bool(map, "negative", pointer)?;
            let coefficient = req_str(map, "coefficient", pointer)?;
            if !valid_coefficient(coefficient) {
                let token = if coefficient.len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
                    "budget"
                } else {
                    "decimal"
                };
                return Err(ConfigError::at(token, child(pointer, "coefficient")));
            }
            if negative && coefficient == "0" {
                return Err(ConfigError::at("decimal", child(pointer, "negative")));
            }
            let scale = req_scale(map, pointer)?;
            let decimal = ExactDecimal::new(negative, coefficient, scale)
                .map_err(|_| ConfigError::at("decimal", pointer))?;
            *payload += decimal.coefficient().len();
            let mut out = Map::new();
            out.insert(
                "coefficient".to_owned(),
                Value::String(decimal.coefficient().to_owned()),
            );
            out.insert("negative".to_owned(), Value::Bool(decimal.is_negative()));
            out.insert("presence".to_owned(), Value::String("value".to_owned()));
            out.insert(
                "scale".to_owned(),
                Value::Number(Number::from(decimal.scale())),
            );
            Ok(Value::Object(out))
        }
        _ => Err(ConfigError::at("schema", pointer)),
    }
}

struct GoldView<'a> {
    kind: &'a str,
    gold_sql: Option<&'a str>,
    ordered: Option<bool>,
    key_columns: Option<&'a Vec<String>>,
    value_columns: Option<&'a Vec<ValueColumn>>,
    typed_gold: Option<&'a Value>,
    interpretations: &'a [Interpretation],
    dual: &'a str,
    relative: bool,
    pointer: &'a str,
}

impl GoldView<'_> {
    fn shape(&self) -> Result<(), ConfigError> {
        self.ordered_shape()?;
        self.column_shape()?;
        if let Some(gold) = self.typed_gold {
            self.bind_typed(gold)?;
        }
        self.selected_sql()
    }

    fn defect(&self) -> Option<Defect> {
        if self.kind == "clarify" && !self.one_selected() {
            return Some(Defect::Ambiguity);
        }
        if !self.complete() {
            return Some(Defect::MissingGold);
        }
        if self.relative {
            return Some(Defect::Tolerance);
        }
        if self.dual == "declared_dual_disagree" {
            return Some(Defect::DualDisagree);
        }
        None
    }

    fn ordered_shape(&self) -> Result<(), ConfigError> {
        if self.ordered == Some(false) && matches!(self.kind, "scalar" | "top_k") {
            return Err(ConfigError::at("schema", child(self.pointer, "ordered")));
        }
        Ok(())
    }

    fn column_shape(&self) -> Result<(), ConfigError> {
        if self.kind == "scalar" {
            if let Some(keys) = self.key_columns
                && !keys.is_empty()
            {
                return Err(ConfigError::at(
                    "schema",
                    child(self.pointer, "key_columns"),
                ));
            }
            if let Some(values) = self.value_columns
                && values.len() != 1
            {
                return Err(ConfigError::at(
                    "schema",
                    child(self.pointer, "value_columns"),
                ));
            }
        }
        if self.kind == "top_k"
            && let Some(keys) = self.key_columns
            && keys.is_empty()
        {
            return Err(ConfigError::at(
                "schema",
                child(self.pointer, "key_columns"),
            ));
        }
        let (Some(keys), Some(values)) = (self.key_columns, self.value_columns) else {
            return Ok(());
        };
        let mut names = BTreeSet::new();
        for name in keys {
            if !names.insert(name.as_str()) {
                return Err(ConfigError::at(
                    "schema",
                    child(self.pointer, "key_columns"),
                ));
            }
        }
        for (index, column) in values.iter().enumerate() {
            if !names.insert(column.name.as_str()) {
                return Err(ConfigError::at(
                    "schema",
                    format!("{}/value_columns/{index}/name", self.pointer),
                ));
            }
        }
        Ok(())
    }

    fn bind_typed(&self, gold: &Value) -> Result<(), ConfigError> {
        let at = child(self.pointer, "typed_gold");
        let map = object(gold, &at)?;
        let gold_kind = req_str(map, "kind", &at)?;
        match self.kind {
            "not_found" => {
                if gold_kind == "not_found" {
                    Ok(())
                } else {
                    Err(ConfigError::at("schema", child(&at, "kind")))
                }
            }
            "clarify" if gold_kind == "not_found" => Ok(()),
            "clarify" | "table" | "scalar" | "top_k" => {
                if gold_kind != "rows" {
                    return Err(ConfigError::at("schema", child(&at, "kind")));
                }
                let strict = self.kind == "scalar" || self.kind == "top_k";
                self.bind_rows(map, &at, strict)
            }
            _ => Err(ConfigError::at("schema", child(&at, "kind"))),
        }
    }

    fn bind_rows(
        &self,
        gold: &Map<String, Value>,
        gold_at: &str,
        strict_counts: bool,
    ) -> Result<(), ConfigError> {
        let Some(keys) = self.key_columns else {
            return Ok(());
        };
        let Some(values) = self.value_columns else {
            return Ok(());
        };
        let columns = array(req(gold, "columns", gold_at)?, &child(gold_at, "columns"))?;
        let rows = array(req(gold, "rows", gold_at)?, &child(gold_at, "rows"))?;
        if strict_counts && rows.is_empty() {
            return Err(ConfigError::at("schema", child(gold_at, "rows")));
        }
        if strict_counts && keys.is_empty() && columns.len() != 1 {
            return Err(ConfigError::at("schema", child(gold_at, "columns")));
        }
        if strict_counts && keys.is_empty() && rows.len() != 1 {
            return Err(ConfigError::at("schema", child(gold_at, "rows")));
        }
        let mut declared = BTreeSet::new();
        for name in keys {
            declared.insert(name.as_str());
        }
        for column in values {
            declared.insert(column.name.as_str());
        }
        if declared.len() != columns.len() {
            return Err(ConfigError::at("schema", child(gold_at, "columns")));
        }
        for column in columns {
            let row = object(column, &child(gold_at, "columns"))?;
            let name = req_str(row, "name", gold_at)?;
            if !declared.contains(name) {
                return Err(ConfigError::at("schema", child(gold_at, "columns")));
            }
        }
        Ok(())
    }

    fn selected_sql(&self) -> Result<(), ConfigError> {
        let mut found = None;
        for (index, item) in self.interpretations.iter().enumerate() {
            if !item.selected {
                continue;
            }
            if found.is_some() {
                return Ok(());
            }
            found = Some(index);
        }
        let Some(index) = found else {
            return Ok(());
        };
        let (Some(sql), Some(interp_sql)) = (
            self.gold_sql,
            self.interpretations[index].gold_sql.as_deref(),
        ) else {
            return Ok(());
        };
        if interp_sql == sql {
            return Ok(());
        }
        let base = self
            .pointer
            .strip_suffix("/expected")
            .unwrap_or(self.pointer);
        Err(ConfigError::at(
            "schema",
            format!("{base}/accepted_interpretations/{index}/gold_sql"),
        ))
    }

    fn complete(&self) -> bool {
        if self.gold_sql.is_none() {
            return false;
        }
        match self.kind {
            "refuse_permission" => self.bare(),
            "not_found" => self.typed_kind() == Some("not_found"),
            "table" | "scalar" | "top_k" => self.row_fields() && self.typed_kind() == Some("rows"),
            "clarify" => self.one_selected() && self.clarify_gold(),
            _ => false,
        }
    }

    fn clarify_gold(&self) -> bool {
        match self.typed_kind() {
            Some("not_found") => self.absent_shape(),
            Some("rows") => self.row_fields(),
            _ => false,
        }
    }

    fn bare(&self) -> bool {
        self.typed_gold.is_none() && self.absent_shape()
    }

    fn absent_shape(&self) -> bool {
        self.ordered.is_none() && self.key_columns.is_none() && self.value_columns.is_none()
    }

    fn row_fields(&self) -> bool {
        self.ordered.is_some() && self.key_columns.is_some() && self.value_columns.is_some()
    }

    fn one_selected(&self) -> bool {
        self.interpretations
            .iter()
            .filter(|item| item.selected)
            .count()
            == 1
    }

    fn typed_kind(&self) -> Option<&str> {
        self.typed_gold
            .and_then(Value::as_object)
            .and_then(|map| map.get("kind"))
            .and_then(Value::as_str)
    }
}

fn first_relative(columns: Option<&[ValueColumn]>, pointer: &str) -> Option<String> {
    let columns = columns?;
    let index = columns
        .iter()
        .position(|column| column.tolerance == "relative")?;
    Some(format!(
        "{}/value_columns/{index}/tolerance",
        pointer.trim_end_matches('/')
    ))
}

fn check_role_groups(questions: &[Question]) -> Result<(), ConfigError> {
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, question) in questions.iter().enumerate() {
        groups
            .entry(question.role_group.as_str())
            .or_default()
            .push(index);
    }
    for indexes in groups.values() {
        let mut roles = BTreeSet::new();
        for index in indexes {
            if !roles.insert(questions[*index].role.as_str()) {
                return Err(ConfigError::at(
                    "schema",
                    format!("/questions/{index}/asker/role"),
                ));
            }
        }
        if indexes.len() < 2 {
            continue;
        }
        let mut seen = BTreeSet::new();
        for index in indexes {
            let canonical = serde_json::to_string(&expected_json(&questions[*index]))
                .map_err(|_| ConfigError::bare("io"))?;
            if !seen.insert(canonical) {
                return Err(ConfigError::at(
                    "role",
                    format!("/questions/{index}/expected"),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn expected_json(question: &Question) -> Value {
    let mut map = Map::new();
    if let Some(sql) = &question.expected.gold_sql {
        map.insert("gold_sql".to_owned(), Value::String(sql.clone()));
    }
    if let Some(keys) = &question.expected.key_columns {
        map.insert("key_columns".to_owned(), string_array(keys));
    }
    map.insert(
        "kind".to_owned(),
        Value::String(question.expected.kind.clone()),
    );
    if let Some(ordered) = question.expected.ordered {
        map.insert("ordered".to_owned(), Value::Bool(ordered));
    }
    if let Some(gold) = &question.typed_gold {
        map.insert("typed_gold".to_owned(), gold.clone());
    }
    if let Some(reason) = &question.expected.unresolved {
        map.insert("unresolved".to_owned(), Value::String(reason.clone()));
    }
    if let Some(columns) = &question.expected.value_columns {
        let mut items = Vec::with_capacity(columns.len());
        for column in columns {
            let mut row = Map::new();
            row.insert("name".to_owned(), Value::String(column.name.clone()));
            row.insert(
                "tolerance".to_owned(),
                Value::String(column.tolerance.clone()),
            );
            items.push(Value::Object(row));
        }
        map.insert("value_columns".to_owned(), Value::Array(items));
    }
    Value::Object(map)
}

fn not_found_gold() -> Value {
    let mut map = Map::new();
    map.insert(
        "completion".to_owned(),
        Value::String("complete".to_owned()),
    );
    map.insert("kind".to_owned(), Value::String("not_found".to_owned()));
    map.insert("warnings".to_owned(), Value::Array(Vec::new()));
    Value::Object(map)
}

fn rows_gold(columns: &[Column], rows: &[Vec<Value>]) -> Value {
    let mut column_items = Vec::with_capacity(columns.len());
    for column in columns {
        let mut row = Map::new();
        row.insert("name".to_owned(), Value::String(column.name.clone()));
        row.insert("type".to_owned(), Value::String(column.ty.to_owned()));
        column_items.push(Value::Object(row));
    }
    let mut map = Map::new();
    map.insert("columns".to_owned(), Value::Array(column_items));
    map.insert(
        "completion".to_owned(),
        Value::String("complete".to_owned()),
    );
    map.insert("kind".to_owned(), Value::String("rows".to_owned()));
    map.insert(
        "rows".to_owned(),
        Value::Array(rows.iter().cloned().map(Value::Array).collect()),
    );
    map.insert("warnings".to_owned(), Value::Array(Vec::new()));
    Value::Object(map)
}

fn string_array(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

fn canonical_as_of(text: &str) -> bool {
    let Ok(parsed) = Rfc3339Timestamp::new(text) else {
        return false;
    };
    let bytes = text.as_bytes();
    parsed.as_str() == text
        && bytes.len() == 25
        && (bytes[19] == b'+' || bytes[19] == b'-')
        && !text.contains('.')
}

fn gold_sql_text(text: &str, pointer: &str) -> Result<String, ConfigError> {
    bounded_bytes(text, 1, MAX_GOLD_SQL_BYTES, pointer)
}

fn column_name(text: &str, pointer: &str) -> Result<String, ConfigError> {
    if text.is_empty() || bad_control(text) || text.contains('/') {
        return Err(ConfigError::at("schema", pointer));
    }
    if text.len() > CANONICAL_MAX_FIELD_NAME_BYTES {
        return Err(ConfigError::at("budget", pointer));
    }
    Ok(text.to_owned())
}

fn bounded_scalars(
    text: &str,
    min: usize,
    max: usize,
    pointer: &str,
) -> Result<String, ConfigError> {
    if bad_control(text) {
        return Err(ConfigError::at("schema", pointer));
    }
    let count = text.chars().count();
    if count < min {
        return Err(ConfigError::at("schema", pointer));
    }
    if count > max {
        return Err(ConfigError::at("budget", pointer));
    }
    Ok(text.to_owned())
}

fn bounded_bytes(text: &str, min: usize, max: usize, pointer: &str) -> Result<String, ConfigError> {
    if bad_control(text) {
        return Err(ConfigError::at("schema", pointer));
    }
    if text.len() < min {
        return Err(ConfigError::at("schema", pointer));
    }
    if text.len() > max {
        return Err(ConfigError::at("budget", pointer));
    }
    Ok(text.to_owned())
}

fn bad_control(text: &str) -> bool {
    text.chars().any(|ch| ch == '\u{7f}' || (ch as u32) < 0x20)
}

fn ident_id(text: &str, pointer: &str) -> Result<String, ConfigError> {
    if !id_spelling(text) || reserved(text) {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(text.to_owned())
}

fn ident_role(text: &str, pointer: &str) -> Result<String, ConfigError> {
    if !role_spelling(text) || text.len() > MAX_ID_BYTES {
        return Err(ConfigError::at("schema", pointer));
    }
    Ok(text.to_owned())
}

fn id_spelling(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_ID_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn role_spelling(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_ID_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn label_spelling(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn bank_version_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_ID_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'.' | b'_' | b':' | b'-')
        })
}

fn reserved(text: &str) -> bool {
    matches!(text, "dev" | "holdout" | "customer" | "private" | "live")
}

fn valid_coefficient(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return false;
    }
    if !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    bytes == b"0" || bytes[0] != b'0'
}

fn canonical_i64(text: &str) -> Result<String, ()> {
    if text == "-0" || text.is_empty() || text.starts_with('+') {
        return Err(());
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return Err(());
    }
    let value: i64 = text.parse().map_err(|_| ())?;
    if value.to_string() != text {
        return Err(());
    }
    Ok(text.to_owned())
}

fn req_scale(map: &Map<String, Value>, pointer: &str) -> Result<i32, ConfigError> {
    let value = req(map, "scale", pointer)?;
    let Value::Number(number) = value else {
        return Err(ConfigError::at("schema", child(pointer, "scale")));
    };
    let text = number.to_string();
    if text.contains(['.', 'e', 'E', '+']) || text == "-0" {
        return Err(ConfigError::at("decimal", child(pointer, "scale")));
    }
    let scale: i32 = text
        .parse()
        .map_err(|_| ConfigError::at("decimal", child(pointer, "scale")))?;
    if scale.unsigned_abs() as usize > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err(ConfigError::at("decimal", child(pointer, "scale")));
    }
    Ok(scale)
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

fn req_bool(map: &Map<String, Value>, key: &str, pointer: &str) -> Result<bool, ConfigError> {
    match req(map, key, pointer)? {
        Value::Bool(value) => Ok(*value),
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
