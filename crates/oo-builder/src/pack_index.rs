//! Lossless, hash-linked manifest indexes. Old inline manifests stay unchanged.
use crate::canonical::sha256_hex;
use serde_json::Value;
use std::collections::BTreeSet;

pub fn manifest_file_records<'a>(
    manifest: &Value,
    lookup: impl Fn(&str) -> Option<&'a str>,
) -> Result<Vec<Value>, &'static str> {
    let inline = manifest["files"].as_array().ok_or("manifest_files")?;
    let Some(root) = manifest.get("file_index") else {
        return Ok(inline.clone());
    };
    if !inline.is_empty()
        || root.as_object().is_none_or(|o| o.len() != 3)
        || root["protocol"] != "oo.knowledge.file-index/v1"
    {
        return Err("manifest_index");
    }
    let count = root["entry_count"].as_u64().ok_or("manifest_index")?;
    let mut next = root["first"].clone();
    let mut pages = Vec::new();
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    while !next.is_null() {
        let path = next["path"].as_str().ok_or("manifest_index")?;
        if next.as_object().is_none_or(|o| o.len() != 3)
            || !path.starts_with("manifest/")
            || crate::normalize_pack_path(path).map_err(|_| "manifest_index")? != path
            || !seen.insert(path.to_owned())
        {
            return Err("manifest_index");
        }
        let body = lookup(path).ok_or("manifest_index_missing")?;
        if next["bytes"].as_u64() != Some(body.len() as u64)
            || next["sha256"].as_str() != Some(sha256_hex(body.as_bytes()).as_str())
        {
            return Err("manifest_index_digest");
        }
        let page: Value = serde_json::from_str(body).map_err(|_| "manifest_index")?;
        if page.as_object().is_none_or(|o| o.len() != 3)
            || page["protocol"] != "oo.knowledge.file-index-page/v1"
            || page.get("next").is_none()
        {
            return Err("manifest_index");
        }
        let entries = page["files"]
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or("manifest_index")?;
        records.extend(entries.iter().cloned());
        pages.push(next);
        next = page["next"].clone();
    }
    if pages.is_empty() || records.len() as u64 != count {
        return Err("manifest_index_count");
    }
    records.extend(pages);
    let mut names = BTreeSet::new();
    for record in &records {
        let path = record["path"].as_str().ok_or("manifest_index")?;
        if !names.insert(path) {
            return Err("manifest_index_duplicate");
        }
    }
    records.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(records)
}
