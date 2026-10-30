//! Business receipt binding checks, NOT issuer authentication or gate admission.
//! The existing test_issuer gate rows cannot authenticate these execution claims.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::canon::{canonical, canonical_sha};
use crate::seal::{hex64, token_ok};

pub const BUSINESS_EVIDENCE_SCHEMA: &str = "oo.release-coordinator/business-evidence/v1";

/// Must come from the stored seal, candidate executor, oracle and issuer policy,
/// not from the receipt being checked. Even valid bindings do not establish trust.
pub struct BusinessEvidenceContext<'a> {
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub candidate_digest: &'a str,
    pub base_candidate_digest: Option<&'a str>,
    pub effective: &'a Value,
    pub visibility: &'a Value,
    pub data_vector_digest: &'a str,
    pub source_coverage: &'a Value,
    pub execution_capability_version: &'a str,
    pub eval_plans: &'a BTreeMap<String, Value>,
    /// Keys are "G5b/metric:name", etc.; values are independent oracle digests.
    pub expected_results: &'a BTreeMap<String, String>,
    pub issuer_policy: &'a Value,
    pub now_unix_seconds: u64,
}

const FIELDS: &[&str] = &[
    "evidence_id",
    "evidence_kind",
    "evidence_schema",
    "issuer",
    "issuer_scope",
    "issuer_policy_digest",
    "authenticated_receipt_reference",
    "candidate_id",
    "candidate_digest",
    "semantic_definition_digest",
    "dependency_digest",
    "data_vector_digest",
    "source_coverage",
    "role",
    "policy_digest",
    "sql_or_plan_digest",
    "execution_capability_version",
    "expected_result_digest",
    "observed_result_digest",
    "started_at",
    "completed_at",
    "expires_at",
    "outcome",
    "unresolved_count",
    "gate_id",
    "asset_id",
];

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("business_field")
}

fn keys(value: &Value, expected: &[&str]) -> Result<(), &'static str> {
    let map = value.as_object().ok_or("business_shape")?;
    if map.len() != expected.len() || !expected.iter().all(|key| map.contains_key(*key)) {
        return Err("business_fields");
    }
    Ok(())
}

fn kind(gate: &str) -> Result<&'static str, &'static str> {
    match gate {
        "G5a" => Ok("metric_definition"),
        "G5b" => Ok("metric_control_total"),
        "G6" => Ok("example_execution"),
        "G10" => Ok("business_eval"),
        "G11" => Ok("semantic_review"),
        _ => Err("business_gate"),
    }
}

fn collection<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], &'static str> {
    match value.get(key) {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        _ => Err("business_collection"),
    }
}

fn required(ctx: &BusinessEvidenceContext<'_>) -> Result<BTreeMap<String, Value>, &'static str> {
    let mut out = BTreeMap::new();
    for metric in collection(ctx.effective, "metrics")? {
        if metric["certified"] == false {
            continue;
        }
        if metric["certified"] != true || !metric.is_object() {
            return Err("business_metric");
        }
        let id = text(metric, "metric")?;
        text(metric, "sql")?;
        text(&metric["control_total"], "sql")?;
        text(metric, "unit")?;
        if metric["grain"].as_array().is_none_or(Vec::is_empty)
            || metric["tests"].as_array().is_none_or(Vec::is_empty)
        {
            return Err("business_metric");
        }
        for gate in ["G5a", "G5b", "G11"] {
            if out
                .insert(format!("{gate}/metric:{id}"), metric.clone())
                .is_some()
            {
                return Err("business_duplicate_asset");
            }
        }
    }
    for example in collection(ctx.effective, "examples")? {
        let id = text(example, "id")?;
        text(example, "sql")?;
        text(example, "verified_by")?;
        if example["last_checked"]["status"] != "ok"
            || example["uses"].as_array().is_none_or(Vec::is_empty)
        {
            return Err("business_example");
        }
        if out
            .insert(format!("G6/example:{id}"), example.clone())
            .is_some()
        {
            return Err("business_duplicate_asset");
        }
    }
    for suite in collection(&ctx.effective["ontology"]["publish"], "require_eval")? {
        let id = suite
            .as_str()
            .filter(|s| token_ok(s))
            .ok_or("business_eval")?;
        let plan = ctx
            .eval_plans
            .get(id)
            .filter(|p| p.is_object() && p["suite"] == id)
            .ok_or("business_eval_plan")?;
        if out.insert(format!("G10/eval:{id}"), plan.clone()).is_some() {
            return Err("business_duplicate_asset");
        }
    }
    if out.is_empty() || out.len() > 4096 {
        return Err("business_required_assets");
    }
    Ok(out)
}

fn plan_digest(ctx: &BusinessEvidenceContext<'_>, gate: &str, definition: &Value) -> String {
    canonical_sha(&match gate {
        "G5a" => {
            json!({"kind":"metric_definition","sql":definition["sql"],"tests":definition["tests"]})
        }
        "G5b" => json!({"kind":"metric_control_total","sql":definition["sql"],
                       "control_total":definition["control_total"]}),
        "G6" => {
            json!({"kind":"example_execution","sql":definition["sql"],"uses":definition["uses"]})
        }
        "G10" => definition.clone(),
        _ => {
            json!({"kind":"semantic_diff_review","base_candidate_digest":ctx.base_candidate_digest,
                    "candidate_digest":ctx.candidate_digest,"definition":definition,
                    "dependency_digest":canonical_sha(ctx.effective)})
        }
    })
}

fn validate_context(ctx: &BusinessEvidenceContext<'_>) -> Result<(), &'static str> {
    if !token_ok(ctx.project_id)
        || !token_ok(ctx.candidate_id)
        || !hex64(ctx.candidate_digest)
        || !hex64(ctx.data_vector_digest)
        || ctx.base_candidate_digest.is_some_and(|hash| !hex64(hash))
        || ctx.effective["project"] != ctx.project_id
        || ctx.visibility["project"] != ctx.project_id
        || !text(ctx.visibility, "role")?.starts_with("oo_r_")
        || ctx
            .source_coverage
            .as_object()
            .is_none_or(serde_json::Map::is_empty)
        || !token_ok(ctx.execution_capability_version)
        || canonical(ctx.effective).len() > 64 * 1024 * 1024
        || canonical(ctx.visibility).len() > 64 * 1024 * 1024
    {
        return Err("business_context");
    }
    let policy = ctx.issuer_policy;
    keys(
        policy,
        &[
            "issuer",
            "issuer_class",
            "scope",
            "policy_version",
            "not_before",
            "expires_at",
            "revoked",
        ],
    )?;
    if !token_ok(text(policy, "issuer")?)
        || !token_ok(text(policy, "policy_version")?)
        || policy["issuer_class"] != "test_issuer"
        || policy["revoked"] != false
    {
        return Err("business_issuer_policy");
    }
    keys(
        &policy["scope"],
        &[
            "scope",
            "project_id",
            "candidate_id",
            "role",
            "gate_ids",
            "source_coverage_digest",
        ],
    )?;
    let scope = &policy["scope"];
    if scope["scope"] != "test_scope"
        || scope["project_id"] != ctx.project_id
        || scope["candidate_id"] != ctx.candidate_id
        || scope["role"] != ctx.visibility["role"]
        || scope["source_coverage_digest"] != canonical_sha(ctx.source_coverage)
    {
        return Err("business_issuer_scope");
    }
    let gates = scope["gate_ids"]
        .as_array()
        .ok_or("business_issuer_scope")?;
    let mut seen = BTreeSet::new();
    for gate in gates {
        let gate = gate.as_str().ok_or("business_issuer_scope")?;
        kind(gate)?;
        if !seen.insert(gate) {
            return Err("business_issuer_scope");
        }
    }
    let start = policy["not_before"].as_u64().ok_or("business_time")?;
    let end = policy["expires_at"].as_u64().ok_or("business_time")?;
    if start > ctx.now_unix_seconds || end <= ctx.now_unix_seconds {
        return Err("business_issuer_expired");
    }
    Ok(())
}

/// Returns a validation-only refusal. No available adapter authenticates the
/// referenced receipt or issuer, so even a perfectly bound set cannot pass a gate.
pub fn check_business_evidence(
    ctx: &BusinessEvidenceContext<'_>,
    receipts: &[Value],
) -> Result<Value, &'static str> {
    validate_context(ctx)?;
    let required = required(ctx)?;
    if receipts.len() != required.len()
        || required.keys().collect::<BTreeSet<_>>() != ctx.expected_results.keys().collect()
    {
        return Err("business_evidence_coverage");
    }
    let dependency = canonical_sha(ctx.effective);
    let policy_digest = canonical_sha(ctx.visibility);
    let mut seen = BTreeSet::new();
    let mut references = BTreeSet::new();
    for receipt in receipts {
        if canonical(receipt).len() > 1_048_576 {
            return Err("business_evidence_budget");
        }
        keys(receipt, FIELDS)?;
        let gate = text(receipt, "gate_id")?;
        let asset = text(receipt, "asset_id")?;
        let key = format!("{gate}/{asset}");
        let definition = required.get(&key).ok_or("business_asset_binding")?;
        if !seen.insert(key.clone()) {
            return Err("business_duplicate_evidence");
        }
        let expected = &ctx.expected_results[&key];
        if !hex64(expected)
            || receipt["expected_result_digest"] != *expected
            || receipt["observed_result_digest"] != *expected
            || receipt["outcome"] != "pass"
            || receipt["unresolved_count"].as_u64() != Some(0)
        {
            return Err("business_result_binding");
        }
        if gate == "G6" && definition["last_checked"]["result_hash"] != *expected {
            return Err("business_example_result_binding");
        }
        if receipt["evidence_schema"] != BUSINESS_EVIDENCE_SCHEMA
            || receipt["evidence_kind"] != kind(gate)?
            || receipt["candidate_id"] != ctx.candidate_id
            || receipt["candidate_digest"] != ctx.candidate_digest
            || receipt["semantic_definition_digest"] != canonical_sha(definition)
            || receipt["dependency_digest"] != dependency
            || receipt["data_vector_digest"] != ctx.data_vector_digest
            || receipt["source_coverage"] != *ctx.source_coverage
            || receipt["role"] != ctx.visibility["role"]
            || receipt["policy_digest"] != policy_digest
            || receipt["sql_or_plan_digest"] != plan_digest(ctx, gate, definition)
            || receipt["execution_capability_version"] != ctx.execution_capability_version
        {
            return Err("business_context_binding");
        }
        if receipt["issuer"] != ctx.issuer_policy["issuer"]
            || receipt["issuer_scope"] != ctx.issuer_policy["scope"]
            || receipt["issuer_policy_digest"] != canonical_sha(ctx.issuer_policy)
            || !ctx.issuer_policy["scope"]["gate_ids"]
                .as_array()
                .is_some_and(|gates| gates.contains(&json!(gate)))
        {
            return Err("business_issuer_binding");
        }
        let start = receipt["started_at"].as_u64().ok_or("business_time")?;
        let end = receipt["completed_at"].as_u64().ok_or("business_time")?;
        let expires = receipt["expires_at"].as_u64().ok_or("business_time")?;
        if start
            < ctx.issuer_policy["not_before"]
                .as_u64()
                .ok_or("business_time")?
            || start > end
            || end > ctx.now_unix_seconds
            || expires <= ctx.now_unix_seconds
            || expires
                > ctx.issuer_policy["expires_at"]
                    .as_u64()
                    .ok_or("business_time")?
            || expires.saturating_sub(start) > 3600
        {
            return Err("business_evidence_expired");
        }
        let reference = &receipt["authenticated_receipt_reference"];
        keys(reference, &["issuer", "receipt_id", "body_sha256"])?;
        let mut body = receipt.clone();
        body.as_object_mut()
            .ok_or("business_shape")?
            .remove("authenticated_receipt_reference");
        if !token_ok(text(receipt, "evidence_id")?)
            || reference["issuer"] != receipt["issuer"]
            || reference["receipt_id"] != receipt["evidence_id"]
            || reference["body_sha256"] != canonical_sha(&body)
        {
            return Err("business_reference_binding");
        }
        if !references.insert((text(receipt, "issuer")?, text(receipt, "evidence_id")?)) {
            return Err("business_duplicate_reference");
        }
    }
    Ok(json!({
        "protocol":"oo.release-coordinator/business-evidence-check/v1",
        "outcome":"refused", "reason":"business_issuer_unavailable",
        "bindings_validated":true, "receipt_count":receipts.len(),
        "candidate_digest":ctx.candidate_digest, "data_vector_digest":ctx.data_vector_digest,
        "role":ctx.visibility["role"], "policy_digest":policy_digest,
        "issuer_policy_digest":canonical_sha(ctx.issuer_policy),
        "authenticated_receipt_reference_resolved":false,
        "test_admissible":false, "production_admissible":false,
        "business_owner_signed":false, "certificate_class":"not_issued",
        "database_used":false, "published":false
    }))
}

#[cfg(test)]
#[path = "../tests/observations/business_evidence.rs"]
mod tests;
