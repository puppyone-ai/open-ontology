//! Calls the existing description compilers. This module does not reimplement S9.

use serde_json::Value;

use description_pack::apply_description_pack;
use description_review::{REVIEW_COMPILER, REVIEW_PROTOCOL, REVIEW_PROTOCOL_V2, apply_description_review};

use crate::canonical::sha256_hex;
use crate::error::{err, LedgerError};

pub struct CompilerOk {
    pub protocol: String,
    pub outcome_sha256: String,
}

const STALE: &[&str] = &[
    "schema",
    "evidence",
    "request",
    "card",
    "candidate",
    "curated",
    "cas",
];

pub fn run_review(
    command: &str,
    object_id: &str,
    candidate: &str,
    input: &str,
    schema: &str,
    evidence: &str,
    choice: &str,
) -> Result<CompilerOk, LedgerError> {
    let text = apply_description_review(command).map_err(map_review)?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| err("compiler.refused", "the review compiler document is not JSON"))?;
    let protocol = text_at(&value, "protocol");
    if protocol != REVIEW_PROTOCOL && protocol != REVIEW_PROTOCOL_V2 {
        return Err(err("compiler.refused", "the review compiler protocol was not accepted"));
    }
    if text_at(&value, "compiler") != REVIEW_COMPILER || text_at(&value, "outcome") != "reviewed" {
        return Err(err("compiler.refused", "the review compiler did not return a reviewed document"));
    }
    require_statements(
        &value,
        &[
            ("authentication", "not_claimed"),
            ("publication", "unavailable"),
            ("grants", "not_authorized"),
            ("files", "not_written"),
        ],
    )?;
    if text_at(&value, "request_sha256") != input {
        return Err(err("stale", "the compiler request hash does not match the decision"));
    }
    if stale_queue(&value, object_id) {
        return Err(err("stale", "the compiler left the dataset unchanged"));
    }
    if !review_bound(&value, object_id, candidate, schema, evidence, choice) {
        return Err(err(
            "receipt.unbound",
            "the compiler document is not bound to the approved candidate",
        ));
    }
    Ok(CompilerOk {
        protocol: protocol.to_owned(),
        outcome_sha256: sha256_hex(text.as_bytes())?,
    })
}

pub fn run_pack(command: &str) -> Result<CompilerOk, LedgerError> {
    let text = apply_description_pack(command).map_err(map_pack)?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| err("compiler.refused", "the pack compiler document is not JSON"))?;
    if text_at(&value, "protocol") != description_pack::PACK_PROTOCOL
        || text_at(&value, "compiler") != description_pack::PACK_COMPILER
    {
        return Err(err("compiler.refused", "the pack compiler protocol was not accepted"));
    }
    let outcome = text_at(&value, "outcome");
    if outcome != "packed" && outcome != "reviewed_unchanged" {
        return Err(err("compiler.refused", "the pack compiler did not return an artifact"));
    }
    require_statements(
        &value,
        &[
            ("authentication", "not_claimed"),
            ("publication", "unavailable"),
            ("grants", "not_authorized"),
        ],
    )?;
    Ok(CompilerOk {
        protocol: description_pack::PACK_PROTOCOL.to_owned(),
        outcome_sha256: sha256_hex(text.as_bytes())?,
    })
}

fn review_bound(
    value: &Value,
    object_id: &str,
    candidate: &str,
    schema: &str,
    evidence: &str,
    choice: &str,
) -> bool {
    let dots = object_id.matches('.').count();
    if dots == 1 {
        return rows(value, "decisions").any(|row| {
            text_at(row, "dataset") == object_id
                && text_at(row, "candidate_sha256") == candidate
                && text_at(row, "schema_hash") == schema
                && text_at(row, "evidence_hash") == evidence
                && text_at(row, "decision") == choice
                && row.get("authenticated").and_then(Value::as_bool) == Some(false)
        });
    }
    let Some((dataset, column)) = object_id.rsplit_once('.') else {
        return false;
    };
    rows(value, "column_decisions").any(|row| {
        text_at(row, "dataset") == dataset
            && text_at(row, "column") == column
            && text_at(row, "candidate_sha256") == candidate
            && text_at(row, "schema_hash") == schema
            && text_at(row, "evidence_hash") == evidence
            && text_at(row, "decision") == choice
            && row.get("authenticated").and_then(Value::as_bool) == Some(false)
    })
}

fn stale_queue(value: &Value, object_id: &str) -> bool {
    let dataset = object_id.rsplit_once('.').map(|(head, _)| head).unwrap_or(object_id);
    let dataset = if object_id.matches('.').count() == 1 {
        object_id
    } else {
        dataset
    };
    rows(value, "queue").any(|item| {
        text_at(item, "dataset") == dataset && STALE.contains(&text_at(item, "reason"))
    })
}

fn rows<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn require_statements(value: &Value, expected: &[(&str, &str)]) -> Result<(), LedgerError> {
    let Some(statements) = value.get("statements").and_then(Value::as_object) else {
        return Err(err("compiler.refused", "the compiler document has no statements"));
    };
    for (key, want) in expected {
        if statements.get(*key).and_then(Value::as_str) != Some(*want) {
            return Err(err(
                "compiler.claim",
                "the compiler claimed authentication, publication, or a grant",
            ));
        }
    }
    Ok(())
}

fn text_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn map_review(error: description_review::DescriptionReviewError) -> LedgerError {
    LedgerError {
        code: "compiler.refused".to_owned(),
        message: error.message,
    }
}

fn map_pack(error: description_pack::DescriptionPackError) -> LedgerError {
    LedgerError {
        code: "compiler.refused".to_owned(),
        message: error.message,
    }
}
