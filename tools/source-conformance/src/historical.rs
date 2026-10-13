//! Read the fixed local PostgreSQL mirror draft. It is not a CC suite.
//! Only the allowlisted files are opened. Column names are not copied out.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::jsonutil::hex64;
use crate::sha256_hex;

const PROTOCOL: &str = "oo.pipeline.local-pg-mirror-manifest/v1";
const CLASSIFICATION: &str = "historical_draft_preview_not_conformance_suite";

#[derive(Debug, Clone, serde::Serialize)]
pub struct Record {
    pub classification: String,
    pub protocol: String,
    pub report_sha256: String,
    pub handoff_sha256: String,
    pub source_commit: String,
    pub implementation_commit: String,
    pub rows: u64,
    pub source_write_tuples: u64,
    pub source_read_only: bool,
    pub import_hash: String,
    pub manifest_sha256: String,
    pub exits: BTreeMap<String, String>,
    pub satisfies_cc_suite: bool,
    pub promotes_grade: bool,
}

pub fn load(dir: &Path) -> Result<Record, String> {
    let directory = fs::symlink_metadata(dir).map_err(|_| format!("historical.unreadable: {}", dir.display()))?;
    if directory.file_type().is_symlink() {
        return Err(format!("path.symlink: {}", dir.display()));
    }
    if !directory.is_dir() {
        return Err(format!("historical.unreadable: {}", dir.display()));
    }
    let report = read_file(&dir.join("REPORT.md"), 65_536)?;
    let handoff = read_file(&dir.join("HANDOFF.md"), 16_384)?;
    let manifest = read_file(&dir.join("six-final").join("manifest.json"), 2_097_152)?;
    let report_text = std::str::from_utf8(&report).map_err(|_| "historical.unreadable: REPORT.md".to_owned())?;
    let source_commit = commit_after(report_text, "source commit")
        .ok_or_else(|| "historical.unexpected_manifest: source commit missing".to_owned())?;
    let implementation_commit = commit_after(report_text, "implementation commit")
        .ok_or_else(|| "historical.unexpected_manifest: implementation commit missing".to_owned())?;
    let value: Value = serde_json::from_slice(&manifest).map_err(|_| "historical.unreadable: manifest.json".to_owned())?;
    let root = value.as_object().ok_or_else(|| "historical.unexpected_manifest: manifest root".to_owned())?;
    let protocol = root
        .get("protocol")
        .and_then(Value::as_str)
        .ok_or_else(|| "historical.unexpected_manifest: protocol missing".to_owned())?;
    if protocol != PROTOCOL {
        let shown = protocol.chars().take(80).collect::<String>();
        return Err(format!("historical.unexpected_manifest: protocol {shown}"));
    }
    let rows = root.get("rows").and_then(Value::as_u64).ok_or_else(|| "historical.unexpected_manifest: rows".to_owned())?;
    let source_write_tuples = root
        .get("source_write_tuples")
        .and_then(Value::as_u64)
        .ok_or_else(|| "historical.unexpected_manifest: source_write_tuples".to_owned())?;
    let source_read_only = root
        .get("source_read_only")
        .and_then(Value::as_bool)
        .ok_or_else(|| "historical.unexpected_manifest: source_read_only".to_owned())?;
    let import_hash = required_hex(root, "import_hash")?;
    let manifest_sha256 = required_hex(root, "manifest_sha256")?;
    let mut exits = BTreeMap::new();
    for name in ["01", "02", "03", "04", "05", "06"] {
        let bytes = read_file(&dir.join("six-final").join(format!("{name}.exit")), 64)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| format!("historical.unreadable: {name}.exit"))?;
        exits.insert(name.to_owned(), text.to_owned());
    }
    Ok(Record {
        classification: CLASSIFICATION.to_owned(),
        protocol: protocol.to_owned(),
        report_sha256: sha256_hex(&report),
        handoff_sha256: sha256_hex(&handoff),
        source_commit,
        implementation_commit,
        rows,
        source_write_tuples,
        source_read_only,
        import_hash,
        manifest_sha256,
        exits,
        satisfies_cc_suite: false,
        promotes_grade: false,
    })
}

fn required_hex(root: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    let text = root.get(key).and_then(Value::as_str).ok_or_else(|| format!("historical.unexpected_manifest: {key}"))?;
    if !hex64(text) {
        return Err(format!("historical.unexpected_manifest: {key}"));
    }
    Ok(text.to_owned())
}

fn commit_after(text: &str, label: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(label)?;
    let tail = text.get(start + label.len()..)?;
    let bytes = tail.as_bytes();
    if bytes.len() < 40 {
        return None;
    }
    for index in 0..=bytes.len() - 40 {
        let window = &tail[index..index + 40];
        if window.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()) {
            return Some(window.to_owned());
        }
    }
    None
}

fn read_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("historical.unreadable: {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("path.symlink: {}", path.display()));
    }
    if !metadata.is_file() {
        return Err(format!("historical.unreadable: {}", path.display()));
    }
    if metadata.len() > max_bytes as u64 {
        return Err(format!("historical.unreadable: {} exceeds {max_bytes}", path.display()));
    }
    let bytes = fs::read(path).map_err(|_| format!("historical.unreadable: {}", path.display()))?;
    if bytes.len() > max_bytes {
        return Err(format!("historical.unreadable: {} exceeds {max_bytes}", path.display()));
    }
    Ok(bytes)
}
