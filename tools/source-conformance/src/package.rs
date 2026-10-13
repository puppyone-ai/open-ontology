//! Evidence package oo-source-conformance/evidence-package/v1.
//! Hashes are checked against the named raw files. The matrix is not rewritten.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};

use serde_json::Value;

use crate::cases::canonical_capability;
use crate::jsonutil::{hex64, only_keys, require_bool, require_exit, require_object, require_str};
use crate::sha256_hex;

const MANIFEST_KEYS: &[&str] = &[
    "schema",
    "evidence_id",
    "target_id",
    "exact_version",
    "tested_versions",
    "variant",
    "driver_digest",
    "suite_version",
    "evidence_type",
    "deployment_scope",
    "identity",
    "cloud",
    "cases",
    "rejections",
    "reviewers",
];
const IDENTITY_KEYS: &[&str] = &["topology", "service_version"];
const CLOUD_KEYS: &[&str] = &["applies", "budget_exceeded", "cycle_status"];
const CASE_KEYS: &[&str] = &["id", "capability", "disposition", "raw_exit", "raw_sha256", "raw_path"];
const REJECTION_KEYS: &[&str] = &["capability", "code", "silent_fallback", "raw_exit", "raw_sha256", "raw_path"];
const REVIEWER_KEYS: &[&str] = &["role", "name"];

#[derive(Debug, Clone)]
pub struct Loaded {
    pub label: String,
    pub package: Package,
    pub files: BTreeMap<String, Vec<u8>>,
    /// Decoded duplicate of `exact_version`, `tested_versions`, `identity`, or `service_version`.
    pub duplicate_identity_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub evidence_id: String,
    pub target_id: String,
    pub exact_version: String,
    pub tested_versions: Vec<String>,
    pub variant: String,
    pub driver_digest: String,
    pub suite_version: String,
    pub evidence_type: String,
    pub deployment_scope: String,
    pub topology: String,
    pub service_version: String,
    pub cloud_applies: bool,
    pub budget_exceeded: bool,
    pub cycle_status: String,
    pub cases: Vec<CaseRow>,
    pub rejections: Vec<Rejection>,
    pub reviewers: Vec<Reviewer>,
}

#[derive(Debug, Clone)]
pub struct CaseRow {
    pub id: String,
    pub capability: String,
    pub disposition: String,
    pub raw_exit: u16,
    pub raw_sha256: String,
    pub raw_path: String,
}

#[derive(Debug, Clone)]
pub struct Rejection {
    pub capability: String,
    pub code: String,
    pub silent_fallback: bool,
    pub raw_exit: u16,
    pub raw_sha256: String,
    pub raw_path: String,
}

#[derive(Debug, Clone)]
pub struct Reviewer {
    pub role: String,
    pub name: String,
}

/// Evidence id of a supplied manifest, including one serde rejects or schema rejects.
pub fn cited_evidence_id(dir: &Path) -> Option<String> {
    let bytes = fs::read(dir.join("manifest.json")).ok()?;
    let text = if let Ok(document) = crate::jsonutil::parse_json_document(&bytes) {
        document.value.get("evidence_id").and_then(Value::as_str).map(str::to_owned)
    } else {
        crate::jsonutil::top_level_string(&bytes, "evidence_id")
    }?;
    if (8..=80).contains(&text.len()) && text.chars().all(id_char) {
        Some(text)
    } else {
        None
    }
}

pub fn load_dir(dir: &Path) -> Result<Loaded, String> {
    let manifest_path = dir.join("manifest.json");
    let bytes = read_manifest(&manifest_path)?;
    let parsed = crate::jsonutil::parse_json_document(&bytes).map_err(|_| "evidence.malformed: manifest.json".to_owned())?;
    if let Some(key) = parsed.duplicate_key.clone() {
        let package = match parse_package_inner(&parsed.value, true) {
            Ok(package) => package,
            Err(_) => return Err(format!("version.identity_duplicate: {key}")),
        };
        let mut files = BTreeMap::new();
        for path in raw_paths(&package) {
            let full = dir.join(&path);
            files.insert(path, read_raw(&full)?);
        }
        return Ok(Loaded {
            label: dir.display().to_string(),
            package,
            files,
            duplicate_identity_key: Some(key),
        });
    }
    let package = parse_package(&parsed.value)?;
    let mut files = BTreeMap::new();
    for path in raw_paths(&package) {
        let full = dir.join(&path);
        files.insert(path, read_raw(&full)?);
    }
    Ok(Loaded {
        label: dir.display().to_string(),
        package,
        files,
        duplicate_identity_key: None,
    })
}

fn raw_paths(package: &Package) -> Vec<String> {
    let mut paths = Vec::new();
    for case in &package.cases {
        paths.push(case.raw_path.clone());
    }
    for rejection in &package.rejections {
        paths.push(rejection.raw_path.clone());
    }
    paths.sort();
    paths.dedup();
    paths
}

pub fn parse_package(value: &Value) -> Result<Package, String> {
    parse_package_inner(value, false)
}

fn parse_package_inner(value: &Value, duplicate_identity_key: bool) -> Result<Package, String> {
    let map = require_object(value, "package")?;
    only_keys(map, MANIFEST_KEYS, "package")?;
    let schema = require_str(map, "schema", "package")?;
    if schema != "oo-source-conformance/evidence-package/v1" {
        return Err(format!("package.schema: {schema}"));
    }
    let evidence_id = require_str(map, "evidence_id", "package")?;
    if evidence_id.len() < 8 || evidence_id.len() > 80 || !evidence_id.chars().all(id_char) {
        return Err("package.evidence_id: invalid".to_owned());
    }
    let target_id = require_str(map, "target_id", "package")?;
    let exact_version = require_str(map, "exact_version", "package")?;
    let tested_versions = string_list(map, "tested_versions")?;
    // A duplicate version key is already ambiguous. Do not let the collapsed
    // last value turn that into a different schema error or a passing claim.
    if !duplicate_identity_key && tested_versions != vec![exact_version.clone()] {
        return Err("package.tested_versions: must equal [exact_version]".to_owned());
    }
    let variant = require_str(map, "variant", "package")?;
    let driver_digest = require_str(map, "driver_digest", "package")?;
    if !hex64(&driver_digest) {
        return Err("package.driver_digest: expected 64 lowercase hex".to_owned());
    }
    let suite_version = require_str(map, "suite_version", "package")?;
    let evidence_type = require_str(map, "evidence_type", "package")?;
    if !matches!(evidence_type.as_str(), "container" | "managed_cloud" | "customer") {
        return Err(format!("package.evidence_type: {evidence_type}"));
    }
    let deployment_scope = require_str(map, "deployment_scope", "package")?;
    if !matches!(deployment_scope.as_str(), "public" | "deployment-local") {
        return Err(format!("package.deployment_scope: {deployment_scope}"));
    }
    let identity = require_object(map.get("identity").ok_or_else(|| "package.identity: missing".to_owned())?, "package.identity")?;
    only_keys(identity, IDENTITY_KEYS, "package.identity")?;
    let topology = require_str(identity, "topology", "package.identity")?;
    let service_version = require_service_version(identity)?;
    let cloud = require_object(map.get("cloud").ok_or_else(|| "package.cloud: missing".to_owned())?, "package.cloud")?;
    only_keys(cloud, CLOUD_KEYS, "package.cloud")?;
    let cloud_applies = require_bool(cloud, "applies", "package.cloud")?;
    let budget_exceeded = require_bool(cloud, "budget_exceeded", "package.cloud")?;
    let cycle_status = require_str(cloud, "cycle_status", "package.cloud")?;
    if !matches!(cycle_status.as_str(), "not_cloud" | "verified_this_cycle" | "not_verified_this_cycle") {
        return Err(format!("package.cloud.cycle_status: {cycle_status}"));
    }
    let cases = parse_cases(map.get("cases").ok_or_else(|| "package.cases: missing".to_owned())?)?;
    let rejections = parse_rejections(map.get("rejections").ok_or_else(|| "package.rejections: missing".to_owned())?)?;
    let reviewers = parse_reviewers(map.get("reviewers").ok_or_else(|| "package.reviewers: missing".to_owned())?)?;
    Ok(Package {
        evidence_id,
        target_id,
        exact_version,
        tested_versions,
        variant,
        driver_digest,
        suite_version,
        evidence_type,
        deployment_scope,
        topology,
        service_version,
        cloud_applies,
        budget_exceeded,
        cycle_status,
        cases,
        rejections,
        reviewers,
    })
}

fn require_service_version(identity: &serde_json::Map<String, Value>) -> Result<String, String> {
    let Some(value) = identity.get("service_version") else {
        return Err("version.identity_missing: service_version".to_owned());
    };
    let Some(text) = value.as_str() else {
        return Err("version.identity_malformed: service_version".to_owned());
    };
    if text.is_empty() || text.len() > 160 || text.chars().any(|ch| ch.is_control()) {
        return Err("version.identity_malformed: service_version".to_owned());
    }
    Ok(text.to_owned())
}

fn parse_cases(value: &Value) -> Result<Vec<CaseRow>, String> {
    let items = value.as_array().ok_or_else(|| "package.cases: expected a list".to_owned())?;
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let where_ = format!("package.cases[{index}]");
        let map = require_object(item, &where_)?;
        only_keys(map, CASE_KEYS, &where_)?;
        let id = require_str(map, "id", &where_)?;
        let capability = canonical_of(&require_str(map, "capability", &where_)?, &where_)?;
        let disposition = require_str(map, "disposition", &where_)?;
        if disposition != "pass" && disposition != "fail" {
            return Err(format!("{where_}.disposition: {disposition}"));
        }
        let raw_exit = require_exit(map, "raw_exit", &where_)?;
        let raw_sha256 = require_str(map, "raw_sha256", &where_)?;
        if !hex64(&raw_sha256) {
            return Err(format!("{where_}.raw_sha256: expected 64 lowercase hex"));
        }
        let raw_path = require_raw_path(&require_str(map, "raw_path", &where_)?)?;
        out.push(CaseRow { id, capability, disposition, raw_exit, raw_sha256, raw_path });
    }
    Ok(out)
}

fn parse_rejections(value: &Value) -> Result<Vec<Rejection>, String> {
    let items = value.as_array().ok_or_else(|| "package.rejections: expected a list".to_owned())?;
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let where_ = format!("package.rejections[{index}]");
        let map = require_object(item, &where_)?;
        only_keys(map, REJECTION_KEYS, &where_)?;
        let capability = canonical_of(&require_str(map, "capability", &where_)?, &where_)?;
        let code = require_str(map, "code", &where_)?;
        let silent_fallback = require_bool(map, "silent_fallback", &where_)?;
        let raw_exit = require_exit(map, "raw_exit", &where_)?;
        let raw_sha256 = require_str(map, "raw_sha256", &where_)?;
        if !hex64(&raw_sha256) {
            return Err(format!("{where_}.raw_sha256: expected 64 lowercase hex"));
        }
        let raw_path = require_raw_path(&require_str(map, "raw_path", &where_)?)?;
        out.push(Rejection { capability, code, silent_fallback, raw_exit, raw_sha256, raw_path });
    }
    Ok(out)
}

fn parse_reviewers(value: &Value) -> Result<Vec<Reviewer>, String> {
    let items = value.as_array().ok_or_else(|| "package.reviewers: expected a list".to_owned())?;
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let where_ = format!("package.reviewers[{index}]");
        let map = require_object(item, &where_)?;
        only_keys(map, REVIEWER_KEYS, &where_)?;
        out.push(Reviewer {
            role: require_str(map, "role", &where_)?,
            name: require_str(map, "name", &where_)?,
        });
    }
    Ok(out)
}

pub fn hash_detail(loaded: &Loaded, path: &str, expected: &str) -> Option<String> {
    mismatch(&loaded.files, path, expected)
}

fn mismatch(files: &BTreeMap<String, Vec<u8>>, path: &str, expected: &str) -> Option<String> {
    let Some(bytes) = files.get(path) else {
        return Some(format!("{path} missing"));
    };
    let got = sha256_hex(bytes);
    if got == expected {
        None
    } else {
        Some(format!("{path} hash {got} != {expected}"))
    }
}

fn canonical_of(raw: &str, where_: &str) -> Result<String, String> {
    canonical_capability(raw)
        .map(str::to_owned)
        .ok_or_else(|| format!("{where_}.capability: unknown {raw}"))
}

fn require_raw_path(path: &str) -> Result<String, String> {
    let as_path = Path::new(path);
    if !path.starts_with("raw/") || path.len() > 120 {
        return Err(format!("package.raw_path: {path}"));
    }
    if as_path.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(format!("package.raw_path: {path}"));
    }
    Ok(path.to_owned())
}

fn string_list(map: &serde_json::Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    let items = map
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("package.{key}: expected a list"))?;
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or_else(|| format!("package.{key}: expected strings"))?;
        out.push(text.to_owned());
    }
    Ok(out)
}

fn id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
}

fn read_manifest(path: &Path) -> Result<Vec<u8>, String> {
    read_capped(path, 1_048_576, "evidence.malformed")
}

fn read_raw(path: &Path) -> Result<Vec<u8>, String> {
    read_capped(path, 1_048_576, "evidence.raw_missing")
}

fn read_capped(path: &Path, max_bytes: usize, missing_code: &str) -> Result<Vec<u8>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return Err(format!("{missing_code}: {} ({error})", path.display())),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!("path.symlink: {}", path.display()));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("{missing_code}: {} is not a file", path.display()));
    }
    if metadata.len() > max_bytes as u64 {
        return Err(format!("evidence.malformed: {} is too large", path.display()));
    }
    let bytes = fs::read(path).map_err(|error| format!("{missing_code}: {} ({error})", path.display()))?;
    if bytes.len() > max_bytes {
        return Err(format!("evidence.malformed: {} is too large", path.display()));
    }
    Ok(bytes)
}

pub fn reviewers_ok(package: &Package) -> Result<(), String> {
    let mut seen = BTreeMap::<&str, &str>::new();
    for reviewer in &package.reviewers {
        if !matches!(reviewer.role.as_str(), "data" | "dba" | "security") {
            return Err(format!("role {}", reviewer.role));
        }
        let folded = reviewer.name.to_ascii_lowercase();
        let blocked = ["pending", "tbd", "todo", "unknown", "n/a", "data", "dba", "security", "role"];
        if reviewer.name.chars().count() < 3 || blocked.contains(&folded.as_str()) || folded == reviewer.role {
            return Err(format!("placeholder {}", reviewer.name));
        }
        if seen.insert(reviewer.role.as_str(), reviewer.name.as_str()).is_some() {
            return Err(format!("duplicate {}", reviewer.role));
        }
    }
    for role in ["data", "dba", "security"] {
        if !seen.contains_key(role) {
            return Err(format!("missing {role}"));
        }
    }
    Ok(())
}
