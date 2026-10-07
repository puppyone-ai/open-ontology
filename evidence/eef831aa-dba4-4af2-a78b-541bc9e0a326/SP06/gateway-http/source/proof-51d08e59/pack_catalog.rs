//! In-memory role-pack catalog. The request path is not a filesystem path.
//!
//! Artifacts follow `oo.builder.role-pack/v1`. This module checks the stored
//! manifest hash and file hashes. It does not re-run the compiler digest.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const ROLE_PACK_PROTOCOL: &str = "oo.builder.role-pack/v1";
pub const MANIFEST_PROTOCOL: &str = "oo.builder.role-pack-manifest/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackCatalog {
    packs: Vec<LoadedPack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LoadedPack {
    project: String,
    role: String,
    policy_version: String,
    knowledge_version: String,
    pack_digest: String,
    files: Vec<PackFile>,
    columns: Vec<PackColumn>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackColumn {
    pub table: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackMetadata {
    pub project: String,
    pub role: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub pack_digest: String,
    pub files: Vec<PackFile>,
    pub columns: Vec<PackColumn>,
}

#[derive(Debug)]
pub struct PackError {
    pub reason: &'static str,
}

impl PackCatalog {
    pub fn empty() -> Self {
        Self { packs: Vec::new() }
    }

    pub fn load_dir(dir: &Path) -> Result<Self, PackError> {
        let entries = fs::read_dir(dir).map_err(|_| PackError { reason: "pack_dir" })?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| PackError { reason: "pack_dir" })?;
            let file_type = entry.file_type().map_err(|_| PackError { reason: "pack_dir" })?;
            if file_type.is_symlink() {
                return Err(PackError { reason: "pack_symlink" });
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or(PackError { reason: "pack_name" })?;
            if !artifact_name(name) {
                return Err(PackError { reason: "pack_name" });
            }
            if !file_type.is_file() {
                return Err(PackError { reason: "pack_name" });
            }
            names.push(name.to_owned());
        }
        names.sort();
        let mut packs = Vec::new();
        for name in names {
            let text = fs::read_to_string(dir.join(&name)).map_err(|_| PackError { reason: "pack_read" })?;
            packs.push(parse_artifact(&text)?);
        }
        let mut seen = Vec::new();
        for pack in &packs {
            let key = (
                pack.project.as_str(),
                pack.role.as_str(),
                pack.policy_version.as_str(),
                pack.knowledge_version.as_str(),
            );
            if seen.contains(&key) {
                return Err(PackError { reason: "pack_duplicate" });
            }
            seen.push(key);
        }
        Ok(Self { packs })
    }

    pub fn find(
        &self,
        project: &str,
        role: &str,
        policy_version: &str,
        knowledge_version: &str,
    ) -> Option<PackMetadata> {
        self.packs.iter().find(|pack| {
            pack.project == project
                && pack.role == role
                && pack.policy_version == policy_version
                && pack.knowledge_version == knowledge_version
        }).map(|pack| PackMetadata {
            project: pack.project.clone(),
            role: pack.role.clone(),
            policy_version: pack.policy_version.clone(),
            knowledge_version: pack.knowledge_version.clone(),
            pack_digest: pack.pack_digest.clone(),
            files: pack.files.clone(),
            columns: pack.columns.clone(),
        })
    }
}

pub fn fixture_artifact(
    project: &str,
    role: &str,
    policy_version: &str,
    knowledge_version: &str,
    table: &str,
    column: &str,
) -> Result<String, PackError> {
    let table_path = format!("tables/{table}.md");
    let table_text = format!(
        "| column | label | type | semantic | pii | notes |\n| --- | --- | --- | --- | --- | --- |\n| `{column}` | visible | text | none | unknown | |\n"
    );
    let manifest = serde_json::json!({
        "knowledge": {"id": "knowledge", "version": knowledge_version},
        "policy": {"id": "policy", "version": policy_version},
        "project": project,
        "protocol": MANIFEST_PROTOCOL,
        "role": role,
    });
    let manifest_text = serde_json::to_string(&manifest).map_err(|_| PackError { reason: "pack_json" })?;
    let manifest_sha = sha256_hex(manifest_text.as_bytes());
    let table_sha = sha256_hex(table_text.as_bytes());
    let artifact = serde_json::json!({
        "files": [
            {
                "bytes": manifest_text.len(),
                "path": "manifest.json",
                "sha256": manifest_sha,
                "text": manifest_text,
            },
            {
                "bytes": table_text.len(),
                "path": table_path,
                "sha256": table_sha,
                "text": table_text,
            }
        ],
        "manifest_sha256": manifest_sha,
        "pack_digest": manifest_sha,
        "protocol": ROLE_PACK_PROTOCOL,
    });
    serde_json::to_string(&artifact).map_err(|_| PackError { reason: "pack_json" })
}

fn parse_artifact(text: &str) -> Result<LoadedPack, PackError> {
    let root: Value = serde_json::from_str(text).map_err(|_| PackError { reason: "pack_json" })?;
    let object = root.as_object().ok_or(PackError { reason: "pack_json" })?;
    require_str(object, "protocol", ROLE_PACK_PROTOCOL)?;
    let digest = object.get("pack_digest").and_then(Value::as_str).ok_or(PackError { reason: "pack_digest" })?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(PackError { reason: "pack_digest" });
    }
    let declared_manifest = object
        .get("manifest_sha256")
        .and_then(Value::as_str)
        .ok_or(PackError { reason: "manifest_sha256" })?;
    let files_value = object.get("files").and_then(Value::as_array).ok_or(PackError { reason: "pack_files" })?;
    let mut files = Vec::new();
    let mut manifest_text = None;
    let mut columns = Vec::new();
    for file in files_value {
        let file = file.as_object().ok_or(PackError { reason: "pack_files" })?;
        let path = file.get("path").and_then(Value::as_str).ok_or(PackError { reason: "pack_path" })?;
        if !safe_pack_path(path) {
            return Err(PackError { reason: "pack_path" });
        }
        let body = file.get("text").and_then(Value::as_str).ok_or(PackError { reason: "pack_text" })?;
        let bytes = file.get("bytes").and_then(Value::as_u64).ok_or(PackError { reason: "pack_bytes" })?;
        if bytes != body.len() as u64 {
            return Err(PackError { reason: "pack_bytes" });
        }
        let sha = file.get("sha256").and_then(Value::as_str).ok_or(PackError { reason: "pack_sha" })?;
        if sha != sha256_hex(body.as_bytes()) {
            return Err(PackError { reason: "pack_sha" });
        }
        if path == "manifest.json" {
            manifest_text = Some(body.to_owned());
        }
        if let Some(table) = path.strip_prefix("tables/").and_then(|rest| rest.strip_suffix(".md"))
            && !table.is_empty()
            && !table.contains('/')
        {
            columns.extend(column_names(table, body));
        }
        files.push(PackFile {
            path: path.to_owned(),
            bytes,
            sha256: sha.to_owned(),
        });
    }
    let manifest_text = manifest_text.ok_or(PackError { reason: "manifest" })?;
    if sha256_hex(manifest_text.as_bytes()) != declared_manifest {
        return Err(PackError { reason: "manifest_sha256" });
    }
    let manifest: Value = serde_json::from_str(&manifest_text).map_err(|_| PackError { reason: "manifest" })?;
    let manifest = manifest.as_object().ok_or(PackError { reason: "manifest" })?;
    require_str(manifest, "protocol", MANIFEST_PROTOCOL)?;
    let project = required_text(manifest, "project")?;
    let role = required_text(manifest, "role")?;
    let policy_version = manifest
        .get("policy")
        .and_then(Value::as_object)
        .and_then(|policy| policy.get("version"))
        .and_then(Value::as_str)
        .ok_or(PackError { reason: "policy" })?
        .to_owned();
    let knowledge_version = manifest
        .get("knowledge")
        .and_then(Value::as_object)
        .and_then(|knowledge| knowledge.get("version"))
        .and_then(Value::as_str)
        .ok_or(PackError { reason: "knowledge" })?
        .to_owned();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    columns.sort_by(|left, right| (left.table.as_str(), left.name.as_str()).cmp(&(right.table.as_str(), right.name.as_str())));
    Ok(LoadedPack {
        project,
        role,
        policy_version,
        knowledge_version,
        pack_digest: digest.to_owned(),
        files,
        columns,
    })
}

fn column_names(table: &str, markdown: &str) -> Vec<PackColumn> {
    let mut columns = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let Some(start) = trimmed.find('`') else {
            continue;
        };
        let rest = &trimmed[start + 1..];
        let Some(end) = rest.find('`') else {
            continue;
        };
        let name = &rest[..end];
        if name.is_empty() || name.contains(['`', '|', '\n']) {
            continue;
        }
        columns.push(PackColumn {
            table: table.to_owned(),
            name: name.to_owned(),
        });
    }
    columns
}

fn require_str(object: &Map<String, Value>, key: &str, expected: &str) -> Result<(), PackError> {
    let got = object.get(key).and_then(Value::as_str).ok_or(PackError { reason: "pack_protocol" })?;
    if got == expected {
        Ok(())
    } else {
        Err(PackError { reason: "pack_protocol" })
    }
}

fn required_text(object: &Map<String, Value>, key: &str) -> Result<String, PackError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(PackError { reason: "pack_field" })
}

fn artifact_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".json") else {
        return false;
    };
    let mut bytes = stem.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    stem.len() <= 63
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-')
}

fn safe_pack_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> std::path::PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let path = std::env::temp_dir().join(format!("oo-pack-{nanos}"));
        fs::create_dir(&path).expect("scratch");
        path
    }

    #[test]
    fn fixture_round_trip_keeps_only_the_named_column() {
        let dir = scratch();
        let text = fixture_artifact("depot", "oo_r_picker", "pv1", "kv1", "orders", "picker_note").expect("artifact");
        fs::write(dir.join("picker.json"), &text).expect("write");
        let catalog = PackCatalog::load_dir(&dir).expect("load");
        let meta = catalog.find("depot", "oo_r_picker", "pv1", "kv1").expect("pack");
        assert_eq!(meta.columns.len(), 1);
        assert_eq!(meta.columns[0].name, "picker_note");
        assert!(meta.files.iter().all(|file| file.path != "orders"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_identity_refuses_startup() {
        let dir = scratch();
        let text = fixture_artifact("depot", "oo_r_picker", "pv1", "kv1", "orders", "picker_note").expect("artifact");
        fs::write(dir.join("a.json"), &text).expect("write");
        fs::write(dir.join("b.json"), &text).expect("write");
        let error = PackCatalog::load_dir(&dir).expect_err("duplicate");
        assert_eq!(error.reason, "pack_duplicate");
        let _ = fs::remove_dir_all(&dir);
    }
}
