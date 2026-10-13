//! Existing `pg-builder` generate path for one adapted build.
//!
//! The argv matches the non-sealed worker: `approve`, then `generate`, then
//! `curated`. A missing bearer file does not spawn and does not read the file.
//! A present bearer with a missing binary, or without the seal and cards inputs
//! that command requires, fails closed. Outcome `validated` is that producer's
//! token. It is not Ready and it is not publication.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use zeroize::Zeroize;

use crate::spawn::{run_child, ChildSpec};

#[derive(Clone, Debug)]
pub struct InvokeSpec {
    pub binary: String,
    pub bearer_file: String,
    pub config_path: PathBuf,
    pub seal_path: PathBuf,
    pub cards_path: PathBuf,
    pub approval_path: PathBuf,
    pub cache_path: PathBuf,
    pub generation_path: PathBuf,
    pub catalog_path: PathBuf,
    pub curated_path: PathBuf,
    pub reviewer_label: String,
    pub reviewer_at: String,
    pub model_cache_dir: String,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Blocked,
    Canceled,
    Failed { detail: &'static str },
    Validated { generation_sha256: String, llm_calls: u64, cache_hit: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelResult {
    pub kind: ModelKind,
    pub spawned: bool,
}

pub fn bearer_metadata_ok(path: &str) -> bool {
    matches!(
        std::fs::symlink_metadata(path),
        Ok(meta) if meta.file_type().is_file() && (1..=16_384).contains(&meta.len())
    )
}

pub async fn invoke(spec: InvokeSpec) -> ModelResult {
    if !bearer_metadata_ok(&spec.bearer_file) {
        return ModelResult { kind: ModelKind::Blocked, spawned: false };
    }
    if !absolute_file(&spec.binary) {
        return ModelResult {
            kind: ModelKind::Failed { detail: "builder.absent" },
            spawned: false,
        };
    }
    if spec.reviewer_label.is_empty() {
        return ModelResult {
            kind: ModelKind::Failed { detail: "dependency.not_ready" },
            spawned: false,
        };
    }
    if let Ok(bytes) = std::fs::read(&spec.catalog_path)
        && serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| v.get("protocol").and_then(Value::as_str).map(str::to_owned)).as_deref().is_some_and(|protocol| matches!(protocol, "oo.pipeline.sealed-import-catalog/v1" | "oo.pipeline.sealed-import-collection/v1")) {
        return invoke_native(spec).await;
    }
    if !spec.config_path.is_file() || !spec.seal_path.is_file() || !spec.cards_path.is_file() || !spec.catalog_path.is_file() {
        return ModelResult {
            kind: ModelKind::Failed { detail: "builder.inputs_absent" },
            spawned: false,
        };
    }
    let mut needles = bearer_needles(&spec.bearer_file);
    let approve = spawn_one(
        &spec,
        &needles,
        vec![
            "approve".to_owned(),
            "--cards".to_owned(),
            path_str(&spec.cards_path),
            "--reviewer-label".to_owned(),
            spec.reviewer_label.clone(),
            "--reviewer-at".to_owned(),
            spec.reviewer_at.clone(),
            "--out".to_owned(),
            path_str(&spec.approval_path),
        ],
        Duration::from_secs(30),
    )
    .await;
    if approve.killed || spec.cancel.load(Ordering::SeqCst) {
        zeroize_needles(&mut needles);
        return ModelResult { kind: ModelKind::Canceled, spawned: approve.child_exited || approve.pid.is_some() };
    }
    if approve.secret_leak || approve.exit != Some(0) {
        zeroize_needles(&mut needles);
        return ModelResult {
            kind: ModelKind::Failed { detail: "approval.failed" },
            spawned: true,
        };
    }
    let _ = std::fs::create_dir_all(&spec.cache_path);
    import_model_cache(&spec.model_cache_dir, &spec.cache_path, &spec.cards_path);
    let generate = spawn_one(
        &spec,
        &needles,
        vec![
            "generate".to_owned(),
            "--config".to_owned(),
            path_str(&spec.config_path),
            "--seal".to_owned(),
            path_str(&spec.seal_path),
            "--cards".to_owned(),
            path_str(&spec.cards_path),
            "--approval".to_owned(),
            path_str(&spec.approval_path),
            "--cache".to_owned(),
            path_str(&spec.cache_path),
            "--out".to_owned(),
            path_str(&spec.generation_path),
        ],
        Duration::from_secs(300),
    )
    .await;
    if generate.killed || spec.cancel.load(Ordering::SeqCst) {
        zeroize_needles(&mut needles);
        return ModelResult { kind: ModelKind::Canceled, spawned: true };
    }
    if generate.secret_leak || generate.exit != Some(0) {
        zeroize_needles(&mut needles);
        return ModelResult {
            kind: ModelKind::Failed { detail: "generate.failed" },
            spawned: true,
        };
    }
    let bytes = std::fs::read(&spec.generation_path).unwrap_or_default();
    let parsed = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    if parsed.get("outcome").and_then(Value::as_str) != Some("validated") {
        zeroize_needles(&mut needles);
        return ModelResult {
            kind: ModelKind::Failed { detail: "generate.failed" },
            spawned: true,
        };
    }
    let llm_calls = match parsed.get("llm_calls") {
        None => parsed.get("calls").and_then(Value::as_array).map(|calls| calls.iter().filter(|call| call.pointer("/transport/socket").and_then(Value::as_str) == Some("opened")).map(|call| call.pointer("/transport/attempt").and_then(Value::as_u64).unwrap_or(0)).sum()).unwrap_or(0),
        Some(value) => match value.as_u64() {
            Some(calls) => calls,
            None => {
                zeroize_needles(&mut needles);
                return ModelResult {
                    kind: ModelKind::Failed { detail: "generate.failed" },
                    spawned: true,
                };
            }
        },
    };
    let cache_hit = match parsed.get("cache_hit") {
        None => parsed.get("cache").and_then(Value::as_str) == Some("hit"),
        Some(Value::Bool(hit)) => *hit,
        Some(_) => {
            zeroize_needles(&mut needles);
            return ModelResult {
                kind: ModelKind::Failed { detail: "generate.failed" },
                spawned: true,
            };
        }
    };
    publish_model_cache(&spec.model_cache_dir, &spec.cache_path, &spec.cards_path);
    let curated = spawn_one(
        &spec,
        &needles,
        vec!["curated".to_owned(), "--catalog".to_owned(), path_str(&spec.catalog_path), "--out".to_owned(), path_str(&spec.curated_path)],
        Duration::from_secs(60),
    )
    .await;
    zeroize_needles(&mut needles);
    if curated.killed {
        return ModelResult { kind: ModelKind::Canceled, spawned: true };
    }
    if curated.secret_leak || curated.exit != Some(0) || !spec.curated_path.is_file() {
        return ModelResult {
            kind: ModelKind::Failed { detail: "curated.failed" },
            spawned: true,
        };
    }
    ModelResult {
        kind: ModelKind::Validated {
            generation_sha256: crate::canonical::sha256_hex(&bytes),
            llm_calls,
            cache_hit,
        },
        spawned: true,
    }
}

fn absolute_file(path: &str) -> bool {
    let file = Path::new(path);
    file.is_absolute() && matches!(std::fs::symlink_metadata(file), Ok(meta) if meta.file_type().is_file())
}

fn bearer_needles(path: &str) -> Vec<Vec<u8>> {
    let mut needles = Vec::new();
    if let Ok(mut bytes) = std::fs::read(path) {
        while bytes.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
            bytes.pop();
        }
        if !bytes.is_empty() {
            needles.push(bytes);
        }
    }
    needles
}

fn zeroize_needles(needles: &mut [Vec<u8>]) {
    for needle in needles {
        needle.zeroize();
    }
}

async fn spawn_one(spec: &InvokeSpec, needles: &[Vec<u8>], args: Vec<String>, deadline: Duration) -> crate::spawn::ChildOutput {
    let command = args.first().cloned().unwrap_or_else(|| "unknown".to_owned());
    let result = run_child(ChildSpec {
        binary: spec.binary.clone(),
        args,
        stdin: Vec::new(),
        deadline,
        needles: needles.to_vec(),
        cancel: spec.cancel.clone(),
        pid_out: Arc::new(AtomicU32::new(0)),
    })
    .await;
    if let Some(parent) = spec.config_path.parent() {
        let root = parent.join("child-evidence"); let _ = std::fs::create_dir_all(&root);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_nanos()).unwrap_or(0);
        let stem = format!("{stamp}-{command}");
        let _ = std::fs::write(root.join(format!("{stem}.stdout")), &result.stdout);
        let _ = std::fs::write(root.join(format!("{stem}.stderr")), &result.stderr);
        let _ = std::fs::write(root.join(format!("{stem}.json")), serde_json::to_vec(&serde_json::json!({"exit":result.exit,"killed":result.killed,"secret_leak":result.secret_leak,"child_exited":result.child_exited})).unwrap_or_default());
    }
    result
}

fn path_str(path: &Path) -> String {
    path.display().to_string()
}

fn import_model_cache(source_root: &str, cache_dir: &Path, cards: &Path) {
    copy_matching_cache(source_root, &cache_dir.join("pg-builder"), cards);
}

fn publish_model_cache(source_root: &str, cache_dir: &Path, cards: &Path) {
    if source_root.is_empty() {
        return;
    }
    copy_matching_cache(cache_dir.to_str().unwrap_or(""), &PathBuf::from(source_root).join("pg-builder"), cards);
}

fn copy_matching_cache(source_root: &str, dest_dir: &Path, cards: &Path) {
    if source_root.is_empty() {
        return;
    }
    let Some(evidence) = cards_evidence(cards) else {
        return;
    };
    copy_matching_cache_evidence(source_root, dest_dir, &evidence);
}

fn copy_matching_cache_evidence(source_root: &str, dest_dir: &Path, evidence: &str) {
    if source_root.is_empty() {
        return;
    }
    let src = PathBuf::from(source_root).join("pg-builder");
    let Ok(entries) = std::fs::read_dir(&src) else {
        return;
    };
    let _ = std::fs::create_dir_all(dest_dir);
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_file() || meta.len() > 2_000_000 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if value.get("outcome").and_then(Value::as_str) != Some("validated") {
            continue;
        }
        if value.get("evidence_hash").and_then(Value::as_str) != Some(evidence) {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let Some(key) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || value.get("input_sha256").and_then(Value::as_str) != Some(key) {
            continue;
        }
        let target = dest_dir.join(name);
        if std::fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let _ = write_private(&target, &bytes);
    }
}

fn cards_evidence(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("evidence_hash").and_then(Value::as_str).map(str::to_owned)
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path)?;
    file.write_all(bytes)
}

async fn invoke_native(spec: InvokeSpec) -> ModelResult {
    let model_path = spec.catalog_path.with_file_name("native-model.json");
    let input = std::fs::read(&model_path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let Some(input) = input else { return ModelResult { kind: ModelKind::Failed { detail: "builder.native_config_absent" }, spawned: false }; };
    let Some(settings) = input.get("settings") else { return ModelResult { kind: ModelKind::Failed { detail: "builder.native_config_absent" }, spawned: false }; };
    let Some(visibility) = input.get("visibility") else { return ModelResult { kind: ModelKind::Failed { detail: "builder.native_config_absent" }, spawned: false }; };
    let Some(dataset) = input.get("dataset").and_then(Value::as_str) else { return ModelResult { kind: ModelKind::Failed { detail: "builder.native_config_absent" }, spawned: false }; };
    let settings_path = model_path.with_file_name("native-settings.json");
    let visibility_path = model_path.with_file_name("native-visibility.json");
    if std::fs::write(&settings_path, serde_json::to_vec(settings).unwrap_or_default()).is_err() || std::fs::write(&visibility_path, serde_json::to_vec(visibility).unwrap_or_default()).is_err() { return ModelResult { kind: ModelKind::Failed { detail: "builder.native_config_absent" }, spawned: false }; }
    let out = model_path.with_file_name("native-model-output");
    let Ok(catalog_bytes) = std::fs::read(&spec.catalog_path) else { return ModelResult { kind: ModelKind::Failed { detail: "builder.inputs_absent" }, spawned: false }; };
    let evidence = crate::canonical::sha256_hex(&catalog_bytes);
    copy_matching_cache_evidence(&spec.model_cache_dir, &spec.cache_path.join("pg-builder"), &evidence);
    let mut needles = bearer_needles(&spec.bearer_file);
    let mut args = vec!["sealed".to_owned(), "--config".to_owned(), path_str(&spec.config_path), "--catalog".to_owned(), path_str(&spec.catalog_path), "--settings".to_owned(), path_str(&settings_path), "--visibility".to_owned(), path_str(&visibility_path), "--reviewer".to_owned(), spec.reviewer_label.clone(), "--reviewed-at".to_owned(), spec.reviewer_at.clone(), "--dataset".to_owned(), dataset.to_owned(), "--cache".to_owned(), path_str(&spec.cache_path), "--out".to_owned(), path_str(&out)];
    if let Some(contract) = input.get("source_contract") {
        let path = model_path.with_file_name("native-source-contract.json");
        if std::fs::write(&path, serde_json::to_vec(contract).unwrap_or_default()).is_err() { return ModelResult { kind: ModelKind::Failed { detail: "builder.native_config_absent" }, spawned: false }; }
        args.extend(["--source-contract".to_owned(), path_str(&path)]);
    }
    let result = spawn_one(&spec, &needles, args, Duration::from_secs(300)).await;
    zeroize_needles(&mut needles);
    if result.killed { return ModelResult { kind: ModelKind::Canceled, spawned: true }; }
    if result.secret_leak || result.exit != Some(0) { return ModelResult { kind: ModelKind::Failed { detail: "generate.failed" }, spawned: true }; }
    let bytes = std::fs::read(out.join("result.json")).unwrap_or_default();
    let parsed = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    if parsed.get("outcome").and_then(Value::as_str) != Some("packed") { return ModelResult { kind: ModelKind::Failed { detail: "generate.failed" }, spawned: true }; }
    if std::fs::write(&spec.generation_path, &bytes).is_err() { return ModelResult { kind: ModelKind::Failed { detail: "generate.failed" }, spawned: true }; }
    ModelResult { kind: ModelKind::Validated { generation_sha256:crate::canonical::sha256_hex(&bytes), llm_calls:parsed.get("attempt").and_then(Value::as_u64).unwrap_or(0), cache_hit:parsed.get("cache").and_then(Value::as_str)==Some("hit") }, spawned:true }
}

#[cfg(test)]
mod native_cache_tests {
    use super::copy_matching_cache_evidence;

    #[test]
    fn imports_only_validated_same_evidence_and_named_key() {
        let root = std::env::temp_dir().join(format!("oo-native-cache-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let source = root.join("source/pg-builder");
        let target = root.join("target/pg-builder");
        std::fs::create_dir_all(&source).unwrap();
        let good = "a".repeat(64);
        let bad = "b".repeat(64);
        let mismatched = "c".repeat(64);
        std::fs::write(source.join(format!("{good}.json")), format!(r#"{{"outcome":"validated","evidence_hash":"same","input_sha256":"{good}"}}"#)).unwrap();
        std::fs::write(source.join(format!("{bad}.json")), format!(r#"{{"outcome":"validated","evidence_hash":"other","input_sha256":"{bad}"}}"#)).unwrap();
        std::fs::write(source.join(format!("{mismatched}.json")), format!(r#"{{"outcome":"validated","evidence_hash":"same","input_sha256":"{bad}"}}"#)).unwrap();
        copy_matching_cache_evidence(source.parent().unwrap().to_str().unwrap(), &target, "same");
        assert!(target.join(format!("{good}.json")).is_file());
        assert!(!target.join(format!("{bad}.json")).exists());
        assert!(!target.join(format!("{mismatched}.json")).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
