//! Seal a candidate by calling the role-pack compiler. No second compiler.

use oo_builder::{RolePack, compile_role_pack, normalize_pack_path};
use serde_json::{Map, Value, json};

use crate::canon::{canonical, canonical_sha};
use crate::sha256::sha256_hex;

pub struct Seal {
    pub effective: String,
    pub visibility: String,
    pub effective_sha256: String,
    pub visibility_sha256: String,
    pub source_hash: String,
    pub pack_digest: String,
    pub manifest_sha256: String,
    pub policy_id: String,
    pub policy_version: String,
    pub knowledge_id: String,
    pub knowledge_version: String,
    pub ontology_version: String,
    pub catalog_version: String,
    pub role: String,
    pub compiler: String,
    pub project: String,
    pub files: Vec<PackFile>,
}

pub struct PackFile {
    pub path: String,
    pub sha256: String,
    pub byte_len: i32,
}

pub struct Expectation<'a> {
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub version: &'a str,
    pub base_version: Option<&'a str>,
    pub policy_id: &'a str,
    pub policy_version: &'a str,
    pub knowledge_id: &'a str,
    pub knowledge_version: &'a str,
    pub catalog_version: &'a str,
}

pub fn authority_statement(authority_id: &str, covers: &[String]) -> Value {
    json!({
        "protocol": "oo.release-coordinator/authority/v1",
        "authority_id": authority_id,
        "class": "bounded_local_fixture",
        "product_mode_can_activate": false,
        "production_ready_certificate": false,
        "covers_gates": covers,
    })
}

pub fn compile_seal(
    effective: &Value,
    visibility: &Value,
    expected: &Expectation<'_>,
) -> Result<Seal, &'static str> {
    let effective_text = canonical(effective);
    let visibility_text = canonical(visibility);
    if effective_text.len() > 1_048_576 || visibility_text.len() > 1_048_576 {
        return Err("document_size");
    }
    let pack = compile_role_pack(&effective_text, &visibility_text).map_err(|_| "compiler")?;
    let manifest_text = pack.file("manifest.json").map_err(|_| "manifest_absent")?;
    let manifest: Value = serde_json::from_str(manifest_text).map_err(|_| "manifest_json")?;
    let manifest_map = manifest.as_object().ok_or("manifest_object")?;
    require_claim(manifest_map, "publication", "not_claimed")?;
    require_claim(manifest_map, "authentication", "not_claimed")?;
    require_claim(manifest_map, "authorization", "not_claimed")?;
    require_claim(manifest_map, "samples", "omitted")?;
    require_claim(manifest_map, "mount", "not_performed")?;
    if manifest_map
        .get("row_security_reproven")
        .and_then(Value::as_bool)
        != Some(false)
    {
        return Err("compiler_claim");
    }
    let project = text_field(manifest_map, "project")?;
    let role = text_field(manifest_map, "role")?;
    let compiler = text_field(manifest_map, "compiler")?;
    let catalog_version = text_field(manifest_map, "catalog_version")?;
    let ontology_version = text_field(manifest_map, "ontology_version")?;
    let (policy_id, policy_version) = pair_field(manifest_map, "policy")?;
    let (knowledge_id, knowledge_version) = pair_field(manifest_map, "knowledge")?;
    if project != expected.project_id
        || policy_id != expected.policy_id
        || policy_version != expected.policy_version
        || knowledge_id != expected.knowledge_id
        || knowledge_version != expected.knowledge_version
        || catalog_version != expected.catalog_version
    {
        return Err("manifest_mismatch");
    }
    if !role.starts_with("oo_r_") || role.len() > 63 {
        return Err("role");
    }
    let files = pack_files(&pack, manifest_map, manifest_text)?;
    let effective_sha256 = sha256_hex(effective_text.as_bytes());
    let visibility_sha256 = sha256_hex(visibility_text.as_bytes());
    let source = json!({
        "protocol": "oo.release-coordinator/seal/v1",
        "project_id": expected.project_id,
        "candidate_id": expected.candidate_id,
        "version": expected.version,
        "base_version": expected.base_version,
        "effective_sha256": effective_sha256,
        "visibility_sha256": visibility_sha256,
        "pack_digest": pack.pack_digest,
        "manifest_sha256": pack.manifest_sha256,
        "policy_id": policy_id,
        "policy_version": policy_version,
        "knowledge_id": knowledge_id,
        "knowledge_version": knowledge_version,
        "ontology_version": ontology_version,
        "catalog_version": catalog_version,
        "role": role,
        "compiler": compiler,
    });
    Ok(Seal {
        effective: effective_text,
        visibility: visibility_text,
        effective_sha256,
        visibility_sha256,
        source_hash: canonical_sha(&source),
        pack_digest: pack.pack_digest,
        manifest_sha256: pack.manifest_sha256,
        policy_id: policy_id.to_owned(),
        policy_version: policy_version.to_owned(),
        knowledge_id: knowledge_id.to_owned(),
        knowledge_version: knowledge_version.to_owned(),
        ontology_version: ontology_version.to_owned(),
        catalog_version: catalog_version.to_owned(),
        role: role.to_owned(),
        compiler: compiler.to_owned(),
        project: project.to_owned(),
        files,
    })
}

pub fn recompile_g8(
    effective: &str,
    visibility: &str,
    pack_digest: &str,
    manifest_sha256: &str,
) -> Result<(), &'static str> {
    let pack = compile_role_pack(effective, visibility).map_err(|_| "compiler")?;
    if pack.pack_digest != pack_digest || pack.manifest_sha256 != manifest_sha256 {
        return Err("recompile_mismatch");
    }
    let manifest_text = pack.file("manifest.json").map_err(|_| "manifest_absent")?;
    let manifest: Value = serde_json::from_str(manifest_text).map_err(|_| "manifest_json")?;
    let map = manifest.as_object().ok_or("manifest_object")?;
    require_claim(map, "samples", "omitted")?;
    require_claim(map, "publication", "not_claimed")?;
    Ok(())
}

fn pack_files(
    pack: &RolePack,
    manifest: &Map<String, Value>,
    manifest_text: &str,
) -> Result<Vec<PackFile>, &'static str> {
    let listed = manifest
        .get("files")
        .and_then(Value::as_array)
        .ok_or("files")?;
    if listed.is_empty() || listed.len() > 127 {
        return Err("files");
    }
    let mut files = Vec::with_capacity(listed.len() + 1);
    for item in listed {
        let object = item.as_object().ok_or("file")?;
        let path = object.get("path").and_then(Value::as_str).ok_or("file")?;
        let sha = object.get("sha256").and_then(Value::as_str).ok_or("file")?;
        let bytes = object.get("bytes").and_then(Value::as_u64).ok_or("file")?;
        let normalized = normalize_pack_path(path).map_err(|_| "path")?;
        if normalized != path
            || path == "manifest.json"
            || sha.len() != 64
            || bytes == 0
            || bytes > 1_048_576
        {
            return Err("path");
        }
        let text = pack.file(path).map_err(|_| "path")?;
        if sha256_hex(text.as_bytes()) != sha || text.len() as u64 != bytes {
            return Err("file_hash");
        }
        let byte_len = i32::try_from(bytes).map_err(|_| "file")?;
        files.push(PackFile {
            path: path.to_owned(),
            sha256: sha.to_owned(),
            byte_len,
        });
    }
    let manifest_sha = sha256_hex(manifest_text.as_bytes());
    if manifest_sha != pack.manifest_sha256 {
        return Err("file_hash");
    }
    let byte_len = i32::try_from(manifest_text.len()).map_err(|_| "file")?;
    files.push(PackFile {
        path: "manifest.json".to_owned(),
        sha256: manifest_sha,
        byte_len,
    });
    Ok(files)
}

fn require_claim(map: &Map<String, Value>, key: &str, want: &str) -> Result<(), &'static str> {
    if map.get(key).and_then(Value::as_str) == Some(want) {
        Ok(())
    } else {
        Err("compiler_claim")
    }
}

fn text_field<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or("manifest_field")
}

fn pair_field<'a>(
    map: &'a Map<String, Value>,
    key: &str,
) -> Result<(&'a str, &'a str), &'static str> {
    let object = map
        .get(key)
        .and_then(Value::as_object)
        .ok_or("manifest_field")?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or("manifest_field")?;
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or("manifest_field")?;
    Ok((id, version))
}

pub struct CompiledFile {
    pub path: String,
    pub text: String,
}

pub struct CompiledPack {
    pub artifact: String,
    pub files: Vec<CompiledFile>,
}

impl CompiledPack {
    /// Digest of the artifact a Gateway can serve. Activation must refuse when
    /// this differs from the staged candidate digest: the readable pack and the
    /// pointer would not be the same version.
    pub fn accessible_pack_digest(&self) -> Result<String, &'static str> {
        let value: Value = serde_json::from_str(&self.artifact).map_err(|_| "artifact")?;
        let digest = value
            .get("pack_digest")
            .and_then(Value::as_str)
            .ok_or("artifact")?;
        if !hex64(digest) {
            return Err("artifact");
        }
        let listed = value
            .get("files")
            .and_then(Value::as_array)
            .ok_or("artifact")?;
        if listed.len() != self.files.len() || listed.is_empty() {
            return Err("artifact");
        }
        Ok(digest.to_owned())
    }
}

pub fn compiled_pack(effective: &Value, visibility: &Value) -> Result<CompiledPack, &'static str> {
    let pack =
        compile_role_pack(&canonical(effective), &canonical(visibility)).map_err(|_| "compiler")?;
    let artifact: Value = serde_json::from_str(&pack.artifact).map_err(|_| "artifact")?;
    let listed = artifact
        .get("files")
        .and_then(Value::as_array)
        .ok_or("artifact")?;
    if listed.is_empty() {
        return Err("artifact");
    }
    let mut files = Vec::with_capacity(listed.len());
    for file in listed {
        let path = file.get("path").and_then(Value::as_str).ok_or("artifact")?;
        let text = file.get("text").and_then(Value::as_str).ok_or("artifact")?;
        if path.is_empty() {
            return Err("artifact");
        }
        files.push(CompiledFile {
            path: path.to_owned(),
            text: text.to_owned(),
        });
    }
    Ok(CompiledPack {
        artifact: pack.artifact,
        files,
    })
}

pub fn token_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b':' | b'-'))
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Verify a native maintenance artifact with the same role-pack compiler used by stage.
pub fn verify_native_artifact(effective: &Value, visibility: &Value, artifact: &Value) -> Result<(), &'static str> {
    let pack = compile_role_pack(&canonical(effective), &canonical(visibility)).map_err(|_| "compiler")?;
    let actual: Value = serde_json::from_str(&pack.artifact).map_err(|_| "artifact")?;
    if &actual != artifact { return Err("recompile_mismatch"); }
    Ok(())
}
