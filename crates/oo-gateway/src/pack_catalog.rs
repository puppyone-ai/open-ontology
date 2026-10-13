//! In-memory role-pack catalog. The request path is not a filesystem path.
//!
//! A loaded file is the sealed `RolePack.artifact` from `oo-builder`. This
//! module recomputes that compiler's pack digest. It does not run the compiler.
//!
//! `canonical_json` and `push_string` are a byte copy of
//! `crates/oo-builder/src/canonical.rs` at
//! `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`. `sha256_hex` is the same
//! SHA-256 lowercase hex as `oo_core::api::payload_sha256`. The preimage is
//! the one `seal` hashes in `crates/oo-builder/src/role_pack.rs`: the
//! canonical JSON array of `{bytes, path, sha256}` for every file, including
//! `manifest.json`, in path order.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const ROLE_PACK_PROTOCOL: &str = "oo.builder.role-pack/v1";
pub const ROLE_PACK_COMPILER: &str = "oo-builder/role-pack/v1";
pub const MANIFEST_PROTOCOL: &str = "oo.builder.role-pack-manifest/v1";

const MAX_FILE_BYTES: u64 = 262_144;
const MAX_TOTAL_BYTES: u64 = 1_048_576;
const MAX_ENTRIES: u64 = 128;
const MAX_PATH_BYTES: usize = 256;

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
    texts: Vec<(String, String)>,
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
            let file_type = entry
                .file_type()
                .map_err(|_| PackError { reason: "pack_dir" })?;
            if file_type.is_symlink() {
                return Err(PackError {
                    reason: "pack_symlink",
                });
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or(PackError {
                reason: "pack_name",
            })?;
            if !artifact_name(name) {
                return Err(PackError {
                    reason: "pack_name",
                });
            }
            if !file_type.is_file() {
                return Err(PackError {
                    reason: "pack_name",
                });
            }
            names.push(name.to_owned());
        }
        names.sort();
        let mut packs = Vec::new();
        for name in names {
            let text = fs::read_to_string(dir.join(&name)).map_err(|_| PackError {
                reason: "pack_read",
            })?;
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
                return Err(PackError {
                    reason: "pack_duplicate",
                });
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
        self.packs
            .iter()
            .find(|pack| {
                pack.project == project
                    && pack.role == role
                    && pack.policy_version == policy_version
                    && pack.knowledge_version == knowledge_version
            })
            .map(|pack| PackMetadata {
                project: pack.project.clone(),
                role: pack.role.clone(),
                policy_version: pack.policy_version.clone(),
                knowledge_version: pack.knowledge_version.clone(),
                pack_digest: pack.pack_digest.clone(),
                files: pack.files.clone(),
                columns: pack.columns.clone(),
            })
    }

    /// File body for the token's exact project, role, policy, and knowledge version.
    pub fn file_text(
        &self,
        project: &str,
        role: &str,
        policy_version: &str,
        knowledge_version: &str,
        path: &str,
    ) -> Option<PackBody> {
        let pack = self.packs.iter().find(|pack| {
            pack.project == project
                && pack.role == role
                && pack.policy_version == policy_version
                && pack.knowledge_version == knowledge_version
        })?;
        let (stored_path, text) = pack.texts.iter().find(|(stored, _)| stored == path)?;
        let file = pack.files.iter().find(|file| file.path == *stored_path)?;
        Some(PackBody {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            bytes: file.bytes,
            text: text.clone(),
        })
    }

    /// Bounded literal search over one identity. The needle is not a path.
    pub fn search_text(
        &self,
        project: &str,
        role: &str,
        policy_version: &str,
        knowledge_version: &str,
        needle: &str,
    ) -> Vec<PackHit> {
        if needle.is_empty() || needle.len() > 200 || needle.contains('\0') {
            return Vec::new();
        }
        let Some(pack) = self.packs.iter().find(|pack| {
            pack.project == project
                && pack.role == role
                && pack.policy_version == policy_version
                && pack.knowledge_version == knowledge_version
        }) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        for (path, text) in &pack.texts {
            let Some(index) = text.find(needle) else {
                continue;
            };
            let start = index.saturating_sub(40);
            let end = (index + needle.len() + 40).min(text.len());
            let start = floor_char(text, start);
            let end = floor_char(text, end);
            let file = pack.files.iter().find(|file| file.path == *path);
            hits.push(PackHit {
                path: path.clone(),
                sha256: file.map(|file| file.sha256.clone()).unwrap_or_default(),
                excerpt: text[start..end].to_owned(),
            });
            if hits.len() == 20 {
                break;
            }
        }
        hits
    }
}

/// One sealed file body. The digest is the pack file digest, not a new hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackBody {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub text: String,
}

/// One search hit. The excerpt is a bounded window, not the whole file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackHit {
    pub path: String,
    pub sha256: String,
    pub excerpt: String,
}

fn floor_char(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut at = index;
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The operator helper used to invent a digest. A file without `compiler` is
/// not a sealed RolePack, and this function does not manufacture one.
pub fn fixture_artifact(
    project: &str,
    role: &str,
    policy_version: &str,
    knowledge_version: &str,
    table: &str,
    column: &str,
) -> Result<String, PackError> {
    let _ = (
        project,
        role,
        policy_version,
        knowledge_version,
        table,
        column,
    );
    Err(PackError {
        reason: "legacy_unsupported",
    })
}

fn parse_artifact(text: &str) -> Result<LoadedPack, PackError> {
    let root: Value = serde_json::from_str(text).map_err(|_| PackError {
        reason: "pack_json",
    })?;
    let object = root.as_object().ok_or(PackError {
        reason: "pack_json",
    })?;
    match object.get("compiler") {
        None => {
            return Err(PackError {
                reason: "legacy_unsupported",
            });
        }
        Some(value) if value.as_str() != Some(ROLE_PACK_COMPILER) => {
            return Err(PackError { reason: "compiler" });
        }
        Some(_) => {}
    }
    require_exact_keys(
        object,
        &[
            "compiler",
            "entry_count",
            "files",
            "manifest_sha256",
            "pack_digest",
            "protocol",
            "total_bytes",
        ],
    )?;
    require_str(object, "protocol", ROLE_PACK_PROTOCOL)?;
    let declared_digest = hex64(object, "pack_digest")?;
    let declared_manifest = hex64(object, "manifest_sha256")?;
    let declared_entries = u64_field(object, "entry_count", "budget")?;
    let declared_total = u64_field(object, "total_bytes", "budget")?;
    let files_value = object
        .get("files")
        .and_then(Value::as_array)
        .ok_or(PackError {
            reason: "pack_files",
        })?;
    if files_value.len() as u64 > MAX_ENTRIES {
        return Err(PackError { reason: "budget" });
    }
    let mut stored = Vec::new();
    let mut seen_paths = Vec::new();
    let mut columns = Vec::new();
    let mut total = 0_u64;
    for file in files_value {
        let file = file.as_object().ok_or(PackError {
            reason: "pack_files",
        })?;
        require_exact_keys(file, &["bytes", "path", "sha256", "text"])?;
        let path = file.get("path").and_then(Value::as_str).ok_or(PackError {
            reason: "pack_path",
        })?;
        if !safe_pack_path(path) {
            return Err(PackError {
                reason: "pack_path",
            });
        }
        if seen_paths.iter().any(|seen| seen == path) {
            return Err(PackError {
                reason: "path_duplicate",
            });
        }
        seen_paths.push(path.to_owned());
        let body = file.get("text").and_then(Value::as_str).ok_or(PackError {
            reason: "pack_text",
        })?;
        let bytes = u64_field(file, "bytes", "pack_bytes")?;
        if bytes != body.len() as u64 {
            return Err(PackError {
                reason: "pack_bytes",
            });
        }
        if bytes > MAX_FILE_BYTES {
            return Err(PackError { reason: "budget" });
        }
        total = total
            .checked_add(bytes)
            .ok_or(PackError { reason: "budget" })?;
        let sha = file
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or(PackError {
                reason: "source_digest",
            })?;
        if !is_hex64(sha) || sha != sha256_hex(body.as_bytes()) {
            return Err(PackError {
                reason: "source_digest",
            });
        }
        let table_name = path
            .strip_prefix("tables/")
            .and_then(|rest| rest.strip_suffix(".md"))
            .filter(|table| !table.is_empty() && !table.contains('/'));
        if table_name.is_some() && unknown_pii(body) {
            return Err(PackError {
                reason: "role_mismatch",
            });
        }
        if let Some(table) = table_name {
            columns.extend(column_names(table, body));
        }
        stored.push(StoredFile {
            path: path.to_owned(),
            bytes,
            sha256: sha.to_owned(),
            text: body.to_owned(),
        });
    }
    if declared_entries != stored.len() as u64 || declared_total != total || total > MAX_TOTAL_BYTES
    {
        return Err(PackError { reason: "budget" });
    }
    let mut ordered = stored.clone();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    let preimage = Value::Array(
        ordered
            .iter()
            .map(|file| {
                serde_json::json!({
                    "bytes": file.bytes,
                    "path": file.path,
                    "sha256": file.sha256,
                })
            })
            .collect(),
    );
    let pack_digest = sha256_hex(canonical_json(&preimage).as_bytes());
    if pack_digest != declared_digest {
        return Err(PackError {
            reason: "pack_digest",
        });
    }
    let manifest_file = stored
        .iter()
        .find(|file| file.path == "manifest.json")
        .ok_or(PackError { reason: "manifest" })?;
    if sha256_hex(manifest_file.text.as_bytes()) != declared_manifest {
        return Err(PackError {
            reason: "source_digest",
        });
    }
    let manifest: Value =
        serde_json::from_str(&manifest_file.text).map_err(|_| PackError { reason: "manifest" })?;
    let manifest = manifest
        .as_object()
        .ok_or(PackError { reason: "manifest" })?;
    require_str(manifest, "protocol", MANIFEST_PROTOCOL)
        .map_err(|_| PackError { reason: "binding" })?;
    require_str(manifest, "compiler", ROLE_PACK_COMPILER)
        .map_err(|_| PackError { reason: "binding" })?;
    require_status(manifest)?;
    let project = slug(manifest, "project")?;
    let role = role_name(manifest)?;
    let policy = manifest
        .get("policy")
        .and_then(Value::as_object)
        .ok_or(PackError { reason: "binding" })?;
    let knowledge = manifest
        .get("knowledge")
        .and_then(Value::as_object)
        .ok_or(PackError { reason: "binding" })?;
    let policy_id = slug(policy, "id")?;
    let policy_version = version_token(policy, "version")?;
    let knowledge_id = slug(knowledge, "id")?;
    let knowledge_version = version_token(knowledge, "version")?;
    let _ = (policy_id, knowledge_id);
    let ontology_version = version_token(manifest, "ontology_version")?;
    let _catalog_version = version_token(manifest, "catalog_version")?;
    let listed = manifest
        .get("files")
        .and_then(Value::as_array)
        .ok_or(PackError {
            reason: "source_digest",
        })?;
    let mut expected: Vec<&StoredFile> = stored
        .iter()
        .filter(|file| file.path != "manifest.json")
        .collect();
    expected.sort_by(|left, right| left.path.cmp(&right.path));
    if listed.len() != expected.len() {
        return Err(PackError {
            reason: "source_digest",
        });
    }
    for (item, file) in listed.iter().zip(expected.iter()) {
        let item = item.as_object().ok_or(PackError {
            reason: "source_digest",
        })?;
        if item.len() != 3
            || ["bytes", "path", "sha256"]
                .iter()
                .any(|key| !item.contains_key(*key))
        {
            return Err(PackError {
                reason: "source_digest",
            });
        }
        let path = item.get("path").and_then(Value::as_str).ok_or(PackError {
            reason: "source_digest",
        })?;
        let bytes = u64_field(item, "bytes", "source_digest")?;
        let sha = item
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or(PackError {
                reason: "source_digest",
            })?;
        if path != file.path || bytes != file.bytes || sha != file.sha256 {
            return Err(PackError {
                reason: "source_digest",
            });
        }
    }
    let readme = stored
        .iter()
        .find(|file| file.path == "README.md")
        .ok_or(PackError { reason: "binding" })?;
    require_line(&readme.text, "compiler", ROLE_PACK_COMPILER, "binding")?;
    require_line(&readme.text, "project", &project, "binding")?;
    require_line(
        &readme.text,
        "ontology_version",
        &ontology_version,
        "binding",
    )?;
    require_line(&readme.text, "role", &role, "role_mismatch")?;
    let tokens = role_tokens(&readme.text);
    if tokens != vec![role.clone()] {
        return Err(PackError {
            reason: "role_mismatch",
        });
    }
    let mut files = Vec::new();
    let mut texts = Vec::new();
    for file in stored {
        texts.push((file.path.clone(), file.text));
        files.push(PackFile {
            path: file.path,
            bytes: file.bytes,
            sha256: file.sha256,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    texts.sort_by(|left, right| left.0.cmp(&right.0));
    columns.sort_by(|left, right| {
        (left.table.as_str(), left.name.as_str()).cmp(&(right.table.as_str(), right.name.as_str()))
    });
    Ok(LoadedPack {
        project,
        role,
        policy_version,
        knowledge_version,
        pack_digest,
        files,
        columns,
        texts,
    })
}

#[derive(Clone)]
struct StoredFile {
    path: String,
    bytes: u64,
    sha256: String,
    text: String,
}

fn require_status(manifest: &Map<String, Value>) -> Result<(), PackError> {
    let pairs = [
        ("authentication", "not_claimed"),
        ("authorization", "not_claimed"),
        ("publication", "not_claimed"),
        ("mount", "not_performed"),
        ("samples", "omitted"),
    ];
    for (key, expected) in pairs {
        if manifest.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(PackError { reason: "binding" });
        }
    }
    if manifest.get("row_security_reproven") != Some(&Value::Bool(false)) {
        return Err(PackError { reason: "binding" });
    }
    Ok(())
}

fn require_line(
    text: &str,
    label: &str,
    expected: &str,
    reason: &'static str,
) -> Result<(), PackError> {
    let prefix = format!("{label}: `");
    let mut found = 0_u32;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        let Some(value) = rest.strip_suffix('`') else {
            return Err(PackError { reason });
        };
        if value != expected {
            return Err(PackError { reason });
        }
        found += 1;
    }
    if found == 1 {
        Ok(())
    } else {
        Err(PackError { reason })
    }
}

fn role_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index + 5 < bytes.len() {
        if &bytes[index..index + 5] == b"oo_r_" {
            let start = index;
            index += 5;
            if index >= bytes.len() || !bytes[index].is_ascii_lowercase() {
                continue;
            }
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_lowercase()
                    || bytes[index].is_ascii_digit()
                    || bytes[index] == b'_')
            {
                index += 1;
            }
            out.push(text[start..index].to_owned());
        } else {
            index += 1;
        }
    }
    out
}

fn unknown_pii(markdown: &str) -> bool {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || !trimmed.contains('`') {
            continue;
        }
        let cells: Vec<&str> = trimmed.split('|').collect();
        if cells.len() >= 7 && cells[5].trim() == "unknown" {
            return true;
        }
    }
    false
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

fn require_exact_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), PackError> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(PackError {
            reason: "pack_json",
        });
    }
    Ok(())
}

fn require_str(object: &Map<String, Value>, key: &str, expected: &str) -> Result<(), PackError> {
    let got = object.get(key).and_then(Value::as_str).ok_or(PackError {
        reason: "pack_protocol",
    })?;
    if got == expected {
        Ok(())
    } else {
        Err(PackError {
            reason: "pack_protocol",
        })
    }
}

fn hex64(object: &Map<String, Value>, key: &str) -> Result<String, PackError> {
    let value = object.get(key).and_then(Value::as_str).ok_or(PackError {
        reason: "pack_digest",
    })?;
    if is_hex64(value) {
        Ok(value.to_owned())
    } else {
        Err(PackError {
            reason: "pack_digest",
        })
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn u64_field(
    object: &Map<String, Value>,
    key: &str,
    reason: &'static str,
) -> Result<u64, PackError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(PackError { reason })
}

fn slug(object: &Map<String, Value>, key: &str) -> Result<String, PackError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(PackError { reason: "binding" })?;
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        });
    if ok {
        Ok(value.to_owned())
    } else {
        Err(PackError { reason: "binding" })
    }
}

fn role_name(object: &Map<String, Value>) -> Result<String, PackError> {
    let value = object
        .get("role")
        .and_then(Value::as_str)
        .ok_or(PackError { reason: "binding" })?;
    let Some(rest) = value.strip_prefix("oo_r_") else {
        return Err(PackError { reason: "binding" });
    };
    let bytes = rest.as_bytes();
    let ok = !bytes.is_empty()
        && value.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
    if ok {
        Ok(value.to_owned())
    } else {
        Err(PackError { reason: "binding" })
    }
}

fn version_token(object: &Map<String, Value>, key: &str) -> Result<String, PackError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(PackError { reason: "binding" })?;
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-'));
    if ok {
        Ok(value.to_owned())
    } else {
        Err(PackError { reason: "binding" })
    }
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
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

fn safe_pack_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.as_bytes().contains(&0)
        || path.contains('\\')
        || path.contains(':')
        || path.starts_with('/')
        || path.starts_with("./")
        || path.ends_with('/')
        || path.contains("//")
    {
        return false;
    }
    if path.chars().any(|ch| {
        let code = u32::from(ch);
        matches!(code, 0x00..=0x1F | 0x7F)
    }) {
        return false;
    }
    path.split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
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
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("oo-pack-{nanos}"));
        fs::create_dir(&path).expect("scratch");
        path
    }

    fn readme(project: &str, role: &str) -> String {
        format!(
            "# {project}\n\nontology_version: `ont-v1`\ncompiler: `{ROLE_PACK_COMPILER}`\nproject: `{project}`\nrole: `{role}`\n"
        )
    }

    fn table(column: &str, pii: &str) -> String {
        format!(
            "| column | label | type | semantic | pii | notes |\n| --- | --- | --- | --- | --- | --- |\n| `{column}` | visible | text | none | {pii} | |\n"
        )
    }

    fn manifest_body(
        project: &str,
        role: &str,
        policy_version: &str,
        knowledge_version: &str,
        files: &[(&str, &str)],
    ) -> String {
        let mut listed = Vec::new();
        for (path, text) in files {
            listed.push(serde_json::json!({
                "bytes": text.len() as u64,
                "path": path,
                "sha256": sha256_hex(text.as_bytes()),
            }));
        }
        let mut manifest = Map::new();
        manifest.insert(
            "authentication".to_owned(),
            Value::String("not_claimed".to_owned()),
        );
        manifest.insert(
            "authorization".to_owned(),
            Value::String("not_claimed".to_owned()),
        );
        manifest.insert(
            "catalog_version".to_owned(),
            Value::String("cat-v1".to_owned()),
        );
        manifest.insert(
            "compiler".to_owned(),
            Value::String(ROLE_PACK_COMPILER.to_owned()),
        );
        manifest.insert("files".to_owned(), Value::Array(listed));
        let mut knowledge = Map::new();
        knowledge.insert("id".to_owned(), Value::String("know_depot".to_owned()));
        knowledge.insert(
            "version".to_owned(),
            Value::String(knowledge_version.to_owned()),
        );
        manifest.insert("knowledge".to_owned(), Value::Object(knowledge));
        manifest.insert(
            "mount".to_owned(),
            Value::String("not_performed".to_owned()),
        );
        manifest.insert(
            "ontology_version".to_owned(),
            Value::String("ont-v1".to_owned()),
        );
        let mut policy = Map::new();
        policy.insert("id".to_owned(), Value::String("pol_depot".to_owned()));
        policy.insert(
            "version".to_owned(),
            Value::String(policy_version.to_owned()),
        );
        manifest.insert("policy".to_owned(), Value::Object(policy));
        manifest.insert("project".to_owned(), Value::String(project.to_owned()));
        manifest.insert(
            "protocol".to_owned(),
            Value::String(MANIFEST_PROTOCOL.to_owned()),
        );
        manifest.insert(
            "publication".to_owned(),
            Value::String("not_claimed".to_owned()),
        );
        manifest.insert("role".to_owned(), Value::String(role.to_owned()));
        manifest.insert("row_security_reproven".to_owned(), Value::Bool(false));
        manifest.insert("samples".to_owned(), Value::String("omitted".to_owned()));
        canonical_json(&Value::Object(manifest))
    }

    fn seal(
        project: &str,
        role: &str,
        policy_version: &str,
        knowledge_version: &str,
        column: &str,
        pii: &str,
    ) -> String {
        let readme = readme(project, role);
        let table = table(column, pii);
        let mut pairs = vec![
            ("README.md", readme.as_str()),
            ("tables/shop.orders.md", table.as_str()),
        ];
        pairs.sort_by(|left, right| left.0.cmp(right.0));
        let manifest = manifest_body(project, role, policy_version, knowledge_version, &pairs);
        let mut files = vec![
            ("README.md".to_owned(), readme),
            ("manifest.json".to_owned(), manifest),
            ("tables/shop.orders.md".to_owned(), table),
        ];
        files.sort_by(|left, right| left.0.cmp(&right.0));
        resign(&files, ROLE_PACK_COMPILER)
    }

    fn resign(files: &[(String, String)], compiler: &str) -> String {
        let mut records = Vec::new();
        let mut total = 0_u64;
        for (path, text) in files {
            let bytes = text.len() as u64;
            total += bytes;
            records.push(serde_json::json!({
                "bytes": bytes,
                "path": path,
                "sha256": sha256_hex(text.as_bytes()),
                "text": text,
            }));
        }
        let mut ordered = records.clone();
        ordered.sort_by(|left, right| {
            left["path"]
                .as_str()
                .unwrap()
                .cmp(right["path"].as_str().unwrap())
        });
        let preimage = Value::Array(
            ordered
                .iter()
                .map(|file| {
                    serde_json::json!({
                        "bytes": file["bytes"].as_u64().unwrap(),
                        "path": file["path"].as_str().unwrap(),
                        "sha256": file["sha256"].as_str().unwrap(),
                    })
                })
                .collect(),
        );
        let manifest_text = files
            .iter()
            .find(|file| file.0 == "manifest.json")
            .expect("manifest")
            .1
            .as_str();
        let artifact = serde_json::json!({
            "compiler": compiler,
            "entry_count": files.len() as u64,
            "files": records,
            "manifest_sha256": sha256_hex(manifest_text.as_bytes()),
            "pack_digest": sha256_hex(canonical_json(&preimage).as_bytes()),
            "protocol": ROLE_PACK_PROTOCOL,
            "total_bytes": total,
        });
        serde_json::to_string(&artifact).expect("artifact")
    }

    fn write_pack(dir: &Path, name: &str, text: &str) {
        fs::write(dir.join(name), text).expect("write");
    }

    #[test]
    fn operator_fixture_is_legacy_unsupported() {
        let error = fixture_artifact(
            "depot",
            "oo_r_picker",
            "pv1",
            "kv1",
            "orders",
            "picker_note",
        )
        .expect_err("legacy");
        assert_eq!(error.reason, "legacy_unsupported");
        let dir = scratch();
        write_pack(
            &dir,
            "old.json",
            "{\"files\":[],\"manifest_sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"pack_digest\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",\"protocol\":\"oo.builder.role-pack/v1\"}",
        );
        let error = PackCatalog::load_dir(&dir).expect_err("missing compiler");
        assert_eq!(error.reason, "legacy_unsupported");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sealed_shape_keeps_the_named_column_and_recomputed_digest() {
        let dir = scratch();
        let text = seal("depot", "oo_r_picker", "pv1", "kv1", "picker_note", "none");
        let parsed: Value = serde_json::from_str(&text).expect("json");
        write_pack(&dir, "picker.json", &text);
        let catalog = PackCatalog::load_dir(&dir).expect("load");
        let meta = catalog
            .find("depot", "oo_r_picker", "pv1", "kv1")
            .expect("pack");
        assert_eq!(meta.pack_digest, parsed["pack_digest"].as_str().unwrap());
        assert_eq!(meta.columns.len(), 1);
        assert_eq!(meta.columns[0].name, "picker_note");
        assert_eq!(meta.columns[0].table, "shop.orders");
        assert!(meta.files.iter().any(|file| file.path == "manifest.json"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_digest_tamper_is_refused() {
        let dir = scratch();
        let mut parsed: Value = serde_json::from_str(&seal(
            "depot",
            "oo_r_picker",
            "pv1",
            "kv1",
            "picker_note",
            "none",
        ))
        .unwrap();
        parsed["pack_digest"] = Value::String("ab".repeat(32));
        write_pack(
            &dir,
            "picker.json",
            &serde_json::to_string(&parsed).unwrap(),
        );
        let error = PackCatalog::load_dir(&dir).expect_err("tamper");
        assert_eq!(error.reason, "pack_digest");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_digest_path_budget_and_role_failures() {
        let dir = scratch();
        let good = seal("depot", "oo_r_picker", "pv1", "kv1", "picker_note", "none");
        let mut parsed: Value = serde_json::from_str(&good).unwrap();
        parsed["files"][0]["sha256"] = Value::String("0".repeat(64));
        write_pack(
            &dir,
            "picker.json",
            &serde_json::to_string(&parsed).unwrap(),
        );
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("sha").reason,
            "source_digest"
        );

        let mut parsed: Value = serde_json::from_str(&good).unwrap();
        let first = parsed["files"][0].clone();
        parsed["files"].as_array_mut().unwrap().push(first);
        parsed["entry_count"] = Value::from(parsed["files"].as_array().unwrap().len() as u64);
        write_pack(
            &dir,
            "picker.json",
            &serde_json::to_string(&parsed).unwrap(),
        );
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("path").reason,
            "path_duplicate"
        );

        let mut parsed: Value = serde_json::from_str(&good).unwrap();
        parsed["total_bytes"] = Value::from(1_u64);
        write_pack(
            &dir,
            "picker.json",
            &serde_json::to_string(&parsed).unwrap(),
        );
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("budget").reason,
            "budget"
        );

        let mut parsed: Value = serde_json::from_str(&good).unwrap();
        parsed["compiler"] = Value::String("other".to_owned());
        write_pack(
            &dir,
            "picker.json",
            &serde_json::to_string(&parsed).unwrap(),
        );
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("compiler").reason,
            "compiler"
        );

        let readme_body = readme("depot", "oo_r_auditor");
        let table_body = table("picker_note", "none");
        let pairs = [
            ("README.md", readme_body.as_str()),
            ("tables/shop.orders.md", table_body.as_str()),
        ];
        let manifest = manifest_body("depot", "oo_r_picker", "pv1", "kv1", &pairs);
        let files = vec![
            ("README.md".to_owned(), readme_body),
            ("manifest.json".to_owned(), manifest),
            ("tables/shop.orders.md".to_owned(), table_body),
        ];
        write_pack(&dir, "picker.json", &resign(&files, ROLE_PACK_COMPILER));
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("role").reason,
            "role_mismatch"
        );

        let readme_body = readme("depot", "oo_r_picker");
        let table_body = table("picker_note", "unknown");
        let pairs = [
            ("README.md", readme_body.as_str()),
            ("tables/shop.orders.md", table_body.as_str()),
        ];
        let manifest = manifest_body("depot", "oo_r_picker", "pv1", "kv1", &pairs);
        let files = vec![
            ("README.md".to_owned(), readme_body),
            ("manifest.json".to_owned(), manifest),
            ("tables/shop.orders.md".to_owned(), table_body),
        ];
        write_pack(&dir, "picker.json", &resign(&files, ROLE_PACK_COMPILER));
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("hidden").reason,
            "role_mismatch"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_identity_refuses_startup() {
        let dir = scratch();
        let text = seal("depot", "oo_r_picker", "pv1", "kv1", "picker_note", "none");
        write_pack(&dir, "a.json", &text);
        write_pack(&dir, "b.json", &text);
        let error = PackCatalog::load_dir(&dir).expect_err("duplicate");
        assert_eq!(error.reason, "pack_duplicate");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_file_list_mismatch_is_source_digest() {
        let dir = scratch();
        let good = seal("depot", "oo_r_picker", "pv1", "kv1", "picker_note", "none");
        let parsed: Value = serde_json::from_str(&good).unwrap();
        let mut files = Vec::new();
        for file in parsed["files"].as_array().unwrap() {
            files.push((
                file["path"].as_str().unwrap().to_owned(),
                file["text"].as_str().unwrap().to_owned(),
            ));
        }
        let manifest_index = files
            .iter()
            .position(|file| file.0 == "manifest.json")
            .unwrap();
        let mut manifest: Value = serde_json::from_str(&files[manifest_index].1).unwrap();
        manifest["files"][0]["sha256"] = Value::String("0".repeat(64));
        files[manifest_index].1 = canonical_json(&manifest);
        write_pack(&dir, "picker.json", &resign(&files, ROLE_PACK_COMPILER));
        assert_eq!(
            PackCatalog::load_dir(&dir).expect_err("list").reason,
            "source_digest"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
