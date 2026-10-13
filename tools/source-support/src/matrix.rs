//! Support-matrix model. Grades stay on the capability that declared them.

use std::collections::BTreeMap;

use crate::yaml::{Yaml, YamlError};

pub const MATRIX_SCHEMA: &str = "oo-support-matrix/v1";
pub const LEVELS: [&str; 6] = ["candidate", "designed", "experimental", "verified", "certified", "excluded"];

#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub schema: String,
    pub captured: String,
    pub levels: Vec<String>,
    pub note: String,
    pub targets: Vec<Target>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub id: String,
    pub family: String,
    pub product: String,
    pub versions: Vec<String>,
    pub variant: Option<String>,
    pub driver: Option<Driver>,
    pub wave: Option<i64>,
    pub grade: BTreeMap<String, String>,
    pub caps: BTreeMap<String, String>,
    pub unknown_caps: Vec<String>,
    pub limits: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
    pub scope: Option<String>,
    pub status_note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Driver {
    pub path: String,
    pub impl_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceRef {
    pub capability: String,
    pub id: String,
    pub commit: String,
    pub suite: String,
    pub sha256: String,
    pub kind: String,
    pub generated: bool,
    pub nights: i64,
    pub reviewers: Vec<String>,
    pub tested_versions: Vec<String>,
    pub releases: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionFit {
    Exact,
    Interval,
    Patch,
    Absent,
}

pub fn canonical_capability(name: &str) -> Option<&'static str> {
    match name {
        "cat" | "catalog" => Some("catalog"),
        "logic" => Some("logic"),
        "qlog" | "query_log" => Some("query_log"),
        "full" | "batch_full" => Some("batch_full"),
        "inc" | "batch_incremental" => Some("batch_incremental"),
        "feed" | "change_feed" => Some("change_feed"),
        "cdc" => Some("cdc"),
        "del" | "delete" => Some("delete"),
        "snap" | "snapshot_token" => Some("snapshot_token"),
        "rep" | "replica_read" => Some("replica_read"),
        "bud" | "budget" => Some("budget"),
        _ => None,
    }
}

pub fn load(text: &str) -> Result<Matrix, YamlError> {
    let yaml = crate::yaml::parse(text)?;
    let schema = required_string(&yaml, "schema")?;
    let captured = required_string(&yaml, "captured")?;
    let note = required_string(&yaml, "note")?;
    let levels = string_list(yaml.get("levels").ok_or(YamlError)?)?;
    let targets_yaml = yaml.get("targets").and_then(Yaml::array).ok_or(YamlError)?;
    let mut targets = Vec::new();
    for item in targets_yaml {
        targets.push(load_target(item)?);
    }
    Ok(Matrix { schema, captured, levels, note, targets })
}

fn load_target(yaml: &Yaml) -> Result<Target, YamlError> {
    let id = required_string(yaml, "id")?;
    let family = required_string(yaml, "family")?;
    let product = required_string(yaml, "product")?;
    let versions = match yaml.get("versions") {
        None => Vec::new(),
        Some(value) => scalar_list(value)?,
    };
    let variant = optional_string(yaml, "variant")?;
    let driver = match yaml.get("driver") {
        None => None,
        Some(value) => Some(load_driver(value)?),
    };
    let wave = optional_int(yaml, "wave")?;
    let grade = match yaml.get("grade") {
        None => BTreeMap::new(),
        Some(value) => string_map(value, true)?,
    };
    let (caps, unknown_caps) = match yaml.get("caps") {
        None => (BTreeMap::new(), Vec::new()),
        Some(value) => capability_map(value)?,
    };
    let limits = match yaml.get("limits") {
        None => Vec::new(),
        Some(value) => string_list(value)?,
    };
    let evidence = match yaml.get("evidence") {
        None => Vec::new(),
        Some(Yaml::Array(items)) => items.iter().map(load_evidence).collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(YamlError),
    };
    let scope = optional_string(yaml, "scope")?;
    let status_note = optional_string(yaml, "status_note")?;
    Ok(Target {
        id,
        family,
        product,
        versions,
        variant,
        driver,
        wave,
        grade,
        caps,
        unknown_caps,
        limits,
        evidence,
        scope,
        status_note,
    })
}

fn load_driver(yaml: &Yaml) -> Result<Driver, YamlError> {
    let path = required_string(yaml, "path")?;
    let impl_name = optional_string(yaml, "impl")?;
    Ok(Driver { path, impl_name })
}

fn load_evidence(yaml: &Yaml) -> Result<EvidenceRef, YamlError> {
    let capability = canonical_capability(&required_string(yaml, "capability")?).ok_or(YamlError)?.to_string();
    Ok(EvidenceRef {
        capability,
        id: required_string(yaml, "id")?,
        commit: required_string(yaml, "commit")?,
        suite: required_string(yaml, "suite")?,
        sha256: required_string(yaml, "sha256")?,
        kind: required_string(yaml, "kind")?,
        generated: yaml.get("generated").and_then(Yaml::bool).ok_or(YamlError)?,
        nights: yaml.get("nights").and_then(Yaml::int).ok_or(YamlError)?,
        reviewers: string_list(yaml.get("reviewers").ok_or(YamlError)?)?,
        tested_versions: match yaml.get("tested_versions") {
            None => Vec::new(),
            Some(value) => scalar_list(value)?,
        },
        releases: yaml.get("releases").and_then(Yaml::int).unwrap_or(0),
    })
}

pub fn version_fit(declared: &[String], detected: &str) -> Option<VersionFit> {
    if declared.is_empty() {
        return if detected.is_empty() { Some(VersionFit::Absent) } else { None };
    }
    let mut best: Option<VersionFit> = None;
    for item in declared {
        let Some(fit) = fit_one(item, detected) else {
            continue;
        };
        best = Some(match (best, fit) {
            (Some(VersionFit::Exact), _) | (_, VersionFit::Exact) => VersionFit::Exact,
            (Some(VersionFit::Interval), _) | (_, VersionFit::Interval) => VersionFit::Interval,
            _ => VersionFit::Patch,
        });
        if matches!(best, Some(VersionFit::Exact)) {
            break;
        }
    }
    best
}

fn fit_one(declared: &str, detected: &str) -> Option<VersionFit> {
    if declared == detected {
        return Some(VersionFit::Exact);
    }
    if let Some((start, end)) = range_pair(declared) {
        if let Some(major) = leading_int(detected) {
            if (start..=end).contains(&major) {
                return Some(VersionFit::Interval);
            }
        }
    }
    if let Some(prefix) = declared.strip_suffix(".x") {
        if detected == prefix || detected.starts_with(&format!("{prefix}.")) {
            return Some(VersionFit::Interval);
        }
    }
    if let Some(rest) = detected.strip_prefix(declared) {
        if let Some(rest) = rest.strip_prefix('.') {
            if !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.') && declared.bytes().any(|byte| byte.is_ascii_digit()) {
                return Some(VersionFit::Patch);
            }
        }
    }
    None
}

fn range_pair(declared: &str) -> Option<(i64, i64)> {
    let (left, right) = declared.split_once('-')?;
    if left.is_empty() || right.is_empty() || left.bytes().any(|byte| !byte.is_ascii_digit()) || right.bytes().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    let start = left.parse::<i64>().ok()?;
    let end = right.parse::<i64>().ok()?;
    if start > end { None } else { Some((start, end)) }
}

fn leading_int(value: &str) -> Option<i64> {
    let digits: String = value.chars().take_while(|char| char.is_ascii_digit()).collect();
    if digits.is_empty() { None } else { digits.parse().ok() }
}

pub fn is_managed_variant(variant: &str) -> bool {
    matches!(
        variant,
        "rds" | "aurora" | "cloud-sql" | "alloydb" | "azure-sql" | "polardb" | "saas" | "aws" | "managed" | "serverless-cloud"
    )
}

fn required_string(yaml: &Yaml, key: &str) -> Result<String, YamlError> {
    scalar_string(yaml.get(key).ok_or(YamlError)?)
}

fn optional_string(yaml: &Yaml, key: &str) -> Result<Option<String>, YamlError> {
    match yaml.get(key) {
        None | Some(Yaml::Null) => Ok(None),
        Some(value) => Ok(Some(scalar_string(value)?)),
    }
}

fn optional_int(yaml: &Yaml, key: &str) -> Result<Option<i64>, YamlError> {
    match yaml.get(key) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::Int(value)) => Ok(Some(*value)),
        Some(_) => Err(YamlError),
    }
}

fn scalar_string(yaml: &Yaml) -> Result<String, YamlError> {
    match yaml {
        Yaml::String(value) => Ok(value.clone()),
        Yaml::Int(value) => Ok(value.to_string()),
        Yaml::Bool(true) => Ok("true".to_string()),
        Yaml::Bool(false) => Ok("false".to_string()),
        _ => Err(YamlError),
    }
}

fn string_list(yaml: &Yaml) -> Result<Vec<String>, YamlError> {
    yaml.array().ok_or(YamlError)?.iter().map(scalar_string).collect()
}

fn scalar_list(yaml: &Yaml) -> Result<Vec<String>, YamlError> {
    string_list(yaml)
}

fn string_map(yaml: &Yaml, canonicalize: bool) -> Result<BTreeMap<String, String>, YamlError> {
    let mut out = BTreeMap::new();
    for (key, value) in yaml.map().ok_or(YamlError)? {
        let name = if canonicalize { canonical_capability(key).ok_or(YamlError)?.to_string() } else { key.clone() };
        if out.contains_key(&name) {
            return Err(YamlError);
        }
        out.insert(name, scalar_string(value)?);
    }
    Ok(out)
}

fn capability_map(yaml: &Yaml) -> Result<(BTreeMap<String, String>, Vec<String>), YamlError> {
    let mut caps = BTreeMap::new();
    let mut unknown = Vec::new();
    for (key, value) in yaml.map().ok_or(YamlError)? {
        let level = scalar_string(value)?;
        match canonical_capability(key) {
            Some(name) => {
                if caps.contains_key(name) {
                    return Err(YamlError);
                }
                caps.insert(name.to_string(), level);
            }
            None => unknown.push(key.clone()),
        }
    }
    Ok((caps, unknown))
}

pub fn level_ok(level: &str) -> bool {
    LEVELS.contains(&level)
}

#[cfg(test)]
mod tests {
    use super::{canonical_capability, version_fit, VersionFit};

    #[test]
    fn unit_delete_native_is_not_delete() {
        assert_eq!(canonical_capability("delete_native"), None);
        assert_eq!(canonical_capability("delete_reconcile"), None);
        assert_eq!(canonical_capability("del"), Some("delete"));
        assert_eq!(canonical_capability("delete"), Some("delete"));
    }

    #[test]
    fn unit_version_fit_exact_interval_and_patch() {
        let fit = |declared: &[&str], detected: &str| {
            let owned: Vec<String> = declared.iter().map(|item| (*item).to_string()).collect();
            version_fit(&owned, detected)
        };
        assert_eq!(fit(&["16.2"], "16.2"), Some(VersionFit::Exact));
        assert_eq!(fit(&["13-17"], "16.2"), Some(VersionFit::Interval));
        assert_eq!(fit(&["13-17"], "18"), None);
        assert_eq!(fit(&["8.0", "8.4"], "8.0.36"), Some(VersionFit::Patch));
        assert_eq!(fit(&["8.0", "8.4"], "5.7"), None);
        assert_eq!(fit(&["16.1"], "16.10"), None);
        assert_eq!(fit(&["3.x"], "3.5"), Some(VersionFit::Interval));
        assert_eq!(fit(&[], ""), Some(VersionFit::Absent));
        assert_eq!(fit(&[], "8"), None);
    }
}
