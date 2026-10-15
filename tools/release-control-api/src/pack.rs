//! Reviewed pack and gate intake. This service does not compile a second pack.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::ApiError;
use crate::ids::{hex64, role_ok, token_ok};
use crate::jsonutil::parse_document;
use crate::sha256::sha256_hex;

const REVIEWED: &str = "oo.release-control.reviewed-pack/v1";
const ENVELOPE: &str = "oo.selfbuild-control-api/v1";
const PRODUCER: &str = "oo.pg-builder.pack-combined/v1";
const VISIBILITY: &str = "oo.builder.role-visibility/v1";
const INTAKE: &str = "oo.release-control.gate-intake/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lineage {
    pub columns: BTreeSet<String>,
    pub role: String,
    pub policy_id: String,
    pub policy_version: String,
    pub knowledge_id: String,
    pub knowledge_version: String,
    pub catalog_version: String,
    pub review_admitted: bool,
}

pub struct LineageFields<'a> {
    pub role: &'a str,
    pub policy_id: &'a str,
    pub policy_version: &'a str,
    pub knowledge_id: &'a str,
    pub knowledge_version: &'a str,
    pub catalog_version: &'a str,
    pub review_admitted: bool,
}

impl Lineage {
    pub fn from_columns(text: &str, fields: LineageFields<'_>) -> Self {
        let columns = text.split('\n').filter(|item| !item.is_empty()).map(str::to_owned).collect();
        Self {
            columns,
            role: fields.role.to_owned(),
            policy_id: fields.policy_id.to_owned(),
            policy_version: fields.policy_version.to_owned(),
            knowledge_id: fields.knowledge_id.to_owned(),
            knowledge_version: fields.knowledge_version.to_owned(),
            catalog_version: fields.catalog_version.to_owned(),
            review_admitted: fields.review_admitted,
        }
    }
}

pub struct Prepared {
    pub bytes_sha256: String,
    pub effective: Value,
    pub visibility: Value,
    pub lineage: Lineage,
    pub columns_text: String,
    pub pack_digest: String,
}

pub struct GateIn {
    pub gate_id: String,
    pub issuer_class: String,
    pub status: String,
    pub blocking: bool,
    pub report_type: String,
}

pub struct CheckpointIn {
    pub kind: String,
    pub epoch: String,
    pub barrier: String,
    pub sessions_terminated: bool,
    pub issuer_class: String,
}

pub struct Intake {
    pub gates: Vec<GateIn>,
    pub checkpoints: Vec<CheckpointIn>,
}

pub fn permission_compatible(restored: Option<&Lineage>, active: &Lineage) -> bool {
    if !active.review_admitted {
        return false;
    }
    let Some(restored) = restored else {
        return true;
    };
    restored.review_admitted
        && restored.role == active.role
        && restored.policy_id == active.policy_id
        && restored.policy_version == active.policy_version
        && restored.knowledge_id == active.knowledge_id
        && restored.knowledge_version == active.knowledge_version
        && restored.catalog_version == active.catalog_version
        && restored.columns.is_subset(&active.columns)
}

pub fn load_pack(dir: &Path, config_commit: &str, role: &str) -> Result<Prepared, ApiError> {
    if !hex64(config_commit) || !role_ok(role) {
        return Err(ApiError::malformed("pack"));
    }
    let path = dir.join(format!("{config_commit}.json"));
    let bytes = read_file(&path)?;
    let digest = sha256_hex(&bytes);
    let value = parse_document(&bytes).map_err(|_| ApiError::pack("shape"))?;
    let object = value.as_object().ok_or(ApiError::pack("shape"))?;
    if object.get("protocol").and_then(Value::as_str) == Some("oo.release-control.native-reviewed-pack/v1") {
        return load_native_pack(object, &digest, config_commit, role);
    }
    require_keys(object, &["config_commit", "envelope", "protocol", "roles"])?;
    if object.get("protocol").and_then(Value::as_str) != Some(REVIEWED) {
        return Err(ApiError::pack("shape"));
    }
    if object.get("config_commit").and_then(Value::as_str) != Some(config_commit) {
        return Err(ApiError::pack("shape"));
    }
    let envelope = object.get("envelope").and_then(Value::as_object).ok_or(ApiError::pack("shape"))?;
    let producer = accept_envelope(envelope)?;
    let roles = object.get("roles").and_then(Value::as_array).ok_or(ApiError::pack("shape"))?;
    if roles.len() != 2 {
        return Err(ApiError::pack("shape"));
    }
    let visibility = roles
        .iter()
        .find(|item| item.get("role").and_then(Value::as_str) == Some(role))
        .ok_or(ApiError::pack("role"))?;
    let lineage = lineage_of(visibility)?;
    let pack_digest = pack_digest_of(producer, role)?;
    let columns_text = lineage.columns.iter().cloned().collect::<Vec<_>>().join("\n");
    let effective = producer.pointer("/product/effective/effective_oos").cloned().ok_or(ApiError::pack("shape"))?;
    if !effective.is_object() {
        return Err(ApiError::pack("shape"));
    }
    Ok(Prepared { bytes_sha256: digest, effective, visibility: visibility.clone(), lineage, columns_text, pack_digest })
}

fn load_native_pack(object: &Map<String, Value>, digest: &str, config_commit: &str, role: &str) -> Result<Prepared, ApiError> {
    require_keys(object, &["protocol","config_commit","envelope","effective","visibility","artifact"])?;
    if object["config_commit"] != config_commit { return Err(ApiError::pack("shape")); }
    let envelope=&object["envelope"];
    if envelope["contract"] != "oo.selfbuild-control-api/role-pack/v1" || !matches!(envelope["operation"].as_str(),Some("getRolePack" | "compileRolePack"))
        || envelope["role"] != role || envelope["compiled_pack"] != "not_published" || envelope["published"] != false
        || envelope["active"] != false || envelope["data_grants"] != "none" || envelope["business_owner_signed"] != false
        || envelope["signer_class"] != "control_plane_test" || envelope["llm_calls"] != 0
    { return Err(ApiError::pack("published")); }
    let receipt=&envelope["native_maintenance"];
    let effective=object["effective"].clone();let visibility=object["visibility"].clone();let artifact=&object["artifact"];
    let effective_bytes=serde_json::to_vec(&effective).map_err(|_|ApiError::pack("shape"))?;
    if receipt["protocol"] != "oo.selfbuild-control-api/native-maintenance/v1" || receipt["publication"] != "not_published"
        || receipt["build_id"] != envelope["build_id"] || receipt["effective_sha256"] != sha256_hex(&effective_bytes)
        || receipt["pack_digest"] != artifact["pack_digest"] || receipt["manifest_sha256"] != artifact["manifest_sha256"]
        || receipt["llm_calls"] != 0 || visibility["role"] != role
        || artifact["files"].as_array().is_none_or(|files| files.iter().any(|file| file["text"].as_str().is_none_or(|text| text.starts_with("UNREVIEWED"))))
    { return Err(ApiError::pack("digest")); }
    release_coordinator::verify_native_artifact(&effective,&visibility,artifact).map_err(|_|ApiError::pack("digest"))?;
    let lineage=lineage_of(&visibility)?;
    let pack_digest=artifact["pack_digest"].as_str().filter(|s|hex64(s)).ok_or(ApiError::pack("digest"))?.to_owned();
    let columns_text=lineage.columns.iter().cloned().collect::<Vec<_>>().join("\n");
    Ok(Prepared {bytes_sha256:digest.to_owned(),effective,visibility,lineage,columns_text,pack_digest})
}

pub fn load_intake(dir: &Path, config_commit: &str) -> Result<Intake, ApiError> {
    if !hex64(config_commit) {
        return Err(ApiError::malformed("pack"));
    }
    let path = dir.join(format!("{config_commit}.gates.json"));
    let bytes = read_file(&path)?;
    let value = parse_document(&bytes).map_err(|_| ApiError::pack("shape"))?;
    let object = value.as_object().ok_or(ApiError::pack("shape"))?;
    require_keys(object, &["checkpoints", "config_commit", "external", "gates", "protocol", "skip"])?;
    if object.get("protocol").and_then(Value::as_str) != Some(INTAKE) {
        return Err(ApiError::pack("shape"));
    }
    if object.get("config_commit").and_then(Value::as_str) != Some(config_commit) {
        return Err(ApiError::pack("shape"));
    }
    let skip = string_set(object.get("skip"))?;
    let mut gates = Vec::new();
    for item in array(object, "gates")? {
        let gate = gate_in(item, false)?;
        if skip.contains(&gate.gate_id) {
            continue;
        }
        gates.push(gate);
    }
    let mut external = Vec::new();
    for item in array(object, "external")? {
        let gate = gate_in(item, true)?;
        if gate.issuer_class != "test_issuer" || gate.report_type != "test_issuer" {
            return Err(ApiError::pack("issuer"));
        }
        external.push(gate);
    }
    gates.extend(external);
    let mut checkpoints = Vec::new();
    for item in array(object, "checkpoints")? {
        checkpoints.push(checkpoint_in(item)?);
    }
    Ok(Intake { gates, checkpoints })
}

fn accept_envelope(envelope: &Map<String, Value>) -> Result<&Value, ApiError> {
    if envelope.get("contract").and_then(Value::as_str) != Some(ENVELOPE)
        || envelope.get("operation").and_then(Value::as_str) != Some("getBuildPack")
        || envelope.get("published") != Some(&Value::Bool(false))
        || envelope.get("active") != Some(&Value::Bool(false))
        || envelope.get("producer_authenticated") != Some(&Value::Bool(false))
    {
        return Err(ApiError::pack("published"));
    }
    if envelope.get("compiled_pack").and_then(Value::as_str) != Some("not_published") {
        return Err(ApiError::pack("published"));
    }
    let output = envelope.get("output_sha256").and_then(Value::as_str).ok_or(ApiError::pack("digest"))?;
    if !hex64(output) {
        return Err(ApiError::pack("digest"));
    }
    let producer = envelope.get("producer").ok_or(ApiError::pack("shape"))?;
    let encoded = serde_json::to_vec(producer).map_err(|_| ApiError::pack("shape"))?;
    if sha256_hex(&encoded) != output {
        return Err(ApiError::pack("digest"));
    }
    let producer_object = producer.as_object().ok_or(ApiError::pack("shape"))?;
    if producer_object.get("protocol").and_then(Value::as_str) != Some(PRODUCER)
        || producer_object.get("authenticated") != Some(&Value::Bool(false))
        || producer_object.get("parent_auth").and_then(Value::as_str) != Some("required")
        || producer.pointer("/statements/model_publication") != Some(&Value::Bool(false))
    {
        return Err(ApiError::pack("shape"));
    }
    let outcome = producer_object.get("outcome").and_then(Value::as_str).unwrap_or("");
    if !matches!(outcome, "packed" | "reviewed_unchanged") {
        return Err(ApiError::pack("outcome"));
    }
    let packs = producer.pointer("/product/packs").and_then(Value::as_array).ok_or(ApiError::pack("shape"))?;
    if packs.len() != 2 {
        return Err(ApiError::pack("shape"));
    }
    if unreviewed_text(packs) {
        return Err(ApiError::unreviewed());
    }
    Ok(producer)
}

fn unreviewed_text(packs: &[Value]) -> bool {
    for pack in packs {
        let Some(files) = pack.pointer("/artifact/files").and_then(Value::as_array) else {
            continue;
        };
        for file in files {
            if file.get("text").and_then(Value::as_str).is_some_and(|text| text.starts_with("UNREVIEWED")) {
                return true;
            }
        }
    }
    false
}

fn pack_digest_of(producer: &Value, role: &str) -> Result<String, ApiError> {
    let packs = producer.pointer("/product/packs").and_then(Value::as_array).ok_or(ApiError::pack("shape"))?;
    let pack = packs.iter().find(|item| item.get("role").and_then(Value::as_str) == Some(role)).ok_or(ApiError::pack("role"))?;
    let digest = pack.get("pack_digest").and_then(Value::as_str).ok_or(ApiError::pack("digest"))?;
    if hex64(digest) { Ok(digest.to_owned()) } else { Err(ApiError::pack("digest")) }
}

fn lineage_of(visibility: &Value) -> Result<Lineage, ApiError> {
    let object = visibility.as_object().ok_or(ApiError::pack("shape"))?;
    if object.get("protocol").and_then(Value::as_str) != Some(VISIBILITY) {
        return Err(ApiError::pack("shape"));
    }
    let role = object.get("role").and_then(Value::as_str).ok_or(ApiError::pack("role"))?;
    if !role_ok(role) {
        return Err(ApiError::pack("role"));
    }
    let policy_id = pair(object, "policy", "id")?;
    let policy_version = pair(object, "policy", "version")?;
    let knowledge_id = pair(object, "knowledge", "id")?;
    let knowledge_version = pair(object, "knowledge", "version")?;
    let catalog_version = object.get("catalog_version").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    if !token_ok(catalog_version) {
        return Err(ApiError::pack("shape"));
    }
    let mut columns = BTreeSet::new();
    for dataset in array(object, "datasets")? {
        let name = dataset.get("dataset").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
        if !token_ok(name) {
            return Err(ApiError::pack("shape"));
        }
        let listed = dataset.get("columns").and_then(Value::as_array).ok_or(ApiError::pack("shape"))?;
        for column in listed {
            let column = column.as_str().ok_or(ApiError::pack("shape"))?;
            if !token_ok(column) {
                return Err(ApiError::pack("shape"));
            }
            columns.insert(format!("{name}.{column}"));
        }
    }
    Ok(Lineage {
        columns,
        role: role.to_owned(),
        policy_id,
        policy_version,
        knowledge_id,
        knowledge_version,
        catalog_version: catalog_version.to_owned(),
        review_admitted: true,
    })
}

fn gate_in(value: &Value, external: bool) -> Result<GateIn, ApiError> {
    let object = value.as_object().ok_or(ApiError::pack("shape"))?;
    require_keys(object, &["blocking", "gate_id", "issuer_class", "report_type", "status"])?;
    let gate_id = object.get("gate_id").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let issuer_class = object.get("issuer_class").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let status = object.get("status").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let report_type = object.get("report_type").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let blocking = object.get("blocking").and_then(Value::as_bool).ok_or(ApiError::pack("shape"))?;
    let gate_ok = if external {
        matches!(gate_id, "credential" | "integrity" | "authenticated_owner" | "customer" | "runsc")
    } else {
        matches!(
            gate_id,
            "G1" | "G2" | "G3" | "G4" | "G5a" | "G5b" | "G6" | "G7" | "G8" | "G9" | "G10" | "G11" | "G12" | "G13"
        )
    };
    if !gate_ok || !matches!(issuer_class, "test_issuer" | "fixture_authority" | "component_authority") {
        return Err(ApiError::pack("issuer"));
    }
    if !matches!(status, "pass" | "fail" | "warn" | "not_run") || !token_ok(report_type) {
        return Err(ApiError::pack("shape"));
    }
    Ok(GateIn { gate_id: gate_id.to_owned(), issuer_class: issuer_class.to_owned(), status: status.to_owned(), blocking, report_type: report_type.to_owned() })
}

fn checkpoint_in(value: &Value) -> Result<CheckpointIn, ApiError> {
    let object = value.as_object().ok_or(ApiError::pack("shape"))?;
    require_keys(object, &["barrier", "epoch", "issuer_class", "kind", "sessions_terminated"])?;
    let kind = object.get("kind").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let epoch = object.get("epoch").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let barrier = object.get("barrier").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let issuer_class = object.get("issuer_class").and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    let sessions_terminated = object.get("sessions_terminated").and_then(Value::as_bool).ok_or(ApiError::pack("shape"))?;
    if !matches!(kind, "fence" | "replica") || !token_ok(epoch) || !matches!(barrier, "closed" | "replayed" | "stale" | "missing") {
        return Err(ApiError::pack("shape"));
    }
    if !matches!(issuer_class, "test_issuer" | "fixture_authority") {
        return Err(ApiError::pack("issuer"));
    }
    Ok(CheckpointIn { kind: kind.to_owned(), epoch: epoch.to_owned(), barrier: barrier.to_owned(), sessions_terminated, issuer_class: issuer_class.to_owned() })
}

fn array<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, ApiError> {
    object.get(key).and_then(Value::as_array).ok_or(ApiError::pack("shape"))
}

fn string_set(value: Option<&Value>) -> Result<BTreeSet<String>, ApiError> {
    let mut out = BTreeSet::new();
    for item in value.and_then(Value::as_array).ok_or(ApiError::pack("shape"))? {
        let text = item.as_str().ok_or(ApiError::pack("shape"))?;
        if !token_ok(text) {
            return Err(ApiError::pack("shape"));
        }
        out.insert(text.to_owned());
    }
    Ok(out)
}

fn pair(object: &Map<String, Value>, key: &str, field: &str) -> Result<String, ApiError> {
    let nested = object.get(key).and_then(Value::as_object).ok_or(ApiError::pack("shape"))?;
    let text = nested.get(field).and_then(Value::as_str).ok_or(ApiError::pack("shape"))?;
    if token_ok(text) { Ok(text.to_owned()) } else { Err(ApiError::pack("shape")) }
}

fn require_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), ApiError> {
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        Err(ApiError::pack("shape"))
    } else {
        Ok(())
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, ApiError> {
    let metadata = fs::metadata(path).map_err(|_| ApiError::not_ready("pack"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 2_097_152 {
        return Err(ApiError::not_ready("pack"));
    }
    fs::read(path).map_err(|_| ApiError::not_ready("pack"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use serde_json::json;

    use super::{load_pack, permission_compatible, Lineage, Prepared};
    use crate::error::ApiError;
    use crate::sha256::sha256_hex;

    fn line(columns: &[&str], admitted: bool) -> Lineage {
        Lineage {
            columns: columns.iter().map(|item| (*item).to_owned()).collect::<BTreeSet<_>>(),
            role: "oo_r_clerk".to_owned(),
            policy_id: "pol-shop".to_owned(),
            policy_version: "1".to_owned(),
            knowledge_id: "know-shop".to_owned(),
            knowledge_version: "1".to_owned(),
            catalog_version: "cat-1".to_owned(),
            review_admitted: admitted,
        }
    }

    #[test]
    fn rollback_is_conservative() {
        let wide = line(&["public.customer.contact", "public.customer.id"], true);
        let narrow = line(&["public.customer.contact"], true);
        assert!(permission_compatible(Some(&narrow), &wide));
        assert!(!permission_compatible(Some(&wide), &narrow));
        assert!(permission_compatible(None, &narrow));
        assert!(!permission_compatible(Some(&narrow), &line(&["public.customer.contact", "public.customer.id"], false)));
    }

    #[test]
    fn unreviewed_text_and_published_flag_are_refused() {
        let dir = std::env::temp_dir().join(format!("oo-a392-pack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let commit = "ab".repeat(32);
        let producer = json!({
            "protocol": "oo.pg-builder.pack-combined/v1",
            "authenticated": false,
            "parent_auth": "required",
            "outcome": "packed",
            "statements": {"model_publication": false},
            "product": {"packs": [
                {"role": "oo_r_clerk", "pack_digest": "cd".repeat(32), "artifact": {"files": [{"text": "UNREVIEWED draft"}]}},
                {"role": "oo_r_audit", "pack_digest": "ef".repeat(32), "artifact": {"files": [{"text": "reviewed"}]}}
            ]}
        });
        let bytes = serde_json::to_vec(&producer).expect("producer");
        write_wrapper(&dir, &commit, &envelope(&producer, &sha256_hex(&bytes), false, "not_published"));
        let refused = must_refuse(load_pack(&dir, &commit, "oo_r_clerk"));
        assert_eq!(refused.code, "pack.unreviewed");
        assert_eq!(refused.detail, "review");
        write_wrapper(&dir, &commit, &envelope(&producer, &sha256_hex(&bytes), true, "not_published"));
        let published = must_refuse(load_pack(&dir, &commit, "oo_r_clerk"));
        assert_eq!(published.code, "pack.rejected");
        assert_eq!(published.detail, "published");
        write_wrapper(&dir, &commit, &envelope(&producer, &sha256_hex(&bytes), false, "published"));
        let compiled = must_refuse(load_pack(&dir, &commit, "oo_r_clerk"));
        assert_eq!(compiled.detail, "published");
        let _ = fs::remove_dir_all(&dir);
    }

    fn must_refuse(result: Result<Prepared, ApiError>) -> ApiError {
        match result {
            Ok(_) => panic!("pack was accepted"),
            Err(error) => error,
        }
    }

    fn envelope(producer: &serde_json::Value, output: &str, published: bool, compiled_pack: &str) -> serde_json::Value {
        json!({
            "active": false,
            "compiled_pack": compiled_pack,
            "contract": "oo.selfbuild-control-api/v1",
            "operation": "getBuildPack",
            "output_sha256": output,
            "producer": producer,
            "producer_authenticated": false,
            "published": published
        })
    }

    fn write_wrapper(dir: &std::path::Path, commit: &str, envelope: &serde_json::Value) {
        let wrapper = json!({
            "config_commit": commit,
            "envelope": envelope,
            "protocol": "oo.release-control.reviewed-pack/v1",
            "roles": []
        });
        fs::write(dir.join(format!("{commit}.json")), serde_json::to_vec(&wrapper).expect("wrapper")).expect("write");
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    #[ignore = "actual reviewed native maintenance artifact required"]
    fn owned_native_intake_recompiles_and_refuses_drift() {
        let path=std::env::var("OO_NATIVE_REVIEWED_PACK_TEST").unwrap();
        let wrapper:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let role=wrapper["visibility"]["role"].as_str().unwrap();
        let commit=wrapper["config_commit"].as_str().unwrap();
        load_native_pack(wrapper.as_object().unwrap(),"test",commit,role).expect("actual native pack");
        for (path,value) in [("/envelope/published",Value::Bool(true)),("/envelope/native_maintenance/pack_digest",Value::String("0".repeat(64))),("/artifact/files/0/text",Value::String("changed".into())),("/effective/ontology/name",Value::String("changed".into()))] {
            let mut wrong=wrapper.clone();*wrong.pointer_mut(path).expect("known fixture field")=value;
            assert!(load_native_pack(wrong.as_object().unwrap(),"test",commit,role).is_err());
        }
        println!("OWNED_NATIVE_INTAKE recompiled actual role pack and rejected publication/digest/artifact/effective drift");
    }
}
