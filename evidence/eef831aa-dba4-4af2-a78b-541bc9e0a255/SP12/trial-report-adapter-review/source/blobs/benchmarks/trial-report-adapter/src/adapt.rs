//! Render one `oo.benchmark.trial-statistics/v1` document. Counts are not computed here.

use serde_json::{Map, Number, Value};

use crate::parse::{Parsed, Question, Score, Variant};

const STATS_VERSION: &str = "oo.benchmark.trial-statistics/v1";
const ALGORITHM: &str = "oo.benchmark.paired-question-bootstrap/v1";

const TUPLE_KEYS: [&str; 10] = [
    "as_of",
    "budget",
    "dataset",
    "harness",
    "model",
    "project",
    "role",
    "scorer_version",
    "snapshot",
    "suite",
];

pub(crate) fn render(parsed: &Parsed) -> String {
    let mut root = Map::new();
    root.insert("algorithm".to_owned(), Value::String(ALGORITHM.to_owned()));
    root.insert(
        "resamples".to_owned(),
        Value::Number(Number::from(parsed.resamples)),
    );
    root.insert("seed".to_owned(), Value::String(parsed.seed_text.clone()));
    root.insert(
        "variants".to_owned(),
        Value::Array(parsed.variants.iter().map(variant_value).collect()),
    );
    root.insert(
        "version".to_owned(),
        Value::String(STATS_VERSION.to_owned()),
    );
    let bytes = serde_json::to_vec(&Value::Object(root)).expect("statistics input");
    String::from_utf8(bytes).expect("utf-8")
}

fn variant_value(variant: &Variant) -> Value {
    let mut map = Map::new();
    map.insert("id".to_owned(), Value::String(variant.id.clone()));
    map.insert(
        "questions".to_owned(),
        Value::Array(variant.questions.iter().map(question_value).collect()),
    );
    map.insert("tuple".to_owned(), tuple_value(&variant.tuple));
    Value::Object(map)
}

fn question_value(question: &Question) -> Value {
    let mut map = Map::new();
    map.insert("id".to_owned(), Value::String(question.id.clone()));
    map.insert(
        "trials".to_owned(),
        Value::Array(
            question
                .trials
                .iter()
                .enumerate()
                .map(trial_value)
                .collect(),
        ),
    );
    Value::Object(map)
}

fn trial_value((index, score): (usize, &Score)) -> Value {
    let mut map = Map::new();
    let index = u64::try_from(index).expect("trial index");
    map.insert("index".to_owned(), Value::Number(Number::from(index)));
    map.insert(
        "status".to_owned(),
        Value::String(score_name(*score).to_owned()),
    );
    Value::Object(map)
}

fn tuple_value(tuple: &[String; 10]) -> Value {
    // Stored order is project, snapshot, as_of, dataset, suite, model, harness, role, budget, scorer.
    // The statistics object emits those fields in alphabetical key order.
    let alpha = [
        tuple[2].clone(),
        tuple[8].clone(),
        tuple[3].clone(),
        tuple[6].clone(),
        tuple[5].clone(),
        tuple[0].clone(),
        tuple[7].clone(),
        tuple[9].clone(),
        tuple[1].clone(),
        tuple[4].clone(),
    ];
    let mut map = Map::new();
    for (key, value) in TUPLE_KEYS.iter().zip(alpha) {
        map.insert((*key).to_owned(), Value::String(value));
    }
    Value::Object(map)
}

fn score_name(score: Score) -> &'static str {
    match score {
        Score::Pass => "pass",
        Score::Fail => "fail",
        Score::NeedsReannotation => "needs_reannotation",
    }
}
