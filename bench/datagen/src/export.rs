//! Copy only `source-inputs/` out of a verified dataset.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::digest::{hash_file, sha256_hex};
use crate::error::{Result, err};
use crate::facts::{DATASET, PROTOCOL};
use crate::jsonutil::{arr, obj, s, u};
use crate::pathsafety::{create_new_file, create_stage, placement, promote, remove_owned_stage};
use crate::verify::verify_dataset;

pub fn export_sources(dataset: &Path, output: &Path, scope: &Path) -> Result<()> {
    verify_dataset(dataset)?;
    let place = placement(scope, output)?;
    let dataset_canon = fs::canonicalize(dataset)?;
    if place.dest == dataset_canon || place.dest.starts_with(&dataset_canon) {
        return Err(err("export output is inside the dataset"));
    }
    if fs::symlink_metadata(&place.dest).is_ok() {
        return Err(err("destination exists"));
    }
    if fs::symlink_metadata(&place.stage).is_ok() || fs::symlink_metadata(&place.mark).is_ok() {
        return Err(err("stage already exists"));
    }
    let manifest = fs::read_to_string(dataset.join("dataset-manifest.json"))?;
    let node = oo_core::canonical::parse_json(manifest.trim_end())
        .map_err(|_| err("manifest json rejected"))?;
    create_stage(&place.stage, &place.mark)?;
    match write_export(&place.stage, dataset, &node) {
        Ok(()) => promote(&place.stage, &place.mark, &place.dest),
        Err(error) => {
            let _ = remove_owned_stage(&place.stage, &place.mark);
            Err(error)
        }
    }
}

fn write_export(
    stage: &Path,
    dataset: &Path,
    manifest: &oo_core::canonical::JsonNode,
) -> Result<()> {
    let files = match manifest {
        oo_core::canonical::JsonNode::Object(pairs) => pairs
            .iter()
            .find(|(key, _)| key == "files")
            .map(|(_, value)| value),
        _ => None,
    }
    .ok_or_else(|| err("missing files"))?;
    let oo_core::canonical::JsonNode::Array(items) = files else {
        return Err(err("expected array"));
    };
    let mut copied = Vec::new();
    for item in items {
        let oo_core::canonical::JsonNode::Object(pairs) = item else {
            return Err(err("expected object"));
        };
        let path = json_str(pairs, "path")?;
        let visibility = json_str(pairs, "visibility")?;
        if visibility != "source-input" {
            continue;
        }
        if !path.starts_with("source-inputs/") {
            return Err(err(format!("visibility mismatch for {path}")));
        }
        let from = dataset.join(&path);
        let to = stage.join(&path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = create_new_file(&to)?;
        let bytes = fs::read(&from)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        let (len, digest) = hash_file(&from)?;
        if len != bytes.len() as u64 || digest != sha256_hex(&bytes) {
            return Err(err(format!("hash mismatch for {path}")));
        }
        copied.push((path, len, digest));
    }
    if copied.is_empty() {
        return Err(err("export found no source inputs"));
    }
    copied.sort_by(|left, right| left.0.cmp(&right.0));
    let list = copied
        .iter()
        .map(|(path, len, digest)| {
            obj(vec![
                ("bytes", u(*len)),
                ("path", s(path)),
                ("sha256", s(digest)),
            ])
        })
        .collect();
    let doc = obj(vec![
        ("contents", s("source-inputs")),
        ("dataset", s(DATASET)),
        ("files", arr(list)),
        ("protocol", s(PROTOCOL)),
    ]);
    let text = doc.render()?;
    if text.contains("hidden-truth") || text.contains(crate::facts::TRUTH_MARKER) {
        return Err(err("consumer manifest leaked truth"));
    }
    let mut out = File::create(stage.join("consumer-manifest.json"))?;
    out.write_all(text.as_bytes())?;
    out.sync_all()?;
    Ok(())
}

fn json_str(pairs: &[(String, oo_core::canonical::JsonNode)], key: &str) -> Result<String> {
    let value = pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| err(format!("missing {key}")))?;
    match value {
        oo_core::canonical::JsonNode::String(text) => Ok(text.clone()),
        _ => Err(err(format!("expected string {key}"))),
    }
}
