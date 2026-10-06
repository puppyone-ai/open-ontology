//! Manifest, agent view, and scorer view.

use serde_json::{Map, Value};

use crate::parse::{Bank, Interpretation, Note, Question, Total, expected_json};
use crate::sha256_hex;

const REPORT: &str = "oo.benchmark.question-bank-report/v1";
const MANIFEST: &str = "oo.benchmark.question-bank-manifest/v1";
const AGENT: &str = "oo.benchmark.question-bank-agent-view/v1";
const SCORER: &str = "oo.benchmark.question-bank-scorer-view/v1";

pub(crate) struct Rendered {
    pub gate_passed: bool,
    pub report: String,
}

pub(crate) fn render(bank: &Bank) -> Rendered {
    let agent = agent_view(bank);
    let scorer = scorer_view(bank);
    let agent_hash = sha256_hex(canonical(&agent).as_bytes());
    let scorer_hash = sha256_hex(canonical(&scorer).as_bytes());
    let manifest = manifest_view(bank, &agent_hash, &scorer_hash);
    let manifest_hash = sha256_hex(canonical(&manifest).as_bytes());
    let mut hashes = Map::new();
    hashes.insert("agent_view".to_owned(), Value::String(agent_hash));
    hashes.insert("manifest".to_owned(), Value::String(manifest_hash));
    hashes.insert("scorer_view".to_owned(), Value::String(scorer_hash));
    let mut report = Map::new();
    report.insert("agent_view".to_owned(), agent);
    report.insert("content_hashes".to_owned(), Value::Object(hashes));
    report.insert("manifest".to_owned(), manifest);
    report.insert("scorer_view".to_owned(), scorer);
    report.insert("version".to_owned(), Value::String(REPORT.to_owned()));
    let needs = bank
        .questions
        .iter()
        .filter(|question| question.review_status == "needs_reannotation")
        .count();
    Rendered {
        gate_passed: needs == 0,
        report: canonical(&Value::Object(report)),
    }
}

fn manifest_view(bank: &Bank, agent_hash: &str, scorer_hash: &str) -> Value {
    let needs = bank
        .questions
        .iter()
        .filter(|question| question.review_status == "needs_reannotation")
        .count();
    let total = bank.questions.len();
    let mut questions = Vec::with_capacity(total);
    for question in &bank.questions {
        let mut row = Map::new();
        row.insert(
            "dual_annotation".to_owned(),
            Value::String(question.dual_annotation.clone()),
        );
        row.insert("id".to_owned(), Value::String(question.id.clone()));
        row.insert(
            "review_status".to_owned(),
            Value::String(question.review_status.clone()),
        );
        row.insert(
            "role_group".to_owned(),
            Value::String(question.role_group.clone()),
        );
        questions.push(Value::Object(row));
    }
    let mut map = Map::new();
    map.insert(
        "agent_view_sha256".to_owned(),
        Value::String(agent_hash.to_owned()),
    );
    map.insert(
        "bank_version".to_owned(),
        Value::String(bank.bank_version.clone()),
    );
    map.insert(
        "dataset_id".to_owned(),
        Value::String(bank.dataset_id.clone()),
    );
    map.insert(
        "distribution".to_owned(),
        Value::String("public_synthetic".to_owned()),
    );
    map.insert(
        "fully_approved_count".to_owned(),
        Value::from(total - needs),
    );
    map.insert("needs_reannotation_count".to_owned(), Value::from(needs));
    map.insert("question_count".to_owned(), Value::from(total));
    map.insert("questions".to_owned(), Value::Array(questions));
    map.insert("scale".to_owned(), Value::String(bank.scale.clone()));
    map.insert(
        "scorer_view_sha256".to_owned(),
        Value::String(scorer_hash.to_owned()),
    );
    map.insert("suite_id".to_owned(), Value::String(bank.suite_id.clone()));
    map.insert("track".to_owned(), Value::String(bank.track.clone()));
    map.insert("version".to_owned(), Value::String(MANIFEST.to_owned()));
    Value::Object(map)
}

fn agent_view(bank: &Bank) -> Value {
    let mut questions = Vec::with_capacity(bank.questions.len());
    for question in &bank.questions {
        let mut row = Map::new();
        row.insert("as_of".to_owned(), Value::String(question.as_of.clone()));
        row.insert("asker".to_owned(), asker_json(question));
        row.insert("category".to_owned(), strings(&question.category));
        row.insert(
            "difficulty".to_owned(),
            Value::String(question.difficulty.clone()),
        );
        row.insert("id".to_owned(), Value::String(question.id.clone()));
        row.insert(
            "question".to_owned(),
            Value::String(question.question.clone()),
        );
        row.insert(
            "required_metrics".to_owned(),
            strings(&question.required_metrics),
        );
        row.insert("split".to_owned(), Value::String(question.split.clone()));
        row.insert(
            "version".to_owned(),
            Value::String(question.version.clone()),
        );
        questions.push(Value::Object(row));
    }
    let mut map = Map::new();
    map.insert(
        "bank_version".to_owned(),
        Value::String(bank.bank_version.clone()),
    );
    map.insert(
        "dataset_id".to_owned(),
        Value::String(bank.dataset_id.clone()),
    );
    map.insert("questions".to_owned(), Value::Array(questions));
    map.insert("suite_id".to_owned(), Value::String(bank.suite_id.clone()));
    map.insert("version".to_owned(), Value::String(AGENT.to_owned()));
    Value::Object(map)
}

fn scorer_view(bank: &Bank) -> Value {
    let mut questions = Vec::with_capacity(bank.questions.len());
    for question in &bank.questions {
        questions.push(scorer_question(question));
    }
    let mut map = Map::new();
    map.insert(
        "bank_version".to_owned(),
        Value::String(bank.bank_version.clone()),
    );
    map.insert(
        "dataset_id".to_owned(),
        Value::String(bank.dataset_id.clone()),
    );
    map.insert(
        "question_count".to_owned(),
        Value::from(bank.questions.len()),
    );
    map.insert("questions".to_owned(), Value::Array(questions));
    map.insert("suite_id".to_owned(), Value::String(bank.suite_id.clone()));
    map.insert("version".to_owned(), Value::String(SCORER.to_owned()));
    Value::Object(map)
}

fn scorer_question(question: &Question) -> Value {
    let mut row = Map::new();
    row.insert(
        "accepted_interpretations".to_owned(),
        interpretations_json(&question.interpretations),
    );
    row.insert("annotations".to_owned(), notes_json(&question.annotations));
    row.insert("as_of".to_owned(), Value::String(question.as_of.clone()));
    row.insert("asker".to_owned(), asker_json(question));
    row.insert("category".to_owned(), strings(&question.category));
    row.insert(
        "control_totals".to_owned(),
        totals_json(&question.control_totals),
    );
    row.insert(
        "difficulty".to_owned(),
        Value::String(question.difficulty.clone()),
    );
    row.insert("expected".to_owned(), expected_json(question));
    row.insert("id".to_owned(), Value::String(question.id.clone()));
    row.insert("provenance".to_owned(), provenance_json());
    row.insert(
        "question".to_owned(),
        Value::String(question.question.clone()),
    );
    row.insert(
        "required_metrics".to_owned(),
        strings(&question.required_metrics),
    );
    row.insert("review".to_owned(), review_json(question));
    row.insert(
        "role_group".to_owned(),
        Value::String(question.role_group.clone()),
    );
    row.insert("split".to_owned(), Value::String(question.split.clone()));
    row.insert(
        "version".to_owned(),
        Value::String(question.version.clone()),
    );
    Value::Object(row)
}

fn asker_json(question: &Question) -> Value {
    let mut claims = Map::new();
    for (key, values) in &question.claims {
        claims.insert(key.clone(), strings(values));
    }
    let mut map = Map::new();
    map.insert("claims".to_owned(), Value::Object(claims));
    map.insert("role".to_owned(), Value::String(question.role.clone()));
    Value::Object(map)
}

fn interpretations_json(items: &[Interpretation]) -> Value {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut row = Map::new();
        if let Some(sql) = &item.gold_sql {
            row.insert("gold_sql".to_owned(), Value::String(sql.clone()));
        }
        row.insert("id".to_owned(), Value::String(item.id.clone()));
        row.insert("selected".to_owned(), Value::Bool(item.selected));
        row.insert(
            "statement".to_owned(),
            Value::String(item.statement.clone()),
        );
        out.push(Value::Object(row));
    }
    Value::Array(out)
}

fn totals_json(items: &[Total]) -> Value {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut row = Map::new();
        row.insert("name".to_owned(), Value::String(item.name.clone()));
        row.insert("value".to_owned(), Value::String(item.value.clone()));
        out.push(Value::Object(row));
    }
    Value::Array(out)
}

fn notes_json(items: &[Note]) -> Value {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut row = Map::new();
        row.insert("label".to_owned(), Value::String(item.label.clone()));
        row.insert("text".to_owned(), Value::String(item.text.clone()));
        out.push(Value::Object(row));
    }
    Value::Array(out)
}

fn provenance_json() -> Value {
    let mut map = Map::new();
    map.insert(
        "annotator".to_owned(),
        Value::String("unassigned".to_owned()),
    );
    map.insert(
        "origin".to_owned(),
        Value::String("public_synthetic_fixture".to_owned()),
    );
    map.insert(
        "reviewed_by".to_owned(),
        Value::String("unassigned".to_owned()),
    );
    Value::Object(map)
}

fn review_json(question: &Question) -> Value {
    let mut map = Map::new();
    map.insert(
        "attestation".to_owned(),
        Value::String("label_only".to_owned()),
    );
    map.insert(
        "dual_annotation".to_owned(),
        Value::String(question.dual_annotation.clone()),
    );
    map.insert(
        "status".to_owned(),
        Value::String(question.review_status.clone()),
    );
    Value::Object(map)
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

fn canonical(value: &Value) -> String {
    serde_json::to_string(value).expect("json")
}
