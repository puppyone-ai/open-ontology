//! Server-side sealed inputs for one self-build.
//!
//! The directory is operator configuration. This path does not open the source
//! password file and it does not spawn the foreign catalog, rules, or
//! suggestions binaries. On the adapted path, S0 calls the existing
//! `pg-builder` seal and cards functions. A compiler refusal is not written as
//! `seal.json`. S5 writes the existing `pg-builder` config. A missing bearer
//! file does not spawn. A present bearer file calls `approve`, `generate`, and
//! `curated` through `builder_call`.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio_postgres::Client;

use crate::canonical::{canonical_json, sha256_hex};
use crate::config::{binaries_ready, regular_file, Binaries, ProjectCfg};
use crate::error::ApiError;
use crate::jsonutil::parse_exact;
use crate::oosyaml::generated_from_inputs;
use crate::overlay;
use crate::store::{self, BuildRow};

pub const BUNDLE_FILES: [&str; 8] = [
    "catalog.json",
    "profile.json",
    "read_logic.json",
    "source_semantics.json",
    "rules.json",
    "suggestions.json",
    "generation.json",
    "roles.json",
];

const BUNDLE_CONTRACT: &str = "oo.selfbuild-control-api/sealed-bundle/v1";

struct Bundle {
    catalog: Value,
    typed_catalog: Value,
    rules: Value,
    suggestions: Value,
    generation_outcome: String,
    roles: BTreeMap<String, Vec<String>>,
    file_sha: BTreeMap<String, String>,
    raw: BTreeMap<String, Vec<u8>>,
    source_adapted: bool,
    receipt: Option<Value>,
    basis_bytes: Vec<Vec<u8>>,
    audit_bytes: Vec<Vec<u8>>,
}

pub fn bundle_ready(dir: &str) -> bool {
    regular_file(&manifest_path(dir))
}

pub fn admits_create(project: &ProjectCfg, binaries: &Binaries) -> bool {
    if project.sealed_bundle.is_empty() {
        binaries_ready(binaries)
    } else {
        bundle_ready(&project.sealed_bundle)
    }
}

pub fn write_manifest(dir: &Path) -> Result<(), ApiError> {
    let mut files = serde_json::Map::new();
    for name in BUNDLE_FILES {
        let bytes = std::fs::read(dir.join(name)).map_err(|_| ApiError::malformed())?;
        files.insert(name.to_owned(), json!(sha256_hex(&bytes)));
    }
    for optional in ["comments.json", "native-model.json"] {
        if dir.join(optional).is_file() {
            let bytes = read_regular(&dir.join(optional))?;
            files.insert(optional.to_owned(), json!(sha256_hex(&bytes)));
        }
    }
    let manifest = json!({
        "contract": BUNDLE_CONTRACT,
        "files": Value::Object(files)
    });
    let bytes = serde_json::to_vec(&manifest).map_err(|_| ApiError::malformed())?;
    write_private(&dir.join("manifest.json"), &bytes).map_err(|_| ApiError::malformed())
}

pub fn bundle_outcome(dir: &str) -> Result<String, ApiError> {
    Ok(load(dir)?.generation_outcome)
}

pub fn role_columns(dir: &str, protocol: Option<&crate::config::SourceProtocol>, role: &str) -> Result<Vec<String>, ApiError> {
    let bundle = load_for(dir, protocol)?;
    bundle.roles.get(role).cloned().ok_or_else(ApiError::membership)
}

pub fn declared_money_units(dir: &str, protocol: Option<&crate::config::SourceProtocol>) -> Result<BTreeMap<String, String>, ApiError> {
    let bundle = load_for(dir, protocol)?;
    money_units(&bundle.rules)
}

fn money_units(rules: &Value) -> Result<BTreeMap<String, String>, ApiError> {
    let mut units = BTreeMap::new();
    for rule in rules.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)? {
        if rule.get("semantic_type").and_then(Value::as_str) != Some("money") { continue; }
        let column = rule.get("column").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let unit = rule.get("unit").and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or_else(ApiError::malformed)?;
        if units.insert(column.to_owned(), unit.to_owned()).is_some() { return Err(ApiError::malformed()); }
    }
    Ok(units)
}

fn project_declared_money(rows: &mut [Value], rules: &Value) -> Result<Value, ApiError> {
    let mut by_id = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let dataset = row.get("dataset").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let column = row.get("column").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        if by_id.insert(format!("{dataset}.{column}"), index).is_some() { return Err(ApiError::malformed()); }
    }
    let declared = rules.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut money = std::collections::BTreeSet::new();
    let mut corrections = Vec::new();
    for rule in declared {
        let id = rule.get("column").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        if !seen.insert(id) { return Err(ApiError::malformed()); }
        let index = *by_id.get(id).ok_or_else(ApiError::malformed)?;
        if rule.get("semantic_type").and_then(Value::as_str) != Some("money") { continue; }
        money.insert(id);
        let unit = rule.get("unit").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(ApiError::malformed)?;
        let pii = rule.get("pii").and_then(Value::as_str).filter(|s| matches!(*s, "unknown" | "none" | "personal" | "sensitive" | "restricted")).ok_or_else(ApiError::malformed)?;
        let original = rows[index].clone();
        rows[index]["semantic_type"] = json!("money");
        rows[index]["unit"] = json!(unit);
        rows[index]["pii"] = json!(pii);
        if rows[index] != original {
            corrections.push(json!({"column": id, "model_candidate": original, "source_declared": rows[index]}));
        }
    }
    for row in rows.iter() {
        let id = format!("{}.{}", row["dataset"].as_str().unwrap_or(""), row["column"].as_str().unwrap_or(""));
        if row.get("semantic_type").and_then(Value::as_str) == Some("money") && !money.contains(id.as_str()) {
            return Err(ApiError::malformed());
        }
    }
    Ok(json!({"contract":"oo.selfbuild-control-api/source-money-projection/v1", "provenance":"sealed_source_rule", "corrections":corrections}))
}

pub fn all_roles(dir: &str, protocol: Option<&crate::config::SourceProtocol>) -> Result<BTreeMap<String, Vec<String>>, ApiError> {
    Ok(load_for(dir, protocol)?.roles)
}

pub struct ModelLink<'a> {
    pub pg_builder: &'a str,
    pub catalog_bin: &'a str,
    pub model_cache_dir: &'a str,
    pub reviewer_label: &'a str,
}

pub async fn run(
    client: &Client,
    project: &ProjectCfg,
    job: &BuildRow,
    stage: &str,
    dir: &Path,
    cancel: &Arc<AtomicBool>,
    model: &ModelLink<'_>,
) -> bool {
    if cancel.load(Ordering::SeqCst) {
        return false;
    }
    let bundle = match load_for(&project.sealed_bundle, project.source_protocol.as_ref()) {
        Ok(bundle) => bundle,
        Err(_) => return fail(client, job, stage, "sealed.malformed").await,
    };
    if let Some(input) = bundle.raw.get("native-model.json").and_then(|b| serde_json::from_slice::<Value>(b).ok())
        && (input.pointer("/settings/project").and_then(Value::as_str) != Some(project.project_id.as_str()) || input.pointer("/visibility/project").and_then(Value::as_str) != Some(project.project_id.as_str())) {
        return fail(client, job, stage, "project.mismatch").await;
    }
    if !tables_match(project, &bundle.catalog) {
        return fail(client, job, stage, "catalog.mismatch").await;
    }
    match stage {
        "S0" => s0(client, job, dir, &bundle).await,
        "S1" => s1(client, job, dir, &bundle).await,
        "S2" => s2(client, project, job, dir, &bundle).await,
        "S3" | "S4" => {
            let mut body = json!({"rules_sha256": bundle.file_sha.get("rules.json"), "stage": stage});
            if bundle.source_adapted {
                let Some(binding) = prepare_binding(dir, &bundle) else {
                    return fail(client, job, stage, "builder.prepare_missing").await;
                };
                attach(&mut body, &binding);
            }
            mark(client, job, dir, stage, &body).await
        }
        "S5" => s5(client, project, job, dir, &bundle, model, cancel).await,
        "S6" | "S7" | "S8" => s6(client, job, dir, &bundle).await,
        "S9" | "S10" => {
            let _ = store::set_stage(client, &job.build_id, stage, "awaiting", &job.input_sha256, None, None).await;
            true
        }
        _ => false,
    }
}

async fn s0(client: &Client, job: &BuildRow, dir: &Path, bundle: &Bundle) -> bool {
    if write_private(&dir.join("catalog.json"), bundle.raw.get("catalog.json").map(Vec::as_slice).unwrap_or(b"")).is_err() {
        return fail(client, job, "S0", "catalog.write").await;
    }
    if let Some(bytes) = bundle.raw.get("native-model.json")
        && write_private(&dir.join("native-model.json"), bytes).is_err() {
        return fail(client, job, "S0", "config.write").await;
    }
    let mut response = json!({
        "catalog_sha256": bundle.file_sha.get("catalog.json"),
        "executed": false,
        "read_logic_sha256": bundle.file_sha.get("read_logic.json"),
        "source_semantics_sha256": bundle.file_sha.get("source_semantics.json"),
        "stage": "S0",
        "tables": table_projection(&bundle.catalog)
    });
    if let Some(receipt) = &bundle.receipt {
        response["source_adapt"] = receipt.clone();
    }
    if bundle.source_adapted {
        let basis_refs: Vec<&[u8]> = bundle.basis_bytes.iter().map(Vec::as_slice).collect();
        let audit_refs: Vec<&[u8]> = bundle.audit_bytes.iter().map(Vec::as_slice).collect();
        let prepared = crate::builder_inputs::prepare_observed(
            bundle.raw.get("catalog.json").map(Vec::as_slice).unwrap_or(b""),
            bundle.raw.get("profile.json").map(Vec::as_slice).unwrap_or(b""),
            bundle.raw.get("comments.json").map(Vec::as_slice),
            &basis_refs,
            &audit_refs,
            16,
        );
        if write_private(&dir.join("builder-prepare.json"), &prepared.document).is_err() {
            return fail(client, job, "S0", "builder.prepare_write").await;
        }
        if let Some(seal) = &prepared.seal
            && write_private(&dir.join("seal.json"), seal).is_err()
        {
            return fail(client, job, "S0", "seal.write").await;
        }
        if let Some(cards) = &prepared.cards
            && write_private(&dir.join("cards.json"), cards).is_err()
        {
            return fail(client, job, "S0", "cards.write").await;
        }
        let Some(binding) = prepare_binding(dir, bundle) else {
            return fail(client, job, "S0", "builder.prepare_missing").await;
        };
        attach(&mut response, &binding);
    }
    let request = json!({
        "executed": false,
        "files": ["catalog.json", "read_logic.json", "source_semantics.json"],
        "stage": "S0"
    });
    finish(client, job, dir, "S0", &request, &response, None).await
}

async fn s1(client: &Client, job: &BuildRow, dir: &Path, bundle: &Bundle) -> bool {
    if write_private(&dir.join("profile.json"), bundle.raw.get("profile.json").map(Vec::as_slice).unwrap_or(b"")).is_err() {
        return fail(client, job, "S1", "profile.write").await;
    }
    let mut request = json!({"stage": "S1", "profile_sha256": bundle.file_sha.get("profile.json")});
    let mut response = json!({"ok": true, "profile_sha256": bundle.file_sha.get("profile.json"), "stage": "S1"});
    if bundle.source_adapted {
        let Some(binding) = prepare_binding(dir, bundle) else {
            return fail(client, job, "S1", "builder.prepare_missing").await;
        };
        attach(&mut request, &binding);
        attach(&mut response, &binding);
    }
    finish(client, job, dir, "S1", &request, &response, None).await
}

async fn s2(client: &Client, project: &ProjectCfg, job: &BuildRow, dir: &Path, bundle: &Bundle) -> bool {
    let auto_accept = if bundle.source_adapted { "none" } else { project.rules_auto_accept.as_str() };
    let yaml = match generated_from_inputs(&bundle.typed_catalog, &bundle.rules, auto_accept) {
        Ok((_, yaml)) => yaml,
        Err(_) => return fail(client, job, "S2", "overlay.malformed").await,
    };
    if overlay::insert_generated_rev1(client, &job.build_id, &yaml).await.is_err() {
        return fail(client, job, "S2", "dependency.not_ready").await;
    }
    if write_private(&dir.join("generated.yaml"), yaml.as_bytes()).is_err() {
        return fail(client, job, "S2", "overlay.write").await;
    }
    let mut request = json!({"rules_sha256": bundle.file_sha.get("rules.json"), "stage": "S2"});
    let mut response = json!({"generated_sha256": sha256_hex(yaml.as_bytes()), "llm_calls": 0, "stage": "S2"});
    if let Some(receipt) = &bundle.receipt {
        response["auto_approved"] = json!(false);
        response["unknown_auto_approved"] = json!(false);
        response["withheld_columns"] = receipt.get("withheld_columns").cloned().unwrap_or(json!(0));
    }
    if bundle.source_adapted {
        let Some(binding) = prepare_binding(dir, bundle) else {
            return fail(client, job, "S2", "builder.prepare_missing").await;
        };
        attach(&mut request, &binding);
        attach(&mut response, &binding);
    }
    finish(client, job, dir, "S2", &request, &response, None).await
}

async fn s5(
    client: &Client,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    bundle: &Bundle,
    model: &ModelLink<'_>,
    cancel: &Arc<AtomicBool>,
) -> bool {
    if bundle.source_adapted {
        return s5_model(client, project, job, dir, bundle, model, cancel).await;
    }
    let outcome = bundle.generation_outcome.as_str();
    let request = json!({"llm_calls": 0, "outcome": outcome, "stage": "S5"});
    let response = json!({"llm_calls": 0, "outcome": outcome, "stage": "S5"});
    if outcome != "structural_draft" {
        let _ = write_cards(dir, false, outcome);
        let _ = write_pair(dir, "S5", &request, &response);
        let detail = match outcome {
            "model_unauthorized" => "model.unauthorized",
            "model_timeout" => "model.timeout",
            _ => "sealed.malformed",
        };
        return fail(client, job, "S5", detail).await;
    }
    let _ = write_cards(dir, false, outcome);
    finish(client, job, dir, "S5", &request, &response, Some("structural_draft")).await
}

async fn s5_model(
    client: &Client,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    bundle: &Bundle,
    model: &ModelLink<'_>,
    cancel: &Arc<AtomicBool>,
) -> bool {
    let mut model_config = crate::stages::builder_config(project, model.catalog_bin);
    if let Some(operator) = bundle.raw.get("native-model.json").and_then(|b| serde_json::from_slice::<Value>(b).ok())
        && let Some(price) = operator.get("declared_price") {
        model_config["declared_price"] = price.clone();
    }
    let config_bytes = serde_json::to_vec(&model_config).unwrap_or_default();
    if write_private(&dir.join("pg-builder.config.json"), &config_bytes).is_err() {
        return fail(client, job, "S5", "config.write").await;
    }
    let present = crate::builder_call::bearer_metadata_ok(&project.llm.bearer_file);
    if !present {
        let Some(binding) = prepare_binding(dir, bundle) else {
            return fail(client, job, "S5", "builder.prepare_missing").await;
        };
        let prepare_sha = binding.get("builder_prepare_sha256").cloned().unwrap_or(Value::Null);
        let seal_written = binding.get("seal_written").cloned().unwrap_or(json!(false));
        let cards_written = binding.get("cards_written").cloned().unwrap_or(json!(false));
        let request = json!({
            "bearer_present": false,
            "builder_argv": ["approve", "generate", "curated"],
            "builder_interface": "oo.pg-builder.config/v1",
            "builder_prepare_sha256": prepare_sha.clone(),
            "builder_spawned": false,
            "cards_written": cards_written.clone(),
            "llm_calls": 0,
            "seal_written": seal_written.clone(),
            "stage": "S5"
        });
        let response = json!({
            "builder_prepare_sha256": prepare_sha,
            "builder_spawned": false,
            "cards_written": cards_written,
            "llm_calls": 0,
            "outcome": "model_blocked",
            "published": false,
            "ready": false,
            "seal_written": seal_written,
            "stage": "S5",
            "structural_draft": false
        });
        if !producer_cards(dir) {
            let _ = write_cards(dir, false, "model_blocked");
        }
        let Ok(hash) = write_pair(dir, "S5", &request, &response) else {
            return fail(client, job, "S5", "stage.write").await;
        };
        return store::set_stage(client, &job.build_id, "S5", "awaiting", &job.input_sha256, Some(&hash), Some("model.blocked"))
            .await
            .unwrap_or(false);
    }
    let spec = crate::builder_call::InvokeSpec {
        binary: model.pg_builder.to_owned(),
        bearer_file: project.llm.bearer_file.clone(),
        config_path: dir.join("pg-builder.config.json"),
        seal_path: dir.join("seal.json"),
        cards_path: dir.join("cards.json"),
        approval_path: dir.join("card-approval.json"),
        cache_path: dir.join("cache"),
        generation_path: dir.join("generation.json"),
        catalog_path: dir.join("catalog.json"),
        curated_path: dir.join("curated.json"),
        reviewer_label: model.reviewer_label.to_owned(),
        reviewer_at: crate::worker::utc_date(),
        model_cache_dir: model.model_cache_dir.to_owned(),
        cancel: cancel.clone(),
    };
    let result = crate::builder_call::invoke(spec).await;
    if result.spawned {
        let _ = store::mark_child_exited(client, &job.build_id).await;
    }
    match result.kind {
        crate::builder_call::ModelKind::Blocked => {
            let request = json!({"bearer_present": false, "builder_spawned": false, "llm_calls": 0, "stage": "S5"});
            let response = json!({
                "builder_spawned": false,
                "llm_calls": 0,
                "outcome": "model_blocked",
                "published": false,
                "ready": false,
                "stage": "S5",
                "structural_draft": false
            });
            if !producer_cards(dir) {
                let _ = write_cards(dir, false, "model_blocked");
            }
            let Ok(hash) = write_pair(dir, "S5", &request, &response) else {
                return fail(client, job, "S5", "stage.write").await;
            };
            store::set_stage(client, &job.build_id, "S5", "awaiting", &job.input_sha256, Some(&hash), Some("model.blocked"))
                .await
                .unwrap_or(false)
        }
        crate::builder_call::ModelKind::Canceled => false,
        crate::builder_call::ModelKind::Failed { detail } => {
            let request = json!({"bearer_present": true, "builder_spawned": result.spawned, "llm_calls": 0, "stage": "S5"});
            let response = json!({
                "builder_spawned": result.spawned,
                "llm_calls": 0,
                "outcome": detail,
                "published": false,
                "ready": false,
                "stage": "S5",
                "structural_draft": false
            });
            let _ = write_cards(dir, false, detail);
            let _ = write_pair(dir, "S5", &request, &response);
            fail(client, job, "S5", detail).await
        }
        crate::builder_call::ModelKind::Validated { generation_sha256, llm_calls, cache_hit } => {
            if bundle.raw.get("catalog.json").and_then(|b| serde_json::from_slice::<Value>(b).ok()).and_then(|v| v.get("protocol").and_then(Value::as_str).map(str::to_owned)).as_deref().is_some_and(|protocol| matches!(protocol, "oo.pipeline.sealed-import-catalog/v1" | "oo.pipeline.sealed-import-collection/v1")) {
                let rows = std::fs::read(dir.join("native-model-output/decisions.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                let Some(mut rows) = rows.and_then(|v| v.as_array().cloned()) else { return fail(client, job, "S5", "model.candidates_absent").await; };
                let Ok(mut projection) = project_declared_money(&mut rows, &bundle.rules) else { return fail(client, job, "S5", "model.candidates_invalid").await; };
                projection["model_decisions_sha256"] = json!(std::fs::read(dir.join("native-model-output/decisions.json")).ok().map(|bytes| sha256_hex(&bytes)).unwrap_or_default());
                projection["source_rules_sha256"] = json!(bundle.file_sha.get("rules.json"));
                let rules = json!({"columns":rows.iter().map(|v| json!({"column":format!("{}.{}",v.get("dataset").and_then(Value::as_str).unwrap_or(""),v.get("column").and_then(Value::as_str).unwrap_or("")),"semantic_type":v.get("semantic_type"),"pii":v.get("pii"),"unit":v.get("unit").and_then(Value::as_str).unwrap_or(""),"confidence":"high"})).collect::<Vec<_>>(),"relationships":bundle.rules.get("relationships").cloned().unwrap_or(json!([]))});
                let Ok((_, yaml)) = generated_from_inputs(&bundle.typed_catalog, &rules, "none") else { return fail(client, job, "S5", "model.candidates_invalid").await; };
                if write_private(&dir.join("source-money-projection.json"), canonical_json(&projection).as_bytes()).is_err() || overlay::insert_generated_model(client, &job.build_id, &yaml).await.is_err() || write_private(&dir.join("generated.yaml"), yaml.as_bytes()).is_err() { return fail(client, job, "S5", "overlay.write").await; }
            }
            let request = json!({"bearer_present": true, "builder_spawned": true, "llm_calls": llm_calls, "stage": "S5"});
            let response = json!({
                "builder_spawned": true,
                "cache_hit": cache_hit,
                "generation_sha256": generation_sha256,
                "llm_calls": llm_calls,
                "outcome": "validated",
                "published": false,
                "ready": false,
                "stage": "S5",
                "structural_draft": false
            });
            let _ = write_cards(dir, false, "validated");
            finish(client, job, dir, "S5", &request, &response, None).await
        }
    }
}

async fn s6(client: &Client, job: &BuildRow, dir: &Path, bundle: &Bundle) -> bool {
    let ready = bundle.suggestions.get("ready") == Some(&Value::Bool(true));
    let merged = bundle.suggestions.get("merged") == Some(&Value::Bool(true));
    let certified = bundle.suggestions.get("certified") == Some(&Value::Bool(true));
    if ready || merged || certified {
        let _ = write_cards(dir, false, "suggestions_refused");
        return fail(client, job, "S6", "suggestions.refused").await;
    }
    if write_private(&dir.join("suggestions.json"), bundle.raw.get("suggestions.json").map(Vec::as_slice).unwrap_or(b"")).is_err() {
        return fail(client, job, "S6", "suggestions.write").await;
    }
    let request = json!({"certified": false, "merged": false, "ready": false, "stage": "S6"});
    let response = json!({
        "certified": false,
        "llm_calls": 0,
        "merged": false,
        "ready": false,
        "stage": "S6",
        "suggestions_sha256": bundle.file_sha.get("suggestions.json")
    });
    let hash = match write_pair(dir, "S6", &request, &response) {
        Ok(hash) => hash,
        Err(()) => return fail(client, job, "S6", "stage.write").await,
    };
    for stage in ["S6", "S7", "S8"] {
        let _ = write_pair(dir, stage, &request, &response);
        if !store::set_stage(client, &job.build_id, stage, "succeeded", &job.input_sha256, Some(&hash), None)
            .await
            .unwrap_or(false)
        {
            return false;
        }
    }
    let _ = store::set_stage(client, &job.build_id, "S10", "awaiting", &job.input_sha256, None, None).await;
    let _ = store::set_stage(client, &job.build_id, "S9", "awaiting", &job.input_sha256, None, None).await;
    let _ = write_cards(dir, false, "structural_draft");
    true
}

async fn mark(client: &Client, job: &BuildRow, dir: &Path, stage: &str, body: &Value) -> bool {
    finish(client, job, dir, stage, body, body, None).await
}

async fn finish(
    client: &Client,
    job: &BuildRow,
    dir: &Path,
    stage: &str,
    request: &Value,
    response: &Value,
    detail: Option<&str>,
) -> bool {
    let Ok(hash) = write_pair(dir, stage, request, response) else {
        return fail(client, job, stage, "stage.write").await;
    };
    store::set_stage(client, &job.build_id, stage, "succeeded", &job.input_sha256, Some(&hash), detail)
        .await
        .unwrap_or(false)
}

async fn fail(client: &Client, job: &BuildRow, stage: &str, detail: &str) -> bool {
    let _ = store::set_stage(client, &job.build_id, stage, "failed", &job.input_sha256, None, Some(detail)).await;
    false
}

fn load(dir: &str) -> Result<Bundle, ApiError> {
    load_for(dir, None)
}

fn load_for(dir: &str, protocol: Option<&crate::config::SourceProtocol>) -> Result<Bundle, ApiError> {
    let root = PathBuf::from(dir);
    if !bundle_ready(dir) {
        return Err(ApiError::not_ready());
    }
    let manifest_bytes = read_regular(&root.join("manifest.json"))?;
    let manifest = parse_exact(&manifest_bytes)?;
    let object = manifest.as_object().ok_or_else(ApiError::malformed)?;
    if object.len() != 2 || object.get("contract").and_then(Value::as_str) != Some(BUNDLE_CONTRACT) {
        return Err(ApiError::malformed());
    }
    let files = object.get("files").and_then(Value::as_object).ok_or_else(ApiError::malformed)?;
    if files.len() != BUNDLE_FILES.len() + usize::from(files.contains_key("comments.json")) + usize::from(files.contains_key("native-model.json")) {
        return Err(ApiError::malformed());
    }
    let mut file_sha = BTreeMap::new();
    let mut raw = BTreeMap::new();
    for name in BUNDLE_FILES {
        let expected = files.get(name).and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        if !crate::ids::hex64(expected) {
            return Err(ApiError::malformed());
        }
        let bytes = read_regular(&root.join(name))?;
        let actual = sha256_hex(&bytes);
        if actual != expected {
            return Err(ApiError::malformed());
        }
        file_sha.insert(name.to_owned(), actual);
        raw.insert(name.to_owned(), bytes);
    }
    for optional in ["comments.json", "native-model.json"] {
      if let Some(expected) = files.get(optional) {
        let expected = expected.as_str().ok_or_else(ApiError::malformed)?;
        let bytes = read_regular(&root.join(optional))?;
        let actual = sha256_hex(&bytes);
        if actual != expected { return Err(ApiError::malformed()); }
        parse_exact(&bytes)?;
        file_sha.insert(optional.to_owned(), actual);
        raw.insert(optional.to_owned(), bytes);
      }
    }
    let catalog = parse_exact(raw.get("catalog.json").ok_or_else(ApiError::malformed)?)?;
    let profile = parse_exact(raw.get("profile.json").ok_or_else(ApiError::malformed)?)?;
    let read_logic = parse_exact(raw.get("read_logic.json").ok_or_else(ApiError::malformed)?)?;
    let semantics = parse_exact(raw.get("source_semantics.json").ok_or_else(ApiError::malformed)?)?;
    let rules = parse_exact(raw.get("rules.json").ok_or_else(ApiError::malformed)?)?;
    let suggestions = parse_exact(raw.get("suggestions.json").ok_or_else(ApiError::malformed)?)?;
    let generation = parse_exact(raw.get("generation.json").ok_or_else(ApiError::malformed)?)?;
    let roles_value = parse_exact(raw.get("roles.json").ok_or_else(ApiError::malformed)?)?;
    reject_sample(&rules)?;
    if read_logic.get("executed") != Some(&Value::Bool(false)) {
        return Err(ApiError::malformed());
    }
    let mut basis_bytes = Vec::new();
    let mut audit_bytes = Vec::new();
    let (catalog, typed_catalog, source_adapted, receipt) = if let Some(protocol) = protocol {
        let bases = read_basis(protocol)?;
        audit_bytes = read_audits(protocol)?;
        basis_bytes = bases.iter().map(|item| item.bytes.clone()).collect();
        let pin = crate::adapt::ProtocolPin {
            catalog: protocol.catalog.clone(),
            profile: protocol.profile.clone(),
            semantics: protocol.semantics.clone(),
            source_commit: protocol.source_commit.clone(),
            semantics_commit: protocol.semantics_commit.clone(),
        };
        let projection = if protocol.catalog == "oo.pipeline.sealed-import-collection/v1" {
            crate::adapt::project_native_collection(raw.get("catalog.json").ok_or_else(ApiError::malformed)?, raw.get("profile.json").ok_or_else(ApiError::malformed)?, raw.get("source_semantics.json").ok_or_else(ApiError::malformed)?, &pin)?
        } else if protocol.catalog == "oo.pipeline.sealed-import-catalog/v1" {
            crate::adapt::project_native(raw.get("catalog.json").ok_or_else(ApiError::malformed)?, raw.get("profile.json").ok_or_else(ApiError::malformed)?, raw.get("source_semantics.json").ok_or_else(ApiError::malformed)?, &pin)?
        } else if protocol.catalog == crate::adapt::CAPTURE_CATALOG {
            crate::adapt::project(
                raw.get("catalog.json").ok_or_else(ApiError::malformed)?,
                raw.get("profile.json").ok_or_else(ApiError::malformed)?,
                raw.get("source_semantics.json").ok_or_else(ApiError::malformed)?,
                &pin,
                &bases,
            )?
        } else {
            crate::adapt::project_measured(
                raw.get("catalog.json").ok_or_else(ApiError::malformed)?,
                raw.get("profile.json").ok_or_else(ApiError::malformed)?,
                raw.get("source_semantics.json").ok_or_else(ApiError::malformed)?,
                &pin,
            )?
        };
        let role_catalog = role_catalog_from_receipt(&projection.receipt)?;
        (role_catalog, projection.typed_catalog, true, Some(projection.receipt))
    } else {
        require_object(&profile)?;
        require_object(&semantics)?;
        if semantics.get("executed").is_some_and(|value| value != &Value::Bool(false)) {
            return Err(ApiError::malformed());
        }
        let typed_catalog = catalog.clone();
        (catalog, typed_catalog, false, None)
    };
    let outcome = generation.get("outcome").and_then(Value::as_str).unwrap_or("");
    if !matches!(outcome, "structural_draft" | "model_unauthorized" | "model_timeout") || generation.get("llm_calls") != Some(&json!(0)) {
        return Err(ApiError::malformed());
    }
    for key in ["certified", "merged", "ready"] {
        if !suggestions.get(key).is_some_and(Value::is_boolean) {
            return Err(ApiError::malformed());
        }
    }
    if source_adapted {
        validate_named_catalog(&catalog)?;
        validate_catalog(&typed_catalog)?;
    } else {
        validate_catalog(&catalog)?;
    }
    let roles = parse_roles(&roles_value, &catalog)?;
    if let Some(bytes) = raw.get("native-model.json") {
        let input = parse_exact(bytes)?;
        let visibility = input.get("visibility").ok_or_else(ApiError::malformed)?;
        let role = visibility.get("role").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let allowed = roles.get(role).ok_or_else(ApiError::malformed)?;
        let datasets = visibility.get("datasets").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
        for dataset in datasets {
            let name = dataset.get("dataset").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
            for column in dataset.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)? {
                let column = column.as_str().ok_or_else(ApiError::malformed)?;
                if !allowed.contains(&format!("{name}.{column}")) { return Err(ApiError::malformed()); }
            }
        }
    }

    Ok(Bundle {
        catalog,
        typed_catalog,
        rules,
        suggestions,
        generation_outcome: outcome.to_owned(),
        roles,
        file_sha,
        raw,
        source_adapted,
        receipt,
        basis_bytes,
        audit_bytes,
    })
}

fn read_audits(protocol: &crate::config::SourceProtocol) -> Result<Vec<Vec<u8>>, ApiError> {
    let mut audits = Vec::new();
    for item in &protocol.schema_audit {
        let bytes = read_regular(Path::new(&item.path))?;
        if sha256_hex(&bytes) != item.sha256 {
            return Err(ApiError::malformed());
        }
        audits.push(bytes);
    }
    Ok(audits)
}

fn read_basis(protocol: &crate::config::SourceProtocol) -> Result<Vec<crate::adapt::BasisPin>, ApiError> {
    let mut bases = Vec::new();
    for item in &protocol.type_basis {
        let bytes = read_regular(Path::new(&item.path))?;
        if sha256_hex(&bytes) != item.sha256 {
            return Err(ApiError::malformed());
        }
        bases.push(crate::adapt::BasisPin {
            protocol: item.protocol.clone(),
            outer_protocol: item.outer_protocol.clone(),
            sha256: item.sha256.clone(),
            bytes,
        });
    }
    Ok(bases)
}

fn role_catalog_from_receipt(receipt: &Value) -> Result<Value, ApiError> {
    let columns = receipt.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    let mut tables: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    for column in columns {
        let dataset = column.get("dataset").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let name = column.get("column").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let Some((schema, table)) = dataset.split_once('.') else {
            return Err(ApiError::malformed());
        };
        let mut object = json!({"name": name});
        if let Some(data_type) = column.get("data_type").and_then(Value::as_str) {
            object["data_type"] = json!(data_type);
        }
        tables.entry((schema.to_owned(), table.to_owned())).or_default().push(object);
    }
    let mut listed = Vec::new();
    for ((schema, table), columns) in tables {
        listed.push(json!({"columns": columns, "schema": schema, "table": table}));
    }
    Ok(json!({"tables": listed}))
}

fn validate_named_catalog(catalog: &Value) -> Result<(), ApiError> {
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    if tables.is_empty() {
        return Err(ApiError::malformed());
    }
    for table in tables {
        let schema = table.get("schema").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let name = table.get("table").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        if !crate::ids::ident_ok(schema) || !crate::ids::source_ident_ok(name) {
            return Err(ApiError::malformed());
        }
        let columns = table.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
        if columns.is_empty() {
            return Err(ApiError::malformed());
        }
        for column in columns {
            let column_name = column.get("name").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
            if !crate::ids::source_ident_ok(column_name) {
                return Err(ApiError::malformed());
            }
            if let Some(data_type) = column.get("data_type").filter(|value| !value.is_null()) {
                let Some(text) = data_type.as_str() else {
                    return Err(ApiError::malformed());
                };
                if text.is_empty() {
                    return Err(ApiError::malformed());
                }
            }
        }
    }
    Ok(())
}

fn validate_catalog(catalog: &Value) -> Result<(), ApiError> {
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    if tables.is_empty() {
        return Err(ApiError::malformed());
    }
    for table in tables {
        let schema = table.get("schema").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let name = table.get("table").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        if !crate::ids::ident_ok(schema) || !crate::ids::source_ident_ok(name) {
            return Err(ApiError::malformed());
        }
        if table.get("reltuples").is_some() && reltuples(table).is_none() {
            return Err(ApiError::malformed());
        }
        let columns = table.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
        if columns.is_empty() {
            return Err(ApiError::malformed());
        }
        for column in columns {
            let column_name = column.get("name").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
            let data_type = column.get("data_type").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
            if !crate::ids::source_ident_ok(column_name) || data_type.is_empty() {
                return Err(ApiError::malformed());
            }
        }
    }
    Ok(())
}

fn parse_roles(value: &Value, catalog: &Value) -> Result<BTreeMap<String, Vec<String>>, ApiError> {
    let object = value.as_object().ok_or_else(ApiError::malformed)?;
    if object.get("contract").and_then(Value::as_str) != Some("oo.selfbuild-control-api/sealed-roles/v1") {
        return Err(ApiError::malformed());
    }
    let roles = object.get("roles").and_then(Value::as_object).ok_or_else(ApiError::malformed)?;
    if roles.is_empty() || object.len() != 2 {
        return Err(ApiError::malformed());
    }
    let mut out = BTreeMap::new();
    for (role, columns) in roles {
        if !crate::ids::ident_ok(role) {
            return Err(ApiError::malformed());
        }
        let Some(items) = columns.as_array() else {
            return Err(ApiError::malformed());
        };
        if items.is_empty() {
            return Err(ApiError::malformed());
        }
        let mut names = Vec::new();
        for item in items {
            let id = item.as_str().ok_or_else(ApiError::malformed)?;
            if !column_in_catalog(catalog, id) {
                return Err(ApiError::malformed());
            }
            names.push(id.to_owned());
        }
        out.insert(role.clone(), names);
    }
    Ok(out)
}

fn column_in_catalog(catalog: &Value, id: &str) -> bool {
    let Some((dataset, column)) = id.rsplit_once('.') else {
        return false;
    };
    let Some((schema, table)) = dataset.split_once('.') else {
        return false;
    };
    catalog
        .get("tables")
        .and_then(Value::as_array)
        .is_some_and(|tables| {
            tables.iter().any(|item| {
                item.get("schema").and_then(Value::as_str) == Some(schema)
                    && item.get("table").and_then(Value::as_str) == Some(table)
                    && item
                        .get("columns")
                        .and_then(Value::as_array)
                        .is_some_and(|columns| columns.iter().any(|column_item| column_item.get("name").and_then(Value::as_str) == Some(column)))
            })
        })
}

fn tables_match(project: &ProjectCfg, catalog: &Value) -> bool {
    let mut expected: Vec<(String, String)> = project.source.tables.clone();
    expected.sort();
    let mut got = Vec::new();
    let Some(tables) = catalog.get("tables").and_then(Value::as_array) else {
        return false;
    };
    for table in tables {
        let Some(schema) = table.get("schema").and_then(Value::as_str) else {
            return false;
        };
        let Some(name) = table.get("table").and_then(Value::as_str) else {
            return false;
        };
        got.push((schema.to_owned(), name.to_owned()));
    }
    got.sort();
    got == expected
}

fn table_projection(catalog: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(tables) = catalog.get("tables").and_then(Value::as_array) else {
        return out;
    };
    for table in tables {
        let mut projected = json!({
            "columns": table.get("columns").cloned().unwrap_or_else(|| json!([])),
            "schema": table.get("schema").cloned().unwrap_or(Value::Null),
            "table": table.get("table").cloned().unwrap_or(Value::Null)
        });
        if let Some(count) = reltuples(table) {
            projected["reltuples"] = count;
        }
        out.push(projected);
    }
    out
}

fn reltuples(table: &Value) -> Option<Value> {
    match table.get("reltuples")? {
        Value::Number(number) if number.is_u64() => Some(Value::Number(number.clone())),
        Value::String(text) if (1..=18).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit()) => Some(Value::String(text.clone())),
        _ => None,
    }
}

fn reject_sample(value: &Value) -> Result<(), ApiError> {
    match value {
        Value::Object(map) if map.contains_key("sample") => Err(ApiError::malformed()),
        Value::Object(map) => {
            for child in map.values() {
                reject_sample(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_sample(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn require_object(value: &Value) -> Result<(), ApiError> {
    if value.as_object().is_some_and(|object| object.contains_key("contract")) {
        Ok(())
    } else {
        Err(ApiError::malformed())
    }
}

fn manifest_path(dir: &str) -> String {
    Path::new(dir).join("manifest.json").to_string_lossy().into_owned()
}

fn read_regular(path: &Path) -> Result<Vec<u8>, ApiError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ApiError::malformed())?;
    if !meta.file_type().is_file() || meta.len() > 1_000_000 {
        return Err(ApiError::malformed());
    }
    std::fs::read(path).map_err(|_| ApiError::malformed())
}

fn write_pair(dir: &Path, stage: &str, request: &Value, response: &Value) -> Result<String, ()> {
    let request_bytes = canonical_json(request).into_bytes();
    let response_bytes = canonical_json(response).into_bytes();
    write_private(&dir.join(format!("{stage}.request.json")), &request_bytes)?;
    write_private(&dir.join(format!("{stage}.response.json")), &response_bytes)?;
    Ok(sha256_hex(&response_bytes))
}

fn prepare_binding(dir: &Path, bundle: &Bundle) -> Option<Value> {
    let prepare = std::fs::read(dir.join("builder-prepare.json")).ok()?;
    if prepare.is_empty() {
        return None;
    }
    let parsed: Value = serde_json::from_slice(&prepare).ok()?;
    let seal = std::fs::read(dir.join("seal.json")).ok().filter(|bytes| !bytes.is_empty());
    let cards = std::fs::read(dir.join("cards.json")).ok().filter(|bytes| producer_cards_bytes(bytes));
    let receipt = bundle.receipt.clone().unwrap_or(Value::Null);
    Some(json!({
        "builder_prepare_sha256": sha256_hex(&prepare),
        "cards_sha256": cards.as_ref().map(|bytes| sha256_hex(bytes)),
        "cards_written": cards.is_some(),
        "classification": {"auto_approved": false, "unknown_auto_approved": false},
        "llm_calls": 0,
        "loss_status": "not_in_source",
        "model": parsed.get("model").cloned().unwrap_or(json!("not_called")),
        "outcome": parsed.get("outcome").cloned().unwrap_or(Value::Null),
        "pack": parsed.get("pack").cloned().unwrap_or(json!("not_produced")),
        "published": false,
        "ready": false,
        "seal_sha256": seal.as_ref().map(|bytes| sha256_hex(bytes)),
        "seal_written": seal.is_some(),
        "semantics": {
            "consistency_grade": receipt.get("consistency_grade").cloned().unwrap_or(Value::Null),
            "delete_detection": receipt.get("delete_detection").cloned().unwrap_or(Value::Null),
            "delete_status": receipt.get("delete_status").cloned().unwrap_or(json!("unknown")),
            "grade_status": receipt.get("grade_status").cloned().unwrap_or(json!("unknown")),
            "semantics_byte_sha256": receipt.get("semantics_byte_sha256").cloned().unwrap_or(Value::Null)
        },
        "structural_draft": false
    }))
}

fn attach(body: &mut Value, extra: &Value) {
    let Some(extra) = extra.as_object() else {
        return;
    };
    let Some(body) = body.as_object_mut() else {
        return;
    };
    for (key, value) in extra {
        body.insert(key.clone(), value.clone());
    }
}

fn producer_cards(dir: &Path) -> bool {
    std::fs::read(dir.join("cards.json")).ok().is_some_and(|bytes| producer_cards_bytes(&bytes))
}

fn producer_cards_bytes(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| value.get("protocol").and_then(Value::as_str).map(str::to_owned))
        .is_some_and(|protocol| protocol == crate::builder_inputs::CARDS_PROTOCOL)
}

fn write_cards(dir: &Path, ready: bool, outcome: &str) -> Result<(), ()> {
    let body = canonical_json(&json!({"llm_calls": 0, "outcome": outcome, "published": false, "ready": ready})).into_bytes();
    let path = if producer_cards(dir) { dir.join("cards-status.json") } else { dir.join("cards.json") };
    write_private(&path, &body)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ())?;
    std::io::Write::write_all(&mut file, bytes).map_err(|_| ())
}

#[cfg(test)]
mod money_tests {
    use super::project_declared_money;
    use serde_json::json;

    #[test]
    fn declared_unknown_currency_overrides_model_without_changing_its_candidate() {
        let rules = json!({"columns":[{"column":"oo_w141.shipments.price","semantic_type":"money","unit":"unspecified","pii":"unknown"}]});
        let original = json!({"dataset":"oo_w141.shipments","column":"price","semantic_type":"money","unit":"USD","pii":"none"});
        let mut rows = vec![original.clone()];
        let receipt = project_declared_money(&mut rows, &rules).unwrap();
        assert_eq!(rows[0]["unit"], "unspecified");
        assert_eq!(rows[0]["pii"], "unknown");
        assert_eq!(receipt["corrections"][0]["model_candidate"], original);
        assert!(project_declared_money(&mut rows, &json!({"columns":[]})).is_err());
        assert!(project_declared_money(&mut rows, &json!({"columns":[rules["columns"][0], rules["columns"][0]]})).is_err());
        assert!(project_declared_money(&mut [], &rules).is_err());
        let mut invalid = rules.clone();
        invalid["columns"][0]["pii"] = json!("maybe");
        assert!(project_declared_money(&mut rows, &invalid).is_err());
    }
}
