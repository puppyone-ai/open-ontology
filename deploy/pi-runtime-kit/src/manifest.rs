//! Check `sha256  path` lines against regular files under one root.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};

use crate::sha256;

pub fn verify_manifest(root: &Path, manifest: &str) -> Result<usize, String> {
    let mut seen = BTreeMap::new();
    let mut count = 0_usize;
    for (number, raw) in manifest.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((digest, relative)) = line.split_once("  ") else {
            return Err(format!("manifest line {} has no separator", number + 1));
        };
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err(format!("manifest line {} digest is not lowercase sha256", number + 1));
        }
        if seen.insert(relative.to_owned(), digest.to_owned()).is_some() {
            return Err(format!("manifest repeats {relative}"));
        }
        let path = rooted(root, relative)?;
        let actual = sha256::hash_file(&path)?;
        if actual != digest {
            return Err(format!("{relative} sha256 mismatch"));
        }
        count += 1;
    }
    if count == 0 {
        return Err("manifest is empty".to_owned());
    }
    Ok(count)
}

pub fn rooted(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\\') {
        return Err(format!("path {relative} is not relative"));
    }
    let relative_path = Path::new(relative);
    if relative_path.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(format!("path {relative} is not a normal relative path"));
    }
    Ok(root.join(relative_path))
}

pub fn read_facts(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut facts = BTreeMap::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("facts line {} has no equals", number + 1));
        };
        if key.is_empty() || value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(format!("facts line {} is empty or padded", number + 1));
        }
        if facts.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!("facts repeat {key}"));
        }
    }
    Ok(facts)
}

pub fn verify_fact_hashes(facts: &BTreeMap<String, String>) -> Result<(), String> {
    let mut checked = 0_usize;
    for (key, digest) in facts {
        let Some(path_key) = key.strip_suffix("_sha256") else {
            continue;
        };
        let path = facts.get(path_key).ok_or_else(|| format!("{key} has no path"))?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err(format!("{key} is not lowercase sha256"));
        }
        let actual = sha256::hash_file(Path::new(path))?;
        if &actual != digest {
            return Err(format!("{path_key} sha256 mismatch"));
        }
        checked += 1;
    }
    if checked < 8 {
        return Err(format!("only {checked} hashed files were checked"));
    }
    Ok(())
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("mkdir {}: {err}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|err| format!("write {}: {err}", path.display()))
}
