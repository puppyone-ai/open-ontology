//! One `pg-builder pack-combined` call from stored decisions and the operator role file.
//!
//! The producer document is stored as returned. `review.state` is not inserted.
//! `compiled_pack` stays `not_published`.

use std::collections::{BTreeMap, BTreeSet};
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
    let producer = serde_json::from_slice(&bytes).map_err(|_| ApiError::not_ready())?;
    accept_producer(&producer)?;
    Ok(envelope("getBuildPack", build_id, &producer))
}

pub async fn compile(config: &AppConfig, build: &BuildRow, subject: &str, body_sha256: &str) -> Result<Value, ApiError> {
    if store::find_pack(&config.postgres, &build.build_id).await?.is_some() {
        let _ = store::finish_reviewed(&config.postgres, build).await;
        let mut loaded = load_stored(config, &build.build_id).await?;
        loaded["operation"] = json!("compileBuildPack");
        return Ok(loaded);
    }
    let stages = store::stages(&config.postgres, &build.build_id).await?;
    let stage_status = |name: &str| {
        stages
            .iter()
            .find(|row| row.stage == name)
            .map(|row| row.status.as_str())
            .unwrap_or("")
    };
    if stage_status("S9") != "succeeded" {
        return Err(ApiError::membership());
    }
    if stage_status("S10") != "succeeded" {
        return Err(ApiError::classification_incomplete());
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
        &serde_json::to_vec(&compiler_settings(&roles_value)?).map_err(|_| ApiError::not_ready())?,
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
    let producer = serde_json::from_slice(&bytes).map_err(|_| ApiError::not_ready())?;
    accept_producer(&producer)?;
    let hash = sha256_hex(&bytes);
    let reseal = producer.get("reseal_count").and_then(Value::as_i64);
    store::insert_pack(&config.postgres, build, subject, &hash, reseal, body_sha256).await?;
    if !store::finish_reviewed(&config.postgres, build).await? {
        return Err(ApiError::not_ready());
    }
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
    // Omitting column_decisions makes the producer treat the list as empty. Coverage still requires every compiler column.
    let mut latest: BTreeMap<&str, &OurDecision> = BTreeMap::new();
    for row in ours.iter().filter(|row| row.decision_kind == "label_description") {
        latest.insert(row.object_id.as_str(), row);
    }
    let mut decisions = Vec::new();
    let mut column_decisions = Vec::new();
    for row in latest.values() {
        let matched = match_ledger(ledger, &row.object_id, &row.decision_kind, &row.choice)?;
        let mut item = json!({
            "dataset": dataset_of(&row.object_id)?,
            "decision": &row.choice,
            "reviewer_label": &matched.reviewer_label,
            "reviewer_at": &matched.reviewer_at
        });
        if row.choice == "approve" {
            item["fields"] = overwrite_only(&matched.fields_json)?;
        }
        match row.object_id.split('.').count() {
            2 => decisions.push(item),
            3 => {
                item["column"] = json!(row.object_id.rsplit('.').next().unwrap_or(""));
                column_decisions.push(item);
            }
            _ => return Err(ApiError::malformed()),
        }
    }
    Ok(json!({
        "protocol": "oo.pg-builder.operator-review/v1",
        "authenticated": false,
        "parent_auth": "required",
        "decisions": decisions,
        "column_decisions": column_decisions
    }))
}

fn overwrite_only(fields_json: &str) -> Result<Value, ApiError> {
    let value = parse_exact(fields_json.as_bytes())?;
    let Some(object) = value.as_object() else {
        return Err(ApiError::malformed());
    };
    let mut out = serde_json::Map::new();
    for (key, field) in object {
        let Some(overwrite) = field.get("overwrite").and_then(Value::as_bool) else {
            return Err(ApiError::malformed());
        };
        out.insert(key.clone(), json!({"overwrite": overwrite}));
    }
    Ok(Value::Object(out))
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
        let semantic = bound_semantic(fields.get("semantic_type").and_then(Value::as_str).unwrap_or(""))?;
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

/// The stored confirm token stays in the ledger. The compiler file uses its closed enum.
fn bound_semantic(token: &str) -> Result<String, ApiError> {
    let bound = match token {
        "flag" => "boolean_flag",
        "label" | "text" => "free_text",
        // `money` requires a column unit. The sealed draft has none, and a unit is not invented here.
        "amount" => "quantity",
        "name" => "person_name",
        "identifier" | "foreign_key" | "entity_name" | "code" | "status_code" | "category"
        | "boolean_flag" | "money" | "quantity" | "ratio" | "event_time" | "date" | "audit_time"
        | "geo" | "address" | "phone" | "email" | "person_name" | "free_text" | "json" | "system" => token,
        _ => return Err(ApiError::malformed()),
    };
    Ok(bound.to_owned())
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
    serde_json::from_slice(&bytes).map_err(|_| ApiError::not_ready())
}

fn compiler_settings(roles: &Value) -> Result<Value, ApiError> {
    let rows = roles.as_array().ok_or_else(ApiError::membership)?;
    let first = rows.first().and_then(Value::as_object).ok_or_else(ApiError::membership)?;
    let project = required_text(first, "project")?;
    let ontology_version = required_text(first, "ontology_version")?;
    let catalog_version = required_text(first, "catalog_version")?;
    let policy = first.get("policy").cloned().ok_or_else(ApiError::membership)?;
    let knowledge = first.get("knowledge").cloned().ok_or_else(ApiError::membership)?;
    let mut schemas = BTreeSet::new();
    for row in rows {
        let object = row.as_object().ok_or_else(ApiError::membership)?;
        if required_text(object, "project")? != project
            || required_text(object, "ontology_version")? != ontology_version
            || required_text(object, "catalog_version")? != catalog_version
            || object.get("policy") != Some(&policy)
            || object.get("knowledge") != Some(&knowledge)
        {
            return Err(ApiError::membership());
        }
        let datasets = object.get("datasets").and_then(Value::as_array).ok_or_else(ApiError::membership)?;
        for dataset in datasets {
            let name = dataset.get("dataset").and_then(Value::as_str).ok_or_else(ApiError::membership)?;
            let schema = name.split('.').next().ok_or_else(ApiError::membership)?;
            if !crate::ids::ident_ok(schema) {
                return Err(ApiError::membership());
            }
            schemas.insert(schema.to_owned());
        }
    }
    if schemas.is_empty() {
        return Err(ApiError::membership());
    }
    Ok(json!({
        "project": project,
        "ontology_version": ontology_version,
        "catalog_version": catalog_version,
        "policy": policy,
        "knowledge": knowledge,
        "ontology": {
            "spec_version": "oos/v1",
            "name": project,
            "locale": "en-US",
            "include_schemas": schemas.into_iter().collect::<Vec<_>>(),
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": ["dataset_descriptions", "column_descriptions", "caveats"]},
            "classification": {"auto_accept": "not_authorized"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    }))
}

fn required_text(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, ApiError> {
    object.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned).ok_or_else(ApiError::membership)
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
        if !output.secret_leak {
            let mut text = String::from_utf8_lossy(&output.stderr).replace(['\n', '\r'], " ");
            if text.len() > 240 {
                text.truncate(240);
            }
            let _ = write_private(&PathBuf::from(&config.work_dir).join("pack-child.err"), text.as_bytes());
        }
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
