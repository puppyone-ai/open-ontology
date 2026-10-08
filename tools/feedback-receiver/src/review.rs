//! Bounded feedback becomes S9 candidates only through `selfbuild_suggestions::produce`.

use serde_json::{Map, Value, json};
use selfbuild_suggestions::{canonical_sha256, parse_document, produce};
use sha2::{Digest, Sha256};

use crate::error::RecvError;

pub const PROTOCOL: &str = "oo.feedback.receiver/v1";
pub const MAX_BODY: usize = 32_768;
const PRODUCER: &str = "oo.selfbuild-suggestions/v1";
const OUTCOME: &str = "oo.selfbuild-suggestions.outcome/v1";

#[derive(Clone, Debug)]
pub struct Operator {
    pub role: String,
    pub source_id: String,
    pub catalog: Value,
    pub relationships: Value,
    pub profile: Value,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub project: String,
    pub kind: String,
    pub text: String,
    pub target: String,
}

#[derive(Clone, Debug)]
pub struct Decoded {
    pub receipt_id: String,
    pub project: String,
    pub payload_hash: [u8; 32],
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Reviewed {
    pub catalog_sha256: String,
    pub relationships_sha256: String,
    pub profile_sha256: String,
    pub feedback_sha256: String,
    pub evidence_sha256: String,
    pub schema_hash: String,
    pub effects: Value,
    pub dedup: Value,
    pub scope_drops: Value,
    pub outcome: Value,
    pub feedback: Vec<StoredFeedback>,
    pub candidates: Vec<StoredCandidate>,
}

#[derive(Clone, Debug)]
pub struct StoredFeedback {
    pub candidate_id: String,
    pub kind: String,
    pub target: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct StoredCandidate {
    pub candidate_id: String,
    pub channel: String,
    pub key: String,
    pub score: i32,
    pub impact: i32,
    pub uncertainty_millis: i32,
    pub prior_overlap: bool,
    pub source: Value,
}

pub fn load_operator_value(bytes: &[u8]) -> Result<Value, RecvError> {
    parse_document(bytes).map_err(|_| RecvError::Bound)
}

pub fn probe(operator: &Operator, project: &str) -> Result<(), RecvError> {
    review(operator, project, &[], &[], None)?;
    Ok(())
}

pub fn decode(identity_project: &str, body: &[u8]) -> Result<Decoded, RecvError> {
    if body.len() > MAX_BODY {
        return Err(RecvError::Oversized);
    }
    let value = parse_document(body).map_err(|_| RecvError::Protocol)?;
    let root = value.as_object().ok_or(RecvError::Protocol)?;
    only(root, &["protocol", "receipt_id", "project", "items"])?;
    if required_str(root, "protocol")? != PROTOCOL {
        return Err(RecvError::Protocol);
    }
    let receipt_id = required_str(root, "receipt_id")?.to_ascii_lowercase();
    uuid_v4(&receipt_id)?;
    let project = required_str(root, "project")?.to_string();
    if !project_ok(&project) {
        return Err(RecvError::Protocol);
    }
    if project != identity_project {
        return Err(RecvError::Forbidden);
    }
    let items_value = root.get("items").and_then(Value::as_array).ok_or(RecvError::Protocol)?;
    if items_value.is_empty() {
        return Err(RecvError::Protocol);
    }
    let mut items = Vec::with_capacity(items_value.len());
    for item in items_value {
        let row = item.as_object().ok_or(RecvError::Protocol)?;
        only(row, &["project", "kind", "text", "target"])?;
        let item_project = required_str(row, "project")?.to_string();
        if !project_ok(&item_project) {
            return Err(RecvError::Protocol);
        }
        if item_project != identity_project {
            return Err(RecvError::CrossProject);
        }
        items.push(Item {
            project: item_project,
            kind: required_str(row, "kind")?.to_string(),
            text: required_str(row, "text")?.to_string(),
            target: required_str(row, "target")?.to_string(),
        });
    }
    Ok(Decoded {
        receipt_id,
        project,
        payload_hash: sha256(body),
        items,
    })
}

pub fn review(
    operator: &Operator,
    project: &str,
    items: &[Item],
    prior: &[String],
    receipt_id: Option<&str>,
) -> Result<Reviewed, RecvError> {
    if !project_ok(project) || !project_ok(&operator.source_id) || !role_ok(&operator.role) {
        return Err(RecvError::Bound);
    }
    if prior.len() > 256 || prior.iter().any(|item| !prior_ok(item)) {
        return Err(RecvError::Bound);
    }
    let feedback_items = items
        .iter()
        .map(|item| {
            json!({
                "kind": item.kind,
                "project": item.project,
                "target": item.target,
                "text": item.text,
            })
        })
        .collect::<Vec<_>>();
    let feedback = json!({
        "items": feedback_items,
        "schema": "oo.selfbuild-suggestions.feedback/v1",
    });
    let catalog_sha256 = canonical_sha256(&operator.catalog);
    let relationships_sha256 = canonical_sha256(&operator.relationships);
    let profile_sha256 = canonical_sha256(&operator.profile);
    let feedback_sha256 = canonical_sha256(&feedback);
    let prior_values = prior.iter().cloned().map(Value::String).collect::<Vec<_>>();
    let command = json!({
        "authorization": {
            "authorized": true,
            "catalog_sha256": catalog_sha256,
            "feedback_sha256": feedback_sha256,
            "profile_sha256": profile_sha256,
            "relationships_sha256": relationships_sha256,
        },
        "catalog": operator.catalog.clone(),
        "curated_overrides": [],
        "feedback": feedback,
        "negative_pairs": [],
        "prior_candidate_ids": prior_values,
        "profile": operator.profile.clone(),
        "relationships": operator.relationships.clone(),
        "schema": "oo.selfbuild-suggestions.command/v1",
        "scope": {
            "evidence_sha256": catalog_sha256,
            "model_scope": "none",
            "project": project,
            "role": operator.role,
            "schema_version": "oos/v1",
            "source_id": operator.source_id,
        },
    });
    let outcome = match produce(&command) {
        Ok(outcome) => outcome,
        Err(selfbuild_suggestions::Error::Malformed(_)) => return Err(RecvError::Protocol),
        Err(selfbuild_suggestions::Error::Refused { reason, .. }) => {
            return Err(match reason {
                "scope" => RecvError::Bound,
                "seal" | "authorization" => RecvError::Producer,
                _ => RecvError::Protocol,
            });
        }
    };
    checked(
        project,
        receipt_id,
        &catalog_sha256,
        &relationships_sha256,
        &profile_sha256,
        &feedback_sha256,
        outcome,
    )
}

fn checked(
    project: &str,
    receipt_id: Option<&str>,
    catalog_sha256: &str,
    relationships_sha256: &str,
    profile_sha256: &str,
    feedback_sha256: &str,
    outcome: Value,
) -> Result<Reviewed, RecvError> {
    let root = outcome.as_object().ok_or(RecvError::Producer)?;
    if root.get("schema").and_then(Value::as_str) != Some(OUTCOME) {
        return Err(RecvError::Producer);
    }
    let effects = root.get("effects").cloned().ok_or(RecvError::Producer)?;
    if !effects_closed(&effects) {
        return Err(RecvError::Producer);
    }
    let drops = root.get("scope_drops").cloned().ok_or(RecvError::Producer)?;
    if drops.get("feedback").and_then(Value::as_u64) != Some(0) {
        return Err(RecvError::CrossProject);
    }
    let schema_hash = root
        .get("scope")
        .and_then(|scope| scope.get("schema_hash"))
        .and_then(Value::as_str)
        .ok_or(RecvError::Producer)?
        .to_string();
    if schema_hash != "unsealed" && !hex_ok(&schema_hash, 64) {
        return Err(RecvError::Producer);
    }
    let feedback_rows = root.get("feedback").and_then(Value::as_array).ok_or(RecvError::Producer)?;
    let mut feedback = Vec::with_capacity(feedback_rows.len());
    let mut feedback_by_id = Map::new();
    for (ordinal, row) in feedback_rows.iter().enumerate() {
        if ordinal > 31 {
            return Err(RecvError::Producer);
        }
        let object = row.as_object().ok_or(RecvError::Producer)?;
        if object.get("untrusted") != Some(&Value::Bool(true))
            || object.get("trusted_metric") != Some(&Value::Bool(false))
            || object.get("knowledge_verified") != Some(&Value::Bool(false))
        {
            return Err(RecvError::Producer);
        }
        let candidate_id = produced(object, "candidate_id")?.to_string();
        let kind = produced(object, "kind")?.to_string();
        let target = produced(object, "target")?.to_string();
        let text = produced(object, "text")?.to_string();
        if !prior_ok(&candidate_id) {
            return Err(RecvError::Producer);
        }
        feedback_by_id.insert(candidate_id.clone(), json!({"kind": kind, "target": target, "text": text}));
        feedback.push(StoredFeedback {
            candidate_id,
            kind,
            target,
            text,
        });
    }
    let queue = root.get("queue").and_then(Value::as_array).ok_or(RecvError::Producer)?;
    if queue.len() > 128 {
        return Err(RecvError::Producer);
    }
    let candidates = {
        let seals = Seals {
            catalog: catalog_sha256,
            relationships: relationships_sha256,
            profile: profile_sha256,
            feedback: feedback_sha256,
            schema_hash: &schema_hash,
        };
        let mut candidates = Vec::with_capacity(queue.len());
        for row in queue {
            candidates.push(candidate_row(project, receipt_id, &seals, &feedback_by_id, row)?);
        }
        candidates
    };
    Ok(Reviewed {
        catalog_sha256: catalog_sha256.to_string(),
        relationships_sha256: relationships_sha256.to_string(),
        profile_sha256: profile_sha256.to_string(),
        feedback_sha256: feedback_sha256.to_string(),
        evidence_sha256: catalog_sha256.to_string(),
        schema_hash,
        effects,
        dedup: root.get("dedup").cloned().ok_or(RecvError::Producer)?,
        scope_drops: drops,
        outcome,
        feedback,
        candidates,
    })
}

struct Seals<'a> {
    catalog: &'a str,
    relationships: &'a str,
    profile: &'a str,
    feedback: &'a str,
    schema_hash: &'a str,
}

fn candidate_row(
    project: &str,
    receipt_id: Option<&str>,
    seals: &Seals<'_>,
    feedback_by_id: &Map<String, Value>,
    row: &Value,
) -> Result<StoredCandidate, RecvError> {
    let object = row.as_object().ok_or(RecvError::Producer)?;
    if object.get("trusted_metric") != Some(&Value::Bool(false))
        || object.get("knowledge_verified") != Some(&Value::Bool(false))
    {
        return Err(RecvError::Producer);
    }
    let candidate_id = produced(object, "candidate_id")?.to_string();
    let queue_id = produced(object, "queue_id")?.to_string();
    if candidate_id != queue_id || !prior_ok(&candidate_id) {
        return Err(RecvError::Producer);
    }
    let channel = produced(object, "channel")?.to_string();
    if !channel_ok(&channel) {
        return Err(RecvError::Producer);
    }
    let key = produced(object, "key")?.to_string();
    let impact = small_int(object, "impact")?;
    let uncertainty_millis = small_int(object, "uncertainty_millis")?;
    let score = small_int(object, "score")?;
    if i64::from(score) != i64::from(impact) * i64::from(uncertainty_millis) {
        return Err(RecvError::Producer);
    }
    let prior_overlap = object.get("prior_overlap").and_then(Value::as_bool).ok_or(RecvError::Producer)?;
    let mut source = json!({
        "catalog_sha256": seals.catalog,
        "channel": channel,
        "evidence_sha256": seals.catalog,
        "feedback_sha256": seals.feedback,
        "outcome_schema": OUTCOME,
        "producer": PRODUCER,
        "profile_sha256": seals.profile,
        "project": project,
        "relationships_sha256": seals.relationships,
        "schema_hash": seals.schema_hash,
        "untrusted": channel == "untrusted_feedback",
    });
    if let Some(receipt_id) = receipt_id {
        let map = source.as_object_mut().ok_or(RecvError::Producer)?;
        map.insert("receipt_id".to_string(), Value::String(receipt_id.to_string()));
    }
    if channel == "untrusted_feedback" {
        let linked = feedback_by_id.get(&candidate_id).ok_or(RecvError::Producer)?;
        let linked = linked.as_object().ok_or(RecvError::Producer)?;
        let text = produced(linked, "text")?.to_string();
        if key != format!("feedback:{}:{}", produced(linked, "kind")?, produced(linked, "target")?) {
            return Err(RecvError::Producer);
        }
        let map = source.as_object_mut().ok_or(RecvError::Producer)?;
        map.insert("feedback_text".to_string(), Value::String(text));
        map.insert("kind".to_string(), Value::String(produced(linked, "kind")?.to_string()));
        map.insert("target".to_string(), Value::String(produced(linked, "target")?.to_string()));
    } else if key.contains('\u{0}') {
        return Err(RecvError::Producer);
    }
    if source.to_string().len() > 8192 {
        return Err(RecvError::Producer);
    }
    Ok(StoredCandidate {
        candidate_id,
        channel,
        key,
        score,
        impact,
        uncertainty_millis,
        prior_overlap,
        source,
    })
}

pub fn ack(receipt_id: &str, payload_hash: &str, project: &str, duplicate: bool, reviewed: &Reviewed) -> Value {
    json!({
        "auto_merge": false,
        "candidates": candidate_json(&reviewed.candidates),
        "curated_written": false,
        "dedup": reviewed.dedup.clone(),
        "duplicate": duplicate,
        "effects": reviewed.effects.clone(),
        "grants_issued": false,
        "metrics_certified": false,
        "payload_hash": payload_hash,
        "project": project,
        "protocol": PROTOCOL,
        "published": false,
        "receipt_id": receipt_id,
        "roles_changed": false,
        "scope_drops": reviewed.scope_drops.clone(),
    })
}

pub fn review_page(project: &str, count: i64, rows: &[StoredCandidate]) -> Value {
    json!({
        "candidates": candidate_json(rows),
        "count": count,
        "grants_issued": false,
        "project": project,
        "protocol": PROTOCOL,
        "published": false,
        "roles_changed": false,
        "truncated": count > i64::try_from(rows.len()).unwrap_or(i64::MAX),
    })
}

fn candidate_json(rows: &[StoredCandidate]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            json!({
                "candidate_id": row.candidate_id,
                "channel": row.channel,
                "impact": row.impact,
                "key": row.key,
                "knowledge_verified": false,
                "prior_overlap": row.prior_overlap,
                "queue_id": row.candidate_id,
                "score": row.score,
                "source": row.source.clone(),
                "trusted_metric": false,
                "uncertainty_millis": row.uncertainty_millis,
            })
        })
        .collect()
}

fn effects_closed(effects: &Value) -> bool {
    flag(effects, "auto_merge")
        && flag(effects, "curated_written")
        && flag(effects, "grants_issued")
        && flag(effects, "metrics_certified")
        && flag(effects, "model_called")
        && flag(effects, "published")
        && flag(effects, "roles_changed")
        && flag(effects, "samples_retained")
        && flag(effects, "source_queried")
        && effects.get("knowledge_verified_count").and_then(Value::as_u64) == Some(0)
}

fn flag(effects: &Value, key: &str) -> bool {
    effects.get(key) == Some(&Value::Bool(false))
}

fn small_int(object: &Map<String, Value>, key: &str) -> Result<i32, RecvError> {
    let value = object.get(key).and_then(Value::as_u64).ok_or(RecvError::Producer)?;
    i32::try_from(value).map_err(|_| RecvError::Producer)
}

fn channel_ok(channel: &str) -> bool {
    matches!(
        channel,
        "entity" | "metric_hypothesis" | "example" | "untrusted_feedback" | "stale_override" | "negative_entity"
    )
}

pub fn project_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn role_ok(value: &str) -> bool {
    value
        .strip_prefix("oo_r_")
        .is_some_and(|rest| project_ok(rest) && rest.len() <= 31)
}

fn prior_ok(value: &str) -> bool {
    value.strip_prefix("cand_").is_some_and(|rest| hex_ok(rest, 32))
}

fn hex_ok(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn only(object: &Map<String, Value>, keys: &[&str]) -> Result<(), RecvError> {
    if object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else {
        Err(RecvError::Protocol)
    }
}

fn required_str<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, RecvError> {
    object.get(key).and_then(Value::as_str).ok_or(RecvError::Protocol)
}

fn produced<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, RecvError> {
    object.get(key).and_then(Value::as_str).ok_or(RecvError::Producer)
}

fn uuid_v4(text: &str) -> Result<(), RecvError> {
    let stripped: String = text.chars().filter(|char| *char != '-').collect();
    if stripped.len() != 32 || !hex_ok(&stripped, 32) {
        return Err(RecvError::Protocol);
    }
    let raw = decode_hex(&stripped).map_err(|_| RecvError::Protocol)?;
    if raw.len() == 16 && raw[6] >> 4 == 4 && (raw[8] & 0xc0) == 0x80 {
        Ok(())
    } else {
        Err(RecvError::Protocol)
    }
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}

pub fn decode_hex(text: &str) -> Result<Vec<u8>, ()> {
    if !text.len().is_multiple_of(2) {
        return Err(());
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let hi = hex_val(bytes[index])?;
        let lo = hex_val(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Ok(out)
}

fn hex_val(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(()),
    }
}
