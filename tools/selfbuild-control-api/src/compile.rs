//! One `pg-builder pack-combined` call from stored decisions and the operator role file.
//!
//! The producer document is stored as returned. `review.state` is not inserted.
//! `compiled_pack` stays `not_published`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::canonical::sha256_hex;
use crate::config::{regular_file, AppConfig, ProjectCfg};
use crate::error::ApiError;
use crate::jsonutil::parse_exact;
use crate::pack::envelope;
use crate::spawn::{run_child, ChildSpec};
use crate::store::{self, BuildRow, LedgerDecision, OurDecision};

pub async fn load_stored(config: &AppConfig, build_id: &str) -> Result<Value, ApiError> {
    let Some(head) = store::find_pack(&config.postgres, build_id).await? else {
        return Err(ApiError::forbidden());
    };
    let bytes = std::fs::read(pack_path(config, build_id)).map_err(|_| ApiError::not_ready())?;
    if sha256_hex(&bytes) != head.output_sha256 {
        return Err(ApiError::not_ready());
    }
    let producer = parse_exact(&bytes)?;
    accept_producer(&producer)?;
    Ok(envelope("getBuildPack", build_id, &producer))
}

pub async fn compile(config: &AppConfig, build: &BuildRow, subject: &str, body_sha256: &str) -> Result<Value, ApiError> {
    if store::find_pack(&config.postgres, &build.build_id).await?.is_some() {
        let mut loaded = load_stored(config, &build.build_id).await?;
        loaded["operation"] = json!("compileBuildPack");
        return Ok(loaded);
    }
    let project = config
        .projects
        .iter()
        .find(|item| item.project_id == build.project_id)
        .ok_or_else(ApiError::forbidden)?;
    if !regular_file(&config.binaries.pg_builder) {
        return Err(ApiError::not_ready());
    }
    let dir = PathBuf::from(&config.work_dir).join("builds").join(&build.build_id);
    let generation = read_json(&dir.join("generation.json"))?;
    if generation.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Err(ApiError::not_ready());
    }
    let ours = store::our_decisions(&config.postgres, &build.build_id).await?;
    let ledger = store::ledger_decisions(&config.ledger, &build.project_id).await?;
    let operator = operator_document(&ours, &ledger)?;
    let classes = classification_document(project, &ours, &ledger)?;
    let roles = std::fs::read(&project.role_policy_file).map_err(|_| ApiError::not_ready())?;
    let roles_value = parse_exact(&roles)?;
    let role_rows = roles_value.as_array().ok_or_else(ApiError::membership)?;
    if role_rows.len() != 2 {
        return Err(ApiError::membership());
    }
    let curated = dir.join("curated.json");
    if !curated.is_file() {
        run_builder(
            config,
            project,
            vec![
                "curated".to_owned(),
                "--catalog".to_owned(),
                dir.join("catalog.json").display().to_string(),
                "--out".to_owned(),
                curated.display().to_string(),
            ],
        )
        .await?;
    }
    let operator_path = dir.join("operator.json");
    let class_path = dir.join("classifications.json");
    let settings_path = dir.join("settings.json");
    let out_path = dir.join("pack.json");
    write_private(&operator_path, &serde_json::to_vec(&operator).map_err(|_| ApiError::not_ready())?)?;
    write_private(&class_path, &serde_json::to_vec(&classes).map_err(|_| ApiError::not_ready())?)?;
    write_private(
        &settings_path,
        &serde_json::to_vec(&json!({
            "authenticated": false,
            "parent_auth": "required",
            "project_id": &project.project_id,
            "config_commit": &project.config_commit,
            "published": false
        }))
        .map_err(|_| ApiError::not_ready())?,
    )?;
    let mut args = vec![
        "pack-combined".to_owned(),
        "--generation".to_owned(),
        dir.join("generation.json").display().to_string(),
        "--curated".to_owned(),
        curated.display().to_string(),
        "--operator".to_owned(),
        operator_path.display().to_string(),
        "--settings".to_owned(),
        settings_path.display().to_string(),
        "--classifications".to_owned(),
        class_path.display().to_string(),
        "--roles".to_owned(),
        project.role_policy_file.clone(),
        "--out".to_owned(),
        out_path.display().to_string(),
    ];
    for hidden in &project.hidden_columns {
        if let Some(name) = hidden.rsplit('.').next() {
            args.push("--hidden".to_owned());
            args.push(name.to_owned());
        }
    }
    run_builder(config, project, args).await?;
    let bytes = std::fs::read(&out_path).map_err(|_| ApiError::not_ready())?;
    let producer = parse_exact(&bytes)?;
    accept_producer(&producer)?;
    let hash = sha256_hex(&bytes);
    let reseal = producer.get("reseal_count").and_then(Value::as_i64);
    store::insert_pack(&config.postgres, build, subject, &hash, reseal, body_sha256).await?;
    Ok(envelope("compileBuildPack", &build.build_id, &producer))
}

fn accept_producer(value: &Value) -> Result<(), ApiError> {
    if value.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.pack-combined/v1")
        || value.get("authenticated") != Some(&Value::Bool(false))
        || value.get("parent_auth").and_then(Value::as_str) != Some("required")
        || value.pointer("/statements/model_publication") == Some(&Value::Bool(true))
    {
        return Err(ApiError::membership());
    }
    match value.pointer("/product/packs").and_then(Value::as_array) {
        Some(packs) if packs.len() == 2 => Ok(()),
        _ => Err(ApiError::membership()),
    }
}

fn operator_document(ours: &[OurDecision], ledger: &[LedgerDecision]) -> Result<Value, ApiError> {
    let mut decisions = Vec::new();
    for row in ours.iter().filter(|row| row.decision_kind == "label_description") {
        let matched = match_ledger(ledger, &row.object_id, &row.decision_kind, &row.choice)?;
        let mut item = json!({
            "dataset": dataset_of(&row.object_id)?,
            "decision": &row.choice,
            "reviewer_label": &matched.reviewer_label,
            "reviewer_at": &matched.reviewer_at
        });
        if row.choice == "approve" {
            item["fields"] = parse_exact(matched.fields_json.as_bytes())?;
        }
        if row.object_id.split('.').count() == 2 {
            decisions.push(item);
        }
    }
    Ok(json!({
        "protocol": "oo.pg-builder.operator-review/v1",
        "authenticated": false,
        "parent_auth": "required",
        "decisions": decisions
    }))
}

fn classification_document(project: &ProjectCfg, ours: &[OurDecision], ledger: &[LedgerDecision]) -> Result<Value, ApiError> {
    let mut decisions = Vec::new();
    for row in ours.iter().filter(|row| row.decision_kind == "classification") {
        if project.hidden_columns.iter().any(|hidden| hidden == &row.object_id) {
            return Err(ApiError::membership());
        }
        let matched = match_ledger(ledger, &row.object_id, "classification", "confirm")?;
        let fields = parse_exact(matched.fields_json.as_bytes())?;
        let pii = fields.get("pii").and_then(Value::as_str).unwrap_or("");
        if pii == "unknown" || pii.is_empty() {
            return Err(ApiError::pii_unknown());
        }
        let semantic = fields.get("semantic_type").and_then(Value::as_str).unwrap_or("");
        let mut parts = row.object_id.split('.');
        let schema = parts.next().unwrap_or("");
        let table = parts.next().unwrap_or("");
        let column = parts.next().unwrap_or("");
        decisions.push(json!({
            "dataset": format!("{schema}.{table}"),
            "column": column,
            "pii": pii,
            "semantic_type": semantic,
            "reviewer": {"by": &matched.reviewer_label, "at": &matched.reviewer_at}
        }));
    }
    if project.visible_columns.iter().any(|column| {
        !decisions.iter().any(|item| {
            let dataset = item.get("dataset").and_then(Value::as_str).unwrap_or("");
            let name = item.get("column").and_then(Value::as_str).unwrap_or("");
            format!("{dataset}.{name}") == *column
        })
    }) {
        return Err(ApiError::classification_incomplete());
    }
    Ok(json!({
        "protocol": "oo.pg-builder.operator-classification/v1",
        "authenticated": false,
        "parent_auth": "required",
        "decisions": decisions
    }))
}

fn match_ledger<'a>(
    ledger: &'a [LedgerDecision],
    object_id: &str,
    kind: &str,
    choice: &str,
) -> Result<&'a LedgerDecision, ApiError> {
    let row = ledger
        .iter()
        .find(|item| item.object_id == object_id && item.decision_kind == kind && item.choice == choice)
        .ok_or_else(ApiError::not_ready)?;
    if row.authenticated {
        return Err(ApiError::membership());
    }
    Ok(row)
}

fn dataset_of(object_id: &str) -> Result<String, ApiError> {
    let mut parts = object_id.split('.');
    let schema = parts.next().ok_or_else(ApiError::malformed)?;
    let table = parts.next().ok_or_else(ApiError::malformed)?;
    Ok(format!("{schema}.{table}"))
}

fn pack_path(config: &AppConfig, build_id: &str) -> PathBuf {
    PathBuf::from(&config.work_dir).join("builds").join(build_id).join("pack.json")
}

fn read_json(path: &Path) -> Result<Value, ApiError> {
    let bytes = std::fs::read(path).map_err(|_| ApiError::not_ready())?;
    parse_exact(&bytes).map_err(|_| ApiError::not_ready())
}

async fn run_builder(config: &AppConfig, project: &ProjectCfg, args: Vec<String>) -> Result<(), ApiError> {
    let mut needles = Vec::new();
    for path in [&project.source.password_file, &project.llm.bearer_file] {
        if let Ok(bytes) = std::fs::read(path) {
            let mut trimmed = bytes;
            while trimmed.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
                trimmed.pop();
            }
            if !trimmed.is_empty() {
                needles.push(trimmed);
            }
        }
    }
    let output = run_child(ChildSpec {
        binary: config.binaries.pg_builder.clone(),
        args,
        stdin: Vec::new(),
        deadline: Duration::from_secs(60),
        needles,
        cancel: Arc::new(AtomicBool::new(false)),
        pid_out: Arc::new(AtomicU32::new(0)),
    })
    .await;
    if output.secret_leak || !output.child_exited || output.killed || output.exit != Some(0) {
        return Err(ApiError::membership());
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ApiError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ApiError::not_ready())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ApiError::not_ready())?;
    file.write_all(bytes).map_err(|_| ApiError::not_ready())
}
