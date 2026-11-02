use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

pub const MAX_BASELINE_PARTS: usize = 10_000;

fn part_id(id: &str) -> bool {
    id.parse::<usize>()
        .is_ok_and(|index| index < MAX_BASELINE_PARTS && index.to_string() == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Initializer {
    Full,
    Multipart,
    Published,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypePreimage {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WideningTypePreimages {
    pub old: TypePreimage,
    pub new: TypePreimage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema: String,
    pub table: String,
    pub schema_fingerprint: String,
    pub key_columns: Vec<String>,
    pub authority_file: String,
    pub target: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initializer: Option<Initializer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_topology_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_parts: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widening_type_preimages: Option<WideningTypePreimages>,
}

impl State {
    pub fn initializer_operation(&self) -> &'static str {
        match self.initializer.unwrap_or(Initializer::Full) {
            Initializer::Full => "initialize_full_if_absent",
            Initializer::Multipart => "initialize_multipart_if_absent",
            Initializer::Published => "initialize_published_if_absent",
            Initializer::Empty => "initialize_empty_if_absent",
        }
    }

    pub fn uses_target_baseline(&self) -> bool {
        matches!(
            self.initializer,
            Some(Initializer::Multipart | Initializer::Published)
        )
    }

    pub fn validate(&self) -> Result<()> {
        let ident = |name: &str| {
            !name.is_empty()
                && name.len() <= 63
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        if !ident(&self.schema)
            || !ident(&self.table)
            || if self.initializer == Some(Initializer::Empty) {
                !self.key_columns.is_empty() || self.chunk_id.is_some() || self.baseline_parts.is_some()
            } else { self.key_columns.is_empty() }
            || self.key_columns.iter().any(|key| !ident(key))
            || self
                .key_columns
                .iter()
                .enumerate()
                .any(|(i, k)| self.key_columns[..i].contains(k))
            || self.schema_fingerprint.len() != 64
            || !self
                .schema_fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !Path::new(&self.authority_file).is_absolute()
            || self.authority_file.contains("..")
            || self.authority_file.contains('\0')
            || self.target["database"].as_str().is_none_or(str::is_empty)
            || self.chunk_topology_revision.unwrap_or(1) < 1
            || self.chunk_id.as_deref().is_some_and(|id| {
                id.is_empty() || id.len() > 63 || !id.bytes().all(|b| b.is_ascii_digit())
            })
            || self.uses_target_baseline() && self.chunk_id.is_some()
            || self.baseline_parts.as_ref().is_some_and(|parts| {
                !self.uses_target_baseline()
                    || parts.len() > MAX_BASELINE_PARTS
                    || parts.iter().any(|(id, path)| {
                        !part_id(id)
                            || !Path::new(path).is_absolute()
                            || path.contains("..")
                            || path.contains('\0')
                    })
            })
            || self.widening_type_preimages.as_ref().is_some_and(|proof| {
                self.initializer != Some(Initializer::Published)
                    || [&proof.old, &proof.new].iter().any(|file| {
                        !Path::new(&file.path).is_absolute()
                            || file.path.contains("..")
                            || file.path.contains('\0')
                            || !super::is_hex64(&file.sha256)
                    })
            })
        {
            return Err(Error::new("c5_state_config"));
        }
        Ok(())
    }

    pub fn request(
        &self,
        source: &str,
        operation: &str,
        task: &str,
        generation: i64,
        base: i64,
        state_data: Option<&Value>,
    ) -> Result<Value> {
        self.validate()?;
        if self.initializer == Some(Initializer::Empty)
            && !["initialize_empty_if_absent", "export", "observe"].contains(&operation)
        {
            return Err(Error::new("c5_state_request"));
        }
        if ![
            self.initializer_operation(),
            "export",
            "begin_scan",
            "observe",
            "observe_partial",
            "hydrate_multipart",
        ]
        .contains(&operation)
            || source.is_empty()
            || source.contains('\0')
            || !crate::api::is_uuid(task)
            || generation < 1
            || base < 1
        {
            return Err(Error::new("c5_state_request"));
        }
        if operation == "hydrate_multipart" {
            let chunk = state_data.ok_or_else(|| Error::new("c5_hydration_scope"))?;
            let id = chunk["chunk_id"]
                .as_str()
                .filter(|id| part_id(id))
                .ok_or_else(|| Error::new("c5_hydration_scope"))?;
            let path = self
                .baseline_parts
                .as_ref()
                .and_then(|parts| parts.get(id))
                .ok_or_else(|| Error::new("c5_baseline_part_missing"))?;
            if !self.uses_target_baseline() || chunk["baseline_membership_pending"] != true {
                return Err(Error::new("c5_hydration_scope"));
            }
            return Ok(
                json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":operation,
                "source_id":source,"schema":self.schema,"table":self.table,"target":self.target,
                "lease_id":task,"lease_generation":generation,"expected_source_generation":base,
                "chunk_id":id,"arrow_path":path}),
            );
        }
        let mut request = json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":operation,"source_id":source,"schema":self.schema,"table":self.table,"target":self.target,"lease_id":task,"lease_generation":generation,"schema_fingerprint":self.schema_fingerprint,"chunk_topology_revision":self.chunk_topology_revision.unwrap_or(1),"key_columns":self.key_columns,"expected_source_generation":base});
        if operation == "initialize_published_if_absent" {
            if let Some(proof) = &self.widening_type_preimages {
                request["widening_type_preimages"] = json!(proof);
            }
        }
        if operation == "export" {
            request["authority_file"] = json!(self.authority_file);
        }
        if operation == "initialize_full_if_absent" {
            request["chunk_id"] = json!(self.chunk_id.as_deref().unwrap_or("0"));
        }
        if operation == "begin_scan" {
            let authority = state_data.ok_or_else(|| Error::new("c5_state_request"))?;
            let chunks = self.authority_chunks(authority, source, base)?;
            if authority.get("scan_id").is_none() || authority.get("expected_chunks").is_none() {
                return Err(Error::new("c5_scan_scope"));
            }
            // The task identifies a new scan; lease retries and later occurrences resume the target ID.
            request["scan_id"] = authority["scan_id"]
                .as_str()
                .map_or_else(|| json!(format!("c5-scan:{task}")), |id| json!(id));
            request["expected_chunks"] = json!(chunks);
            let layers: [bool; 4] = serde_json::from_value(authority["scan_layers"].clone())
                .map_err(|_| Error::new("c5_scan_scope"))?;
            let observed = authority["scan_observed_at_ms"]
                .as_u64()
                .ok_or_else(|| Error::new("c5_scan_scope"))?;
            request["scan_layers"] = json!(layers);
            request["scan_observed_at_ms"] = json!(observed);
        }
        if operation == "observe" {
            let value = state_data.ok_or_else(|| Error::new("c5_state_request"))?;
            if value["protocol"] != "oo.jobs.native-c5-nochange/v1"
                || value["source_id"] != source
                || value["source_generation"] != base
                || value["acknowledged_source_generation"] != base
                || value["data_gen"] != false
                || value["apply"] != false
                || value["ack"] != false
            {
                return Err(Error::new("c5_state_request"));
            }
            if (self.initializer == Some(Initializer::Empty))
                != (value["c5_observation"]["protocol"] == "oo.c5.empty-observation/v1")
            {
                return Err(Error::new("c5_state_request"));
            }
            request["nochange"] = value.clone();
        }
        if operation == "observe_partial" {
            let data = state_data.ok_or_else(|| Error::new("c5_state_request"))?;
            let range = &data["range_request"];
            self.validate_range_keys(range)?;
            let receipt = &data["loader_receipt"];
            let observation = &range["c5_observation"];
            let chunks = observation["chunks"]
                .as_array()
                .filter(|chunks| !chunks.is_empty())
                .ok_or_else(|| Error::new("c5_observation_receipt"))?;
            let parts = range["protocol"] == "oo.pipeline.native-c5-range-parts/v1";
            if (!parts && range["protocol"] != "oo.pipeline.native-c5-range/v1")
                || range["source_id"] != source
                || range["schema"] != self.schema
                || range["table"] != self.table
                || range["schema_fingerprint"] != self.schema_fingerprint
                || range["chunk_topology_revision"] != self.chunk_topology_revision.unwrap_or(1)
                || range["source_generation"] != base
                || range["parent_generation"]
                    .as_i64()
                    .and_then(|n| n.checked_add(1))
                    != Some(base)
                || range["target"]["database"] != self.target["database"]
                || receipt["protocol"] != "oo.pipeline.native-update-result/v1"
                || receipt["source_id"] != source
                || receipt["source_generation"] != base
                || receipt["logical_schema"] != self.schema
                || receipt["logical_table"] != self.table
                || receipt["receipt"].as_str().is_none_or(str::is_empty)
                || receipt["run_id"]
                    .as_str()
                    .is_none_or(|id| !crate::api::is_uuid(id))
                || receipt["target_generation"].as_i64().is_none_or(|n| n < 1)
                || receipt["payload_digest"]
                    .as_str()
                    .is_none_or(|digest| !super::is_hex64(digest))
                || receipt["jobs_lease_receipt"]["id"] != task
                || receipt["jobs_lease_receipt"]["generation"] != generation
                || !matches!(
                    crate::capture_source::native_decision(receipt),
                    crate::capture_source::NativeDecision::Advance
                        | crate::capture_source::NativeDecision::Replay
                )
                || observation["source_id"] != source
                || observation["schema"] != self.schema
                || observation["table"] != self.table
                || observation["schema_fingerprint"] != self.schema_fingerprint
                || observation["chunk_topology_revision"] != range["chunk_topology_revision"]
                || observation["parent_generation"] != range["parent_generation"]
                || observation["full_digest_scan_completed"] != false
                || observation["next_chunk_index"].as_u64() != Some(chunks.len() as u64)
                || chunks.last() != Some(&observation["chunk"])
                || observation["chunk"]["chunk_id"] != range["chunk_id"]
                || &observation["chunk"]["arrow_sha256"] != if parts { &range["parts_index"]["sha256"] } else { &range["arrow_sha256"] }
                || observation["chunk"]["lo"] != range["lo"]
                || observation["chunk"]["hi"] != range["hi"]
                || observation["chunk"]["row_count"] != range["row_count"]
                || range["prior_membership_revision"].as_u64().is_none()
                || observation["chunk"]["membership_revision"].as_u64()
                    != range["prior_membership_revision"]
                        .as_u64()
                        .and_then(|n| n.checked_add(1))
            {
                return Err(Error::new("c5_observation_receipt"));
            }
            match (
                observation.get("scan_id"),
                observation.get("expected_chunks"),
            ) {
                (Some(Value::Null), Some(Value::Null)) => {}
                (Some(Value::String(id)), Some(Value::Array(expected)))
                    if !id.is_empty()
                        && !id.contains('\0')
                        && expected.len() >= chunks.len()
                        && expected
                            .iter()
                            .take(chunks.len())
                            .zip(chunks)
                            .all(|(id, chunk)| id.is_string() && *id == chunk["chunk_id"]) => {}
                _ => return Err(Error::new("c5_scan_scope")),
            }
            let mut scope = range.clone();
            let object = scope
                .as_object_mut()
                .ok_or_else(|| Error::new("c5_state_request"))?;
            for field in [
                "native_request",
                "input_digest",
                "arrow_path",
                "consistency_grade",
                "cross_table_consistent",
                "target",
                "c5_completed_layers",
                "c5_observation",
                "binding",
                "pin",
                "key_columns",
                "type_policy",
                "normalization_rejects",
                "parts_index",
                "admitted_index",
                "part_count",
                "c5_capture_work",
                "lease_id",
                "lease_generation",
                "mirror",
            ] {
                object.remove(field);
            }
            if parts {
                object.insert("parts_task".into(),receipt["jobs_lease_receipt"]["id"].clone());
                object.insert("arrow_sha256".into(),range["parts_index"]["sha256"].clone());
            }
            if object.len() != 20 {
                return Err(Error::new("c5_observation_receipt"));
            }
            request["range_scope"] = scope;
            request["observation"] = observation.clone();
            request["loader_receipt"] = receipt.clone();
        }
        Ok(request)
    }

    pub fn validate_range_keys(&self, range: &Value) -> Result<()> {
        if self.initializer == Some(Initializer::Empty) {
            return Err(Error::new("c5_empty_source_changed"));
        }
        if self.initializer == Some(Initializer::Published)
            && (range["key_columns"] != json!(self.key_columns)
                || (range["protocol"] != "oo.pipeline.native-c5-range-parts/v1"
                    && range["native_request"]["key_columns"] != json!(self.key_columns)))
        {
            return Err(Error::new("c5_key_order"));
        }
        Ok(())
    }

    pub fn hydration_chunks(
        &self,
        authority: &Value,
        source: &str,
        base: i64,
    ) -> Result<Vec<Value>> {
        self.authority_chunks(authority, source, base)?;
        if !self.uses_target_baseline() {
            return Ok(vec![]);
        }
        let chunks = authority["chunks"]
            .as_array()
            .ok_or_else(|| Error::new("c5_hydration_scope"))?;
        if chunks.len() > MAX_BASELINE_PARTS
            || self.baseline_parts.as_ref().is_some_and(|parts| {
                parts.keys().any(|id| {
                    !chunks
                        .iter()
                        .any(|chunk| chunk["chunk_id"].as_str() == Some(id.as_str()))
                })
            })
        {
            return Err(Error::new("c5_hydration_scope"));
        }
        let mut pending = Vec::new();
        for chunk in chunks {
            match chunk["baseline_membership_pending"].as_bool() {
                Some(false) => {}
                Some(true) => {
                    let id = chunk["chunk_id"]
                        .as_str()
                        .filter(|id| part_id(id))
                        .ok_or_else(|| Error::new("c5_hydration_scope"))?;
                    if self
                        .baseline_parts
                        .as_ref()
                        .is_none_or(|parts| !parts.contains_key(id))
                    {
                        return Err(Error::new("c5_baseline_part_missing"));
                    }
                    pending.push(chunk.clone());
                }
                None => return Err(Error::new("c5_hydration_scope")),
            }
        }
        Ok(pending)
    }

    pub fn scan_context(&self, authority: &Value, input: &mut Value) -> Result<Value> {
        let now = input["c5_observed_at_ms"]
            .as_u64()
            .ok_or_else(|| Error::new("c5plus_clock"))?;
        let mut context = authority.clone();
        if authority["scan_id"].is_null() {
            let layers: [bool; 4] = serde_json::from_value(input["c5_due"].clone())
                .map_err(|_| Error::new("c5plus_pin"))?;
            context["scan_layers"] = json!(layers);
            context["scan_observed_at_ms"] = json!(now);
        } else {
            let layers: [bool; 4] = serde_json::from_value(authority["scan_layers"].clone())
                .map_err(|_| Error::new("c5_scan_scope"))?;
            if authority["scan_observed_at_ms"]
                .as_u64()
                .is_none_or(|started| started > now)
            {
                return Err(Error::new("c5_scan_scope"));
            }
            input["c5_due"] = json!(layers);
        }
        Ok(context)
    }

    pub fn authority_chunks(
        &self,
        authority: &Value,
        source: &str,
        base: i64,
    ) -> Result<Vec<Value>> {
        self.validate()?;
        if authority["source_id"] != source
            || authority["schema"] != self.schema
            || authority["table"] != self.table
            || authority["schema_fingerprint"] != self.schema_fingerprint
            || authority["chunk_topology_revision"] != self.chunk_topology_revision.unwrap_or(1)
            || authority["source_generation"] != base
            || authority["input_digest"].as_str().is_none_or(|digest| {
                digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(Error::new("c5_state_scope"));
        }
        if self.initializer == Some(Initializer::Empty) {
            let table = authority["table_source_generation"].as_i64();
            let proof = &authority["baseline_receipt"];
            if authority["protocol"] != "oo.pipeline.native-c5-empty-authority/v1"
                || authority["empty_only"] != true
                || authority["chunks"] != json!([])
                || authority["key_columns"] != json!([])
                || authority["next_chunk_index"] != 0
                || !authority["scan_id"].is_null()
                || !matches!(table, Some(t) if 1 <= t && t <= base)
                || proof["source_generation"] != authority["table_source_generation"]
                || proof["row_count"] != 0
                || proof["input_digest"] != authority["input_digest"]
                || authority["empty_observation"]["id"].as_str().is_none_or(|id| !crate::api::is_uuid(id))
            {
                return Err(Error::new("c5_state_scope"));
            }
            return Ok(vec![]);
        }
        if self.initializer == Some(Initializer::Published) {
            let table = authority["table_source_generation"].as_i64();
            let baseline = authority["baseline"]["source_generation"].as_i64();
            if !matches!((baseline, table), (Some(b), Some(t)) if 1 <= b && b <= t && t <= base)
                || authority["baseline"]["key_columns"] != json!(self.key_columns)
                || !matches!(
                    authority["baseline_kind"].as_str(),
                    Some("single" | "multipart")
                )
                || authority["baseline_kind"] != authority["baseline"]["baseline_kind"]
            {
                return Err(Error::new("c5_state_scope"));
            }
        }
        let chunks = authority["chunks"]
            .as_array()
            .filter(|chunks| !chunks.is_empty())
            .ok_or_else(|| Error::new("c5_state_scope"))?;
        let mut ids = HashSet::new();
        let mut expected = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let id = chunk["chunk_id"]
                .as_str()
                .filter(|id| !id.is_empty() && !id.contains('\0'))
                .ok_or_else(|| Error::new("c5_state_scope"))?;
            if !ids.insert(id) || chunk.get("lo").is_none() || chunk.get("hi").is_none() {
                return Err(Error::new("c5_state_scope"));
            }
            expected.push(json!(id));
        }
        if self.initializer.unwrap_or(Initializer::Full) == Initializer::Full
            && (chunks.len() != 1
                || chunks[0]["chunk_id"] != self.chunk_id.as_deref().unwrap_or("0")
                || !chunks[0]["lo"].is_null()
                || !chunks[0]["hi"].is_null())
        {
            return Err(Error::new("c5_state_scope"));
        }
        if self.initializer == Some(Initializer::Published)
            && authority["baseline_kind"] == "single"
            && (chunks.len() != 1
                || chunks[0]["chunk_id"] != "0"
                || !chunks[0]["lo"].is_null()
                || !chunks[0]["hi"].is_null())
        {
            return Err(Error::new("c5_state_scope"));
        }
        let next = authority["next_chunk_index"]
            .as_u64()
            .filter(|next| *next <= chunks.len() as u64)
            .ok_or_else(|| Error::new("c5_scan_scope"))?;
        // Legacy full mode can read old exports, but beginning a scan requires explicit identity.
        match (authority.get("scan_id"), authority.get("expected_chunks")) {
            (None, None) if self.initializer.unwrap_or(Initializer::Full) == Initializer::Full => {}
            (Some(Value::Null), Some(Value::Null)) if next == 0 => {}
            (Some(Value::String(id)), Some(Value::Array(scan_chunks)))
                if !id.is_empty() && !id.contains('\0') && scan_chunks == &expected => {}
            _ => return Err(Error::new("c5_scan_scope")),
        }
        Ok(expected)
    }

    pub fn validate_result(
        &self,
        operation: &str,
        result: &Value,
        source: &str,
        base: i64,
    ) -> Result<()> {
        if !result.is_object()
            || result.get("error").is_some()
            || !result["receipt"].is_null()
            || ["data_gen", "apply", "ack"]
                .iter()
                .any(|field| result.get(*field).is_some_and(|value| value != false))
        {
            return Err(Error::new("c5_state_result"));
        }
        match operation {
            "export" => {
                self.authority_chunks(result, source, base)?;
            }
            "begin_scan" if result["next_chunk_index"].as_u64().is_some() => {}
            "hydrate_multipart"
                if result["hydrated"] == true
                    && result["chunk_id"].as_str().is_some_and(part_id)
                    && result["rows"].as_u64().is_some()
                    && result["data_gen"] == false
                    && result["apply"] == false
                    && result["ack"] == false => {}
            "observe" | "observe_partial"
                if result["data_gen"] == false
                    && result["apply"] == false
                    && result["ack"] == false => {}
            "initialize_empty_if_absent" if self.initializer == Some(Initializer::Empty) => {
                self.authority_chunks(result, source, base)?;
            }
            op if op == self.initializer_operation() && result["initialized"] == true => {}
            _ => return Err(Error::new("c5_state_result")),
        }
        Ok(())
    }
}

pub fn scan_completed(result: &Value, observation: &Value) -> Result<bool> {
    match result.get("full_digest_scan_completed") {
        None | Some(Value::Bool(false)) => Ok(false),
        Some(Value::Bool(true)) => {
            let expected = observation["expected_chunks"]
                .as_array()
                .filter(|chunks| !chunks.is_empty())
                .ok_or_else(|| Error::new("c5_scan_scope"))?;
            if observation["scan_id"].as_str().is_none_or(str::is_empty)
                || result["next_chunk_index"].as_u64() != Some(expected.len() as u64)
            {
                return Err(Error::new("c5_scan_scope"));
            }
            Ok(true)
        }
        _ => Err(Error::new("c5_scan_scope")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK: &str = "11111111-1111-4111-8111-111111111111";

    fn state_document() -> Value {
        json!({"schema":"sample","table":"items","schema_fingerprint":"a".repeat(64),"key_columns":["id"],"authority_file":"/owned/authority.json","target":{"database":"agent"}})
    }

    #[test]
    fn empty_mode_is_explicit_zero_only_and_never_a_range_fallback() {
        let mut document = state_document();
        document["initializer"] = json!("empty");
        document["key_columns"] = json!([]);
        let state: State = serde_json::from_value(document.clone()).unwrap();
        assert!(state.validate().is_ok());
        assert_eq!(state.initializer_operation(), "initialize_empty_if_absent");
        assert!(!state.uses_target_baseline());
        assert!(state.validate_range_keys(&json!({})).is_err());
        for operation in ["begin_scan", "observe_partial", "hydrate_multipart"] {
            assert!(state.request("fixture", operation, TASK, 1, 7, None).is_err());
        }
        let authority = json!({"protocol":"oo.pipeline.native-c5-empty-authority/v1",
            "empty_only":true,"source_id":"fixture","schema":"sample","table":"items",
            "source_generation":7,"table_source_generation":3,"schema_fingerprint":"a".repeat(64),
            "chunk_topology_revision":1,"input_digest":"b".repeat(64),"chunks":[],
            "key_columns":[],"next_chunk_index":0,"scan_id":null,
            "baseline_receipt":{"source_generation":3,"row_count":0,"input_digest":"b".repeat(64)},
            "empty_observation":{"id":TASK}});
        assert!(state.authority_chunks(&authority, "fixture", 7).unwrap().is_empty());
        assert!(state.validate_result("initialize_empty_if_absent", &authority, "fixture", 7).is_ok());
        let mut nonempty = authority.clone();
        nonempty["baseline_receipt"]["row_count"] = json!(1);
        assert!(state.authority_chunks(&nonempty, "fixture", 7).is_err());
        document["key_columns"] = json!(["id"]);
        assert!(serde_json::from_value::<State>(document).unwrap().validate().is_err());
        assert!(serde_json::from_value::<State>(state_document()).unwrap()
            .authority_chunks(&authority, "fixture", 7).is_err());
    }

    fn multipart() -> State {
        let mut document = state_document();
        document["initializer"] = json!("multipart");
        serde_json::from_value(document).unwrap()
    }

    fn authority() -> Value {
        let bound = json!([{"kind":"int","value":40}]);
        json!({
            "source_id":"fixture", "schema":"sample", "table":"items",
            "source_generation":7, "table_source_generation":5,
            "schema_fingerprint":"a".repeat(64), "chunk_topology_revision":1,
            "input_digest":"b".repeat(64), "scan_id":null, "expected_chunks":null,
            "scan_layers":[true,false,true,false], "scan_observed_at_ms":1234,
            "next_chunk_index":0,
            "chunks":[
                {"chunk_id":"4","lo":null,"hi":bound,"membership_revision":1,"row_count":4,"baseline_membership_pending":false},
                {"chunk_id":"9","lo":bound,"hi":null,"membership_revision":3,"row_count":9,"baseline_membership_pending":false}
            ]
        })
    }

    fn published() -> (State, Value) {
        let mut state = multipart();
        state.initializer = Some(Initializer::Published);
        state.key_columns = vec!["tenant_id".into(), "id".into()];
        state.baseline_parts = Some(BTreeMap::from([("0".into(), "/owned/single.arrow".into())]));
        let mut authority = authority();
        authority["baseline_kind"] = json!("single");
        authority["baseline"] = json!({"source_generation":5,"baseline_kind":"single","key_columns":["tenant_id","id"]});
        authority["chunks"] = json!([{"chunk_id":"0","lo":null,"hi":null,
            "membership_revision":1,"row_count":2,"baseline_membership_pending":true}]);
        (state, authority)
    }

    fn widening_proof() -> Value {
        json!({"old":{"path":"/owned/old-types.json","sha256":"a".repeat(64)},
            "new":{"path":"/owned/new-types.json","sha256":"b".repeat(64)}})
    }

    #[test]
    fn widening_proof_only_reaches_published_initialize_and_preserves_old_manifests() {
        let (mut state, authority) = published();
        let legacy = serde_json::to_value(&state).unwrap();
        assert!(legacy.get("widening_type_preimages").is_none());
        assert_eq!(serde_json::from_value::<State>(legacy).unwrap(), state);
        assert!(state.request("fixture", state.initializer_operation(), TASK, 3, 7, None)
            .unwrap().get("widening_type_preimages").is_none());
        state.widening_type_preimages = Some(serde_json::from_value(widening_proof()).unwrap());
        let initialized = state.request("fixture", state.initializer_operation(), TASK, 3, 7, None).unwrap();
        assert_eq!(initialized["widening_type_preimages"], widening_proof());
        let quiet = json!({"protocol":"oo.jobs.native-c5-nochange/v1","source_id":"fixture",
            "source_generation":7,"acknowledged_source_generation":7,"data_gen":false,"apply":false,"ack":false});
        let mut partial = receipted_observation();
        partial["range_request"]["key_columns"] = json!(state.key_columns);
        partial["range_request"]["native_request"]["key_columns"] = json!(state.key_columns);
        for (operation, base, data) in [
            ("export", 7, None),
            ("begin_scan", 7, Some(&authority)),
            ("hydrate_multipart", 7, Some(&authority["chunks"][0])),
            ("observe", 7, Some(&quiet)),
            ("observe_partial", 8, Some(&partial)),
        ] {
            let request = state.request("fixture", operation, TASK, 3, base, data).unwrap();
            assert!(request.get("widening_type_preimages").is_none(), "{operation}");
        }
    }

    #[test]
    fn widening_proof_rejects_unsafe_paths_digests_and_open_shapes() {
        let (state, _) = published();
        for side in ["old", "new"] {
            for (field, value) in [
                ("path", json!("relative.json")),
                ("path", json!("/owned/../types.json")),
                ("path", json!("/owned/types\0.json")),
                ("sha256", json!("a".repeat(63))),
                ("sha256", json!("a".repeat(65))),
                ("sha256", json!("g".repeat(64))),
            ] {
                let mut document = serde_json::to_value(&state).unwrap();
                document["widening_type_preimages"] = widening_proof();
                document["widening_type_preimages"][side][field] = value;
                assert!(serde_json::from_value::<State>(document).unwrap().validate().is_err(), "{side}/{field}");
            }
        }
        for proof in [
            json!({"old":{"path":"/owned/old.json","sha256":"a".repeat(64)}}),
            json!({"old":{"path":"/owned/old.json","sha256":"a".repeat(64),"extra":true},
                "new":{"path":"/owned/new.json","sha256":"b".repeat(64)}}),
        ] {
            let mut document = serde_json::to_value(&state).unwrap();
            document["widening_type_preimages"] = proof;
            assert!(serde_json::from_value::<State>(document).is_err());
        }
    }

    #[test]
    fn widening_proof_rejects_every_nonpublished_initializer() {
        for initializer in [None, Some(Initializer::Full), Some(Initializer::Multipart), Some(Initializer::Empty)] {
            let mut state: State = serde_json::from_value(state_document()).unwrap();
            state.initializer = initializer;
            if initializer == Some(Initializer::Empty) {
                state.key_columns.clear();
            }
            state.validate().unwrap();
            state.widening_type_preimages = Some(serde_json::from_value(widening_proof()).unwrap());
            assert!(state.validate().is_err(), "{initializer:?}");
            assert!(state.request("fixture", state.initializer_operation(), TASK, 3, 7, None).is_err());
        }
    }

    #[test]
    fn published_single_baseline_below_global_watermark_uses_target_proof_and_original_keys() {
        let (state, authority) = published();
        let document = serde_json::to_value(&state).unwrap();
        assert_eq!(document["initializer"], "published");
        assert_eq!(serde_json::from_value::<State>(document).unwrap(), state);
        let request = state
            .request("fixture", state.initializer_operation(), TASK, 3, 7, None)
            .unwrap();
        assert_eq!(request["operation"], "initialize_published_if_absent");
        assert_eq!(request["expected_source_generation"], 7);
        assert_eq!(request["key_columns"], json!(["tenant_id", "id"]));
        for forbidden in [
            "chunk_id",
            "lo",
            "hi",
            "chunks",
            "multipart_scope",
            "baseline_generation",
            "table_source_generation",
            "baseline_parts",
            "authority_file",
        ] {
            assert!(request.get(forbidden).is_none(), "{forbidden}");
        }
        state
            .validate_result("export", &authority, "fixture", 7)
            .unwrap();
        let pending = state.hydration_chunks(&authority, "fixture", 7).unwrap();
        assert_eq!(pending.len(), 1);
        let hydration = state
            .request(
                "fixture",
                "hydrate_multipart",
                TASK,
                3,
                7,
                Some(&pending[0]),
            )
            .unwrap();
        assert_eq!(hydration["arrow_path"], "/owned/single.arrow");
        assert_eq!(hydration["expected_source_generation"], 7);
        assert_eq!(authority["table_source_generation"], 5);
        let scan = state
            .request("fixture", "begin_scan", TASK, 3, 7, Some(&authority))
            .unwrap();
        assert_eq!(scan["expected_chunks"], json!(["0"]));
        assert!(state.uses_target_baseline());
        assert!(
            state
                .request("fixture", "initialize_full_if_absent", TASK, 3, 7, None)
                .is_err()
        );
    }

    #[test]
    fn published_authority_refuses_future_table_generations_or_reordered_keys() {
        let (state, authority) = published();
        for (pointer, value) in [
            ("/source_generation", json!(5)),
            ("/table_source_generation", json!(8)),
            ("/table_source_generation", json!(0)),
            ("/baseline/source_generation", json!(6)),
            ("/baseline/key_columns", json!(["id", "tenant_id"])),
            ("/baseline_kind", json!("other")),
            ("/chunks/0/hi", json!([{"kind":"int","value":3}])),
        ] {
            let mut invalid = authority.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(
                state
                    .validate_result("export", &invalid, "fixture", 7)
                    .is_err(),
                "{pointer}"
            );
        }
        let mut advanced = authority;
        advanced["source_generation"] = json!(9);
        advanced["table_source_generation"] = json!(8);
        state
            .validate_result("export", &advanced, "fixture", 9)
            .unwrap();
        let mut invalid = state;
        invalid.chunk_id = Some("0".into());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn published_range_key_order_is_checked_before_loading_and_kept_outside_the_closed_scope() {
        let (mut state, _) = published();
        state.key_columns = vec!["id".into()];
        let mut data = receipted_observation();
        data["range_request"]["key_columns"] = json!(["id"]);
        data["range_request"]["native_request"]["key_columns"] = json!(["id"]);
        data["range_request"]["type_policy"] = json!({"version":"type-policy/v1"});
        data["range_request"]["normalization_rejects"] = Value::Null;
        state.validate_range_keys(&data["range_request"]).unwrap();
        let request = state
            .request("fixture", "observe_partial", TASK, 3, 8, Some(&data))
            .unwrap();
        assert_eq!(request["range_scope"].as_object().unwrap().len(), 20);
        assert!(request["range_scope"].get("key_columns").is_none());
        assert_eq!(request["loader_receipt"], data["loader_receipt"]);
        assert_eq!(
            request["observation"],
            data["range_request"]["c5_observation"]
        );
        state.key_columns = vec!["tenant_id".into(), "id".into()];
        data["range_request"]["key_columns"] = json!(["tenant_id", "id"]);
        data["range_request"]["native_request"]["key_columns"] = json!(["tenant_id", "id"]);
        state.validate_range_keys(&data["range_request"]).unwrap();
        for pointer in [
            "/range_request/key_columns",
            "/range_request/native_request/key_columns",
        ] {
            let mut invalid = data.clone();
            *invalid.pointer_mut(pointer).unwrap() = json!(["id", "tenant_id"]);
            assert!(
                state
                    .validate_range_keys(&invalid["range_request"])
                    .is_err()
            );
        }
    }

    #[test]
    fn hydration_uses_only_operator_pinned_pending_parts_and_the_current_lease() {
        let mut state = multipart();
        state.baseline_parts = Some(BTreeMap::from([
            ("4".into(), "/owned/baseline/4.arrow".into()),
            ("9".into(), "/owned/baseline/9.arrow".into()),
        ]));
        let mut authority = authority();
        for chunk in authority["chunks"].as_array_mut().unwrap() {
            chunk["baseline_membership_pending"] = json!(true);
        }
        let pending = state.hydration_chunks(&authority, "fixture", 7).unwrap();
        assert_eq!(pending.len(), 2);
        let request = state
            .request(
                "fixture",
                "hydrate_multipart",
                TASK,
                3,
                7,
                Some(&pending[0]),
            )
            .unwrap();
        assert_eq!(request.as_object().unwrap().len(), 11);
        assert_eq!(request["chunk_id"], "4");
        assert_eq!(request["arrow_path"], "/owned/baseline/4.arrow");
        assert_eq!(request["lease_id"], TASK);
        assert_eq!(request["lease_generation"], 3);
        assert_eq!(request["expected_source_generation"], 7);
        for forbidden in [
            "key_columns",
            "schema_fingerprint",
            "chunk_topology_revision",
            "baseline_parts",
            "authority_file",
            "receipt",
        ] {
            assert!(request.get(forbidden).is_none(), "{forbidden}");
        }
        let mut untrusted = pending[0].clone();
        untrusted["arrow_path"] = json!("/other/unpinned.arrow");
        assert_eq!(
            state
                .request("fixture", "hydrate_multipart", TASK, 3, 7, Some(&untrusted))
                .unwrap(),
            request
        );
        authority["chunks"][0]["baseline_membership_pending"] = json!(false);
        let resumed = state.hydration_chunks(&authority, "fixture", 7).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0]["chunk_id"], "9");
        authority["chunks"][1]["baseline_membership_pending"] = json!(false);
        assert!(
            state
                .hydration_chunks(&authority, "fixture", 7)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn hydration_rejects_missing_foreign_unbounded_or_unsafe_part_configuration() {
        let mut state = multipart();
        let mut authority = authority();
        authority["chunks"][0]["baseline_membership_pending"] = json!(true);
        assert!(state.hydration_chunks(&authority, "fixture", 7).is_err());
        for (id, path) in [
            ("4", "relative.arrow"),
            ("4", "/owned/../private.arrow"),
            ("4", "/owned/a\0.arrow"),
            ("04", "/owned/4.arrow"),
            ("10000", "/owned/4.arrow"),
            ("-1", "/owned/4.arrow"),
        ] {
            state.baseline_parts = Some(BTreeMap::from([(id.into(), path.into())]));
            assert!(state.validate().is_err(), "{id}/{path:?}");
        }
        state.baseline_parts = Some(BTreeMap::from([("4".into(), "/owned/4.arrow".into())]));
        assert!(state.hydration_chunks(&authority, "other", 7).is_err());
        assert!(state.hydration_chunks(&authority, "fixture", 8).is_err());
        authority["chunks"][1]["baseline_membership_pending"] = json!(true);
        assert!(state.hydration_chunks(&authority, "fixture", 7).is_err());
        state
            .baseline_parts
            .as_mut()
            .unwrap()
            .insert("8".into(), "/owned/8.arrow".into());
        assert!(state.hydration_chunks(&authority, "fixture", 7).is_err());
        state.initializer = Some(Initializer::Full);
        assert!(state.validate().is_err());
    }

    #[test]
    fn resumed_scan_pins_its_original_layers_and_window_without_rewriting_the_clock() {
        let state = multipart();
        let mut authority = authority();
        let mut input = json!({"c5_due":[false,true,false,true],"c5_observed_at_ms":3000});
        let fresh = state.scan_context(&authority, &mut input).unwrap();
        assert_eq!(fresh["scan_layers"], input["c5_due"]);
        assert_eq!(fresh["scan_observed_at_ms"], 3000);
        authority["scan_id"] = json!("previous-task-scan");
        authority["expected_chunks"] = json!(["4", "9"]);
        authority["next_chunk_index"] = json!(1);
        let resumed = state.scan_context(&authority, &mut input).unwrap();
        assert_eq!(input["c5_due"], authority["scan_layers"]);
        assert_eq!(input["c5_observed_at_ms"], 3000);
        let request = state
            .request("fixture", "begin_scan", TASK, 3, 7, Some(&resumed))
            .unwrap();
        assert_eq!(request["scan_id"], "previous-task-scan");
        assert_eq!(request["scan_observed_at_ms"], 1234);
        assert_eq!(request["scan_layers"], authority["scan_layers"]);
        for (field, value) in [
            ("scan_layers", json!([true])),
            ("scan_observed_at_ms", json!(3001)),
            ("scan_observed_at_ms", Value::Null),
        ] {
            let mut invalid = authority.clone();
            invalid[field] = value;
            assert!(
                state.scan_context(&invalid, &mut input.clone()).is_err(),
                "{field}"
            );
        }
    }

    fn receipted_observation() -> Value {
        let bound = json!([{"kind":"int","value":40}]);
        let chunk = json!({
            "chunk_id":"9","lo":bound,"hi":null,"row_count":1,
            "membership_revision":4,"arrow_sha256":"c".repeat(64),
            "digest":[1,"a".repeat(16),"b".repeat(16)],
            "digest_codec":"mysql-md5-xor/v1","projection_sha256":"d".repeat(64)
        });
        let range = json!({
            "protocol":"oo.pipeline.native-c5-range/v1","source_engine":"mysql",
            "capture_method":"c5_diff","source_id":"fixture","schema":"sample","table":"items",
            "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,
            "chunk_id":"9","lo":bound,"hi":null,"prior_membership_revision":3,
            "parent_generation":7,"source_generation":8,"arrow_sha256":"c".repeat(64),
            "row_count":1,"delete_detection":"reconcile","position_codec":"snapshot-zero/v1",
            "applied_pos":"0","members":[[{"kind":"int","value":41}]],
            "target":{"database":"agent"},"native_request":{"lease_id":TASK,"lease_generation":3},
            "c5_observation":{
                "source_id":"fixture","schema":"sample","table":"items",
                "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,
                "parent_generation":7,"scan_id":"persisted-scan","expected_chunks":["4","9"],
                "chunks":[authority()["chunks"][0],chunk],"chunk":chunk,"next_chunk_index":2,
                "full_digest_scan_completed":false,"checked_at_ms":1234,
                "selected_columns":["id"],"work":{"captured_blocks":1}
            }
        });
        json!({
            "range_request":range,
            "loader_receipt":{
                "protocol":"oo.pipeline.native-update-result/v1","source_id":"fixture",
                "source_generation":8,"logical_schema":"sample","logical_table":"items",
                "load_state":"awaiting_publish","mirror_committed":true,"loader_invoked":true,
                "source_bound":true,"producer_trust":{"trusted":true,"entry":"verify_inputs"},
                "jobs_lease_receipt":{"id":TASK,"generation":3,"owner":"worker",
                    "inserted":false,"target_fence_certified":false},
                "jobs_observation":{"call":"locked","target_fence_certified":false},
                "receipt":"applied","run_id":"33333333-3333-4333-8333-333333333333",
                "payload_digest":"e".repeat(64),"target_generation":4
            }
        })
    }

    #[test]
    fn parts_observation_is_compact_and_bound_to_actual_loader_task() {
        let state = multipart();
        let mut data = receipted_observation();
        let range = data["range_request"].as_object_mut().unwrap();
        range.remove("members");
        range.remove("native_request");
        let digest = range.remove("arrow_sha256").unwrap();
        range.insert("protocol".into(),json!("oo.pipeline.native-c5-range-parts/v1"));
        range.insert("parts_index".into(),json!({"path":"/owned/parts.jsonl","sha256":digest,"bytes":200}));
        range.insert("admitted_index".into(),json!({"path":"/owned/admitted.jsonl","sha256":"e".repeat(64),"bytes":200}));
        range.insert("part_count".into(),json!(2));
        range.insert("c5_capture_work".into(),json!({"rows":1,"bytes":10}));
        let request=state.request("fixture","observe_partial",TASK,3,8,Some(&data)).unwrap();
        assert_eq!(request["range_scope"].as_object().unwrap().len(),20);
        assert_eq!(request["range_scope"]["parts_task"],TASK);
        assert_eq!(request["range_scope"]["arrow_sha256"],"c".repeat(64));
        data["range_request"]["parts_index"]["sha256"]=json!("f".repeat(64));
        assert!(state.request("fixture","observe_partial",TASK,3,8,Some(&data)).is_err());
    }

    #[test]
    fn partial_observation_preserves_the_real_receipt_scope_and_producer_evidence() {
        let state = multipart();
        let data = receipted_observation();
        let request = state
            .request("fixture", "observe_partial", TASK, 3, 8, Some(&data))
            .unwrap();
        assert_eq!(request["loader_receipt"], data["loader_receipt"]);
        assert_eq!(
            request["observation"],
            data["range_request"]["c5_observation"]
        );
        assert_eq!(request["range_scope"].as_object().unwrap().len(), 20);
        assert_eq!(request["range_scope"]["lo"], data["range_request"]["lo"]);
        assert_eq!(
            request["range_scope"]["members"],
            data["range_request"]["members"]
        );
        assert_eq!(request["expected_source_generation"], 8);
        assert_eq!(request["observation"]["parent_generation"], 7);
        for field in ["nochange", "receipt", "ack", "data_gen", "authority_file"] {
            assert!(request.get(field).is_none(), "{field}");
        }
        let mut replay = data;
        replay["loader_receipt"]["load_state"] = json!("replay");
        replay["loader_receipt"]["loader_invoked"] = json!(false);
        replay["loader_receipt"]["receipt"] = json!("replay");
        replay["loader_receipt"]["mirror"] = json!("replay");
        let replayed = state
            .request("fixture", "observe_partial", TASK, 3, 8, Some(&replay))
            .unwrap();
        assert_eq!(replayed["loader_receipt"], replay["loader_receipt"]);
    }

    #[test]
    fn only_pipeline_completion_of_the_entire_scan_can_advance_the_digest_deadline() {
        let data = receipted_observation();
        let observation = &data["range_request"]["c5_observation"];
        assert!(!scan_completed(&json!({"next_chunk_index":2}), observation).unwrap());
        assert!(
            !scan_completed(&json!({"full_digest_scan_completed":false}), observation).unwrap()
        );
        assert!(
            scan_completed(
                &json!({"full_digest_scan_completed":true,"next_chunk_index":2}),
                observation
            )
            .unwrap()
        );
        for result in [
            json!({"full_digest_scan_completed":true,"next_chunk_index":1}),
            json!({"full_digest_scan_completed":true,"next_chunk_index":3}),
            json!({"full_digest_scan_completed":true}),
            json!({"full_digest_scan_completed":"true","next_chunk_index":2}),
        ] {
            assert!(scan_completed(&result, observation).is_err());
        }
        let mut recent_only = observation.clone();
        recent_only["scan_id"] = Value::Null;
        recent_only["expected_chunks"] = Value::Null;
        assert!(
            scan_completed(
                &json!({"full_digest_scan_completed":true,"next_chunk_index":2}),
                &recent_only
            )
            .is_err()
        );
    }

    #[test]
    fn partial_observation_refuses_unreceipted_foreign_or_incomplete_prefixes() {
        let state = multipart();
        let data = receipted_observation();
        for (pointer, value) in [
            ("/loader_receipt", Value::Null),
            ("/loader_receipt/receipt", Value::Null),
            ("/loader_receipt/run_id", json!("unsealed")),
            ("/loader_receipt/payload_digest", json!("")),
            ("/loader_receipt/source_generation", json!(7)),
            ("/loader_receipt/logical_table", json!("other")),
            ("/loader_receipt/jobs_lease_receipt/generation", json!(2)),
            ("/loader_receipt/load_state", json!("not_sealed")),
            ("/loader_receipt/mirror_committed", json!(false)),
            ("/loader_receipt/producer_trust/trusted", json!(false)),
            ("/range_request/parent_generation", json!(6)),
            ("/range_request/schema", json!("other")),
            ("/range_request/c5_observation/parent_generation", json!(8)),
            ("/range_request/c5_observation/next_chunk_index", json!(1)),
            (
                "/range_request/c5_observation/full_digest_scan_completed",
                json!(true),
            ),
            ("/range_request/c5_observation/chunks", json!([])),
            (
                "/range_request/c5_observation/expected_chunks",
                json!(["9", "4"]),
            ),
            ("/range_request/c5_observation/scan_id", Value::Null),
            (
                "/range_request/c5_observation/chunk/arrow_sha256",
                json!("f".repeat(64)),
            ),
            (
                "/range_request/c5_observation/chunk/membership_revision",
                json!(3),
            ),
        ] {
            let mut invalid = data.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(
                state
                    .request("fixture", "observe_partial", TASK, 3, 8, Some(&invalid))
                    .is_err(),
                "{pointer}"
            );
        }
        assert!(
            state
                .request("fixture", "observe_partial", TASK, 3, 8, None)
                .is_err()
        );
        assert!(
            state
                .request("fixture", "observe_partial", TASK, 3, 7, Some(&data))
                .is_err()
        );
        assert!(
            state
                .request("fixture", "observe", TASK, 3, 8, Some(&data))
                .is_err()
        );
    }

    #[test]
    fn initializer_is_explicit_and_old_manifest_serialization_is_unchanged() {
        let document = state_document();
        let mut state: State = serde_json::from_value(document.clone()).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), document);
        let legacy = state
            .request("fixture", state.initializer_operation(), TASK, 2, 7, None)
            .unwrap();
        assert_eq!(legacy["operation"], "initialize_full_if_absent");
        assert_eq!(legacy["chunk_id"], "0");

        state.chunk_id = Some("31".into());
        state.chunk_topology_revision = Some(2);
        let configured = state
            .request("fixture", state.initializer_operation(), TASK, 2, 7, None)
            .unwrap();
        assert_eq!(configured["chunk_id"], "31");
        assert_eq!(configured["chunk_topology_revision"], 2);

        let multipart = multipart();
        let request = multipart
            .request(
                "fixture",
                multipart.initializer_operation(),
                TASK,
                2,
                7,
                None,
            )
            .unwrap();
        assert_eq!(request["operation"], "initialize_multipart_if_absent");
        assert_eq!(request["expected_source_generation"], 7);
        for field in [
            "authority_file",
            "chunk_id",
            "lo",
            "hi",
            "chunks",
            "expected_chunks",
            "boundaries",
        ] {
            assert!(request.get(field).is_none(), "{field}");
        }
        let exported = multipart
            .request("fixture", "export", TASK, 2, 7, None)
            .unwrap();
        assert_eq!(exported["authority_file"], multipart.authority_file);
        assert!(
            multipart
                .request("fixture", "initialize_full_if_absent", TASK, 2, 7, None)
                .is_err()
        );
        assert!(
            state
                .request(
                    "fixture",
                    "initialize_multipart_if_absent",
                    TASK,
                    2,
                    7,
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn configuration_does_not_accept_untrusted_bounds_or_invalid_topologies() {
        for (field, value) in [
            ("initializer", json!("automatic")),
            ("lo", json!(null)),
            ("hi", json!(null)),
            ("chunks", json!([])),
            ("expected_chunks", json!(["0"])),
            ("boundaries", json!([])),
        ] {
            let mut document = state_document();
            document[field] = value;
            assert!(
                serde_json::from_value::<State>(document).is_err(),
                "{field}"
            );
        }
        let mut state = multipart();
        state.chunk_id = Some("0".into());
        assert!(state.validate().is_err());
        state.chunk_id = None;
        state.chunk_topology_revision = Some(0);
        assert!(state.validate().is_err());
        state.chunk_topology_revision = Some(1);
        state.authority_file.push('\0');
        assert!(state.validate().is_err());
    }

    #[test]
    fn scan_identity_and_complete_chunk_ids_survive_retries_and_other_table_generations() {
        let state = multipart();
        let mut authority = authority();
        let begin = state
            .request("fixture", "begin_scan", TASK, 2, 7, Some(&authority))
            .unwrap();
        assert_eq!(begin["expected_chunks"], json!(["4", "9"]));
        assert_eq!(begin["scan_id"], format!("c5-scan:{TASK}"));
        let retry = state
            .request("fixture", "begin_scan", TASK, 3, 7, Some(&authority))
            .unwrap();
        assert_eq!(retry["scan_id"], begin["scan_id"]);
        assert_eq!(retry["lease_generation"], 3);

        authority["scan_id"] = begin["scan_id"].clone();
        authority["expected_chunks"] = begin["expected_chunks"].clone();
        authority["next_chunk_index"] = json!(1);
        // A different table advanced the source, not this table's digest or membership.
        authority["source_generation"] = json!(8);
        let resumed = state
            .request(
                "fixture",
                "begin_scan",
                "22222222-2222-4222-8222-222222222222",
                1,
                8,
                Some(&authority),
            )
            .unwrap();
        assert_eq!(resumed["scan_id"], begin["scan_id"]);
        assert_eq!(resumed["expected_chunks"], begin["expected_chunks"]);
        assert_eq!(resumed["expected_source_generation"], 8);
        assert_eq!(authority["table_source_generation"], 5);
        assert_eq!(authority["next_chunk_index"], 1);
        assert!(resumed.get("receipt").is_none());
        assert!(resumed.get("ack").is_none());
    }

    #[test]
    fn scan_refuses_missing_identity_partial_manifests_and_invalid_cursors() {
        let state = multipart();
        let mut running = authority();
        running["scan_id"] = json!("persisted-scan");
        running["expected_chunks"] = json!(["4", "9"]);
        for (field, value) in [
            ("scan_id", json!("")),
            ("scan_id", json!(null)),
            ("expected_chunks", json!(null)),
            ("expected_chunks", json!(["4"])),
            ("expected_chunks", json!(["4", "4"])),
            ("expected_chunks", json!(["9", "4"])),
            ("expected_chunks", json!(["4", "9", "10"])),
            ("next_chunk_index", json!(-1)),
            ("next_chunk_index", json!(3)),
            ("next_chunk_index", json!("1")),
        ] {
            let mut invalid = running.clone();
            invalid[field] = value;
            assert!(
                state
                    .request("fixture", "begin_scan", TASK, 1, 7, Some(&invalid))
                    .is_err(),
                "{field}"
            );
        }
        for field in ["scan_id", "expected_chunks", "next_chunk_index"] {
            let mut old_export = authority();
            old_export.as_object_mut().unwrap().remove(field);
            assert!(
                state.authority_chunks(&old_export, "fixture", 7).is_err(),
                "{field}"
            );
        }
        let mut duplicate = authority();
        duplicate["chunks"][1]["chunk_id"] = json!("4");
        assert!(state.authority_chunks(&duplicate, "fixture", 7).is_err());
        running["next_chunk_index"] = json!(2);
        assert!(state.authority_chunks(&running, "fixture", 7).is_ok());
    }

    #[test]
    fn authority_is_never_locally_rebased_or_coerced_into_full_table_mode() {
        let state = multipart();
        let authority = authority();
        for (field, value) in [
            ("source_id", json!("other")),
            ("schema", json!("other")),
            ("table", json!("other")),
            ("source_generation", json!(6)),
            ("source_generation", json!(8)),
            ("schema_fingerprint", json!("c".repeat(64))),
            ("chunk_topology_revision", json!(2)),
            ("input_digest", json!(null)),
        ] {
            let mut invalid = authority.clone();
            invalid[field] = value;
            assert!(
                state
                    .validate_result("export", &invalid, "fixture", 7)
                    .is_err(),
                "{field}"
            );
        }
        let full: State = serde_json::from_value(state_document()).unwrap();
        assert!(full.authority_chunks(&authority, "fixture", 7).is_err());
        assert!(
            state
                .validate_result("export", &authority, "fixture", 7)
                .is_ok()
        );
    }

    #[test]
    fn legacy_full_exports_remain_readable_but_cannot_invent_a_scan_identity() {
        let full: State = serde_json::from_value(state_document()).unwrap();
        let mut legacy = authority();
        legacy["chunks"] = json!([{"chunk_id":"0","lo":null,"hi":null}]);
        legacy.as_object_mut().unwrap().remove("scan_id");
        legacy.as_object_mut().unwrap().remove("expected_chunks");
        full.validate_result("export", &legacy, "fixture", 7)
            .unwrap();
        assert!(
            full.request("fixture", "begin_scan", TASK, 1, 7, Some(&legacy))
                .is_err()
        );
        assert!(
            multipart()
                .validate_result("export", &legacy, "fixture", 7)
                .is_err()
        );
        legacy["chunks"][0]["hi"] = json!([{"kind":"int","value":40}]);
        assert!(
            full.validate_result("export", &legacy, "fixture", 7)
                .is_err()
        );
    }

    #[test]
    fn state_operations_cannot_substitute_for_a_loader_receipt() {
        let state = multipart();
        for (operation, result) in [
            (
                state.initializer_operation(),
                json!({"initialized":true,"receipt":null}),
            ),
            ("begin_scan", json!({"next_chunk_index":0})),
            (
                "observe",
                json!({"data_gen":false,"apply":false,"ack":false,"receipt":null}),
            ),
            (
                "observe_partial",
                json!({"data_gen":false,"apply":false,"ack":false,"receipt":null}),
            ),
            (
                "hydrate_multipart",
                json!({"hydrated":true,"chunk_id":"4","rows":4,"data_gen":false,"apply":false,"ack":false,"receipt":null}),
            ),
            ("export", authority()),
        ] {
            state
                .validate_result(operation, &result, "fixture", 7)
                .unwrap();
            for (field, value) in [
                ("error", json!("failed")),
                ("receipt", json!("applied")),
                ("data_gen", json!(true)),
                ("apply", json!(true)),
                ("ack", json!(true)),
            ] {
                let mut fake = result.clone();
                fake[field] = value;
                assert!(
                    state
                        .validate_result(operation, &fake, "fixture", 7)
                        .is_err(),
                    "{operation}/{field}"
                );
            }
            assert!(
                state
                    .validate_result(operation, &json!({}), "fixture", 7)
                    .is_err()
            );
        }
    }

    #[test]
    fn observations_keep_the_live_lease_and_forbid_data_acknowledgments() {
        let state: State = serde_json::from_value(json!({"schema":"sample","table":"items","schema_fingerprint":"a".repeat(64),"key_columns":["id"],"authority_file":"/owned/authority.json","target":{"database":"agent"}})).unwrap();
        let task = "11111111-1111-4111-8111-111111111111";
        let quiet = json!({"protocol":"oo.jobs.native-c5-nochange/v1","source_id":"fixture","source_generation":2,"acknowledged_source_generation":2,"data_gen":false,"apply":false,"ack":false});
        let request = state
            .request("fixture", "observe", task, 7, 2, Some(&quiet))
            .unwrap();
        assert_eq!(request["nochange"], quiet);
        assert_eq!(request["lease_id"], task);
        assert_eq!(request["lease_generation"], 7);
        for field in ["data_gen", "apply", "ack"] {
            let mut invalid = quiet.clone();
            invalid[field] = json!(true);
            assert!(
                state
                    .request("fixture", "observe", task, 7, 2, Some(&invalid))
                    .is_err()
            );
        }
        assert!(
            state
                .request("other", "observe", task, 7, 2, Some(&quiet))
                .is_err()
        );
        assert!(
            state
                .request("fixture", "observe", task, 7, 3, Some(&quiet))
                .is_err()
        );
        assert!(
            state
                .request("fixture", "arbitrary", task, 7, 2, None)
                .is_err()
        );
        assert!(
            state
                .request("fixture", "observe", "bad", 7, 2, Some(&quiet))
                .is_err()
        );
    }
}
