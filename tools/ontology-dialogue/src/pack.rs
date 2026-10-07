use serde::Deserialize;
use serde_json::{Map, Value};

use crate::error::Fault;
use crate::json::{self, JsonError};
use crate::sha::{canonical_sha256, sha256_hex};

pub const PACK_PROTOCOL: &str = "oo.builder.role-pack/v1";
pub const PACK_COMPILER: &str = "oo-builder/role-pack/v1";
pub const MANIFEST_PROTOCOL: &str = "oo.builder.role-pack-manifest/v1";
const FILE_MAX: usize = 128;

#[derive(Debug, Clone)]
pub struct PublicFile {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct BoundPack {
    pub files: Vec<PublicFile>,
    pub pack_digest: String,
    pub manifest_sha256: String,
    pub project: String,
    pub role: String,
    pub policy_id: String,
    pub policy_version: String,
    pub knowledge_id: String,
    pub knowledge_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    protocol: String,
    compiler: String,
    pack_digest: String,
    manifest_sha256: String,
    entry_count: u64,
    total_bytes: u64,
    files: Vec<PackFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    path: String,
    sha256: String,
    bytes: u64,
    text: String,
}

pub fn load_pack(bytes: &[u8]) -> Result<BoundPack, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::pack())?;
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::pack()),
    };
    if json::contains_null(&value) {
        return Err(Fault::pack());
    }
    let artifact: Artifact = serde_json::from_value(value).map_err(|_| Fault::pack())?;
    if artifact.protocol != PACK_PROTOCOL || artifact.compiler != PACK_COMPILER {
        return Err(Fault::pack());
    }
    if artifact.files.len() > FILE_MAX {
        return Err(Fault::pack());
    }
    if u64::try_from(artifact.files.len()).ok() != Some(artifact.entry_count) {
        return Err(Fault::pack());
    }
    let mut files = artifact.files;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut seen = Vec::new();
    let mut total = 0_u64;
    let mut preimage = Vec::new();
    let mut public = Vec::new();
    let mut manifest_text = None;
    for file in &files {
        if !path_ok(&file.path) || seen.iter().any(|path| path == &file.path) {
            return Err(Fault::pack());
        }
        seen.push(file.path.clone());
        if !hex64(&file.sha256) {
            return Err(Fault::pack());
        }
        if u64::try_from(file.text.len()).ok() != Some(file.bytes) {
            return Err(Fault::pack());
        }
        if sha256_hex(file.text.as_bytes()) != file.sha256 {
            return Err(Fault::pack());
        }
        if leaked(&file.text) {
            return Err(Fault::pack());
        }
        total = total.checked_add(file.bytes).ok_or_else(Fault::pack)?;
        let mut object = Map::new();
        object.insert(
            "bytes".to_string(),
            Value::Number(file.bytes.into()),
        );
        object.insert("path".to_string(), Value::String(file.path.clone()));
        object.insert("sha256".to_string(), Value::String(file.sha256.clone()));
        preimage.push(Value::Object(object));
        if file.path == "manifest.json" {
            manifest_text = Some(file.text.clone());
        }
        public.push(PublicFile {
            path: file.path.clone(),
            text: file.text.clone(),
        });
    }
    if total != artifact.total_bytes {
        return Err(Fault::pack());
    }
    let digest = canonical_sha256(&Value::Array(preimage));
    if digest != artifact.pack_digest {
        return Err(Fault::pack());
    }
    let manifest = manifest_text.ok_or_else(Fault::pack)?;
    let manifest_sha = sha256_hex(manifest.as_bytes());
    if manifest_sha != artifact.manifest_sha256 {
        return Err(Fault::pack());
    }
    let bound = manifest_fields(&manifest)?;
    Ok(BoundPack {
        files: public,
        pack_digest: digest,
        manifest_sha256: manifest_sha,
        project: bound.0,
        role: bound.1,
        policy_id: bound.2,
        policy_version: bound.3,
        knowledge_id: bound.4,
        knowledge_version: bound.5,
    })
}

fn manifest_fields(text: &str) -> Result<(String, String, String, String, String, String), Fault> {
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::pack()),
    };
    let Some(map) = value.as_object() else {
        return Err(Fault::pack());
    };
    if map.get("protocol").and_then(Value::as_str) != Some(MANIFEST_PROTOCOL) {
        return Err(Fault::pack());
    }
    let project = required_string(map, "project")?;
    let role = required_string(map, "role")?;
    let policy = map.get("policy").and_then(Value::as_object).ok_or_else(Fault::pack)?;
    let knowledge = map
        .get("knowledge")
        .and_then(Value::as_object)
        .ok_or_else(Fault::pack)?;
    let policy_id = required_string(policy, "id")?;
    let policy_version = required_string(policy, "version")?;
    let knowledge_id = required_string(knowledge, "id")?;
    let knowledge_version = required_string(knowledge, "version")?;
    Ok((
        project,
        role,
        policy_id,
        policy_version,
        knowledge_id,
        knowledge_version,
    ))
}

fn required_string(map: &Map<String, Value>, key: &str) -> Result<String, Fault> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(Fault::pack)
}

pub fn path_ok(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains(':') || path.contains("//")
    {
        return false;
    }
    path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn leaked(text: &str) -> bool {
    text.contains("secret://")
        || text.contains("postgres://")
        || text.contains("mysql://")
        || text.contains("password=")
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
