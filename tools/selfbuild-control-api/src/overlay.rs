//! Generated, curated, and review documents. Curated is insert-only.
//! A compiled role pack is not publication.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::{json, Map, Value};
use tokio_postgres::Client;

use crate::canonical::{canonical_sha256, sha256_hex};
use crate::config::AppConfig;
use crate::error::ApiError;
use crate::ids::{hex64, ident_ok, idempotency_ok};
use crate::oosyaml::{self, apply_fact_edit, gate_report, merge, model_to_y, parse_model, sealed_types, Review, COMPILER};
use crate::sealed;
use crate::store::{self, BuildRow};

pub async fn insert_generated_rev1(client: &Client, build_id: &str, yaml: &str) -> Result<(), ApiError> {
    let sha = sha256_hex(yaml.as_bytes());
    let inserted = client
        .execute(
            "INSERT INTO oo_selfbuild_api.overlay_doc (build_id, kind, revision, doc_sha256, body)
             VALUES ($1, 'generated', 1, $2, $3)
             ON CONFLICT (build_id, kind, revision) DO NOTHING",
            &[&build_id, &sha, &yaml],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    if inserted == 1 {
        return Ok(());
    }
    let row = client
        .query_one(
            "SELECT doc_sha256 FROM oo_selfbuild_api.overlay_doc
             WHERE build_id = $1 AND kind = 'generated' AND revision = 1",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    let stored: String = row.get(0);
    if stored == sha {
        Ok(())
    } else {
        Err(ApiError::membership())
    }
}

pub async fn view(config: &AppConfig, row: &BuildRow) -> Result<Value, ApiError> {
    let client = store::connect(&config.postgres).await?;
    let docs = load_docs(&client, &row.build_id).await?;
    Ok(view_value(row, &docs))
}

pub async fn maintenance(config: &AppConfig, row: &BuildRow, value: &Value, body_sha: &str) -> Result<Value, ApiError> {
    refuse_closed(row)?;
    let (trigger, edit, note) = maintenance_fields(value)?;
    let client = store::connect(&config.postgres).await?;
    if let Some(done) = replay(&client, &row.build_id, "maintenance", key_of(value)?, body_sha).await? {
        return Ok(done);
    }
    let docs = load_docs(&client, &row.build_id).await?;
    let note_sha = note.as_deref().map(|item| sha256_hex(item.as_bytes())).unwrap_or_default();
    let mut cache_key = maintenance_key(row, trigger, edit.as_ref(), &note_sha, &docs);
    if trigger == "source_generation_advanced" { cache_key = canonical_sha256(&json!({"maintenance":cache_key,"catalog_sha256":value["catalog_sha256"],"source_semantics_sha256":value.get("source_semantics_sha256")})); }
    if trigger != "source_generation_advanced" && let Some(hit) = cache_hit(&client, &cache_key, &row.build_id, "maintenance", key_of(value)?, body_sha).await? {
        return Ok(hit);
    }
    if trigger == "schema_changed" {
        apply_edit(&client, &row.build_id, &docs, edit.ok_or_else(ApiError::malformed)?).await?;
    } else if trigger == "source_fact_corrected" {
        let units = source_money_units(config, row)?;
        apply_source_unit_correction(&client, &row.build_id, &docs, edit.ok_or_else(ApiError::malformed)?, &units).await?;
    }
    if trigger == "feedback" {
        let text = note.ok_or_else(ApiError::malformed)?;
        insert_note(&client, &row.build_id, &text).await?;
    }
    let docs = load_docs(&client, &row.build_id).await?;
    let mut response = view_value(row, &docs);
    response["operation"] = json!("postMaintenance");
    response["trigger"] = json!(trigger);
    response["stale_count"] = json!(docs.effective.stale.len());
    if trigger == "source_generation_advanced" {
        let sha = value.get("catalog_sha256").and_then(Value::as_str).filter(|s| hex64(s)).ok_or_else(ApiError::malformed)?;
        let semantics_sha = value.get("source_semantics_sha256").and_then(Value::as_str);
        let rebound = native_source_rebind(config, row, sha, semantics_sha)?;
        if let Some(previous) = latest_native_rebind(&client, row).await? {
            check_generation_progress(recorded_progress(&previous), &rebound["progress_vector"])?;
        }
        response["native_maintenance"] = compile_native_maintenance(config, row, &docs, Some(&rebound))?;
    }
    remember(&client, Remember {
        cache_key: &cache_key,
        build_id: &row.build_id,
        kind: if trigger == "feedback" { "feedback" } else { "maintenance" },
        route: "maintenance",
        idem: key_of(value)?,
        body_sha,
    }, &response).await?;
    Ok(response)
}

pub async fn curated(config: &AppConfig, row: &BuildRow, value: &Value, body_sha: &str) -> Result<Value, ApiError> {
    refuse_closed(row)?;
    let object = exact(value, &["identity_revision", "idempotency_key", "yaml"])?;
    let yaml = object.get("yaml").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
    if yaml.is_empty() || yaml.len() > 60_000 {
        return Err(ApiError::malformed());
    }
    parse_model(yaml, "curated")?;
    let client = store::connect(&config.postgres).await?;
    let idem = key_of(value)?;
    if let Some(done) = replay(&client, &row.build_id, "curated", idem, body_sha).await? {
        return Ok(done);
    }
    let before = load_docs(&client, &row.build_id).await?;
    let sha = sha256_hex(yaml.as_bytes());
    if before.curated_sha != sha {
        insert_doc(&client, &row.build_id, "curated", yaml, &sha).await?;
    }
    let docs = load_docs(&client, &row.build_id).await?;
    if docs.generated_sha != before.generated_sha {
        return Err(ApiError::membership());
    }
    let mut response = view_value(row, &docs);
    response["operation"] = json!("postCurated");
    response["curated_replaced_generated"] = json!(false);
    let cache_key = canonical_sha256(&json!({"compiler": COMPILER, "kind": "curated", "sha": sha, "build": row.build_id}));
    remember(&client, Remember { cache_key: &cache_key, build_id: &row.build_id, kind: "curated", route: "curated", idem, body_sha }, &response).await?;
    if before.curated_sha == sha {
        return Ok(flag(response, true, false));
    }
    Ok(response)
}

pub async fn review(config: &AppConfig, row: &BuildRow, value: &Value, body_sha: &str) -> Result<Value, ApiError> {
    refuse_closed(row)?;
    let object = exact(value, &["binds_fact", "decision", "identity_revision", "idempotency_key", "name"])?;
    let name = text(object, "name")?;
    let decision = text(object, "decision")?;
    let binds = text(object, "binds_fact")?;
    if !column_id(&name) || !hex64(&binds) || !matches!(decision.as_str(), "confirmed" | "rejected") {
        return Err(ApiError::malformed());
    }
    let client = store::connect(&config.postgres).await?;
    let idem = key_of(value)?;
    if let Some(done) = replay(&client, &row.build_id, "review", idem, body_sha).await? {
        return Ok(done);
    }
    let docs = load_docs(&client, &row.build_id).await?;
    let (dataset, column) = name.rsplit_once('.').ok_or_else(ApiError::malformed)?;
    let current = docs
        .effective
        .datasets
        .get(dataset)
        .and_then(|item| item.columns.get(column))
        .ok_or_else(ApiError::membership)?;
    if current.fact_sha256 != binds || current.pii != "none" {
        return Err(ApiError::membership());
    }
    insert_review(&client, &row.build_id, &name, &decision, &binds).await?;
    let docs = load_docs(&client, &row.build_id).await?;
    if decision == "confirmed" && docs.effective.stale.is_empty() {
        let _ = store::set_stage(&client, &row.build_id, "S9", "succeeded", &row.input_sha256, Some(&docs.effective_sha), None).await;
    }
    let mut response = view_value(row, &docs);
    response["operation"] = json!("postReview");
    response["s9"] = json!(if decision == "confirmed" && docs.effective.stale.is_empty() { "succeeded" } else { "awaiting" });
    response["business_owner_signed"] = json!(false);
    let cache_key = canonical_sha256(&json!({"build": row.build_id, "compiler": COMPILER, "decision": decision, "fact": binds, "kind": "review", "name": name}));
    remember(&client, Remember { cache_key: &cache_key, build_id: &row.build_id, kind: "review", route: "review", idem, body_sha }, &response).await?;
    Ok(response)
}

pub async fn confirm(config: &AppConfig, row: &BuildRow, value: &Value, body_sha: &str) -> Result<Value, ApiError> {
    refuse_closed(row)?;
    let object = exact(value, &["columns", "identity_revision", "idempotency_key"])?;
    let requested = string_list(object.get("columns").ok_or_else(ApiError::malformed)?)?;
    let bundle = bundle_dir(config, &row.project_id)?;
    let protocol = config
        .projects
        .iter()
        .find(|item| item.project_id == row.project_id)
        .and_then(|item| item.source_protocol.as_ref());
    let roles = sealed::all_roles(bundle, protocol)?;
    let client = store::connect(&config.postgres).await?;
    let idem = key_of(value)?;
    if let Some(done) = replay(&client, &row.build_id, "confirm", idem, body_sha).await? {
        return Ok(done);
    }
    if stage_status(&client, &row.build_id, "S9").await? != "succeeded" {
        return Err(ApiError::classification_incomplete());
    }
    let docs = load_docs(&client, &row.build_id).await?;
    if !docs.effective.stale.is_empty() {
        return Err(ApiError::pack_refused());
    }
    for id in &requested {
        confirm_one(&client, &row.build_id, &docs, id).await?;
    }
    let docs = load_docs(&client, &row.build_id).await?;
    require_classified(&docs, &roles, requested.is_empty())?;
    if !store::set_stage(&client, &row.build_id, "S10", "succeeded", &row.input_sha256, Some(&docs.effective_sha), None)
        .await
        .unwrap_or(false)
    {
        return Err(ApiError::not_ready());
    }
    if !store::set_job(&client, &row.build_id, "succeeded", "S10", None, &row.input_sha256)
        .await
        .unwrap_or(false)
    {
        return Err(ApiError::not_ready());
    }
    write_cards(&config.work_dir, &row.build_id, true)?;
    let mut response = view_value(row, &docs);
    response["operation"] = json!("postConfirm");
    response["ready"] = json!(true);
    response["s10"] = json!("succeeded");
    response["business_owner_signed"] = json!(false);
    response["signer_class"] = json!("control_plane_test");
    let cache_key = canonical_sha256(&json!({"build": row.build_id, "columns": requested, "compiler": COMPILER, "effective": docs.effective_sha, "kind": "confirm"}));
    remember(&client, Remember { cache_key: &cache_key, build_id: &row.build_id, kind: "confirm", route: "confirm", idem, body_sha }, &response).await?;
    Ok(response)
}

pub async fn compile_role_pack(config: &AppConfig, row: &BuildRow, value: &Value, body_sha: &str) -> Result<Value, ApiError> {
    refuse_closed(row)?;
    let object = exact(value, &["identity_revision", "idempotency_key", "policy_version", "role"])?;
    let role = text(object, "role")?;
    let policy = text(object, "policy_version")?;
    if !ident_ok(&role) || policy != "oos/v1" {
        return Err(ApiError::malformed());
    }
    let client = store::connect(&config.postgres).await?;
    let idem = key_of(value)?;
    if let Some(done) = replay(&client, &row.build_id, "role_pack", idem, body_sha).await? {
        return Ok(done);
    }
    if stage_status(&client, &row.build_id, "S10").await? != "succeeded" {
        return Err(ApiError::pack_refused());
    }
    let bundle = bundle_dir(config, &row.project_id)?;
    let protocol = config
        .projects
        .iter()
        .find(|item| item.project_id == row.project_id)
        .and_then(|item| item.source_protocol.as_ref());
    let columns = sealed::role_columns(bundle, protocol, &role)?;
    let docs = load_docs(&client, &row.build_id).await?;
    ensure_source_money(&docs.effective, &sealed::declared_money_units(bundle, protocol)?)?;
    let pack = oosyaml::render_pack(&docs.effective, &role, &columns, &policy)?;
    let rebound = accepted_native_rebind(config, &client, row).await?;
    let native_maintenance = compile_native_maintenance(config, row, &docs, rebound.as_ref())?;
    let cache_key = canonical_sha256(&json!({
        "build": row.build_id,
        "columns": columns,
        "compiler": COMPILER,
        "effective": docs.effective_sha,
        "kind": "role_pack",
        "native_maintenance": native_maintenance,
        "policy": policy,
        "role": role
    }));
    if let Some(hit) = cache_hit(&client, &cache_key, &row.build_id, "role_pack", idem, body_sha).await? {
        return Ok(hit);
    }
    let files = pack_files(&pack);
    insert_pack(&client, &row.build_id, &role, &pack.pack_sha256, &pack.manifest_sha256, &pack.manifest, &files).await?;
    let mut response = pack_value(&row.build_id, &role, &pack.pack_sha256, &pack.manifest_sha256, &pack.manifest, &files, false);
    response["operation"] = json!("compileRolePack");
    response["native_maintenance"] = native_maintenance;
    remember(&client, Remember { cache_key: &cache_key, build_id: &row.build_id, kind: "role_pack", route: "role_pack", idem, body_sha }, &response).await?;
    Ok(response)
}

pub async fn read_role_pack(config: &AppConfig, row: &BuildRow, role: &str) -> Result<Value, ApiError> {
    if !ident_ok(role) {
        return Err(ApiError::malformed());
    }
    let client = store::connect(&config.postgres).await?;
    let found = client
        .query_opt(
            "SELECT pack_sha256, manifest_sha256, manifest, files_json
             FROM oo_selfbuild_api.role_pack
             WHERE build_id = $1 AND role_name = $2
             ORDER BY revision DESC LIMIT 1",
            &[&row.build_id, &role],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    let Some(stored) = found else {
        return Err(ApiError::not_found());
    };
    let bundle = bundle_dir(config, &row.project_id)?;
    let protocol = config.projects.iter().find(|item| item.project_id == row.project_id).and_then(|item| item.source_protocol.as_ref());
    let docs = load_docs(&client, &row.build_id).await?;
    ensure_source_money(&docs.effective, &sealed::declared_money_units(bundle, protocol)?)?;
    let columns = sealed::role_columns(bundle, protocol, role)?;
    let current = oosyaml::render_pack(&docs.effective, role, &columns, "oos/v1")?;
    let pack_sha: String = stored.get(0);
    if pack_sha != current.pack_sha256 { return Err(ApiError::pack_refused()); }
    let stale: bool = client.query_one(
        "SELECT EXISTS (SELECT 1 FROM oo_selfbuild_api.overlay_doc d WHERE d.build_id = $1 AND d.kind = 'generated' AND d.created_at > (SELECT MAX(created_at) FROM oo_selfbuild_api.role_pack WHERE build_id = $1 AND role_name = $2))",
        &[&row.build_id, &role],
    ).await.map_err(|_| ApiError::not_ready())?.get(0);
    if stale { return Err(ApiError::pack_refused()); }
    let manifest_sha: String = stored.get(1);
    let manifest: String = stored.get(2);
    let files: String = stored.get(3);
    let files_value: Value = serde_json::from_str(&files).map_err(|_| ApiError::not_ready())?;
    Ok(pack_value(&row.build_id, role, &pack_sha, &manifest_sha, &manifest, &files_value, false))
}

pub async fn gates(config: &AppConfig, row: &BuildRow, value: &Value, body_sha: &str) -> Result<Value, ApiError> {
    exact(value, &["identity_revision", "idempotency_key"])?;
    let client = store::connect(&config.postgres).await?;
    let idem = key_of(value)?;
    if let Some(done) = replay(&client, &row.build_id, "gates", idem, body_sha).await? {
        return Ok(done);
    }
    let docs = load_docs(&client, &row.build_id).await?;
    let mut report = gate_report(&docs.effective, &sealed_types(&docs.generated));
    if ensure_source_money(&docs.effective, &source_money_units(config, row)?).is_err() {
        report["gates"][0]["status"] = json!("block");
        report["gates"][0]["evidence"] = json!("money unit differs from the sealed source rule");
        report["gates"][1]["status"] = json!("block");
    }
    report["operation"] = json!("postGates");
    report["build_id"] = json!(row.build_id);
    report["llm_calls"] = json!(0);
    report["cache_hit"] = json!(false);
    report["replay"] = json!(false);
    report["signer_class"] = json!("control_plane_test");
    let cache_key = canonical_sha256(&json!({"build": row.build_id, "compiler": COMPILER, "effective": docs.effective_sha, "kind": "gates"}));
    remember(&client, Remember { cache_key: &cache_key, build_id: &row.build_id, kind: "gates", route: "gates", idem, body_sha }, &report).await?;
    Ok(report)
}

struct Docs {
    generated_sha: String,
    generated: oosyaml::Model,
    curated_sha: String,
    reviews: Vec<Review>,
    effective: oosyaml::Model,
    effective_yaml: String,
    effective_sha: String,
    feedback: Vec<Value>,
    curated_revisions: i64,
    generated_revisions: i64,
}

async fn load_docs(client: &Client, build_id: &str) -> Result<Docs, ApiError> {
    let generated = latest_doc(client, build_id, "generated").await?.ok_or_else(ApiError::not_ready)?;
    let generated_model = parse_model(&generated.1, "generated")?;
    let curated = latest_doc(client, build_id, "curated").await?;
    let curated_model = match &curated {
        Some((_, yaml)) => parse_model(yaml, "curated")?,
        None => empty_model(),
    };
    let reviews = load_reviews(client, build_id).await?;
    let effective = merge(&generated_model, &curated_model, &reviews);
    let effective_yaml = oosyaml::canonical_doc(&model_to_y(&effective, "effective"))?;
    let feedback = load_notes(client, build_id).await?;
    Ok(Docs {
        generated_sha: generated.0,
        generated: generated_model,
        curated_sha: curated.as_ref().map(|(sha, _)| sha.clone()).unwrap_or_default(),
        reviews,
        effective,
        effective_sha: sha256_hex(effective_yaml.as_bytes()),
        effective_yaml,
        feedback,
        curated_revisions: count_docs(client, build_id, "curated").await?,
        generated_revisions: count_docs(client, build_id, "generated").await?,
    })
}

fn empty_model() -> oosyaml::Model {
    oosyaml::Model {
        datasets: BTreeMap::new(),
        relationships: BTreeMap::new(),
        rejections: BTreeMap::new(),
        stale: Vec::new(),
        candidates: Vec::new(),
    }
}

fn view_value(row: &BuildRow, docs: &Docs) -> Value {
    json!({
        "contract": "oo.selfbuild-control-api/overlay/v1",
        "operation": "getOverlay",
        "build_id": row.build_id,
        "project_id": row.project_id,
        "generated_sha256": docs.generated_sha,
        "generated_revisions": docs.generated_revisions,
        "curated_sha256": if docs.curated_sha.is_empty() { Value::Null } else { json!(docs.curated_sha) },
        "curated_revisions": docs.curated_revisions,
        "effective_sha256": docs.effective_sha,
        "effective_yaml": docs.effective_yaml,
        "stale": docs.effective.stale.iter().map(|item| json!({
            "name": item.name,
            "kind": item.kind,
            "bound_fact": item.bound_fact,
            "current_fact": item.current_fact
        })).collect::<Vec<_>>(),
        "rejections": docs.effective.rejections.iter().map(|(name, state)| json!({"name": name, "state": state})).collect::<Vec<_>>(),
        "candidates": docs.effective.candidates.iter().map(|item| json!({"name": item.name, "kind": item.kind, "note": item.note})).collect::<Vec<_>>(),
        "columns": docs.effective.datasets.iter().flat_map(|(dataset, item)| {
            item.columns.values().map(move |column| json!({
                "classification_state": column.classification_state,
                "data_type": column.data_type,
                "dataset": dataset,
                "fact_sha256": column.fact_sha256,
                "label": column.label,
                "name": column.name,
                "pii": column.pii,
                "sealed_data_type": column.sealed_data_type
            }))
        }).collect::<Vec<_>>(),
        "feedback": docs.feedback,
        "llm_calls": 0,
        "cache_hit": false,
        "replay": false,
        "published": false,
        "active": false,
        "data_grants": "none",
        "compiled_pack": "not_published",
        "business_owner_signed": false,
        "owner_certification": "not_issued",
        "signer_class": "control_plane_test"
    })
}

async fn apply_edit(client: &Client, build_id: &str, docs: &Docs, edit: Value) -> Result<(), ApiError> {
    let object = edit.as_object().ok_or_else(ApiError::malformed)?;
    if object.len() != 3 {
        return Err(ApiError::malformed());
    }
    let dataset = text(object, "dataset")?;
    let column = text(object, "column")?;
    let data_type = text(object, "data_type")?;
    if !dataset_ok(&dataset) || !crate::ids::source_ident_ok(&column) {
        return Err(ApiError::malformed());
    }
    let current = docs
        .generated
        .datasets
        .get(&dataset)
        .and_then(|item| item.columns.get(&column))
        .ok_or_else(ApiError::membership)?;
    if current.data_type == data_type {
        if current.sealed_data_type == data_type {
            return Err(ApiError::malformed());
        }
        return Ok(());
    }
    let mut model = docs.generated.clone();
    apply_fact_edit(&mut model, &dataset, &column, &data_type)?;
    let yaml = oosyaml::canonical_doc(&model_to_y(&model, "generated"))?;
    insert_doc(client, build_id, "generated", &yaml, &sha256_hex(yaml.as_bytes())).await
}

fn source_money_units(config: &AppConfig, row: &BuildRow) -> Result<BTreeMap<String, String>, ApiError> {
    let bundle = bundle_dir(config, &row.project_id)?;
    let protocol = config.projects.iter().find(|item| item.project_id == row.project_id).and_then(|item| item.source_protocol.as_ref());
    sealed::declared_money_units(bundle, protocol)
}

fn ensure_source_money(model: &oosyaml::Model, units: &BTreeMap<String, String>) -> Result<(), ApiError> {
    for (id, unit) in units {
        let (dataset, column) = id.rsplit_once('.').ok_or_else(ApiError::membership)?;
        let item = model.datasets.get(dataset).and_then(|entry| entry.columns.get(column)).ok_or_else(ApiError::membership)?;
        if item.semantic_type != "money" || &item.unit != unit { return Err(ApiError::pack_refused()); }
    }
    Ok(())
}

fn project_native_source_money(effective: &mut Value, model: &oosyaml::Model, units: &BTreeMap<String, String>) -> Result<Vec<Value>, ApiError> {
    let mut corrections = Vec::new();
    for (id, unit) in units {
        let (dataset_name, column_name) = id.rsplit_once('.').ok_or_else(ApiError::pack_refused)?;
        let source = model.datasets.get(dataset_name).and_then(|dataset| dataset.columns.get(column_name)).ok_or_else(ApiError::pack_refused)?;
        if source.semantic_type != "money" || &source.unit != unit || source.provenance != "declared" { return Err(ApiError::pack_refused()); }
        if source.pii == "unknown" && source.classification_state != "pending" { return Err(ApiError::pack_refused()); }
        let datasets = effective.get_mut("datasets").and_then(Value::as_array_mut).ok_or_else(ApiError::pack_refused)?;
        let mut matches = datasets.iter_mut().filter(|dataset| dataset.get("dataset").and_then(Value::as_str) == Some(dataset_name)).collect::<Vec<_>>();
        if matches.len() != 1 { return Err(ApiError::pack_refused()); }
        let columns = matches.pop().ok_or_else(ApiError::pack_refused)?.get_mut("columns").and_then(Value::as_array_mut).ok_or_else(ApiError::pack_refused)?;
        let mut matches = columns.iter_mut().filter(|column| column.get("name").and_then(Value::as_str) == Some(column_name)).collect::<Vec<_>>();
        if matches.len() != 1 { return Err(ApiError::pack_refused()); }
        let column = matches.pop().ok_or_else(ApiError::pack_refused)?;
        let original = column.clone();
        column["semantic_type"] = json!("money");
        column["unit"] = json!(unit);
        column["pii"] = if source.pii == "unknown" { Value::Null } else { json!(source.pii) };
        column["provenance"] = json!("declared");
        let state = if matches!(source.classification_state.as_str(), "confirmed" | "auto_accepted") { "confirmed" } else { "pending" };
        if state == "pending" { column["classification"] = json!({"state":"pending","by":null,"at":null}); }
        else { column["classification"]["state"] = json!("confirmed"); }
        if *column != original { corrections.push(json!({"column":id,"reviewed_candidate":original,"source_declared":column})); }
    }
    Ok(corrections)
}

async fn apply_source_unit_correction(client: &Client, build_id: &str, docs: &Docs, edit: Value, units: &BTreeMap<String, String>) -> Result<(), ApiError> {
    let object = edit.as_object().ok_or_else(ApiError::malformed)?;
    if object.len() != 3 { return Err(ApiError::malformed()); }
    let dataset = text(object, "dataset")?;
    let column = text(object, "column")?;
    let unit = text(object, "unit")?;
    if !dataset_ok(&dataset) || !crate::ids::source_ident_ok(&column) { return Err(ApiError::malformed()); }
    let id = format!("{dataset}.{column}");
    if units.get(&id).map(String::as_str) != Some(unit.as_str()) { return Err(ApiError::membership()); }
    let current = docs.generated.datasets.get(&dataset).and_then(|entry| entry.columns.get(&column)).ok_or_else(ApiError::membership)?;
    if current.unit == unit && current.semantic_type == "money" { return Ok(()); }
    let mut model = docs.generated.clone();
    let item = model.datasets.get_mut(&dataset).and_then(|entry| entry.columns.get_mut(&column)).ok_or_else(ApiError::membership)?;
    item.semantic_type = "money".to_owned();
    item.unit = unit;
    item.provenance = "declared".to_owned();
    item.classification_state = "pending".to_owned();
    let yaml = oosyaml::canonical_doc(&model_to_y(&model, "generated"))?;
    // The review event is append-only and revokes the earlier confirmation even
    // when the physical column fact hash is unchanged by a semantic correction.
    insert_review(client, build_id, &id, "rejected", &current.fact_sha256).await?;
    insert_doc(client, build_id, "generated", &yaml, &sha256_hex(yaml.as_bytes())).await
}

async fn confirm_one(client: &Client, build_id: &str, docs: &Docs, id: &str) -> Result<(), ApiError> {
    if !column_id(id) {
        return Err(ApiError::malformed());
    }
    let (dataset, column) = id.rsplit_once('.').ok_or_else(ApiError::malformed)?;
    let item = docs
        .effective
        .datasets
        .get(dataset)
        .and_then(|entry| entry.columns.get(column))
        .ok_or_else(ApiError::membership)?;
    if item.pii == "unknown" {
        return Err(ApiError::pii_unknown());
    }
    if item.pii != "none" {
        return Err(ApiError::pii_withheld());
    }
    if item.classification_state == "pending" {
        insert_review(client, build_id, id, "confirmed", &item.fact_sha256).await?;
    }
    Ok(())
}

fn require_classified(docs: &Docs, roles: &BTreeMap<String, Vec<String>>, empty: bool) -> Result<(), ApiError> {
    let mut needed = BTreeSet::new();
    for columns in roles.values() {
        for column in columns {
            needed.insert(column.clone());
        }
    }
    if empty && needed.is_empty() {
        return Err(ApiError::malformed());
    }
    for id in needed {
        let (dataset, column) = id.rsplit_once('.').ok_or_else(ApiError::malformed)?;
        let Some(item) = docs.effective.datasets.get(dataset).and_then(|entry| entry.columns.get(column)) else {
            return Err(ApiError::membership());
        };
        if item.pii != "none" {
            continue;
        }
        if item.classification_state != "auto_accepted" && item.classification_state != "confirmed" {
            return Err(ApiError::classification_incomplete());
        }
    }
    if !docs.effective.stale.is_empty() {
        return Err(ApiError::pack_refused());
    }
    Ok(())
}

fn maintenance_fields(value: &Value) -> Result<(&str, Option<Value>, Option<String>), ApiError> {
    let object = value.as_object().ok_or_else(ApiError::malformed)?;
    let trigger = object.get("trigger").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
    let mut source_generation_keys = vec!["catalog_sha256", "identity_revision", "idempotency_key", "trigger"];
    if object.contains_key("source_semantics_sha256") { source_generation_keys.push("source_semantics_sha256"); }
    let keys: &[&str] = match trigger {
        "source_generation_advanced" => &source_generation_keys,
        "schema_changed" | "source_fact_corrected" => &["fact_edit", "identity_revision", "idempotency_key", "trigger"],
        "feedback" => &["identity_revision", "idempotency_key", "note", "trigger"],
        "table_ready" | "scheduled" | "manual" => &["identity_revision", "idempotency_key", "trigger"],
        _ => return Err(ApiError::malformed()),
    };
    exact(value, keys)?;
    if object.get("source_semantics_sha256").is_some_and(|item| item.as_str().is_none_or(|sha| !hex64(sha))) { return Err(ApiError::malformed()); }
    let edit = if matches!(trigger, "schema_changed" | "source_fact_corrected") {
        Some(object.get("fact_edit").cloned().ok_or_else(ApiError::malformed)?)
    } else {
        None
    };
    let note = if trigger == "feedback" {
        let note = text(object, "note")?;
        if note.chars().count() > 200 || note.chars().any(|item| item == '\0' || item == '\r') {
            return Err(ApiError::malformed());
        }
        Some(note)
    } else {
        None
    };
    Ok((trigger, edit, note))
}

fn maintenance_key(row: &BuildRow, trigger: &str, edit: Option<&Value>, note_sha: &str, docs: &Docs) -> String {
    let mut body = json!({
        "build_id": row.build_id,
        "compiler": COMPILER,
        "curated_sha256": docs.curated_sha,
        "kind": "maintenance",
        "project_id": row.project_id,
        "reviews_sha256": reviews_sha(&docs.reviews),
        "trigger": trigger
    });
    if matches!(trigger, "schema_changed" | "source_fact_corrected") {
        body["fact_edit"] = edit.cloned().unwrap_or(Value::Null);
    } else {
        body["generated_sha256"] = json!(docs.generated_sha);
    }
    if trigger == "feedback" {
        body["note_sha256"] = json!(note_sha);
    }
    canonical_sha256(&body)
}

fn reviews_sha(reviews: &[Review]) -> String {
    let items: Vec<Value> = reviews
        .iter()
        .map(|review| json!({"binds_fact": review.binds_fact, "decision": review.decision, "name": review.name}))
        .collect();
    canonical_sha256(&json!(items))
}

fn pack_value(build_id: &str, role: &str, pack_sha: &str, manifest_sha: &str, manifest: &str, files: &Value, cache_hit: bool) -> Value {
    json!({
        "contract": "oo.selfbuild-control-api/role-pack/v1",
        "operation": "getRolePack",
        "build_id": build_id,
        "role": role,
        "pack_sha256": pack_sha,
        "manifest_sha256": manifest_sha,
        "manifest": manifest,
        "files": files,
        "llm_calls": 0,
        "cache_hit": cache_hit,
        "replay": false,
        "published": false,
        "active": false,
        "data_grants": "none",
        "compiled_pack": "not_published",
        "business_owner_signed": false,
        "owner_certification": "not_issued",
        "signer_class": "control_plane_test",
        "policy_version": "oos/v1",
        "compiler": COMPILER
    })
}

fn pack_files(pack: &oosyaml::RolePack) -> Value {
    let files: Vec<Value> = pack
        .files
        .iter()
        .map(|file| json!({"body": file.body, "bytes": file.body.len(), "path": file.path, "sha256": sha256_hex(file.body.as_bytes())}))
        .collect();
    json!(files)
}

async fn latest_doc(client: &Client, build_id: &str, kind: &str) -> Result<Option<(String, String)>, ApiError> {
    let row = client
        .query_opt(
            "SELECT doc_sha256, body FROM oo_selfbuild_api.overlay_doc
             WHERE build_id = $1 AND kind = $2 ORDER BY revision DESC LIMIT 1",
            &[&build_id, &kind],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.map(|item| (item.get(0), item.get(1))))
}

async fn count_docs(client: &Client, build_id: &str, kind: &str) -> Result<i64, ApiError> {
    let row = client
        .query_one(
            "SELECT COUNT(*) FROM oo_selfbuild_api.overlay_doc WHERE build_id = $1 AND kind = $2",
            &[&build_id, &kind],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.get(0))
}

async fn load_reviews(client: &Client, build_id: &str) -> Result<Vec<Review>, ApiError> {
    let rows = client
        .query(
            "SELECT name, decision, binds_fact FROM oo_selfbuild_api.overlay_review
             WHERE build_id = $1 ORDER BY revision",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows
        .into_iter()
        .map(|row| Review { name: row.get(0), decision: row.get(1), binds_fact: row.get(2) })
        .collect())
}

async fn load_notes(client: &Client, build_id: &str) -> Result<Vec<Value>, ApiError> {
    let rows = client
        .query(
            "SELECT note FROM oo_selfbuild_api.overlay_candidate WHERE build_id = $1 ORDER BY revision",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let note: String = row.get(0);
            json!({"kind": "feedback", "note": note, "note_sha256": sha256_hex(note.as_bytes())})
        })
        .collect())
}

async fn insert_doc(client: &Client, build_id: &str, kind: &str, body: &str, sha: &str) -> Result<(), ApiError> {
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.overlay_doc (build_id, kind, revision, doc_sha256, body)
             SELECT $1, $2, COALESCE(MAX(revision), 0) + 1, $3, $4
             FROM oo_selfbuild_api.overlay_doc WHERE build_id = $1 AND kind = $2",
            &[&build_id, &kind, &sha, &body],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

async fn insert_review(client: &Client, build_id: &str, name: &str, decision: &str, binds: &str) -> Result<(), ApiError> {
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.overlay_review
                (build_id, revision, name, decision, binds_fact, signer_class, business_owner_signed)
             SELECT $1, COALESCE(MAX(revision), 0) + 1, $2, $3, $4, 'control_plane_test', false
             FROM oo_selfbuild_api.overlay_review WHERE build_id = $1",
            &[&build_id, &name, &decision, &binds],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

async fn insert_note(client: &Client, build_id: &str, note: &str) -> Result<(), ApiError> {
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.overlay_candidate (build_id, revision, note)
             SELECT $1, COALESCE(MAX(revision), 0) + 1, $2
             FROM oo_selfbuild_api.overlay_candidate WHERE build_id = $1",
            &[&build_id, &note],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

async fn insert_pack(
    client: &Client,
    build_id: &str,
    role: &str,
    pack_sha: &str,
    manifest_sha: &str,
    manifest: &str,
    files: &Value,
) -> Result<(), ApiError> {
    let files_json = serde_json::to_string(files).map_err(|_| ApiError::not_ready())?;
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.role_pack
                (build_id, role_name, revision, pack_sha256, manifest_sha256, manifest, files_json, published, active)
             SELECT $1, $2, COALESCE(MAX(revision), 0) + 1, $3, $4, $5, $6, false, false
             FROM oo_selfbuild_api.role_pack WHERE build_id = $1 AND role_name = $2",
            &[&build_id, &role, &pack_sha, &manifest_sha, &manifest, &files_json],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

async fn replay(client: &Client, build_id: &str, route: &str, idem: &str, body_sha: &str) -> Result<Option<Value>, ApiError> {
    let row = client
        .query_opt(
            "SELECT body_sha256, response_text FROM oo_selfbuild_api.maintenance_receipt
             WHERE build_id = $1 AND route = $2 AND idempotency_key = $3",
            &[&build_id, &route, &idem],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    let Some(stored) = row else {
        return Ok(None);
    };
    let stored_sha: String = stored.get(0);
    if stored_sha != body_sha {
        return Err(ApiError::idempotency());
    }
    let text: String = stored.get(1);
    Ok(Some(flag_text(&text, true, true)?))
}

async fn cache_hit(
    client: &Client,
    cache_key: &str,
    build_id: &str,
    route: &str,
    idem: &str,
    body_sha: &str,
) -> Result<Option<Value>, ApiError> {
    let row = client
        .query_opt(
            "SELECT response_text FROM oo_selfbuild_api.overlay_cache WHERE cache_key = $1",
            &[&cache_key],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    let Some(stored) = row else {
        return Ok(None);
    };
    let text: String = stored.get(0);
    let value = flag_text(&text, true, false)?;
    store_receipt(client, build_id, route, idem, body_sha, &text).await?;
    Ok(Some(value))
}

struct Remember<'a> {
    cache_key: &'a str,
    build_id: &'a str,
    kind: &'a str,
    route: &'a str,
    idem: &'a str,
    body_sha: &'a str,
}

async fn remember(client: &Client, slot: Remember<'_>, response: &Value) -> Result<(), ApiError> {
    let text = serde_json::to_string(response).map_err(|_| ApiError::not_ready())?;
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.overlay_cache (cache_key, build_id, kind, response_text, llm_calls)
             VALUES ($1, $2, $3, $4, 0)
             ON CONFLICT (cache_key) DO NOTHING",
            &[&slot.cache_key, &slot.build_id, &slot.kind, &text],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    store_receipt(client, slot.build_id, slot.route, slot.idem, slot.body_sha, &text).await
}

async fn store_receipt(client: &Client, build_id: &str, route: &str, idem: &str, body_sha: &str, text: &str) -> Result<(), ApiError> {
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.maintenance_receipt
                (build_id, route, idempotency_key, body_sha256, response_text)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (build_id, route, idempotency_key) DO NOTHING",
            &[&build_id, &route, &idem, &body_sha, &text],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

async fn stage_status(client: &Client, build_id: &str, stage: &str) -> Result<String, ApiError> {
    let row = client
        .query_one(
            "SELECT status FROM oo_selfbuild_api.stage_checkpoint WHERE build_id = $1 AND stage = $2",
            &[&build_id, &stage],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.get(0))
}

fn flag(mut value: Value, cache_hit: bool, replay: bool) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("cache_hit".to_owned(), json!(cache_hit));
        object.insert("replay".to_owned(), json!(replay));
        object.insert("llm_calls".to_owned(), json!(0));
    }
    value
}

fn flag_text(text: &str, cache_hit: bool, replay: bool) -> Result<Value, ApiError> {
    let value: Value = serde_json::from_str(text).map_err(|_| ApiError::not_ready())?;
    Ok(flag(value, cache_hit, replay))
}

fn refuse_closed(row: &BuildRow) -> Result<(), ApiError> {
    if matches!(row.status.as_str(), "failed" | "canceled" | "cancel_requested") {
        Err(ApiError::pack_refused())
    } else {
        Ok(())
    }
}

fn bundle_dir<'a>(config: &'a AppConfig, project_id: &str) -> Result<&'a str, ApiError> {
    let project = config.projects.iter().find(|item| item.project_id == project_id).ok_or_else(ApiError::forbidden)?;
    if project.sealed_bundle.is_empty() {
        Err(ApiError::not_ready())
    } else {
        Ok(project.sealed_bundle.as_str())
    }
}

fn exact<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ApiError> {
    let object = value.as_object().ok_or_else(ApiError::malformed)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(ApiError::malformed());
    }
    let _ = revision(object)?;
    let idem = text(object, "idempotency_key")?;
    if !idempotency_ok(&idem) {
        return Err(ApiError::malformed());
    }
    Ok(object)
}

fn key_of(value: &Value) -> Result<&str, ApiError> {
    value.get("idempotency_key").and_then(Value::as_str).ok_or_else(ApiError::malformed)
}

fn revision(object: &Map<String, Value>) -> Result<i64, ApiError> {
    let revision = object.get("identity_revision").and_then(Value::as_i64).ok_or_else(ApiError::malformed)?;
    if revision < 1 || object.get("identity_revision").and_then(Value::as_u64).is_none() {
        return Err(ApiError::malformed());
    }
    Ok(revision)
}

fn text(object: &Map<String, Value>, key: &str) -> Result<String, ApiError> {
    object.get(key).and_then(Value::as_str).filter(|item| !item.is_empty()).map(str::to_owned).ok_or_else(ApiError::malformed)
}

fn string_list(value: &Value) -> Result<Vec<String>, ApiError> {
    let items = value.as_array().ok_or_else(ApiError::malformed)?;
    if items.len() > 64 {
        return Err(ApiError::malformed());
    }
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or_else(ApiError::malformed)?;
        if !column_id(text) {
            return Err(ApiError::malformed());
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

fn column_id(value: &str) -> bool {
    let mut parts = value.split('.');
    crate::ids::source_ident_ok(parts.next().unwrap_or("")) && crate::ids::source_ident_ok(parts.next().unwrap_or("")) && crate::ids::source_ident_ok(parts.next().unwrap_or("")) && parts.next().is_none()
}

fn dataset_ok(value: &str) -> bool {
    value.split_once('.').is_some_and(|(schema, table)| crate::ids::source_ident_ok(schema) && crate::ids::source_ident_ok(table) && !table.contains('.'))
}

fn write_cards(work_dir: &str, build_id: &str, ready: bool) -> Result<(), ApiError> {
    let path = Path::new(work_dir).join("builds").join(build_id).join("cards.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ApiError::not_ready())?;
    }
    let body = crate::canonical::canonical_json(&json!({"llm_calls": 0, "outcome": "structural_draft", "published": false, "ready": ready}));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ApiError::not_ready())?;
    std::io::Write::write_all(&mut file, body.as_bytes()).map_err(|_| ApiError::not_ready())
}

/// Persist model candidates as generated content. Curated and review rows are untouched.
pub async fn insert_generated_model(client: &Client, build_id: &str, yaml: &str) -> Result<(), ApiError> {
    parse_model(yaml, "generated")?;
    insert_doc(client, build_id, "generated", yaml, &sha256_hex(yaml.as_bytes())).await
}

fn semantic_basis(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        for key in ["source_generation", "target_generation", "table_generations", "binding_vector_sha256", "collection_sha256"] { object.remove(key); }
        if object.get("protocol").and_then(Value::as_str) == Some("oo.source.doris.audit/v1") {
            for key in ["capture_methods", "evidence", "evidence_set_id", "generation_semantics"] { object.remove(key); }
            if let Some(quality) = object.get_mut("quality").and_then(Value::as_object_mut) { quality.remove("warnings"); }
        }
        if object.get("protocol").and_then(Value::as_str) == Some("oo.source.mysql.audit/v1") {
            for key in ["applied_pos", "evidence", "evidence_set_id", "c1_evidence", "capture_methods"] { object.remove(key); }
            if let Some(capture) = object.get_mut("continuous_capture").and_then(Value::as_object_mut) { capture.remove("tested_gtid_transactions"); }
            if let Some(quality) = object.get_mut("quality").and_then(Value::as_object_mut) { quality.remove("zero_date_rejects"); }
        }
        if object.get("protocol").and_then(Value::as_str) == Some("oo.oracle.native-c5-source-semantics/v1") {
            for key in ["executed", "evidence", "evidence_set_id", "consistency_scope"] { object.remove(key); }
            if let Some(contract) = object.get_mut("source_contract").and_then(Value::as_object_mut) { contract.remove("position"); }
            if let Some(quality) = object.get_mut("quality").and_then(Value::as_object_mut) {
                for key in ["rows", "typed_cells", "typed_cell_differences", "delete_quality", "key_2_absent"] { quality.remove(key); }
            }
        }
        if let Some(contract) = object.get_mut("source_contract").and_then(Value::as_object_mut) { contract.remove("applied_end_lsn"); }
        if let Some(quality) = object.get_mut("quality").and_then(Value::as_object_mut) {
            quality.remove("rows"); quality.remove("typed_cells");
        }
    }
    value
}

fn decode_oracle_name(value: &Value) -> Result<String, ApiError> {
    let text = value.as_str().and_then(|text| text.strip_prefix("q_")).ok_or_else(ApiError::pack_refused)?;
    if text.is_empty() || text.len() % 2 != 0 { return Err(ApiError::pack_refused()); }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).map_err(|_| ApiError::pack_refused())?;
        bytes.push(u8::from_str_radix(pair, 16).map_err(|_| ApiError::pack_refused())?);
    }
    String::from_utf8(bytes).map_err(|_| ApiError::pack_refused())
}

fn recorded_progress(receipt: &Value) -> &Value {
    if receipt.get("progress_vector").and_then(Value::as_array).is_some() { &receipt["progress_vector"] } else { &receipt["binding_vector"] }
}

fn compile_maintenance_catalog(catalog: &Value) -> Result<oo_builder::SealedImportDraft, ApiError> {
    let text = crate::canonical::canonical_json(catalog);
    match catalog.get("protocol").and_then(Value::as_str) {
        Some(oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL) => oo_builder::compile_sealed_import_collection(&text),
        Some(oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL) => oo_builder::compile_sealed_import_catalog(&text),
        _ => return Err(ApiError::pack_refused()),
    }.map_err(|_| ApiError::pack_refused())
}

/// Per-table progress. A collection uses the compiler's binding vector, never the draft's high-watermark generation.
/// A single catalog uses its own table fields and does not gain a binding_vector.
fn maintenance_progress(catalog: &Value, compiled: &oo_builder::SealedImportDraft) -> Result<Value, ApiError> {
    if catalog.get("protocol").and_then(Value::as_str) == Some(oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL) {
        return compiled.binding_vector.clone().filter(|vector| vector.as_array().is_some_and(|rows| !rows.is_empty())).ok_or_else(ApiError::pack_refused);
    }
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    if tables.len() != compiled.tables.len() { return Err(ApiError::pack_refused()); }
    let mut rows = Vec::with_capacity(tables.len());
    for table in tables {
        let schema = table.get("logical_schema").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let name = table.get("logical_table").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let dataset = format!("{schema}.{name}");
        if !compiled.tables.iter().any(|item| item.dataset == dataset) { return Err(ApiError::pack_refused()); }
        rows.push(json!({
            "dataset": dataset,
            "input_digest": catalog["input_digest"],
            "run_id": catalog["run_id"],
            "source_generation": catalog["source_generation"],
            "source_id": catalog["source_id"],
            "stored_physical": table["stored_physical"],
            "target_generation": catalog["target_generation"]
        }));
    }
    Ok(Value::Array(rows))
}

fn same_sealed_identity(old: &Value, new: &Value) -> bool {
    for key in ["capture", "config_digest", "engine", "hook_checked", "schema_digest", "source_id", "source_introspection", "state", "tool_hook_digest", "type_digest"] {
        if new[key] != old[key] { return false; }
    }
    let quality = |value: &Value| { let mut q = value["quality"].clone(); if let Some(object) = q.as_object_mut() { object.remove("stored_rows"); } q };
    quality(new) == quality(old)
}

fn rebind_sealed_catalogs(command_catalog: &Value, catalog: &Value) -> Result<Value, ApiError> {
    let protocol = catalog.get("protocol").and_then(Value::as_str);
    if protocol.is_none() || protocol != command_catalog.get("protocol").and_then(Value::as_str) { return Err(ApiError::pack_refused()); }
    let old = compile_maintenance_catalog(command_catalog)?;
    let new = compile_maintenance_catalog(catalog)?;
    if old.columns != new.columns { return Err(ApiError::pack_refused()); }
    let progress = maintenance_progress(catalog, &new)?;
    check_generation_progress(&maintenance_progress(command_catalog, &old)?, &progress)?;
    if protocol == Some(oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL) {
        let before = command_catalog.get("entries").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
        let after = catalog.get("entries").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
        for old_entry in before {
            let new_entry = after.iter().find(|entry| entry["tables"][0]["logical_schema"] == old_entry["tables"][0]["logical_schema"] && entry["tables"][0]["logical_table"] == old_entry["tables"][0]["logical_table"]).ok_or_else(ApiError::pack_refused)?;
            if !same_sealed_identity(old_entry, new_entry) { return Err(ApiError::pack_refused()); }
        }
    } else if !same_sealed_identity(command_catalog, catalog) {
        return Err(ApiError::pack_refused());
    } else {
        let before = command_catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
        let after = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
        for old_table in before {
            if !after.iter().any(|table| table["logical_schema"] == old_table["logical_schema"] && table["logical_table"] == old_table["logical_table"]) { return Err(ApiError::pack_refused()); }
        }
    }
    Ok(progress)
}

fn check_generation_progress(previous: &Value, current: &Value) -> Result<(), ApiError> {
    let previous = previous.as_array().ok_or_else(ApiError::pack_refused)?;
    let current = current.as_array().ok_or_else(ApiError::pack_refused)?;
    if previous.len() != current.len() { return Err(ApiError::pack_refused()); }
    for old in previous {
        let new = current.iter().find(|new| new["dataset"] == old["dataset"]).ok_or_else(ApiError::pack_refused)?;
        if new["source_id"] != old["source_id"] { return Err(ApiError::pack_refused()); }
        for key in ["source_generation", "target_generation"] {
            let a = old[key].as_i64().ok_or_else(ApiError::pack_refused)?;
            let b = new[key].as_i64().ok_or_else(ApiError::pack_refused)?;
            if b < a { return Err(ApiError::pack_refused()); }
            if b == a && (new["run_id"] != old["run_id"] || new["input_digest"] != old["input_digest"] || new["stored_physical"] != old["stored_physical"]) { return Err(ApiError::pack_refused()); }
        }
    }
    Ok(())
}

fn oracle_c5_capture_methods(catalog: &Value, semantics: &Value, observation: &Value) -> Result<Value, ApiError> {
    if semantics.get("protocol").and_then(Value::as_str) != Some("oo.oracle.native-c5-source-semantics/v1")
        || semantics.get("capture_method").and_then(Value::as_str) != Some("C5")
        || semantics.get("consistency_grade").and_then(Value::as_str) != Some("S0")
        || semantics.get("delete_detection").and_then(Value::as_str) != Some("reconcile")
        || observation.get("source_bound").and_then(|bound| bound.get("source_id")) != semantics.get("source_id")
        || observation.get("source_bound").and_then(|bound| bound.get("source_generation")) != semantics.get("source_generation")
        || observation.get("source_bound").and_then(|bound| bound.get("target_generation")) != semantics.get("target_generation")
        || catalog.get("source_id") != semantics.get("source_id")
        || catalog.get("source_generation") != semantics.get("source_generation")
        || catalog.get("target_generation") != semantics.get("target_generation")
    {
        return Err(ApiError::pack_refused());
    }
    let provenance = observation.get("provenance").ok_or_else(ApiError::pack_refused)?;
    let capture_method = provenance.get("capture_method").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let consistency = provenance.get("consistency_grade").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let delete_detection = provenance.get("delete_detection").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    if capture_method != "c5_diff"
        || consistency != semantics["consistency_grade"].as_str().unwrap_or("")
        || delete_detection != semantics["delete_detection"].as_str().unwrap_or("")
    {
        return Err(ApiError::pack_refused());
    }
    let tables = catalog.get("tables").and_then(Value::as_array).filter(|rows| rows.len() == 1).ok_or_else(ApiError::pack_refused)?;
    let table = &tables[0];
    let schema = table.get("logical_schema").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let name = table.get("logical_table").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let dataset = format!("{schema}.{name}");
    Ok(json!({(dataset.clone()): {
        "logical_table":dataset,
        "capture_method":capture_method,
        "consistency":consistency,
        "delete_detection":delete_detection,
        "source_generation":semantics["source_generation"],
        "target_generation":semantics["target_generation"]
    }}))
}

fn native_source_rebind(config: &AppConfig, row: &BuildRow, expected_sha: &str, source_semantics_sha256: Option<&str>) -> Result<Value, ApiError> {
    let project = config.projects.iter().find(|p| p.project_id == row.project_id).ok_or_else(ApiError::forbidden)?;
    let (raw, hashes) = sealed::native_maintenance_inputs(&project.sealed_bundle, project.source_protocol.as_ref())?;
    if hashes.get("catalog.json").map(String::as_str) != Some(expected_sha) { return Err(ApiError::pack_refused()); }
    let parse = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes).map_err(|_| ApiError::pack_refused());
    let read = |name: &str| raw.get(name).ok_or_else(ApiError::pack_refused).and_then(|b| parse(b));
    let root = Path::new(&config.work_dir).join("builds").join(&row.build_id);
    let original = |name: &str| std::fs::read(root.join(name)).map_err(|_| ApiError::not_ready()).and_then(|b| parse(&b));
    let command = original("native-model-output/command.json")?;
    let catalog = read("catalog.json")?;
    if read("native-model.json")? != original("native-model.json")?
        || hashes.get("rules.json").map(String::as_str) != original("S2.request.json")?["rules_sha256"].as_str()
        || semantic_basis(read("source_semantics.json")?) != semantic_basis(original("S0.response.json")?["source_adapt"]["source_semantics"].clone()) { return Err(ApiError::pack_refused()); }
    let mut allowed: Vec<String> = original("native-model.json")?["visibility"]["datasets"].as_array().ok_or_else(ApiError::pack_refused)?.iter().flat_map(|d| d["columns"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(|c| format!("{}.{}",d["dataset"].as_str().unwrap_or(""),c)))).collect();
    let model = original("native-model.json")?;
    let role = model["visibility"]["role"].as_str().ok_or_else(ApiError::pack_refused)?;
    let mut declared: Vec<String> = read("roles.json")?["roles"][role].as_array().ok_or_else(ApiError::pack_refused)?.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(ApiError::pack_refused)).collect::<Result<_,_>>()?;
    allowed.sort(); declared.sort(); if allowed != declared { return Err(ApiError::pack_refused()); }
    let progress = rebind_sealed_catalogs(&command["catalog"], &catalog)?;
    let mut oracle_projection = None;
    let current_semantics = if let Some(expected) = source_semantics_sha256 {
        if !hex64(expected) { return Err(ApiError::malformed()); }
        let path = root.join("source-semantics").join(format!("{expected}.json"));
        let dir = path.parent().ok_or_else(ApiError::pack_refused)?;
        let dir_meta = std::fs::symlink_metadata(dir).map_err(|_| ApiError::not_ready())?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| ApiError::not_ready())?;
        let work_uid = std::fs::metadata(&root).map_err(|_| ApiError::not_ready())?.uid();
        if !dir_meta.file_type().is_dir() || dir_meta.uid() != work_uid || dir_meta.mode() & 0o777 != 0o700
            || !metadata.file_type().is_file() || metadata.uid() != work_uid || metadata.mode() & 0o777 != 0o600 { return Err(ApiError::pack_refused()); }
        let bytes = std::fs::read(&path).map_err(|_| ApiError::not_ready())?;
        if sha256_hex(&bytes) != expected { return Err(ApiError::pack_refused()); }
        let semantics: Value = serde_json::from_slice(&bytes).map_err(|_| ApiError::pack_refused())?;
        if !matches!(semantics.get("protocol").and_then(Value::as_str), Some("oo.source.doris.audit/v1" | "oo.source.mysql.audit/v1" | "oo.oracle.native-c5-source-semantics/v1"))
            || semantic_basis(semantics.clone()) != semantic_basis(read("source_semantics.json")?)
        { return Err(ApiError::pack_refused()); }
        if semantics["protocol"] == "oo.source.mysql.audit/v1" { verify_mysql_c1_observation(&root, &catalog, &semantics)?; }
        else if semantics["protocol"] == "oo.source.doris.audit/v1" { verify_doris_observation(&root, &catalog, &semantics)?; }
        else { oracle_projection = Some(verify_oracle_c5_observation(&root, &catalog, &semantics)?); }
        Some(semantics)
    } else { None };
    let capture_methods = if let (Some(observation), Some(semantics)) = (oracle_projection.as_ref(), current_semantics.as_ref()) {
        Some(oracle_c5_capture_methods(&catalog, semantics, observation)?)
    } else {
        current_semantics.as_ref().and_then(|semantics| semantics.get("capture_methods")).cloned()
    };
    Ok(json!({"catalog":catalog,"catalog_sha256":expected_sha,"bundle_files":hashes,"progress_vector":progress,"source_semantics_sha256":source_semantics_sha256,"current_source_semantics":current_semantics,"oracle_source_semantics":oracle_projection,"capture_methods":capture_methods}))
}

fn verify_oracle_c5_provenance(
    provenance: &Value,
    request: &Value,
    bound: &Value,
    reader: &Value,
    load: &Value,
    catalog: &Value,
    catalog_table: &Value,
) -> Result<Value, ApiError> {
    let raw = |key: &str| -> Result<&str, ApiError> {
        provenance.get(key).and_then(Value::as_str).ok_or_else(ApiError::pack_refused)
    };
    let registry_raw = raw("source_registry_raw_json")?;
    let admission_raw = raw("source_admission_raw_json")?;
    let source_catalog_raw = raw("source_catalog_raw_json")?;
    let source_request_raw = raw("source_request_raw_json")?;
    let capture_raw = raw("capture_receipt_raw_json")?;
    for (field, bytes_key) in [
        ("source_registry_sha256", registry_raw),
        ("source_admission_sha256", admission_raw),
        ("source_catalog_sha256", source_catalog_raw),
        ("source_request_sha256", source_request_raw),
        ("capture_receipt_sha256", capture_raw),
    ] {
        if provenance.get(field).and_then(Value::as_str) != Some(sha256_hex(bytes_key.as_bytes()).as_str()) {
            return Err(ApiError::pack_refused());
        }
    }
    let registry: Value = serde_json::from_str(registry_raw).map_err(|_| ApiError::pack_refused())?;
    let admission: Value = serde_json::from_str(admission_raw).map_err(|_| ApiError::pack_refused())?;
    let source_catalog: Value = serde_json::from_str(source_catalog_raw).map_err(|_| ApiError::pack_refused())?;
    let source_request: Value = serde_json::from_str(source_request_raw).map_err(|_| ApiError::pack_refused())?;
    let capture: Value = serde_json::from_str(capture_raw).map_err(|_| ApiError::pack_refused())?;
    let manifest = provenance.get("scheduler_manifest").ok_or_else(ApiError::pack_refused)?;
    if provenance.get("protocol").and_then(Value::as_str) != Some("oo.oracle.native-c5-provenance/v1")
        || registry.get("protocol").and_then(Value::as_str) != Some("oo.source-admission.binding-registry/v1")
        || admission.get("schema").and_then(Value::as_str) != Some("oo.source-admission.capture-receipt/v1")
        || source_catalog.get("protocol").and_then(Value::as_str) != Some(oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL)
        || capture.get("protocol").and_then(Value::as_str) != Some("oo.oracle.capture-tick/v1")
        || source_request.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-update/v1")
        || manifest.get("protocol").and_then(Value::as_str) != Some("oo.jobs.native-schedule/v1")
        || provenance.get("task_state").and_then(Value::as_str) != Some("succeeded")
    {
        return Err(ApiError::pack_refused());
    }
    let bindings = registry.get("bindings").and_then(Value::as_array).filter(|rows| rows.len() == 1).ok_or_else(ApiError::pack_refused)?;
    let entry = &bindings[0];
    let source_fields = entry.get("fields").and_then(Value::as_array).filter(|rows| !rows.is_empty()).ok_or_else(ApiError::pack_refused)?;
    let registry_stamp = entry.get("admission_receipt_sha256").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
    let blank = "0".repeat(64);
    if registry_raw.matches(registry_stamp).count() != 1 {
        return Err(ApiError::pack_refused());
    }
    let registry_preimage = registry_raw.replacen(registry_stamp, &blank, 1);
    let preimage_sha = sha256_hex(registry_preimage.as_bytes());
    let admission_sha = sha256_hex(admission_raw.as_bytes());
    let source_catalog_sha = sha256_hex(source_catalog_raw.as_bytes());
    let source_request_sha = sha256_hex(source_request_raw.as_bytes());
    let capture_sha = sha256_hex(capture_raw.as_bytes());
    let source_schema = entry.get("schema_digest").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
    let source_id = catalog.get("source_id").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let source_generation = catalog.get("source_generation").and_then(Value::as_i64).ok_or_else(ApiError::pack_refused)?;
    let source_position = request.pointer("/snapshot/value").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let ack_request = provenance.get("acknowledged_request").ok_or_else(ApiError::pack_refused)?;
    let source_registry_path = raw("source_registry_path")?;
    let source_admission_path = raw("source_admission_path")?;
    let source_request_path = raw("source_request_path")?;
    let source_catalog_path = raw("source_catalog_path")?;
    let capture_receipt_path = raw("capture_receipt_path")?;
    let source_run_root = source_registry_path.strip_suffix("/sealed/registry.json").ok_or_else(ApiError::pack_refused)?;
    if preimage_sha != admission.get("registry_sha256").and_then(Value::as_str).unwrap_or("")
        || admission.get("registry_sha256").and_then(Value::as_str) != Some(preimage_sha.as_str())
        || registry_stamp != admission_sha
        || source_catalog_sha != entry.get("catalog_sha256").and_then(Value::as_str).unwrap_or("")
        || source_catalog.get("source_id").and_then(Value::as_str) != Some(source_id)
        || source_catalog.get("engine").and_then(Value::as_str) != Some("oracle")
        || source_catalog.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
        || entry.get("source_engine").and_then(Value::as_str) != Some("oracle")
        || entry.get("source_id").and_then(Value::as_str) != Some(source_id)
        || entry.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
        || entry.get("type_digest") != catalog.get("type_digest")
        || entry.get("config_digest") != catalog.get("config_digest")
        || entry.get("input_digest") != catalog.get("input_digest")
        || entry.get("type_digest") != admission.get("type_digest")
        || entry.get("config_digest") != admission.get("config_digest")
        || entry.get("input_digest") != admission.get("input_digest")
        || entry.get("schema_digest") != admission.get("schema_digest")
        || entry.get("schema_digest") != source_catalog.get("schema_digest")
        || source_catalog.get("type_digest") != entry.get("type_digest")
        || source_catalog.get("config_digest") != entry.get("config_digest")
        || source_catalog.get("input_digest") != entry.get("input_digest")
        || entry.get("admission_receipt_path").and_then(Value::as_str) != Some(source_admission_path)
        || entry.get("capture_receipt_path").and_then(Value::as_str) != Some(capture_receipt_path)
        || entry.get("catalog_path").and_then(Value::as_str) != Some(source_catalog_path)
        || source_request_path != format!("{source_run_root}/request.json")
        || source_catalog_path != format!("{source_run_root}/sealed/catalog.json")
        || request.get("schema_digest").and_then(Value::as_str) != Some(source_schema)
        || source_request.get("schema_digest").and_then(Value::as_str) != Some(source_schema)
        || provenance.get("source_schema_digest").and_then(Value::as_str) != Some(source_schema)
        || provenance.get("target_schema_digest") != catalog.get("schema_digest")
        || bound.get("schema_digest") != catalog.get("schema_digest")
        || provenance.get("source_request_sha256").and_then(Value::as_str) != Some(source_request_sha.as_str())
        || provenance.get("source_admission_sha256").and_then(Value::as_str) != Some(admission_sha.as_str())
        || provenance.get("source_catalog_sha256").and_then(Value::as_str) != Some(source_catalog_sha.as_str())
        || provenance.get("capture_receipt_sha256").and_then(Value::as_str) != Some(capture_sha.as_str())
        || ack_request != &source_request
    {
        return Err(ApiError::pack_refused());
    }
    if canonical_sha256(ack_request) != provenance.get("acknowledged_request_sha256").and_then(Value::as_str).unwrap_or("")
        || entry.get("snapshot") != request.get("snapshot")
        || source_request.get("snapshot") != request.get("snapshot")
        || admission.get("snapshot") != request.get("snapshot")
        || capture.get("snapshot") != request.get("snapshot")
        || entry.get("capture_receipt_sha256").and_then(Value::as_str) != Some(capture_sha.as_str())
        || source_request.get("audit_digest").and_then(Value::as_str) != Some(capture_sha.as_str())
        || source_request.get("admission_path") != Some(&json!(provenance.get("source_admission_path").and_then(Value::as_str).unwrap_or("")))
        || source_request.get("registry_path") != Some(&json!(provenance.get("source_registry_path").and_then(Value::as_str).unwrap_or("")))
        || admission.get("source_id").and_then(Value::as_str) != Some(source_id)
        || admission.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
        || admission.get("run_id") != entry.get("run_id")
        || admission.get("capture_method").and_then(Value::as_str) != Some("C5")
        || admission.get("delete_detection").and_then(Value::as_str) != Some("reconcile")
        || admission.get("consistency").and_then(Value::as_str) != Some("S0")
        || admission.get("write_attempts").and_then(Value::as_u64) != Some(0)
        || capture.get("source_id").and_then(Value::as_str) != Some(source_id)
        || capture.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
        || capture.get("label").and_then(Value::as_str) != Some(source_position)
        || capture.get("database").and_then(Value::as_str) != request.get("logical_schema").and_then(Value::as_str)
        || capture.get("product_reader_write_attempts").and_then(Value::as_u64) != Some(0)
        || provenance.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
        || load.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
    {
        return Err(ApiError::pack_refused());
    }
    let source_catalog_table = entry.get("objects").and_then(Value::as_array).filter(|rows| rows.len() == 1).and_then(|rows| rows.first()).ok_or_else(ApiError::pack_refused)?;
    if source_catalog_table.get("logical_schema") != Some(&json!(request["logical_schema"]))
        || source_catalog_table.get("logical_table") != Some(&json!(request["logical_table"]))
        || source_catalog_table.get("arrow_sha256") != request.get("input_digest")
        || source_catalog_table.get("row_count").and_then(Value::as_u64) != load.get("row_count").and_then(Value::as_u64)
        || source_request.get("source_id") != Some(&json!(source_id))
        || source_request.get("source_generation") != Some(&json!(source_generation))
    {
        return Err(ApiError::pack_refused());
    }
    let capture_tables = capture.get("tables").and_then(Value::as_array).filter(|rows| rows.len() == 1).and_then(|rows| rows.first()).ok_or_else(ApiError::pack_refused)?;
    let source_request_columns = source_request.get("columns").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    for key in ["protocol", "source_engine", "source_id", "source_generation", "parent_source_generation", "full_snapshot", "snapshot", "capture_method", "delete_detection", "consistency_grade", "cross_table_consistent", "position_codec", "position_lineage", "applied_pos", "logical_schema", "logical_table", "columns", "tombstones", "config_digest", "schema_digest", "type_digest", "input_digest"] {
        if source_request.get(key) != request.get(key) { return Err(ApiError::pack_refused()); }
    }
    if capture_tables.get("name").and_then(Value::as_str) != request.get("logical_table").and_then(Value::as_str)
        || capture_tables.get("route").and_then(Value::as_str) != Some("C5")
        || capture_tables.get("consistency").and_then(Value::as_str) != Some("S0")
        || capture_tables.get("delete_mode").and_then(Value::as_str) != Some("reconcile")
        || capture_tables.get("quality").and_then(Value::as_str) != Some("DELETES_LAGGED")
        || capture_tables.get("rows").and_then(Value::as_array).map_or(true, |rows| rows.len() as u64 != load.get("row_count").and_then(Value::as_u64).unwrap_or(u64::MAX))
        || capture_tables.get("tombstones").and_then(Value::as_array).is_none_or(|rows| !rows.iter().any(|item| item.as_str().is_some_and(|value| value.starts_with("2:"))))
        || source_request.get("admission_expect").and_then(|value| value.get("declared_receipt_sha256")).and_then(Value::as_str) != Some(registry_stamp)
    {
        return Err(ApiError::pack_refused());
    }
    let source_request_columns = source_request_columns.len();
    if source_request_columns != source_fields.len() { return Err(ApiError::pack_refused()); }
    let request_fields = request.get("columns").and_then(Value::as_array).filter(|rows| rows.len() == source_fields.len()).ok_or_else(ApiError::pack_refused)?;
    let oracle_fields = reader.get("oracle_columns").and_then(Value::as_array).filter(|rows| rows.len() == source_fields.len()).ok_or_else(ApiError::pack_refused)?;
    let target_columns = provenance.get("target_columns").and_then(Value::as_array).filter(|rows| rows.len() == source_fields.len() + 1).ok_or_else(ApiError::pack_refused)?;
    let internal = &target_columns[0];
    if internal.get("name").and_then(Value::as_str) != Some("_oo_pos")
        || internal.get("oid").and_then(Value::as_u64) != Some(1700)
        || internal.get("typmod").and_then(Value::as_i64) != Some(2_490_372)
        || internal.get("nullable").and_then(Value::as_bool) != Some(false)
        || internal.get("type").and_then(Value::as_str) != Some("numeric(38,0)")
        || internal.get("ordinal").and_then(Value::as_u64) != Some(1)
    {
        return Err(ApiError::pack_refused());
    }
    let target_catalog_columns = catalog_table.get("columns").and_then(Value::as_array).filter(|rows| rows.len() == source_fields.len()).ok_or_else(ApiError::pack_refused)?;
    let mut column_mapping = Vec::with_capacity(source_fields.len());
    for index in 0..source_fields.len() {
        let field = &source_fields[index];
        let request_field = &request_fields[index];
        let oracle_field = &oracle_fields[index];
        let target_catalog_field = &target_catalog_columns[index];
        let target_field = &target_columns[index + 1];
        let name = field.get("name").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let arrow_type = field.get("arrow_type").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let nullable = field.get("nullable").and_then(Value::as_bool).ok_or_else(ApiError::pack_refused)?;
        let mut request_field_without_logical = request_field.clone();
        request_field_without_logical.as_object_mut().ok_or_else(ApiError::pack_refused)?.remove("logical_name");
        if &request_field_without_logical != field
            || request_field.get("logical_name").and_then(Value::as_str) != Some(name)
            || decode_oracle_name(&target_catalog_field["logical_name"])? != name
            || target_field.get("name").and_then(Value::as_str) != target_catalog_field.get("stored_physical").and_then(Value::as_str)
            || target_field.get("ordinal").and_then(Value::as_u64) != Some(index as u64 + 2)
        {
            return Err(ApiError::pack_refused());
        }
        let source_native = oracle_field.get("data_type").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        if oracle_field.get("name").and_then(Value::as_str) != Some(name)
            || oracle_field.get("nullable").and_then(Value::as_bool) != Some(nullable)
        {
            return Err(ApiError::pack_refused());
        }
        let (expected_source_type, expected_oid, expected_typmod, expected_pg_type) = match arrow_type {
            "decimal128" => {
                if source_native != "NUMBER" || oracle_field.get("precision") != field.get("precision") || oracle_field.get("scale") != field.get("scale") {
                    return Err(ApiError::pack_refused());
                }
                let precision = field.get("precision").and_then(Value::as_u64).filter(|value| *value > 0 && *value <= 38).ok_or_else(ApiError::pack_refused)?;
                let scale = field.get("scale").and_then(Value::as_i64).filter(|value| *value >= 0 && *value <= 38).ok_or_else(ApiError::pack_refused)?;
                (format!("decimal128_{precision}_{scale}"), 1700_u64, (((precision << 16) | scale as u64) + 4) as i64, format!("numeric({precision},{scale})"))
            }
            "timestamp_micros" => {
                if source_native != "DATE" || oracle_field.get("precision").is_some_and(|value| !value.is_null()) || oracle_field.get("scale").is_some_and(|value| !value.is_null()) {
                    return Err(ApiError::pack_refused());
                }
                ("timestamp_micros".to_owned(), 1114_u64, -1_i64, "timestamp without time zone".to_owned())
            }
            "utf8" => {
                if !matches!(source_native, "VARCHAR2" | "CHAR") || oracle_field.get("char_length").and_then(Value::as_u64).is_none_or(|value| value == 0) {
                    return Err(ApiError::pack_refused());
                }
                ("utf8".to_owned(), 1043_u64, -1_i64, "character varying".to_owned())
            }
            "binary" => {
                if source_native != "RAW" { return Err(ApiError::pack_refused()); }
                ("binary".to_owned(), 17_u64, -1_i64, "bytea".to_owned())
            }
            _ => return Err(ApiError::pack_refused()),
        };
        if target_catalog_field.get("source_type").and_then(Value::as_str) != Some(expected_source_type.as_str())
            || target_catalog_field.get("oid").and_then(Value::as_u64) != Some(expected_oid)
            || target_catalog_field.get("typmod").and_then(Value::as_i64) != Some(expected_typmod)
            || target_field.get("oid").and_then(Value::as_u64) != Some(expected_oid)
            || target_field.get("typmod").and_then(Value::as_i64) != Some(expected_typmod)
            || target_field.get("nullable").and_then(Value::as_bool) != Some(nullable)
            || target_field.get("type").and_then(Value::as_str) != Some(expected_pg_type.as_str())
        {
            return Err(ApiError::pack_refused());
        }
        column_mapping.push(json!({"source_name":name,"source_arrow_type":arrow_type,"source_nullable":nullable,
            "oracle_type":source_native,"target_logical_name":target_catalog_field["logical_name"],
            "target_physical_name":target_catalog_field["stored_physical"],"target_type":expected_pg_type,"target_nullable":target_field["nullable"]}));
    }
    let manifest_registration = manifest.get("registration").ok_or_else(ApiError::pack_refused)?;
    let manifest_path = manifest_registration.get("registry_path").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let binding = provenance.get("binding").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let pin = provenance.get("pin").and_then(Value::as_u64).ok_or_else(ApiError::pack_refused)?;
    let task = provenance.get("task").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
    let lease_generation = provenance.get("lease_generation").and_then(Value::as_u64).ok_or_else(ApiError::pack_refused)?;
    let uuid_shape = |value: &str| {
        value.len() == 36
            && value.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) { byte == b'-' } else { byte.is_ascii_hexdigit() }
            })
    };
    let expected_registry_path = manifest_path.replace("{binding}", binding).replace("{pin}", &pin.to_string());
    let producer = manifest.get("producer").ok_or_else(ApiError::pack_refused)?;
    let loader = manifest.get("loader").ok_or_else(ApiError::pack_refused)?;
    if provenance.get("source_schema_digest").and_then(Value::as_str) != Some(source_schema)
        || provenance.get("source_registry_preimage_sha256").and_then(Value::as_str) != Some(preimage_sha.as_str())
        || provenance.get("source_admission_path").and_then(Value::as_str) != source_request.get("admission_path").and_then(Value::as_str)
        || provenance.get("source_registry_path").and_then(Value::as_str) != source_request.get("registry_path").and_then(Value::as_str)
        || expected_registry_path != source_request.get("registry_path").and_then(Value::as_str).unwrap_or("")
        || provenance.get("binding") != manifest.get("binding")
        || manifest.get("source_id").and_then(Value::as_str) != Some(source_id)
        || manifest.get("method").and_then(Value::as_str) != Some("C5")
        || manifest.get("cycle").and_then(Value::as_str) != Some("pull")
        || producer.get("path").and_then(Value::as_str) != Some("/opt/oracle-driver-probe")
        || producer.get("sha256") != entry.get("driver_digest")
        || producer.get("sha256") != entry.get("driver_sha256")
        || loader.get("path").and_then(Value::as_str) != Some("/opt/loader")
        || !loader.get("sha256").and_then(Value::as_str).is_some_and(hex64)
        || manifest_registration.get("admission_binary").and_then(Value::as_str) != Some("/opt/admission")
        || !manifest_registration.get("admission_sha256").and_then(Value::as_str).is_some_and(hex64)
        || provenance.get("task_state").and_then(Value::as_str) != Some("succeeded")
        || pin == 0 || lease_generation == 0 || !uuid_shape(binding) || !uuid_shape(task)
        || provenance.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
        || provenance.get("acknowledged_request_sha256").and_then(Value::as_str) != Some(canonical_sha256(ack_request).as_str())
        || provenance.get("scheduler_manifest_sha256").and_then(Value::as_str) != Some(canonical_sha256(manifest).as_str())
    {
        return Err(ApiError::pack_refused());
    }
    Ok(json!({
        "source_schema_digest":source_schema,
        "target_schema_digest":catalog["schema_digest"],
        "source_registry_sha256":provenance["source_registry_sha256"],
        "source_registry_preimage_sha256":preimage_sha,
        "source_admission_sha256":admission_sha,
        "source_catalog_sha256":source_catalog_sha,
        "source_request_sha256":source_request_sha,
        "acknowledged_request_sha256":provenance["acknowledged_request_sha256"],
        "scheduler_manifest_sha256":provenance["scheduler_manifest_sha256"],
        "capture_receipt_sha256":capture_sha,
        "source_run_id":entry["run_id"],
        "target_run_id":load["run_id"],
        "target_schema_compiler":"oo-builder/sealed-import-catalog",
        "capture_method":source_request["capture_method"],
        "consistency_grade":source_request["consistency_grade"],
        "delete_detection":source_request["delete_detection"],
        "column_mapping":column_mapping
    }))
}

fn verify_oracle_c5_observation(
    root: &Path,
    catalog: &Value,
    semantics: &Value,
) -> Result<Value, ApiError> {
    let id = semantics
        .get("evidence_set_id")
        .and_then(Value::as_str)
        .filter(|id| hex64(id))
        .ok_or_else(ApiError::pack_refused)?;
    if catalog.get("protocol").and_then(Value::as_str)
        != Some(oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL)
        || catalog.get("engine").and_then(Value::as_str) != Some("oracle")
        || semantics.get("source_id") != catalog.get("source_id")
        || semantics.get("source_generation") != catalog.get("source_generation")
        || semantics.get("target_generation") != catalog.get("target_generation")
        || semantics.get("executed").and_then(Value::as_bool) != Some(true)
        || semantics.get("product").and_then(Value::as_str) != Some("Oracle")
        || semantics.get("capture_method").and_then(Value::as_str) != Some("C5")
        || semantics.get("c1").and_then(Value::as_str) != Some("off")
        || semantics.get("consistency_grade").and_then(Value::as_str) != Some("S0")
        || semantics.get("delete_detection").and_then(Value::as_str) != Some("reconcile")
    {
        return Err(ApiError::pack_refused());
    }

    let source_contract = semantics
        .get("source_contract")
        .and_then(Value::as_object)
        .ok_or_else(ApiError::pack_refused)?;
    if source_contract.get("reader").and_then(Value::as_str) != Some("OO04_READER")
        || source_contract
            .get("write_attempts")
            .and_then(Value::as_u64)
            != Some(0)
        || source_contract.get("empty_string").and_then(Value::as_str) != Some("null")
        || !matches!(
            source_contract.get("log_mode").and_then(Value::as_str),
            Some("ARCHIVELOG" | "NOARCHIVELOG")
        )
        || !matches!(
            source_contract
                .get("supplemental_log")
                .and_then(Value::as_str),
            Some("YES" | "NO")
        )
    {
        return Err(ApiError::pack_refused());
    }

    let evidence = semantics
        .get("evidence")
        .and_then(Value::as_object)
        .ok_or_else(ApiError::pack_refused)?;
    let expected_names = [
        "capture-request",
        "db-contract",
        "load-result",
        "reader-observation",
        "source-provenance",
        "shadow",
        "source-bound",
        "source-gold",
        "target-gold",
        "typed-gold",
    ];
    if evidence.len() != expected_names.len()
        || expected_names
            .iter()
            .any(|name| !evidence.contains_key(*name))
    {
        return Err(ApiError::pack_refused());
    }
    let uid = std::fs::metadata(root)
        .map_err(|_| ApiError::not_ready())?
        .uid();
    let evidence_root = root.join("source-evidence");
    let evidence_dir = evidence_root.join(id);
    for dir in [&evidence_root, &evidence_dir] {
        let metadata = std::fs::symlink_metadata(dir).map_err(|_| ApiError::not_ready())?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != uid
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(ApiError::pack_refused());
        }
    }
    let mut hashes = BTreeMap::new();
    let mut records = BTreeMap::new();
    for name in expected_names {
        let item = evidence
            .get(name)
            .and_then(Value::as_object)
            .ok_or_else(ApiError::pack_refused)?;
        let expected_path = format!("source-evidence/{id}/{name}.json");
        if item.get("path").and_then(Value::as_str) != Some(expected_path.as_str()) {
            return Err(ApiError::pack_refused());
        }
        let path = evidence_dir.join(format!("{name}.json"));
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| ApiError::not_ready())?;
        let expected_sha = item
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|sha| hex64(sha))
            .ok_or_else(ApiError::pack_refused)?;
        if !metadata.file_type().is_file()
            || metadata.uid() != uid
            || metadata.mode() & 0o777 != 0o600
            || metadata.len() > 16 * 1024 * 1024
            || item.get("uid").and_then(Value::as_u64) != Some(uid as u64)
            || item.get("mode").and_then(Value::as_u64) != Some(0o600)
            || item.get("size").and_then(Value::as_u64) != Some(metadata.len())
        {
            return Err(ApiError::pack_refused());
        }
        let bytes = std::fs::read(&path).map_err(|_| ApiError::not_ready())?;
        if sha256_hex(&bytes) != expected_sha {
            return Err(ApiError::pack_refused());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| ApiError::pack_refused())?;
        hashes.insert(name.to_owned(), expected_sha.to_owned());
        records.insert(name.to_owned(), value);
    }
    if canonical_sha256(&json!(hashes)) != id {
        return Err(ApiError::pack_refused());
    }

    let request = &records["capture-request"];
    let db_contract = &records["db-contract"];
    let load = &records["load-result"];
    let reader = &records["reader-observation"];
    let shadow = &records["shadow"];
    let source_gold = records["source-gold"]
        .as_array()
        .ok_or_else(ApiError::pack_refused)?;
    let target_gold = records["target-gold"]
        .as_array()
        .ok_or_else(ApiError::pack_refused)?;
    let typed = &records["typed-gold"];
    let catalog_table = catalog
        .get("tables")
        .and_then(Value::as_array)
        .filter(|tables| tables.len() == 1)
        .and_then(|tables| tables.first())
        .ok_or_else(ApiError::pack_refused)?;
    let rows = catalog_table
        .get("accepted_rows")
        .and_then(Value::as_u64)
        .ok_or_else(ApiError::pack_refused)?;
    let columns = catalog_table
        .get("columns")
        .and_then(Value::as_array)
        .filter(|columns| !columns.is_empty())
        .ok_or_else(ApiError::pack_refused)?;
    let position = request
        .pointer("/snapshot/value")
        .and_then(Value::as_str)
        .ok_or_else(ApiError::pack_refused)?;
    if request.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-update/v1")
        || request.get("source_engine").and_then(Value::as_str) != Some("oracle")
        || request.get("source_id") != catalog.get("source_id")
        || request.get("source_generation") != catalog.get("source_generation")
        || request.get("target_generation") != catalog.get("target_generation")
        || request.get("run_id") != catalog.get("run_id")
        || request.get("config_digest") != catalog.get("config_digest")
        || request.get("type_digest") != catalog.get("type_digest")
        || request.get("input_digest") != catalog.get("input_digest")
        || request.get("capture_method").and_then(Value::as_str) != Some("c5_diff")
        || request.get("delete_detection").and_then(Value::as_str) != Some("reconcile")
        || request.get("consistency_grade").and_then(Value::as_str) != Some("S0")
        || request
            .get("cross_table_consistent")
            .and_then(Value::as_bool)
            != Some(false)
        || request.get("full_snapshot").and_then(Value::as_bool) != Some(true)
        || request.pointer("/snapshot/kind").and_then(Value::as_str) != Some("scn")
        || source_contract.get("position").and_then(Value::as_str) != Some(position)
        || request.get("logical_schema").and_then(Value::as_str) != Some(decode_oracle_name(&catalog_table["logical_schema"])?.as_str())
        || request.get("logical_table").and_then(Value::as_str) != Some(decode_oracle_name(&catalog_table["logical_table"])?.as_str())
        || request
            .get("tombstones")
            .and_then(Value::as_array)
            .is_none()
        || request
            .get("columns")
            .and_then(Value::as_array)
            .map_or(true, |items| items.len() != columns.len())
    {
        return Err(ApiError::pack_refused());
    }
    if load.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-update-result/v1")
        || load.get("source_id") != catalog.get("source_id")
        || load.get("source_generation") != catalog.get("source_generation")
        || load.get("target_generation") != catalog.get("target_generation")
        || load.get("run_id") != catalog.get("run_id")
        || load.get("logical_schema") != catalog_table.get("logical_schema")
        || load.get("logical_table") != catalog_table.get("logical_table")
        || load.get("physical_name") != catalog_table.get("stored_physical")
        || load.get("row_count").and_then(Value::as_u64) != Some(rows)
        || load.get("loader_invoked").and_then(Value::as_bool) != Some(true)
        || load.get("source_bound").and_then(Value::as_bool) != Some(true)
        || load.get("mirror_committed").and_then(Value::as_bool) != Some(true)
        || load.get("load_state").and_then(Value::as_str) != Some("awaiting_publish")
    {
        return Err(ApiError::pack_refused());
    }
    if shadow.get("generation") != catalog.get("source_generation")
        || shadow.get("position").and_then(Value::as_str) != Some(position)
        || shadow.get("method").and_then(Value::as_str) != Some("C5")
        || shadow.get("consistency_grade").and_then(Value::as_str) != Some("S0")
        || shadow.get("delete_detection").and_then(Value::as_str) != Some("reconcile")
        || shadow
            .get("cells")
            .and_then(Value::as_object)
            .is_none_or(|cells| cells.len() as u64 != rows)
        || source_gold != target_gold
        || source_gold.len() as u64 != rows
        || typed.get("source_generation") != catalog.get("source_generation")
        || typed.get("source_id") != catalog.get("source_id")
        || typed.get("rows").and_then(Value::as_u64) != Some(rows)
        || typed.get("cells").and_then(Value::as_u64) != Some(rows * columns.len() as u64)
        || typed.get("differences").and_then(Value::as_u64) != Some(0)
        || semantics.pointer("/quality/tables").and_then(Value::as_u64) != Some(1)
        || semantics.pointer("/quality/rows").and_then(Value::as_u64) != Some(rows)
        || semantics
            .pointer("/quality/columns")
            .and_then(Value::as_u64)
            != Some(columns.len() as u64)
        || semantics
            .pointer("/quality/typed_cells")
            .and_then(Value::as_u64)
            != Some(rows * columns.len() as u64)
        || semantics
            .pointer("/quality/typed_cell_differences")
            .and_then(Value::as_u64)
            != Some(0)
        || semantics
            .pointer("/quality/delete_quality")
            .and_then(Value::as_str)
            != Some("DELETES_LAGGED")
    {
        return Err(ApiError::pack_refused());
    }
    let source_hash = sha256_hex(
        &std::fs::read(evidence_dir.join("source-gold.json")).map_err(|_| ApiError::not_ready())?,
    );
    let target_hash = sha256_hex(
        &std::fs::read(evidence_dir.join("target-gold.json")).map_err(|_| ApiError::not_ready())?,
    );
    if typed.get("source_gold_sha256").and_then(Value::as_str) != Some(source_hash.as_str())
        || typed.get("target_gold_sha256").and_then(Value::as_str) != Some(target_hash.as_str())
    {
        return Err(ApiError::pack_refused());
    }

    let bound = &records["source-bound"];
    let provenance_projection = verify_oracle_c5_provenance(
        &records["source-provenance"],
        request,
        bound,
        reader,
        load,
        catalog,
        catalog_table,
    )?;
    let dictionary_columns = bound.pointer("/dictionary/columns").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    let mut reader_dictionary = reader.get("dictionary_columns").and_then(Value::as_array).cloned().ok_or_else(ApiError::pack_refused)?;
    let mut bound_dictionary = dictionary_columns.clone();
    reader_dictionary.sort_by_key(|item| item.as_str().unwrap_or("").to_owned());
    bound_dictionary.sort_by_key(|item| item.as_str().unwrap_or("").to_owned());
    let session_privileges = reader.get("session_privileges").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    let object_privileges = reader.get("object_privileges").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    let source_privileges: Vec<&str> = object_privileges.iter().filter(|item| item.get("object").and_then(Value::as_str) == Some("OO04_FIX.ONBOARD_ORDERS")).filter_map(|item| item.get("privilege").and_then(Value::as_str)).collect();
    if db_contract.get("protocol").and_then(Value::as_str) != Some("oo.oracle.database-contract-observation/v1")
        || db_contract.get("user").and_then(Value::as_str) != Some("SYSTEM")
        || db_contract.get("log_mode") != source_contract.get("log_mode")
        || db_contract.get("supplemental_log") != source_contract.get("supplemental_log")
        || reader.get("protocol").and_then(Value::as_str) != Some("oo.oracle.reader-observation/v1")
        || reader.get("user").and_then(Value::as_str) != Some("OO04_READER")
        || reader.get("container").and_then(Value::as_str) != Some("FREEPDB1")
        || reader.get("banner") != bound.get("banner")
        || reader_dictionary != bound_dictionary
        || !session_privileges.iter().any(|item| item.as_str() == Some("CREATE SESSION"))
        || source_privileges.contains(&"INSERT") || source_privileges.contains(&"UPDATE") || source_privileges.contains(&"DELETE")
        || !source_privileges.contains(&"SELECT") || !source_privileges.contains(&"FLASHBACK")
    { return Err(ApiError::pack_refused()); }
    let shadow_cells = shadow
        .get("cells")
        .and_then(Value::as_object)
        .ok_or_else(ApiError::pack_refused)?;
    let mut expected_keys: Vec<String> = shadow_cells.keys().cloned().collect();
    expected_keys.sort_by_key(|key| key.parse::<u64>().unwrap_or(u64::MAX));
    let mut gold_keys: Vec<String> = source_gold
        .iter()
        .map(|row| {
            row.get("key")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(ApiError::pack_refused)
        })
        .collect::<Result<_, _>>()?;
    gold_keys.sort_by_key(|key| key.parse::<u64>().unwrap_or(u64::MAX));
    let bindings = bound
        .get("bindings")
        .and_then(Value::as_array)
        .ok_or_else(ApiError::pack_refused)?;
    let mut expected_columns: Vec<String> = bindings
        .iter()
        .map(|binding| {
            binding
                .get("dictionary_column")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(ApiError::pack_refused)
        })
        .collect::<Result<_, _>>()?;
    expected_columns.sort();
    let mut request_columns: Vec<String> = request
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(ApiError::pack_refused)?
        .iter()
        .map(|item| {
            item.get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(ApiError::pack_refused)
        })
        .collect::<Result<_, _>>()?;
    request_columns.sort();
    let mut gold_columns: Vec<String> = typed
        .get("all_approved_columns")
        .and_then(Value::as_array)
        .ok_or_else(ApiError::pack_refused)?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(ApiError::pack_refused)
        })
        .collect::<Result<_, _>>()?;
    gold_columns.sort();
    let catalog_text = crate::canonical::canonical_json(catalog);
    let bound_text = crate::canonical::canonical_json(bound);
    let derived = oo_core::source_bound::derive_source_bound(&catalog_text, &bound_text)
        .map_err(|_| ApiError::pack_refused())?;
    if derived.scn != position.rsplit('-').next().unwrap_or("")
        || derived.registry_snapshot != position
        || derived.consistency_grade.as_deref() != Some("S0")
        || derived.delete_detection.as_deref() != Some("reconcile")
        || derived.columns.len() != columns.len()
        || request_columns != expected_columns
        || gold_columns != expected_columns
        || gold_keys != expected_keys
        || typed.get("reader").and_then(Value::as_str) != Some("OO04_READER")
        || typed.get("reader_exit").and_then(Value::as_u64) != Some(0)
        || semantics
            .pointer("/quality/key_2_absent")
            .and_then(Value::as_bool)
            != Some(true)
        || shadow
            .get("quality")
            .and_then(Value::as_array)
            .is_none_or(|quality| {
                !quality
                    .iter()
                    .any(|item| item.as_str() == Some("DELETES_LAGGED"))
            })
        || shadow
            .get("tombstones")
            .and_then(Value::as_array)
            .is_none_or(|tombstones| {
                !tombstones
                    .iter()
                    .any(|item| item.as_str().is_some_and(|value| value.starts_with("2:")))
            })
    {
        return Err(ApiError::pack_refused());
    }
    Ok(json!({
        "source_bound":bound,
        "derived_identity_sha256":derived.identity_hash,
        "source_gold_sha256":source_hash,
        "target_gold_sha256":target_hash,
        "typed_gold":typed,
        "provenance":provenance_projection
    }))
}

fn project_oracle_c5_semantics(effective: &mut Value, catalog: &Value, bound: &Value) -> Result<(), ApiError> {
    pg_builder::project_sealed_source_contract(effective, catalog, Some(bound))
        .map_err(|error| { eprintln!("native-maintenance Oracle source-bound refused: {}", error.code); ApiError::pack_refused() })?;
    let tables = catalog.get("tables").and_then(Value::as_array).filter(|tables| tables.len() == 1).ok_or_else(ApiError::pack_refused)?;
    let dataset = format!("{}.{}", tables[0].get("logical_schema").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?, tables[0].get("logical_table").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?);
    let bindings = bound.get("bindings").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    let dictionary = bound.pointer("/dictionary/columns").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    let date_columns: BTreeSet<String> = bindings.iter().filter_map(|binding| {
        let dictionary_name = binding.get("dictionary_column")?.as_str()?;
        let sealed_name = binding.get("sealed_column")?.as_str()?;
        let is_date = dictionary.iter().filter_map(Value::as_str).any(|line| {
            let mut parts = line.split('|');
            parts.next() == Some(dictionary_name) && parts.next() == Some("DATE")
        });
        is_date.then(|| sealed_name.to_owned())
    }).collect();
    let datasets = effective.get_mut("datasets").and_then(Value::as_array_mut).ok_or_else(ApiError::pack_refused)?;
    let current = datasets.iter_mut().find(|row| row.get("dataset").and_then(Value::as_str) == Some(dataset.as_str())).ok_or_else(ApiError::pack_refused)?;
    let source = current.get_mut("source_semantics").and_then(Value::as_object_mut).ok_or_else(ApiError::pack_refused)?;
    source.insert("capture".to_owned(), json!("c5_diff"));
    let source_columns = source.get_mut("columns").and_then(Value::as_array_mut).ok_or_else(ApiError::pack_refused)?;
    for name in date_columns {
        if let Some(column) = source_columns.iter_mut().find(|column| column.get("column").and_then(Value::as_str) == Some(name.as_str())) {
            column.as_object_mut().ok_or_else(ApiError::pack_refused)?.insert("native_note".to_owned(), json!("oracle_date_includes_time"));
        } else {
            source_columns.push(json!({"column":name,"native_note":"oracle_date_includes_time"}));
        }
    }
    Ok(())
}

fn verify_mysql_c1_observation(root: &Path, catalog: &Value, semantics: &Value) -> Result<(), ApiError> {
    let id = semantics["evidence_set_id"].as_str().filter(|id| hex64(id)).ok_or_else(ApiError::pack_refused)?;
    let uid = std::fs::metadata(root).map_err(|_| ApiError::not_ready())?.uid();
    let evidence_dir = root.join("source-evidence").join(id);
    for dir in [root.join("source-evidence"), evidence_dir.clone()] {
        let m = std::fs::symlink_metadata(dir).map_err(|_| ApiError::not_ready())?;
        if !m.is_dir() || m.file_type().is_symlink() || m.uid()!=uid || m.mode()&0o777!=0o700 { return Err(ApiError::pack_refused()); }
    }
    let read = |kind: &str| -> Result<Value, ApiError> {
        let item = &semantics["c1_evidence"][kind];
        let name = format!("{kind}.json");
        let expected_path = format!("source-evidence/{id}/{name}");
        if item["path"] != expected_path { return Err(ApiError::pack_refused()); }
        let path = evidence_dir.join(name);
        let m = std::fs::symlink_metadata(&path).map_err(|_| ApiError::not_ready())?;
        if !m.file_type().is_file() || m.uid()!=uid || m.mode()&0o777!=0o600 || m.len()>16*1024*1024 { return Err(ApiError::pack_refused()); }
        let bytes = std::fs::read(path).map_err(|_| ApiError::not_ready())?;
        if item["sha256"] != sha256_hex(&bytes) { return Err(ApiError::pack_refused()); }
        serde_json::from_slice(&bytes).map_err(|_| ApiError::pack_refused())
    };
    let request=read("request")?; let load=read("load")?; let ack=read("ack")?;
    if semantics["capture_method"]!="C1" || semantics["consistency_grade"]!="T" || semantics["delete_detection"]!="native"
        || semantics["cross_table_consistent"]!=true || semantics["position_codec"]!="mysql-gtid/v1"
        || semantics["source_epoch"].as_i64().is_none_or(|n|n<1)
        || request["protocol"]!="oo.pipeline.native-c1-baseline/v1" || request["capture_method"]!="c1_cdc"
        || request["baseline_scope"]["complete"]!=true || request["baseline_scope"]["consistency_grade"]!="T"
        || request["baseline_scope"]["cross_table_consistent"]!=true
        || request["baseline_scope"]["lineage"]!=semantics["position_lineage"]
        || load["protocol"]!="oo.pipeline.native-c1-baseline-result/v1" || load["load_state"]!="awaiting_publish"
        || ack["protocol"]!="oo.mysql.c1-ack-state/v1" || ack["acknowledged"]!=true || ack["task_state"]!="succeeded"
        || ack["receipt"]!=load || ack["request_digest"]!=load["request_digest"]
        || request["baseline_scope"]["config_digest"]!=load["source_instance"]["config_digest"]
        || request["source_identity"]["measured_id"]!=semantics["position_lineage"]
    { return Err(ApiError::pack_refused()); }
    for key in ["source_id","source_generation","source_epoch"] {
        if request[key]!=semantics[key] || load[key]!=semantics[key] || ack[key]!=semantics[key] { return Err(ApiError::pack_refused()); }
    }
    for key in ["target_generation","applied_pos"] {
        if load[key]!=semantics[key] || ack[key]!=semantics[key] { return Err(ApiError::pack_refused()); }
    }
    let native = request["native_requests"].as_array().ok_or_else(ApiError::pack_refused)?;
    let loaded = load["entries"].as_array().ok_or_else(ApiError::pack_refused)?;
    let entries = catalog["entries"].as_array().ok_or_else(ApiError::pack_refused)?;
    if entries.is_empty() || entries.len()!=native.len() || entries.len()!=loaded.len() { return Err(ApiError::pack_refused()); }
    let mut seen=BTreeSet::new(); let mut rejects=0u64;
    for entry in entries {
        if entry["engine"]!="mysql" || entry["source_id"]!=semantics["source_id"]
            || entry["source_generation"]!=semantics["source_generation"] || entry["target_generation"]!=semantics["target_generation"] {return Err(ApiError::pack_refused());}
        let tables=entry["tables"].as_array().filter(|t|t.len()==1).ok_or_else(ApiError::pack_refused)?;
        let t=&tables[0]; let schema=&t["logical_schema"]; let table=&t["logical_table"];
        if !seen.insert(format!("{}.{}",schema.as_str().ok_or_else(ApiError::pack_refused)?,table.as_str().ok_or_else(ApiError::pack_refused)?)) {return Err(ApiError::pack_refused());}
        let req=native.iter().find(|r|&r["logical_schema"]==schema && &r["logical_table"]==table).ok_or_else(ApiError::pack_refused)?;
        let result=loaded.iter().find(|r|&r["schema"]==schema && &r["table"]==table).ok_or_else(ApiError::pack_refused)?;
        for key in ["source_id","source_generation","applied_pos","position_codec","position_lineage","consistency_grade","cross_table_consistent"] {
            if req[key]!=semantics[key] {return Err(ApiError::pack_refused());}
        }
        let dataset=format!("{}.{}",schema.as_str().unwrap(),table.as_str().unwrap());
        let method=&semantics["capture_methods"][&dataset];
        if method["logical_table"]!=dataset || method["capture_method"]!="c1_cdc" || method["consistency"]!="T" || method["delete_detection"]!="native"
            || method["source_generation"]!=req["source_generation"] || method["target_generation"]!=semantics["target_generation"] {return Err(ApiError::pack_refused());}
        if req["capture_method"]!="c1_cdc" || req["delete_detection"]!="native" || result["run_id"]!=entry["run_id"]
            || result["physical_id"]!=t["stored_physical"] || result["row_count"]!=t["accepted_rows"] {return Err(ApiError::pack_refused());}
        rejects+=t["rejects"].as_array().ok_or_else(ApiError::pack_refused)?.len() as u64;
    }
    if semantics["capture_methods"].as_object().is_none_or(|m|m.len()!=seen.len()) || semantics["quality"]["zero_date_rejects"].as_u64()!=Some(rejects) {return Err(ApiError::pack_refused());}
    Ok(())
}

fn verify_doris_observation(root: &Path, catalog: &Value, semantics: &Value) -> Result<(), ApiError> {
    let entries = catalog.get("entries").and_then(Value::as_array).ok_or_else(ApiError::pack_refused)?;
    let evidence = semantics.get("evidence").and_then(Value::as_object).ok_or_else(ApiError::pack_refused)?;
    let methods = semantics.get("capture_methods").and_then(Value::as_object).ok_or_else(ApiError::pack_refused)?;
    let evidence_set_id = semantics.get("evidence_set_id").and_then(Value::as_str).filter(|id| hex64(id)).ok_or_else(ApiError::pack_refused)?;
    if evidence.len() != methods.len() { return Err(ApiError::pack_refused()); }
    let current_uid = std::fs::metadata(root).map_err(|_| ApiError::not_ready())?.uid();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if entry.get("engine").and_then(Value::as_str) != Some("doris") { return Err(ApiError::pack_refused()); }
        let tables = entry.get("tables").and_then(Value::as_array).filter(|rows| !rows.is_empty()).ok_or_else(ApiError::pack_refused)?;
        let source_generation = entry.get("source_generation").and_then(Value::as_i64).ok_or_else(ApiError::pack_refused)?;
        let target_generation = entry.get("target_generation").and_then(Value::as_i64).ok_or_else(ApiError::pack_refused)?;
        for table_object in tables {
        let schema = table_object.get("logical_schema").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let table = table_object.get("logical_table").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let dataset = format!("{schema}.{table}");
        if !seen.insert(dataset.clone()) { return Err(ApiError::pack_refused()); }
        let method = methods.get(&dataset).ok_or_else(ApiError::pack_refused)?;
        let method_name = method.get("capture_method").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let deletion = method.get("delete_detection").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
        let expected_deletion = if method_name == "c4_partition" { "partition_replace" } else { "reconcile" };
        if entry.get("source_id").and_then(Value::as_str) != semantics.get("source_id").and_then(Value::as_str)
            || method.get("logical_table").and_then(Value::as_str) != Some(&dataset)
            || !matches!(method_name, "c4_partition" | "c5_diff") || method.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
            || method.get("target_generation").and_then(Value::as_i64) != Some(target_generation)
            || deletion != expected_deletion
            || method.get("consistency").and_then(Value::as_str) != Some("S0")
        { return Err(ApiError::pack_refused()); }
        let pair = evidence.get(&dataset).and_then(Value::as_object).ok_or_else(ApiError::pack_refused)?;
        let mut request_receipt = None;
        let mut load_receipt = None;
        for kind in ["request", "load"] {
            let item = pair.get(kind).and_then(Value::as_object).ok_or_else(ApiError::pack_refused)?;
            let relative = item.get("path").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
            let rel = Path::new(relative);
            let components: Vec<_> = rel.components().collect();
            let evidence_root = root.join("source-evidence");
            let evidence_dir = evidence_root.join(evidence_set_id);
            let evidence_root_meta = std::fs::symlink_metadata(&evidence_root).map_err(|_| ApiError::not_ready())?;
            let evidence_dir_meta = std::fs::symlink_metadata(&evidence_dir).map_err(|_| ApiError::not_ready())?;
            let expected_file = format!("{dataset}.{kind}.json");
            if rel.is_absolute() || components.iter().any(|part| !matches!(part, std::path::Component::Normal(_))) || components.len() != 3
                || components[0].as_os_str() != "source-evidence" || components[1].as_os_str() != evidence_set_id || components[2].as_os_str().to_str() != Some(expected_file.as_str())
                || !evidence_root_meta.file_type().is_dir() || evidence_root_meta.uid() != current_uid || evidence_root_meta.mode() & 0o777 != 0o700
                || !evidence_dir_meta.file_type().is_dir() || evidence_dir_meta.uid() != current_uid || evidence_dir_meta.mode() & 0o777 != 0o700
            { return Err(ApiError::pack_refused()); }
            let path = root.join(rel);
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| ApiError::not_ready())?;
            if !metadata.file_type().is_file() || metadata.uid() != current_uid || metadata.mode() & 0o777 != 0o600
                || item.get("uid").and_then(Value::as_u64) != Some(current_uid as u64)
                || item.get("mode").and_then(Value::as_u64) != Some(0o600)
                || item.get("size").and_then(Value::as_u64) != Some(metadata.len())
            { return Err(ApiError::pack_refused()); }
            let bytes = std::fs::read(&path).map_err(|_| ApiError::not_ready())?;
            let sha = item.get("sha256").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
            if sha256_hex(&bytes) != sha { return Err(ApiError::pack_refused()); }
            let receipt: Value = serde_json::from_slice(&bytes).map_err(|_| ApiError::pack_refused())?;
            if receipt.get("logical_table").and_then(Value::as_str) != Some(table)
                || receipt.get("logical_schema").and_then(Value::as_str) != Some(schema)
                || receipt.get("source_id").and_then(Value::as_str) != Some(semantics["source_id"].as_str().ok_or_else(ApiError::pack_refused)?)
                || receipt.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
            { return Err(ApiError::pack_refused()); }
            if kind == "request" {
                let (protocol, deletion, full_snapshot) = if method_name == "c4_partition" { ("oo.pipeline.native-partition/v1", "partition_replace", false) } else { ("oo.pipeline.native-update/v1", "reconcile", true) };
                if receipt.get("protocol").and_then(Value::as_str) != Some(protocol)
                    || receipt.get("capture_method").and_then(Value::as_str) != Some(method_name)
                    || receipt.get("delete_detection").and_then(Value::as_str) != Some(deletion)
                    || receipt.get("full_snapshot").and_then(Value::as_bool) != Some(full_snapshot)
                { return Err(ApiError::pack_refused()); }
                request_receipt = Some(receipt.clone());
            } else if receipt.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.native-update-result/v1")
                || receipt.get("target_generation").and_then(Value::as_i64) != Some(target_generation)
                || receipt.get("load_state").and_then(Value::as_str) != Some("awaiting_publish")
            { return Err(ApiError::pack_refused()); }
            if kind == "load" { load_receipt = Some(receipt); }
        }
        if method_name == "c4_partition" {
            // A C4 load describes one changed partition, while the catalog seals
            // the complete, ACKed source partition vector. Bind those two views
            // through the producer's admitted digests and the current target
            // sidecar; a fixture partition name is not an identity check.
            let request = request_receipt.as_ref().ok_or_else(ApiError::pack_refused)?;
            let loaded = load_receipt.as_ref().ok_or_else(ApiError::pack_refused)?;
            let scope = loaded.get("partition_target_receipt")
                .and_then(|receipt| receipt.get("partition_scope"))
                .filter(|scope| scope.is_object()).ok_or_else(ApiError::pack_refused)?;
            let valid_partition_id = |id: &str| !id.is_empty() && id.len() <= 128
                && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            let partition = scope.get("partition_id").and_then(Value::as_str).filter(|id| valid_partition_id(id)).ok_or_else(ApiError::pack_refused)?;
            let observed = scope.get("observed_partition_ids").and_then(Value::as_array).filter(|ids| !ids.is_empty() && ids.len() <= 1024).ok_or_else(ApiError::pack_refused)?;
            let observed_ids = observed.iter().map(|id| id.as_str().filter(|id| valid_partition_id(id)).map(str::to_owned).ok_or_else(ApiError::pack_refused)).collect::<Result<Vec<_>, _>>()?;
            let mut sorted_ids = observed_ids.clone();
            sorted_ids.sort();
            sorted_ids.dedup();
            let schema_digest = scope.get("schema_digest").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
            let type_digest = scope.get("type_digest").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
            let table_epoch = scope.get("table_epoch").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
            let schema_fingerprint = scope.get("schema_fingerprint").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
            let arrow_digest = scope.get("arrow_sha256").and_then(Value::as_str).filter(|value| hex64(value)).ok_or_else(ApiError::pack_refused)?;
            let current_rows = scope.get("row_count").and_then(Value::as_u64).ok_or_else(ApiError::pack_refused)?;
            let vector = entry.get("partition_vector").and_then(Value::as_array).filter(|rows| rows.len() == observed_ids.len()).ok_or_else(ApiError::pack_refused)?;
            if sorted_ids != observed_ids || !observed_ids.iter().any(|id| id == partition)
                || scope.get("source_id").and_then(Value::as_str) != Some(semantics["source_id"].as_str().ok_or_else(ApiError::pack_refused)?)
                || scope.get("source_engine").and_then(Value::as_str) != Some("doris")
                || scope.get("logical_schema").and_then(Value::as_str) != Some(schema)
                || scope.get("logical_table").and_then(Value::as_str) != Some(table)
                || scope.get("source_generation").and_then(Value::as_i64) != Some(source_generation)
                || scope.get("complete").and_then(Value::as_bool) != Some(true)
                || scope.get("mutations_pending").and_then(Value::as_bool) != Some(false)
                || request.get("source_engine").and_then(Value::as_str) != Some("doris")
                || request.get("schema_digest").and_then(Value::as_str) != Some(schema_digest)
                || request.get("type_digest").and_then(Value::as_str) != Some(type_digest)
                || request.get("input_digest").and_then(Value::as_str) != Some(arrow_digest)
                || loaded.get("run_id") != entry.get("run_id")
                || loaded.get("source_engine").and_then(Value::as_str) != Some("doris")
                || loaded.get("target_generation").and_then(Value::as_i64) != Some(target_generation)
                || loaded.get("row_count").and_then(Value::as_u64) != table_object.get("accepted_rows").and_then(Value::as_u64)
                || table_object.get("accepted_rows").and_then(Value::as_u64).is_none()
            { return Err(ApiError::pack_refused()); }
            let mut vector_rows = 0_u64;
            let mut current_matches = 0;
            for (id, row) in observed_ids.iter().zip(vector) {
                let row_id = row.get("partition_id").and_then(Value::as_str).ok_or_else(ApiError::pack_refused)?;
                let count = row.get("row_count").and_then(Value::as_u64).ok_or_else(ApiError::pack_refused)?;
                let row_scope = row.get("partition_scope").filter(|value| value.is_object()).ok_or_else(ApiError::pack_refused)?;
                let row_source_generation = row.get("source_generation").and_then(Value::as_i64).filter(|value| *value > 0 && *value <= source_generation).ok_or_else(ApiError::pack_refused)?;
                let row_target_generation = row.get("target_generation").and_then(Value::as_i64).filter(|value| *value > 0 && *value <= target_generation).ok_or_else(ApiError::pack_refused)?;
                if row_id != id
                    || row_scope.get("partition_id").and_then(Value::as_str) != Some(id)
                    || row_scope.get("observed_partition_ids") != scope.get("observed_partition_ids")
                    || row_scope.get("source_id").and_then(Value::as_str) != Some(semantics["source_id"].as_str().ok_or_else(ApiError::pack_refused)?)
                    || row_scope.get("source_engine").and_then(Value::as_str) != Some("doris")
                    || row_scope.get("logical_schema").and_then(Value::as_str) != Some(schema)
                    || row_scope.get("logical_table").and_then(Value::as_str) != Some(table)
                    || row_scope.get("table_epoch").and_then(Value::as_str) != Some(table_epoch)
                    || row_scope.get("schema_fingerprint").and_then(Value::as_str) != Some(schema_fingerprint)
                    || row_scope.get("schema_digest").and_then(Value::as_str) != Some(schema_digest)
                    || row_scope.get("type_digest").and_then(Value::as_str) != Some(type_digest)
                    || row_scope.get("source_generation").and_then(Value::as_i64) != Some(row_source_generation)
                    || row_scope.get("arrow_sha256").and_then(Value::as_str).is_none_or(|value| !hex64(value))
                    || row_scope.get("complete").and_then(Value::as_bool) != Some(true)
                    || row_scope.get("mutations_pending").and_then(Value::as_bool) != Some(false)
                    || row_scope.get("row_count").and_then(Value::as_u64) != Some(count)
                    || row_scope.get("current_signature").and_then(Value::as_str).is_none_or(str::is_empty)
                { return Err(ApiError::pack_refused()); }
                if id == partition {
                    if row_scope != scope || count != current_rows || row_source_generation != source_generation
                        || row_target_generation != target_generation || row.get("run_id") != entry.get("run_id")
                    { return Err(ApiError::pack_refused()); }
                    current_matches += 1;
                }
                vector_rows = vector_rows.checked_add(count).ok_or_else(ApiError::pack_refused)?;
            }
            if current_matches != 1 || vector_rows != table_object["accepted_rows"].as_u64().ok_or_else(ApiError::pack_refused)? {
                return Err(ApiError::pack_refused());
            }
        }
        }
    }
    if seen.len() != methods.len() { return Err(ApiError::pack_refused()); }
    Ok(())
}

async fn latest_native_rebind(client: &Client, row: &BuildRow) -> Result<Option<Value>, ApiError> {
    let result = client.query_opt("SELECT (response_text::jsonb->'native_maintenance'->'source_rebind')::text FROM oo_selfbuild_api.maintenance_receipt WHERE build_id=$1 AND route='maintenance' AND response_text::jsonb->'native_maintenance'->'source_rebind'->>'catalog_sha256' IS NOT NULL ORDER BY created_at DESC, idempotency_key DESC LIMIT 1", &[&row.build_id]).await.map_err(|_| ApiError::not_ready())?;
    result.map(|r| { let text: String = r.try_get(0).map_err(|_| ApiError::not_ready())?; serde_json::from_str(&text).map_err(|_| ApiError::not_ready()) }).transpose()
}

async fn accepted_native_rebind(config: &AppConfig, client: &Client, row: &BuildRow) -> Result<Option<Value>, ApiError> {
    let Some(receipt) = latest_native_rebind(client, row).await? else { return Ok(None); };
    let sha = receipt["catalog_sha256"].as_str().ok_or_else(ApiError::pack_refused)?;
    native_source_rebind(config, row, sha, receipt.get("source_semantics_sha256").and_then(Value::as_str)).map(Some)
}

/// A collection's decision schema hash is the collection identity. A single catalog's is the schema digest.
fn bind_rebound_catalog(command: &mut Value, compiled: &oo_builder::SealedImportDraft) -> Result<(), ApiError> {
    let collection = command.get("catalog").and_then(|catalog| catalog.get("protocol")).and_then(Value::as_str) == Some(oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL);
    let previous = command["settings"]["catalog_version"].clone();
    for decision in command["decisions"].as_array_mut().ok_or_else(ApiError::pack_refused)? {
        if collection {
            if decision["schema_hash"] != previous { return Err(ApiError::pack_refused()); }
            decision["schema_hash"] = json!(compiled.identity_hash);
        } else if decision["schema_hash"] != json!(compiled.schema_digest) {
            return Err(ApiError::pack_refused());
        }
    }
    command["settings"]["catalog_version"] = json!(compiled.identity_hash);
    Ok(())
}

// Reuse the reviewed native compiler after durable owner curation. The sealed
// catalog and decisions are unchanged; only fact-bound descriptive labels pass.
fn compile_native_maintenance(config: &AppConfig, row: &BuildRow, docs: &Docs, rebound: Option<&Value>) -> Result<Value, ApiError> {
    let units = source_money_units(config, row)?;
    ensure_source_money(&docs.effective, &units)?;
    let root = Path::new(&config.work_dir).join("builds").join(&row.build_id);
    let original = root.join("native-model-output");
    if !original.join("command.json").is_file() { return Ok(Value::Null); }
    let read = |path: &Path| -> Result<Value, ApiError> { serde_json::from_slice(&std::fs::read(path).map_err(|_| ApiError::not_ready())?).map_err(|_| ApiError::pack_refused()) };
    let mut command = read(&original.join("command.json"))?;
    let original_command_sha = canonical_sha256(&command);
    let original_decisions_sha = canonical_sha256(&command["decisions"]);
    if let Some(rebound) = rebound { command["catalog"] = rebound["catalog"].clone(); }
    let catalog = command.get("catalog").ok_or_else(ApiError::pack_refused)?;
    let catalog_text = crate::canonical::canonical_json(catalog);
    let compiled = if catalog.get("protocol").and_then(Value::as_str) == Some(oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL) {
        oo_builder::compile_sealed_import_collection(&catalog_text)
    } else {
        oo_builder::compile_sealed_import_catalog(&catalog_text)
    }.map_err(|_| ApiError::pack_refused())?;
    if rebound.is_some() { bind_rebound_catalog(&mut command, &compiled)?; }
    let mut labels = Vec::new();
    for (dataset, entry) in &docs.effective.datasets {
        for (name, column) in &entry.columns {
            if column.label_from != "curated" { continue; }
            if column.binds_fact != column.fact_sha256 || column.classification_state != "confirmed" { return Err(ApiError::pack_refused()); }
            let sealed = compiled.columns.iter().find(|c| &c.dataset == dataset && &c.name == name).ok_or_else(ApiError::pack_refused)?;
            if column.sealed_data_type != crate::adapt::oos_from_target_name(&sealed.target_type).unwrap_or(&sealed.target_type) { eprintln!("native-maintenance type binding refused"); return Err(ApiError::pack_refused()); }
            labels.push(json!({"dataset":dataset,"column":name,"fact_fingerprint":sealed.fact_fingerprint,"label":column.label}));
        }
    }
    command["curated_labels"] = json!(labels);
    let reviewed_text = oo_builder::compile_reviewed_sealed_import(&crate::canonical::canonical_json(&command)).map_err(|error| { eprintln!("native-maintenance core refused: {}", error.code); ApiError::pack_refused() })?;
    let reviewed: Value = serde_json::from_str(&reviewed_text).map_err(|_| ApiError::pack_refused())?;
    let mut effective = reviewed.get("effective_oos").cloned().ok_or_else(ApiError::pack_refused)?;
    let native_input = read(&root.join("native-model.json"))?;
    pg_builder::project_sealed_source_contract(&mut effective, &command["catalog"], native_input.get("source_contract")).map_err(|error| { eprintln!("native-maintenance source refused: {}", error.code); ApiError::pack_refused() })?;
    if let Some(observation) = rebound.and_then(|value| value.get("current_source_semantics")).filter(|value| value.get("protocol").and_then(Value::as_str).is_some()) {
        match observation.get("protocol").and_then(Value::as_str) {
            Some("oo.source.mysql.audit/v1") => pg_builder::project_mysql_c1(&mut effective, &command["catalog"], observation).map_err(|error| { eprintln!("native-maintenance capture method refused: {}", error.code); ApiError::pack_refused() })?,
            Some("oo.source.doris.audit/v1") => pg_builder::project_doris_capture_methods(&mut effective, &command["catalog"], observation).map_err(|error| { eprintln!("native-maintenance capture method refused: {}", error.code); ApiError::pack_refused() })?,
            Some("oo.oracle.native-c5-source-semantics/v1") => {
                let bound = rebound.and_then(|value| value.get("oracle_source_semantics")).and_then(|value| value.get("source_bound")).ok_or_else(ApiError::pack_refused)?;
                project_oracle_c5_semantics(&mut effective, &command["catalog"], bound)?;
            }
            _ => return Err(ApiError::pack_refused()),
        }
    }
    let source_fact_corrections = project_native_source_money(&mut effective, &docs.effective, &units)?;
    let source_unit_corrections: Vec<Value> = source_fact_corrections.iter().filter_map(|item| {
        let before = item.get("reviewed_candidate")?.get("unit")?.as_str()?;
        let after = item.get("source_declared")?.get("unit")?.as_str()?;
        (before != after).then(|| json!({"column":item["column"],"candidate_unit":before,"declared_unit":after}))
    }).collect();
    let mut visibility = read(&original.join("visibility.json"))?;
    if rebound.is_some() { visibility["catalog_version"] = json!(compiled.identity_hash); }
    let pack = oo_builder::compile_role_pack(&crate::canonical::canonical_json(&effective), &crate::canonical::canonical_json(&visibility)).map_err(|error| { eprintln!("native-maintenance role refused: {}", error.code); ApiError::pack_refused() })?;
    let source_summary = rebound.and_then(|v| v.get("current_source_semantics")).map(|semantics| json!({"actual_core_effective_sha256":canonical_sha256(&effective),"capture_methods":rebound.and_then(|value| value.get("capture_methods")).cloned().unwrap_or_else(|| semantics.get("capture_method").cloned().unwrap_or(semantics["capture_methods"].clone())),"applied_pos":semantics.pointer("/source_contract/position").cloned().unwrap_or(semantics["applied_pos"].clone()),"source_epoch":semantics["source_epoch"],"position_lineage":semantics.pointer("/source_contract/position").cloned().unwrap_or(semantics["position_lineage"].clone()),"quality":semantics["quality"],"datasets":effective["datasets"].as_array().map(|rows| rows.iter().map(|row| json!({"dataset":row["dataset"],"source_semantics":row["source_semantics"]})).collect::<Vec<_>>()),"source_id":semantics["source_id"],"source_semantics_sha256":rebound.unwrap()["source_semantics_sha256"],"source_bound_identity_sha256":rebound.unwrap().pointer("/oracle_source_semantics/derived_identity_sha256"),"source_gold_sha256":rebound.unwrap().pointer("/oracle_source_semantics/source_gold_sha256"),"target_gold_sha256":rebound.unwrap().pointer("/oracle_source_semantics/target_gold_sha256"),"table_generations":semantics.get("source_generation").cloned().unwrap_or(semantics["table_generations"].clone()),"publication":"not_claimed","role_approval":"not_claimed"}));
    let source_text = source_summary.as_ref().map(crate::canonical::canonical_json);
    let source_json_sha256 = source_text.as_ref().map(|text| sha256_hex(text.as_bytes()));
    let receipt = json!({"source_semantics_sha256":rebound.map(|v| &v["source_semantics_sha256"]),"protocol":"oo.selfbuild-control-api/native-maintenance/v1","build_id":row.build_id,"overlay_effective_sha256":docs.effective_sha,"curated_sha256":docs.curated_sha,"generated_sha256":docs.generated_sha,"reviewed_sha256":sha256_hex(reviewed_text.as_bytes()),"source_unit_corrections":source_unit_corrections,"source_fact_corrections":source_fact_corrections,"effective_sha256":canonical_sha256(&effective),"source_json_sha256":source_json_sha256,"pack_digest":pack.pack_digest,"manifest_sha256":pack.manifest_sha256,"publication":"not_published","llm_calls":0,"source_rebind":rebound.map(|v| json!({"catalog_sha256":v["catalog_sha256"],"bundle_files":v["bundle_files"],"binding_vector":if v["catalog"]["protocol"]==oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL { v["catalog"]["binding_vector"].clone() } else { Value::Null },"progress_vector":v["progress_vector"],"source_semantics_sha256":v["source_semantics_sha256"],"capture_methods":v["capture_methods"],"original_command_sha256":original_command_sha,"original_decisions_sha256":original_decisions_sha,"rebound_decisions_sha256":canonical_sha256(&command["decisions"])}))});
    let digest = canonical_sha256(&receipt);
    let dest = root.join("native-maintenance").join(&digest);
    let mut files = BTreeMap::new();
    files.insert("command.json", crate::canonical::canonical_json(&command));
    files.insert("reviewed.json", reviewed_text);
    files.insert("effective.json", crate::canonical::canonical_json(&effective));
    files.insert("visibility.json", crate::canonical::canonical_json(&visibility));
    files.insert("role-pack.json", pack.artifact);
    files.insert("receipt.json", crate::canonical::canonical_json(&receipt));
    if let Some(source_text) = source_text { files.insert("source.json", source_text); }
    if dest.exists() {
        for (name, text) in &files { if std::fs::read(dest.join(name)).map_err(|_| ApiError::not_ready())? != text.as_bytes() { return Err(ApiError::pack_refused()); } }
    } else {
        std::fs::create_dir_all(&dest).map_err(|_| ApiError::not_ready())?;
        for (name, text) in &files { let mut file=OpenOptions::new().write(true).create_new(true).mode(0o600).open(dest.join(name)).map_err(|_| ApiError::not_ready())?; std::io::Write::write_all(&mut file,text.as_bytes()).map_err(|_| ApiError::not_ready())?; }
    }
    let mut result = receipt;
    result["artifact_directory"] = json!(dest.to_string_lossy());
    result["artifact_revision"] = json!(digest);
    Ok(result)
}

#[cfg(test)]
mod source_money_tests {
    use super::{ensure_source_money, project_native_source_money, semantic_basis, verify_doris_observation};
    use crate::oosyaml::generated_from_inputs;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn source_rebind_observations_advance_without_hiding_semantic_drift() {
        let original = json!({"source_id":"pg","source_generation":3,"target_generation":6,"capture_method":"C1","consistency_grade":"T","delete_detection":"native","source_contract":{"reader":"reader","write_attempts":0,"cross_table_consistent":true,"applied_end_lsn":"0/40"},"quality":{"tables":4,"columns":38,"rows":100054,"typed_cells":928358,"typed_cell_differences":0}});
        for generation in [4,5] {
            let mut next = original.clone(); next["source_generation"] = json!(generation);
            next["target_generation"] = json!(generation + 3);
            next["source_contract"]["applied_end_lsn"] = json!(format!("0/{generation}0"));
            next["quality"]["rows"] = json!(100054 + generation);
            next["quality"]["typed_cells"] = json!(928358 + generation * 38);
            assert_eq!(semantic_basis(original.clone()), semantic_basis(next));
        }
        for (path,value) in [("/consistency_grade",json!("S1")),("/delete_detection",json!("unknown")),("/capture_method",json!("C5")),("/source_contract/write_attempts",json!(1)),("/source_contract/cross_table_consistent",json!(false)),("/source_contract/reader",json!("other")),("/quality/typed_cell_differences",json!(1)),("/quality/columns",json!(39))] {
            let mut changed = original.clone(); *changed.pointer_mut(path).unwrap() = value;
            assert_ne!(semantic_basis(original.clone()), semantic_basis(changed));
        }
    }

    #[test]
    fn doris_runtime_capture_methods_are_observations_but_source_identity_is_immutable() {
        let baseline = json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","consistency_grade":"per_table","capture_methods":{"s.t":{"capture_method":"c5_diff"}},"quality":{"warnings":{}},"table_generations":{"s.t":{"source_generation":1,"target_generation":2}},"evidence_set_id":"a".repeat(64),"evidence":{"s.t":{}}});
        let current = json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","consistency_grade":"per_table","capture_methods":{"s.t":{"capture_method":"c4_partition"}},"quality":{"warnings":{"s.t":"DELETES_LAGGED"}},"table_generations":{"s.t":{"source_generation":3,"target_generation":4}},"evidence_set_id":"b".repeat(64),"evidence":{"s.t":{"request":{"sha256":"a"}}}});
        assert_eq!(semantic_basis(baseline.clone()), semantic_basis(current));
        let mut wrong_source = baseline;
        wrong_source["source_id"] = json!("other");
        assert_ne!(semantic_basis(wrong_source.clone()), semantic_basis(json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","consistency_grade":"per_table","capture_methods":{"s.t":{"capture_method":"c5_diff"}},"quality":{"warnings":{}},"table_generations":{"s.t":{"source_generation":1,"target_generation":2}},"evidence":{"s.t":{}}})));
    }

    #[test]
    fn current_source_semantics_pin_is_optional_but_must_be_a_sha256() {
        let ordinary = json!({"catalog_sha256":"a".repeat(64),"identity_revision":1,"idempotency_key":"b".repeat(32),"trigger":"source_generation_advanced"});
        assert!(super::maintenance_fields(&ordinary).is_ok());
        let pinned = json!({"catalog_sha256":"a".repeat(64),"identity_revision":1,"idempotency_key":"b".repeat(32),"source_semantics_sha256":"c".repeat(64),"trigger":"source_generation_advanced"});
        assert!(super::maintenance_fields(&pinned).is_ok());
        let malformed = json!({"catalog_sha256":"a".repeat(64),"identity_revision":1,"idempotency_key":"b".repeat(32),"source_semantics_sha256":"../../path","trigger":"source_generation_advanced"});
        assert!(super::maintenance_fields(&malformed).is_err());
        let wrong_trigger = json!({"identity_revision":1,"idempotency_key":"b".repeat(32),"source_semantics_sha256":"c".repeat(64),"trigger":"manual"});
        assert!(super::maintenance_fields(&wrong_trigger).is_err());
    }

    #[test]
    fn doris_observation_requires_private_hashed_receipts_bound_to_catalog_generation() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("oo-doris-observation-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(root.join("source-evidence").join("c".repeat(64))).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(root.join("source-evidence"), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(root.join("source-evidence").join("c".repeat(64)), std::fs::Permissions::from_mode(0o700)).unwrap();
        let catalog = json!({"protocol":"oo.pipeline.sealed-import-collection/v1","entries":[
            {"engine":"doris","source_id":"dorispreview","source_generation":1,"target_generation":2,"tables":[{"logical_schema":"s","logical_table":"t"}]}
        ]});
        let request = json!({"protocol":"oo.pipeline.native-update/v1","source_id":"dorispreview","logical_schema":"s","logical_table":"t","source_generation":1,"capture_method":"c5_diff","delete_detection":"reconcile","full_snapshot":true});
        let load = json!({"protocol":"oo.pipeline.native-update-result/v1","source_id":"dorispreview","logical_schema":"s","logical_table":"t","source_generation":1,"target_generation":2,"load_state":"awaiting_publish"});
        let mut evidence = serde_json::Map::new();
        for (kind, body) in [("request", request), ("load", load)] {
            let path = root.join("source-evidence").join("c".repeat(64)).join(format!("s.t.{kind}.json"));
            let bytes = crate::canonical::canonical_json(&body).into_bytes();
            std::fs::write(&path, &bytes).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let metadata = std::fs::metadata(&path).unwrap();
            let mut item = serde_json::Map::new();
            item.insert("path".into(), json!(format!("source-evidence/{}/s.t.{kind}.json", "c".repeat(64))));
            item.insert("sha256".into(), json!(crate::canonical::sha256_hex(&bytes)));
            item.insert("mode".into(), json!(0o600));
            item.insert("size".into(), json!(metadata.len()));
            item.insert("uid".into(), json!(metadata.uid()));
            evidence.entry(kind.to_owned()).or_insert_with(|| json!({}));
            evidence.get_mut(kind).unwrap().as_object_mut().unwrap().insert(kind.into(), Value::Object(item));
        }
        let mut semantics = json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","evidence_set_id":"c".repeat(64),"capture_methods":{
            "s.t":{"logical_table":"s.t","capture_method":"c5_diff","consistency":"S0","delete_detection":"reconcile","source_generation":1,"target_generation":2}
        },"table_generations":{"s.t":{"source_generation":1,"target_generation":2}},"evidence":{
            "s.t":{"request":evidence["request"]["request"].clone(),"load":evidence["load"]["load"].clone()}
        }});
        verify_doris_observation(&root, &catalog, &semantics).unwrap();
        semantics["evidence"]["s.t"]["request"]["sha256"] = json!("0".repeat(64));
        assert!(verify_doris_observation(&root, &catalog, &semantics).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn doris_c4_binds_sealed_scope_to_the_complete_acknowledged_partition_vector() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("oo-doris-c4-vector-{}-{nonce}", std::process::id()));
        let evidence_id = "c".repeat(64);
        let evidence_dir = root.join("source-evidence").join(&evidence_id);
        std::fs::create_dir_all(&evidence_dir).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(root.join("source-evidence"), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&evidence_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

        let source = "dorispreview";
        let source_generation = 9;
        let target_generation = 9;
        let run_id = "fdb0cb0a-370c-4056-a185-8adb777ceabe";
        let schema_digest = "a".repeat(64);
        let type_digest = "b".repeat(64);
        let table_epoch = "d".repeat(64);
        let schema_fingerprint = "e".repeat(64);
        let input_digest = "f".repeat(64);
        let observed_ids = json!(["p202603", "p202604"]);
        let current_scope = json!({
            "source_id":source,"source_engine":"doris","logical_schema":"s","logical_table":"part_day",
            "source_generation":source_generation,"partition_id":"p202603","observed_partition_ids":observed_ids,
            "complete":true,"mutations_pending":false,"schema_digest":schema_digest,"type_digest":type_digest,
            "arrow_sha256":input_digest,"table_epoch":table_epoch,"schema_fingerprint":schema_fingerprint,
            "row_count":1,"current_signature":"sig-p202603"
        });
        let prior_scope = json!({
            "source_id":source,"source_engine":"doris","logical_schema":"s","logical_table":"part_day",
            "source_generation":8,"partition_id":"p202604","observed_partition_ids":observed_ids,
            "complete":true,"mutations_pending":false,"schema_digest":schema_digest,"type_digest":type_digest,
            "arrow_sha256":"9".repeat(64),"table_epoch":table_epoch,"schema_fingerprint":schema_fingerprint,
            "row_count":7,"current_signature":"sig-p202604"
        });
        let catalog = json!({"protocol":"oo.pipeline.sealed-import-collection/v1","entries":[{
            "engine":"doris","source_id":source,"run_id":run_id,
            "source_generation":source_generation,"target_generation":target_generation,
            "tables":[{"logical_schema":"s","logical_table":"part_day","accepted_rows":8}],
            "partition_vector":[
                {"partition_id":"p202603","row_count":1,"source_generation":source_generation,
                 "target_generation":target_generation,"run_id":run_id,"partition_scope":current_scope},
                {"partition_id":"p202604","row_count":7,"source_generation":8,"target_generation":8,
                 "run_id":"prior-run","partition_scope":prior_scope}
            ]
        }]});
        let request = json!({"protocol":"oo.pipeline.native-partition/v1","source_engine":"doris",
            "source_id":source,"logical_schema":"s","logical_table":"part_day","source_generation":source_generation,
            "capture_method":"c4_partition","delete_detection":"partition_replace","full_snapshot":false,
            "schema_digest":schema_digest,"type_digest":type_digest,"input_digest":input_digest});
        let load = json!({"protocol":"oo.pipeline.native-update-result/v1","source_id":source,
            "source_engine":"doris","logical_schema":"s","logical_table":"part_day","run_id":run_id,
            "source_generation":source_generation,"target_generation":target_generation,
            "load_state":"awaiting_publish","row_count":8,
            "partition_target_receipt":{"acknowledged":false,"partition_scope":current_scope}});
        let mut evidence = serde_json::Map::new();
        for (kind, body) in [("request", request), ("load", load)] {
            let path = evidence_dir.join(format!("s.part_day.{kind}.json"));
            let bytes = crate::canonical::canonical_json(&body).into_bytes();
            std::fs::write(&path, &bytes).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let metadata = std::fs::metadata(&path).unwrap();
            evidence.insert(kind.to_owned(), json!({"path":format!("source-evidence/{evidence_id}/s.part_day.{kind}.json"),
                "sha256":crate::canonical::sha256_hex(&bytes),"mode":0o600,"size":metadata.len(),"uid":metadata.uid()}));
        }
        let semantics = json!({"protocol":"oo.source.doris.audit/v1","source_id":source,
            "evidence_set_id":evidence_id,"capture_methods":{"s.part_day":{
                "logical_table":"s.part_day","capture_method":"c4_partition","consistency":"S0",
                "delete_detection":"partition_replace","source_generation":source_generation,"target_generation":target_generation}},
            "evidence":{"s.part_day":{"request":evidence["request"].clone(),"load":evidence["load"].clone()}}});
        verify_doris_observation(&root, &catalog, &semantics).unwrap();

        let mut missing_partition = catalog.clone();
        missing_partition["entries"][0]["partition_vector"].as_array_mut().unwrap().pop();
        assert!(verify_doris_observation(&root, &missing_partition, &semantics).is_err());
        let mut wrong_scope = catalog.clone();
        wrong_scope["entries"][0]["partition_vector"][0]["partition_scope"]["arrow_sha256"] = json!("0".repeat(64));
        assert!(verify_doris_observation(&root, &wrong_scope, &semantics).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn source_rebind_rejects_rewind_identity_and_same_generation_drift() {
        let old = json!([{"dataset":"s.t","source_id":"source","source_generation":3,"target_generation":5,"run_id":"r","input_digest":"i","stored_physical":"p"}]);
        let mut new = old.clone();
        new[0]["source_generation"] = json!(4); new[0]["target_generation"] = json!(6);
        assert!(super::check_generation_progress(&old, &new).is_ok());
        new[0]["source_id"] = json!("foreign");
        assert!(super::check_generation_progress(&old, &new).is_err());
        new = old.clone(); new[0]["source_generation"] = json!(2);
        assert!(super::check_generation_progress(&old, &new).is_err());
        new = old.clone(); new[0]["input_digest"] = json!("replacement");
        assert!(super::check_generation_progress(&old, &new).is_err());
        let semantic = json!({"source_id":"source","source_generation":3,"consistency_grade":"B","quality":{"t":["DELETES_UNDETECTED"]}});
        let mut changed = semantic.clone(); changed["source_generation"] = json!(4);
        assert_eq!(super::semantic_basis(semantic.clone()), super::semantic_basis(changed.clone()));
        changed["consistency_grade"] = json!("S0");
        assert_ne!(super::semantic_basis(semantic), super::semantic_basis(changed));
    }

    #[test]
    fn role_pack_rejects_unsupported_model_currency() {
        let catalog = json!({"tables":[{"schema":"oo_w141","table":"shipments","columns":[{"name":"price","data_type":"numeric"}]}]});
        let rules = json!({"columns":[{"column":"oo_w141.shipments.price","semantic_type":"money","pii":"none","unit":"unspecified","confidence":"high"}],"relationships":[]});
        let (mut model, _) = generated_from_inputs(&catalog, &rules, "none").unwrap();
        let units = BTreeMap::from([("oo_w141.shipments.price".to_owned(), "unspecified".to_owned())]);
        assert!(ensure_source_money(&model, &units).is_ok());
        model.datasets.get_mut("oo_w141.shipments").unwrap().columns.get_mut("price").unwrap().unit = "USD".to_owned();
        assert!(ensure_source_money(&model, &units).is_err());
    }

    #[test]
    fn native_maintenance_keeps_hidden_source_money_unknown_and_pending() {
        let catalog = json!({"tables":[{"schema":"oo_w141","table":"shipments","columns":[{"name":"price","data_type":"numeric"}]}]});
        let rules = json!({"columns":[{"column":"oo_w141.shipments.price","semantic_type":"money","pii":"unknown","unit":"unspecified","confidence":"high"}],"relationships":[]});
        let (mut model, _) = generated_from_inputs(&catalog, &rules, "none").unwrap();
        let units = BTreeMap::from([("oo_w141.shipments.price".to_owned(), "unspecified".to_owned())]);
        let original = json!({"dataset":"oo_w141.shipments","columns":[{"name":"price","semantic_type":"money","unit":"USD","pii":"none","provenance":"generated","classification":{"state":"confirmed","by":"test","at":"2026-10-13"}}]});
        let mut effective = json!({"datasets":[original]});
        let receipt = project_native_source_money(&mut effective, &model, &units).unwrap();
        let price = &effective["datasets"][0]["columns"][0];
        assert_eq!(price["unit"], "unspecified");
        assert!(price["pii"].is_null());
        assert_eq!(price["classification"]["state"], "pending");
        assert!(price["classification"]["by"].is_null());
        assert_eq!(price["provenance"], "declared");
        assert_eq!(receipt[0]["reviewed_candidate"]["unit"], "USD");
        model.datasets.get_mut("oo_w141.shipments").unwrap().columns.get_mut("price").unwrap().classification_state = "confirmed".to_owned();
        assert!(project_native_source_money(&mut effective, &model, &units).is_err());
        assert!(project_native_source_money(&mut json!({"datasets":[]}), &model, &units).is_err());
    }

    fn hex_tag(seed: &str) -> String {
        let mut text = seed.to_owned();
        while text.len() < 60 { text.push('a'); }
        text.truncate(60);
        text
    }

    fn one_catalog(engine: &str, source_id: &str, schema: &str, table: &str, columns: &[(&str, i64)], source_generation: i64, target_generation: i64, run_id: &str, input_digest: &str, tag: &str) -> Value {
        let shadow = hex_tag(tag);
        let mut preimage = String::new();
        oo_core::sealed_import::push_schema_table(&mut preimage, schema, table);
        for (name, oid) in columns { oo_core::sealed_import::push_schema_column(&mut preimage, name, *oid, -1, None); }
        let schema_digest = oo_core::api::payload_sha256(preimage.as_bytes());
        let digest = "a".repeat(64);
        let typed: Vec<Value> = columns.iter().enumerate().map(|(index, (name, oid))| json!({
            "caveat": null, "key": false, "logical_name": name, "oid": oid, "source_type": null,
            "stored_physical": format!("pc_{}", hex_tag(&format!("{tag}{index:02}"))), "typmod": -1
        })).collect();
        json!({
            "capture": "sealed_import", "config_digest": digest, "engine": engine, "hook_checked": false,
            "input_digest": input_digest, "protocol": oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL,
            "quality": {"loss_concealed": false, "manifest_reject_count": 0, "payload_includes_reject_lines": false, "stored_omits_manifest_rejects": false, "stored_rejected_rows": 0, "stored_rows": [{
                "accepted_rows": 1, "attnum": null, "batch_id": "t0.b0", "key_digest": null, "kind": "batch",
                "physical_name": format!("pt_{shadow}"), "reason": null, "rejected_rows": 0, "row_index": null, "value_len": null
            }]},
            "run_id": run_id, "schema_digest": schema_digest, "shadows_queried": false,
            "source_generation": source_generation, "source_id": source_id, "source_introspection": "not_used", "state": "awaiting_publish",
            "tables": [{"accepted_rows": 1, "canonical_digest": digest, "columns": typed, "logical_schema": schema, "logical_table": table, "rejects": [], "stored_namespace": format!("pn_{shadow}"), "stored_physical": format!("pt_{shadow}")}],
            "target_generation": target_generation, "tool_hook_digest": digest, "type_digest": digest
        })
    }

    fn collection_of(entries: Vec<Value>) -> Value {
        let binding_vector: Vec<Value> = entries.iter().map(|entry| {
            let table = &entry["tables"][0];
            json!({
                "config_digest": entry["config_digest"],
                "dataset": format!("{}.{}", table["logical_schema"].as_str().unwrap(), table["logical_table"].as_str().unwrap()),
                "input_digest": entry["input_digest"], "run_id": entry["run_id"], "schema_digest": entry["schema_digest"],
                "source_generation": entry["source_generation"], "source_id": entry["source_id"],
                "stored_namespace": table["stored_namespace"], "stored_physical": table["stored_physical"],
                "target_generation": entry["target_generation"], "type_digest": entry["type_digest"]
            })
        }).collect();
        let mut document = json!({"binding_vector": binding_vector, "entries": entries, "protocol": oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL});
        document["collection_sha256"] = json!(crate::canonical::sha256_hex(crate::canonical::canonical_json(&document).as_bytes()));
        document
    }

    #[test]
    fn oracle_catalog_advances_without_a_fabricated_vector() {
        let old = one_catalog("oracle", "oo_oracle", "shop", "orders", &[("id", 23)], 3, 3, "de8276fc-8634-4543-994f-ce82a46a3c18", &"a".repeat(64), "aa");
        let new = one_catalog("oracle", "oo_oracle", "shop", "orders", &[("id", 23)], 5, 5, "beecbbc7-8dda-4779-81ba-6ff0383f72ec", &"b".repeat(64), "aa");
        let progress = super::rebind_sealed_catalogs(&old, &new).expect("oracle advance");
        assert!(new.get("binding_vector").is_none());
        assert_eq!(progress[0]["dataset"], "shop.orders");
        assert_eq!(progress[0]["source_generation"], 5);
        assert_eq!(progress[0]["target_generation"], 5);
        assert_eq!(progress[0]["run_id"], "beecbbc7-8dda-4779-81ba-6ff0383f72ec");
        assert_eq!(progress[0]["input_digest"], "b".repeat(64));
        assert_eq!(progress[0]["stored_physical"], new["tables"][0]["stored_physical"]);
        let mut rewound = new.clone();
        rewound["source_generation"] = json!(2);
        rewound["target_generation"] = json!(2);
        assert!(super::rebind_sealed_catalogs(&old, &rewound).is_err());
        let mut drifted = old.clone();
        let replacement = format!("pt_{}", hex_tag("bb"));
        drifted["tables"][0]["stored_physical"] = json!(replacement.clone());
        drifted["quality"]["stored_rows"][0]["physical_name"] = json!(replacement);
        assert!(super::rebind_sealed_catalogs(&old, &drifted).is_err());
        let mut illegal = new.clone();
        illegal["schema_digest"] = json!("ab".repeat(32));
        assert!(super::rebind_sealed_catalogs(&old, &illegal).is_err());
        let mut foreign = new.clone();
        foreign["source_id"] = json!("other_source");
        assert!(super::rebind_sealed_catalogs(&old, &foreign).is_err());
    }

    #[test]
    fn mongo_collection_keeps_each_table_generation() {
        let old = collection_of(vec![
            one_catalog("mongodb", "oo_mongo", "shop", "orders", &[("id", 23)], 3, 3, "de8276fc-8634-4543-994f-ce82a46a3c18", &"a".repeat(64), "c1"),
            one_catalog("mongodb", "oo_mongo", "shop", "lines", &[("id", 23)], 6, 6, "beecbbc7-8dda-4779-81ba-6ff0383f72ec", &"b".repeat(64), "c2"),
        ]);
        let new = collection_of(vec![
            one_catalog("mongodb", "oo_mongo", "shop", "orders", &[("id", 23)], 5, 5, "f9992961-632f-4c51-a85d-710733e2c398", &"c".repeat(64), "c1"),
            one_catalog("mongodb", "oo_mongo", "shop", "lines", &[("id", 23)], 7, 7, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", &"d".repeat(64), "c2"),
        ]);
        let progress = super::rebind_sealed_catalogs(&old, &new).expect("mongo advance");
        let orders = progress.as_array().unwrap().iter().find(|row| row["dataset"] == "shop.orders").unwrap();
        let lines = progress.as_array().unwrap().iter().find(|row| row["dataset"] == "shop.lines").unwrap();
        assert_eq!(orders["source_generation"], 5);
        assert_eq!(lines["source_generation"], 7);
        let compiled = oo_builder::compile_sealed_import_collection(&crate::canonical::canonical_json(&new)).unwrap();
        assert_eq!(compiled.source_generation, 7);
        assert_eq!(compiled.binding_vector.as_ref(), Some(&progress));
        assert_ne!(orders["source_generation"].as_i64(), Some(compiled.source_generation));
        let mut rewind = new.clone();
        rewind["entries"][0]["source_generation"] = json!(4);
        rewind["entries"][0]["target_generation"] = json!(4);
        rewind["binding_vector"][0]["source_generation"] = json!(4);
        rewind["binding_vector"][0]["target_generation"] = json!(4);
        rewind.as_object_mut().unwrap().remove("collection_sha256");
        rewind["collection_sha256"] = json!(crate::canonical::sha256_hex(crate::canonical::canonical_json(&rewind).as_bytes()));
        assert!(super::rebind_sealed_catalogs(&new, &rewind).is_err());
        assert!(super::rebind_sealed_catalogs(&old, &new["entries"][0]).is_err());
    }

    #[test]
    fn oracle_bundle_rebind_refuses_permission_rewind_without_writes() {
        let root = std::env::temp_dir().join(format!("oo-rebind-{}-{}", std::process::id(), "oracle"));
        let _ = std::fs::remove_dir_all(&root);
        let bundle = root.join("bundle");
        let work = root.join("work");
        let old = one_catalog("oracle", "oo_oracle", "shop", "orders", &[("id", 23), ("note", 25)], 3, 3, "de8276fc-8634-4543-994f-ce82a46a3c18", &"a".repeat(64), "d0");
        let new = one_catalog("oracle", "oo_oracle", "shop", "orders", &[("id", 23), ("note", 25)], 5, 5, "beecbbc7-8dda-4779-81ba-6ff0383f72ec", &"b".repeat(64), "d0");
        write_oracle_bundle(&bundle, &new, &["shop.orders.id", "shop.orders.note"]);
        let rules = std::fs::read(bundle.join("rules.json")).unwrap();
        let rules_sha = crate::canonical::sha256_hex(&rules);
        let semantics = std::fs::read(bundle.join("source_semantics.json")).unwrap();
        let model = std::fs::read(bundle.join("native-model.json")).unwrap();
        write_oracle_build(&work, &old, &rules_sha, &semantics, &model);
        let config = oracle_config(&work, &bundle);
        let row = oracle_row();
        let sha = crate::canonical::sha256_hex(&std::fs::read(bundle.join("catalog.json")).unwrap());
        let rebound = super::native_source_rebind(&config, &row, &sha, None).expect("bundle advance");
        assert!(rebound["catalog"].get("binding_vector").is_none());
        assert_eq!(rebound["progress_vector"][0]["source_generation"], 5);
        assert_eq!(rebound["progress_vector"][0]["target_generation"], 5);
        assert_eq!(std::fs::read(bundle.join("catalog.json")).unwrap(), serde_json::to_vec(&new).unwrap());
        let mut roles: Value = serde_json::from_slice(&std::fs::read(bundle.join("roles.json")).unwrap()).unwrap();
        roles["roles"]["analyst"] = json!(["shop.orders.id"]);
        std::fs::write(bundle.join("roles.json"), serde_json::to_vec(&roles).unwrap()).unwrap();
        crate::sealed::write_manifest(&bundle).unwrap();
        let narrowed = dir_bytes(&root);
        assert!(super::native_source_rebind(&config, &row, &sha, None).is_err());
        assert_eq!(dir_bytes(&root), narrowed);
        write_oracle_bundle(&bundle, &old, &["shop.orders.id", "shop.orders.note"]);
        write_oracle_build(&work, &new, &rules_sha, &semantics, &model);
        let rewind_sha = crate::canonical::sha256_hex(&std::fs::read(bundle.join("catalog.json")).unwrap());
        let parked = dir_bytes(&root);
        assert!(super::native_source_rebind(&config, &row, &rewind_sha, None).is_err());
        assert_eq!(dir_bytes(&root), parked);
        let _ = std::fs::remove_dir_all(&root);
    }

    pub(super) fn write_oracle_bundle(dir: &std::path::Path, catalog: &Value, roles: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("catalog.json"), serde_json::to_vec(catalog).unwrap()).unwrap();
        std::fs::write(dir.join("profile.json"), br#"{"protocol":"oo.source.oracle.profile/v1"}"#).unwrap();
        std::fs::write(dir.join("source_semantics.json"), br#"{"consistency_grade":"T","protocol":"oo.source.oracle.semantics/v1","quality":{"rows":1,"typed_cell_differences":0,"typed_cells":1},"source_id":"oo_oracle"}"#).unwrap();
        std::fs::write(dir.join("read_logic.json"), br#"{"executed":false}"#).unwrap();
        std::fs::write(dir.join("rules.json"), br#"{"columns":[],"relationships":[]}"#).unwrap();
        std::fs::write(dir.join("suggestions.json"), br#"{"certified":false,"merged":false,"ready":false}"#).unwrap();
        std::fs::write(dir.join("generation.json"), br#"{"llm_calls":0,"outcome":"structural_draft"}"#).unwrap();
        std::fs::write(dir.join("roles.json"), serde_json::to_vec(&json!({"contract":"oo.selfbuild-control-api/sealed-roles/v1","roles":{"analyst":roles}})).unwrap()).unwrap();
        std::fs::write(dir.join("native-model.json"), br#"{"dataset":"shop.orders","settings":{"project":"oracle"},"visibility":{"datasets":[{"columns":["id","note"],"dataset":"shop.orders"}],"project":"oracle","role":"analyst"}}"#).unwrap();
        crate::sealed::write_manifest(dir).unwrap();
    }

    pub(super) fn write_oracle_build(work: &std::path::Path, catalog: &Value, rules_sha: &str, semantics: &[u8], model: &[u8]) {
        let root = work.join("builds").join("b_739b3a14d1167f50b5f48ae22a8317e2");
        std::fs::create_dir_all(root.join("native-model-output")).unwrap();
        std::fs::write(root.join("native-model-output").join("command.json"), serde_json::to_vec(&json!({"catalog": catalog, "decisions": [], "settings": {"catalog_version": "pending"}})).unwrap()).unwrap();
        std::fs::write(root.join("native-model.json"), model).unwrap();
        std::fs::write(root.join("S2.request.json"), serde_json::to_vec(&json!({"rules_sha256": rules_sha})).unwrap()).unwrap();
        let semantics: Value = serde_json::from_slice(semantics).unwrap();
        std::fs::write(root.join("S0.response.json"), serde_json::to_vec(&json!({"source_adapt": {"source_semantics": semantics}})).unwrap()).unwrap();
    }

    pub(super) fn oracle_row() -> crate::store::BuildRow {
        crate::store::BuildRow {
            build_id: "b_739b3a14d1167f50b5f48ae22a8317e2".into(), project_id: "oracle".into(), status: "open".into(), stage: "S10".into(),
            identity_revision: 1, input_sha256: "a".repeat(64), failure_code: None, body_sha256: "b".repeat(64), config_commit: "c".into(),
            source_ref: "s".into(), allowance_id: "a".into(), subject: "owner".into(), role_name: "owner".into(), child_exited: true, child_pid: None, lease_generation: 1,
        }
    }

    pub(super) fn oracle_config(work: &std::path::Path, bundle: &std::path::Path) -> crate::config::AppConfig {
        let endpoint = crate::config::Endpoint { host: "127.0.0.1".into(), port: 1, database: "d".into(), user: "u".into(), password_file: "pw".into(), ca_file: "ca".into(), server_name: "db".into() };
        crate::config::AppConfig {
            deployment_origins: vec![],
            listen: crate::config::Listen { bind: "127.0.0.1:1".into(), cert_file: "c".into(), key_file: "k".into() },
            identity: crate::config::IdentityCfg { origin: "https://127.0.0.1".into(), ca_file: "ca".into() },
            postgres: endpoint.clone(), work_dir: work.display().to_string(), model_cache_dir: "cache".into(),
            binaries: crate::config::Binaries { catalog: "c".into(), pg_builder: "p".into(), rules: "r".into(), suggestions: "s".into(), review_ledger: "l".into() },
            projects: vec![crate::config::ProjectCfg {
                project_id: "oracle".into(), config_commit: "c".into(), source_ref: "s".into(), allowance_id: "a".into(), llm_secret_ref: "secret://llm".into(), source_secret_ref: "secret://source".into(),
                source: crate::config::SourceCfg { host: "127.0.0.1".into(), port: 1, database: "d".into(), username: "u".into(), password_file: "pw".into(), ca_file: "ca".into(), tables: vec![("shop".into(), "orders".into())] },
                sample_policy: "none".into(), rules_auto_accept: "none".into(),
                llm: crate::config::LlmCfg { bearer_file: "b".into(), origin: "http://127.0.0.1:8317".into(), model_id: "grok-4.7".into() },
                role_policy_file: "r".into(), hidden_columns: vec![], visible_columns: vec![], sealed_bundle: bundle.display().to_string(),
                source_protocol: Some(crate::config::SourceProtocol { catalog: "oo.pipeline.sealed-import-catalog/v1".into(), profile: "oo.source.oracle.profile/v1".into(), semantics: "oo.source.oracle.semantics/v1".into(), source_commit: "0".repeat(40), semantics_commit: "0".repeat(40), type_basis: vec![], schema_audit: vec![] }),
            }],
            ledger: endpoint, principals: vec![],
        }
    }

    pub(super) fn oracle_bytes(name: &str) -> &'static [u8] {
        match name {
            "g3" => include_bytes!("../tests/oracle-g3-g5/catalog-g3.json"),
            "g5" => include_bytes!("../tests/oracle-g3-g5/catalog-g5.json"),
            "command" => include_bytes!("../tests/oracle-g3-g5/command.json"),
            "visibility" => include_bytes!("../tests/oracle-g3-g5/visibility.json"),
            "roles" => include_bytes!("../tests/oracle-g3-g5/roles.json"),
            _ => panic!("fixture"),
        }
    }

    pub(super) fn oracle_value(name: &str) -> Value {
        serde_json::from_slice(oracle_bytes(name)).unwrap()
    }

    #[test]
    fn oracle_g5_keeps_schema_digest_and_advances_identity() {
        const SCHEMA: &str = "f002b65dac929bfd96866a1936003e92ed366a186e4dc624fb978ef4af2cc425";
        const OLD_IDENTITY: &str = "415359c3aed4f8e887c8e130ea5dbe60cd53bbbf02b9e66d62d18fcc3f7cc0a2";
        const NEW_IDENTITY: &str = "010ffb76bd60822f69ea5bcdeed7ad215c7c4707efc0ddc55c99191223010dca";
        assert_eq!(crate::canonical::sha256_hex(oracle_bytes("g3")), "5c94877dc45b51d3606c47b4ca4965df6363b6d07809ad1eb453b4bbe9ebaf4a");
        assert_eq!(crate::canonical::sha256_hex(oracle_bytes("g5")), "c0134177172947521de65e2b9c31cf80adfb16790a5ebc15d261dc1127a10588");
        let g3 = oracle_value("g3");
        let g5 = oracle_value("g5");
        let command = oracle_value("command");
        assert_eq!(command["catalog"], g3);
        assert_eq!(command["settings"]["catalog_version"], OLD_IDENTITY);
        assert!(command["decisions"].as_array().unwrap().iter().all(|decision| decision["schema_hash"] == SCHEMA));
        let progress = super::rebind_sealed_catalogs(&g3, &g5).expect("g3 to g5");
        assert!(g5.get("binding_vector").is_none());
        assert_eq!(progress[0]["source_generation"], 5);
        assert_eq!(progress[0]["target_generation"], 5);
        let mut schema_changed = g5.clone();
        schema_changed["tables"][0]["columns"][0]["oid"] = json!(9999);
        assert!(super::rebind_sealed_catalogs(&g3, &schema_changed).is_err());
        let mut digest_changed = g5.clone();
        digest_changed["schema_digest"] = json!("ab".repeat(32));
        assert!(super::rebind_sealed_catalogs(&g3, &digest_changed).is_err());
        assert!(super::rebind_sealed_catalogs(&g5, &g3).is_err());

        let compiled = oo_builder::compile_sealed_import_catalog(&crate::canonical::canonical_json(&g5)).unwrap();
        assert_eq!(compiled.schema_digest, SCHEMA);
        assert_eq!(compiled.identity_hash, NEW_IDENTITY);
        let mut rebound_command = command.clone();
        rebound_command["catalog"] = g5.clone();
        let decisions_before = crate::canonical::canonical_sha256(&rebound_command["decisions"]);
        super::bind_rebound_catalog(&mut rebound_command, &compiled).unwrap();
        assert_eq!(crate::canonical::canonical_sha256(&rebound_command["decisions"]), decisions_before);
        assert!(rebound_command["decisions"].as_array().unwrap().iter().all(|decision| decision["schema_hash"] == SCHEMA));
        assert_eq!(rebound_command["settings"]["catalog_version"], NEW_IDENTITY);
        let reviewed: Value = serde_json::from_str(&oo_builder::compile_reviewed_sealed_import(&crate::canonical::canonical_json(&rebound_command)).unwrap()).unwrap();
        assert!(reviewed["hidden"].as_array().unwrap().iter().all(|item| item["reason"] != "decision.stale_schema"));
        assert!(reviewed["hidden"].as_array().unwrap().is_empty());
        let mut wrong_pin = rebound_command.clone();
        wrong_pin["decisions"][0]["schema_hash"] = json!(OLD_IDENTITY);
        wrong_pin["settings"]["catalog_version"] = json!(OLD_IDENTITY);
        assert!(super::bind_rebound_catalog(&mut wrong_pin, &compiled).is_err());
        let mut stale_fact = rebound_command.clone();
        stale_fact["decisions"][0]["fact_fingerprint"] = json!("ab".repeat(32));
        let hidden: Value = serde_json::from_str(&oo_builder::compile_reviewed_sealed_import(&crate::canonical::canonical_json(&stale_fact)).unwrap()).unwrap();
        assert_eq!(hidden["hidden"][0]["reason"], "decision.stale_fact");
        assert!(hidden["hidden"].as_array().unwrap().iter().all(|item| item["reason"] != "decision.stale_schema"));

        let old = collection_of(vec![one_catalog("mongodb", "oo_mongo", "shop", "orders", &[("id", 23)], 3, 3, "de8276fc-8634-4543-994f-ce82a46a3c18", &"a".repeat(64), "e1")]);
        let new = collection_of(vec![one_catalog("mongodb", "oo_mongo", "shop", "orders", &[("id", 23)], 5, 5, "beecbbc7-8dda-4779-81ba-6ff0383f72ec", &"b".repeat(64), "e1")]);
        let old_compiled = oo_builder::compile_sealed_import_collection(&crate::canonical::canonical_json(&old)).unwrap();
        let new_compiled = oo_builder::compile_sealed_import_collection(&crate::canonical::canonical_json(&new)).unwrap();
        assert_ne!(old_compiled.identity_hash, new_compiled.identity_hash);
        let mut collection_command = json!({
            "protocol": command["protocol"], "catalog": new, "settings": command["settings"],
            "decisions": new_compiled.columns.iter().map(|column| json!({"column": column.name, "dataset": column.dataset, "fact_fingerprint": column.fact_fingerprint, "pii": "none", "reviewer": command["decisions"][0]["reviewer"], "schema_hash": old_compiled.identity_hash, "semantic_type": "quantity"})).collect::<Vec<_>>()
        });
        collection_command["settings"]["catalog_version"] = json!(old_compiled.identity_hash);
        collection_command["settings"]["ontology"]["include_schemas"] = json!(["shop"]);
        super::bind_rebound_catalog(&mut collection_command, &new_compiled).unwrap();
        assert!(collection_command["decisions"].as_array().unwrap().iter().all(|decision| decision["schema_hash"] == new_compiled.identity_hash));
        let collection_reviewed: Value = serde_json::from_str(&oo_builder::compile_reviewed_sealed_import(&crate::canonical::canonical_json(&collection_command)).unwrap()).unwrap();
        assert!(collection_reviewed["hidden"].as_array().unwrap().iter().all(|item| item["reason"] != "decision.stale_schema"));

        let root = std::env::temp_dir().join(format!("oo-oracle-g5-{}-{}", std::process::id(), "identity"));
        let _ = std::fs::remove_dir_all(&root);
        let bundle = root.join("bundle");
        let work = root.join("work");
        write_oracle_bundle(&bundle, &g5, &[]);
        std::fs::write(bundle.join("catalog.json"), oracle_bytes("g5")).unwrap();
        std::fs::write(bundle.join("roles.json"), oracle_bytes("roles")).unwrap();
        let mut model: Value = serde_json::from_slice(&std::fs::read(bundle.join("native-model.json")).unwrap()).unwrap();
        model["visibility"]["role"] = json!("oo_r_oracle_orders");
        model["visibility"]["datasets"] = json!([{"columns":["q_4b45595f4944","q_414d4f554e54","q_4f43435552524544","q_4c4142454c","q_5041594c4f4144"],"dataset":"q_4f4f30345f464958.q_4f4e424f4152445f4f5244455253"}]);
        std::fs::write(bundle.join("native-model.json"), serde_json::to_vec(&model).unwrap()).unwrap();
        let mut semantics: Value = serde_json::from_slice(&std::fs::read(bundle.join("source_semantics.json")).unwrap()).unwrap();
        semantics["source_id"] = json!("oracle_oneclick_orders");
        std::fs::write(bundle.join("source_semantics.json"), serde_json::to_vec(&semantics).unwrap()).unwrap();
        crate::sealed::write_manifest(&bundle).unwrap();
        let rules = std::fs::read(bundle.join("rules.json")).unwrap();
        let semantics = std::fs::read(bundle.join("source_semantics.json")).unwrap();
        let model_bytes = std::fs::read(bundle.join("native-model.json")).unwrap();
        write_oracle_build(&work, &g3, &crate::canonical::sha256_hex(&rules), &semantics, &model_bytes);
        let build = work.join("builds").join("b_739b3a14d1167f50b5f48ae22a8317e2");
        let command_path = build.join("native-model-output").join("command.json");
        std::fs::write(&command_path, oracle_bytes("command")).unwrap();
        std::fs::write(build.join("native-model-output").join("visibility.json"), oracle_bytes("visibility")).unwrap();
        let config = oracle_config(&work, &bundle);
        let row = oracle_row();
        let sha = crate::canonical::sha256_hex(&std::fs::read(bundle.join("catalog.json")).unwrap());
        let rebound = super::native_source_rebind(&config, &row, &sha, None).expect("real bundle advance");
        let docs = super::Docs {
            generated_sha: "11".repeat(32), generated: super::empty_model(), curated_sha: String::new(), reviews: Vec::new(),
            effective: super::empty_model(), effective_yaml: String::new(), effective_sha: "22".repeat(32), feedback: Vec::new(),
            curated_revisions: 0, generated_revisions: 0,
        };
        let maintained = super::compile_native_maintenance(&config, &row, &docs, Some(&rebound)).expect("maintenance");
        assert_eq!(maintained["llm_calls"], 0);
        assert_eq!(maintained["source_rebind"]["original_decisions_sha256"], maintained["source_rebind"]["rebound_decisions_sha256"]);
        assert!(maintained["source_rebind"]["binding_vector"].is_null());
        let artifact = std::path::PathBuf::from(maintained["artifact_directory"].as_str().unwrap());
        let kept: Value = serde_json::from_slice(&std::fs::read(artifact.join("command.json")).unwrap()).unwrap();
        assert!(kept["decisions"].as_array().unwrap().iter().all(|decision| decision["schema_hash"] == SCHEMA));
        assert_eq!(kept["settings"]["catalog_version"], NEW_IDENTITY);
        let visibility: Value = serde_json::from_slice(&std::fs::read(artifact.join("visibility.json")).unwrap()).unwrap();
        assert_eq!(visibility["catalog_version"], NEW_IDENTITY);
        let reviewed_file: Value = serde_json::from_slice(&std::fs::read(artifact.join("reviewed.json")).unwrap()).unwrap();
        assert!(reviewed_file["hidden"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read(&command_path).unwrap(), oracle_bytes("command"));
        let mut roles: Value = serde_json::from_slice(oracle_bytes("roles")).unwrap();
        roles["roles"]["oo_r_oracle_orders"] = json!(["q_4f4f30345f464958.q_4f4e424f4152445f4f5244455253.q_4b45595f4944"]);
        std::fs::write(bundle.join("roles.json"), serde_json::to_vec(&roles).unwrap()).unwrap();
        crate::sealed::write_manifest(&bundle).unwrap();
        assert!(super::native_source_rebind(&config, &row, &sha, None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn dir_bytes(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()).collect();
            entries.sort();
            for path in entries {
                let name = path.file_name().unwrap().to_string_lossy();
                let rel = if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
                if path.is_dir() { walk(&path, &rel, out); } else { out.push((rel, std::fs::read(&path).unwrap())); }
            }
        }
        walk(dir, "", &mut out);
        out
    }
}

#[cfg(test)]
mod mysql_c1_observation_tests {
    use super::*;
    #[test]
    #[ignore = "owned MySQL import evidence required"]
    fn owned_mysql_c1_observation_refuses_drift() {
        let config:Value=serde_json::from_slice(&std::fs::read(std::env::var("OO_MYSQL_OBSERVATION_TEST_CONFIG").unwrap()).unwrap()).unwrap();
        let root=Path::new(config["root"].as_str().unwrap());
        let catalog:Value=serde_json::from_slice(&std::fs::read(config["catalog"].as_str().unwrap()).unwrap()).unwrap();
        let observation:Value=serde_json::from_slice(&std::fs::read(config["observation"].as_str().unwrap()).unwrap()).unwrap();
        verify_mysql_c1_observation(root,&catalog,&observation).unwrap();
        for (key,value) in [("source_id",json!("another")),("applied_pos",json!("37")),("source_epoch",json!(2)),("source_generation",json!(3)),("delete_detection",json!("none"))] {
            let mut wrong=observation.clone();wrong[key]=value;
            assert!(verify_mysql_c1_observation(root,&catalog,&wrong).is_err());
        }
        let mut wrong=catalog.clone();wrong["entries"][0]["run_id"]=json!("00000000-0000-0000-0000-000000000000");
        assert!(verify_mysql_c1_observation(root,&wrong,&observation).is_err());
        let mut wrong=observation.clone();wrong["c1_evidence"]["ack"]["sha256"]=json!("0".repeat(64));
        assert!(verify_mysql_c1_observation(root,&catalog,&wrong).is_err());
        println!("OWNED_MYSQL_C1_OBSERVATION positive and source/position/epoch/generation/delete/run/ACK-hash refusal passed");
    }
}

#[cfg(test)]
mod oracle_c5_observation_tests {
    use super::{compile_native_maintenance, decode_oracle_name, empty_model, native_source_rebind, oracle_c5_capture_methods, project_oracle_c5_semantics, semantic_basis, verify_oracle_c5_observation, verify_oracle_c5_provenance, Docs};
    use super::source_money_tests::{oracle_bytes, oracle_config, oracle_row, oracle_value, write_oracle_build, write_oracle_bundle};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    fn pretty(value: &Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(value).unwrap();
        bytes.push(b'\n');
        bytes
    }

    fn source_provenance_fixture(request: &mut Value, catalog: &Value, load: &Value) -> Value {
        let source_schema = "a".repeat(64);
        let binding = "e7325034-f8f4-4b72-91bd-4f3f9a48128a";
        let task = "cb491c0f-5290-4fbf-bde7-e685ac8997db";
        let pin = 24_u64;
        let source_generation = catalog["source_generation"].as_i64().unwrap();
        let source_run_id = "oracle-source-run-10";
        let source_root = format!("/private/tmp/oo-test/{binding}/{pin}");
        let registry_path = format!("{source_root}/sealed/registry.json");
        let admission_path = format!("{source_root}/sealed/admission-receipt.json");
        let source_catalog_path = format!("{source_root}/sealed/catalog.json");
        let source_request_path = format!("{source_root}/request.json");
        let capture_path = format!("{source_root}/capture/witness/oracle_oneclick_orders/tick.json");
        request["schema_digest"] = json!(source_schema);
        request["registry_path"] = json!(registry_path);
        request["admission_path"] = json!(admission_path);

        let capture = json!({
            "protocol":"oo.oracle.capture-tick/v1", "source_id":catalog["source_id"],
            "source_generation":source_generation, "snapshot":request["snapshot"],
            "label":request.pointer("/snapshot/value").unwrap(), "database":request["logical_schema"],
            "product_reader_write_attempts":0, "cross_table_consistency":"B", "snapshot_token":false, "tables":[{
                "name":request["logical_table"], "arrow":"onboard_orders-gen-10.arrow", "route":"C5",
                "consistency":"S0", "cursor_safety":"no_cursor", "delete_mode":"reconcile",
                "quality":"DELETES_LAGGED", "rows":[
                    "KEY_ID=1\u{001f}AMOUNT=1.00\u{001f}OCCURRED=2026-10-14T08:00:00\u{001f}LABEL=a\u{001f}PAYLOAD=00FF",
                    "KEY_ID=3\u{001f}AMOUNT=3.00\u{001f}OCCURRED=2026-10-14T10:00:00\u{001f}LABEL=c\u{001f}PAYLOAD=AABB",
                    "KEY_ID=4\u{001f}AMOUNT=4.00\u{001f}OCCURRED=2026-10-16T04:05:06\u{001f}LABEL=d\u{001f}PAYLOAD=ABCD"
                ], "tombstones":["2:3"]
            }]
        });
        let capture_raw = pretty(&capture);
        let capture_sha = crate::canonical::sha256_hex(&capture_raw);
        request["audit_digest"] = json!(capture_sha);

        let source_fields: Vec<Value> = request["columns"].as_array().unwrap().iter().map(|column| {
            let mut field = column.clone();
            field.as_object_mut().unwrap().remove("logical_name");
            field
        }).collect();
        let source_catalog = json!({
            "protocol":"oo.pipeline.sealed-import-catalog/v1", "engine":"oracle",
            "source_id":catalog["source_id"], "source_generation":source_generation,
            "schema_digest":source_schema, "type_digest":catalog["type_digest"],
            "config_digest":catalog["config_digest"], "input_digest":catalog["input_digest"]
        });
        let source_catalog_raw = pretty(&source_catalog);
        let source_catalog_sha = crate::canonical::sha256_hex(&source_catalog_raw);
        let config_path = format!("{source_root}/capture/witness/oracle_oneclick_orders/config.json");
        let report_path = format!("{source_root}/capture/witness/oracle_oneclick_orders/report.json");
        let driver_path = "/opt/oracle-driver-probe";
        let driver_sha = "b".repeat(64);
        let mut registry = json!({
            "protocol":"oo.source-admission.binding-registry/v1",
            "bindings":[{
                "protocol":"oo.source-admission.capture-binding/v1",
                "source_engine":"oracle", "source_id":catalog["source_id"], "source_generation":source_generation,
                "schema_digest":source_schema, "type_digest":catalog["type_digest"],
                "config_digest":catalog["config_digest"], "input_digest":catalog["input_digest"],
                "snapshot":request["snapshot"], "run_id":source_run_id,
                "admission_receipt_sha256":"0".repeat(64), "admission_receipt_path":admission_path,
                "capture_receipt_sha256":capture_sha, "capture_receipt_path":capture_path,
                "catalog_sha256":source_catalog_sha, "catalog_path":source_catalog_path,
                "arrow_schema_sha256":"e".repeat(64), "config_path":config_path,
                "config_sha256":catalog["config_digest"], "decision_sha256":"c".repeat(64),
                "driver_digest":driver_sha, "driver_kind":"command-line", "driver_name":"oracle-driver-probe",
                "driver_path":driver_path, "driver_sha256":driver_sha, "driver_version":"oracledb-26.0.0-beta.4",
                "report_path":report_path, "report_sha256":"d".repeat(64),
                "fields":source_fields, "objects":[{
                    "logical_schema":request["logical_schema"], "logical_table":request["logical_table"],
                    "arrow_sha256":request["input_digest"], "row_count":load["row_count"]
                }]
            }]
        });
        let registry_preimage = pretty(&registry);
        let registry_preimage_sha = crate::canonical::sha256_hex(&registry_preimage);
        let admission = json!({
            "schema":"oo.source-admission.capture-receipt/v1", "registry_sha256":registry_preimage_sha,
            "source_engine":"oracle",
            "source_id":catalog["source_id"], "source_generation":source_generation, "run_id":source_run_id,
            "schema_digest":source_schema, "type_digest":catalog["type_digest"],
            "config_digest":catalog["config_digest"], "input_digest":catalog["input_digest"],
            "catalog_sha256":source_catalog_sha, "arrow_schema_sha256":"e".repeat(64),
            "arrow_sha256":[catalog["input_digest"]], "logical_schema":request["logical_schema"],
            "logical_table":request["logical_table"], "row_count":load["row_count"],
            "snapshot":request["snapshot"], "snapshot_token":false, "source_bound":true,
            "capture_method":"C5", "delete_detection":"reconcile", "delete_observation":"observed",
            "consistency":"S0", "cross_table_consistency":"B", "quality":"DELETES_LAGGED",
            "driver_digest":driver_sha, "write_class":"R0", "write_attempts":0
        });
        let admission_raw = pretty(&admission);
        let admission_sha = crate::canonical::sha256_hex(&admission_raw);
        registry["bindings"][0]["admission_receipt_sha256"] = json!(admission_sha);
        let registry_raw = pretty(&registry);
        request["admission_expect"] = json!({
            "declared_receipt_sha256":admission_sha, "source_engine":"oracle",
            "source_generation":source_generation, "source_id":catalog["source_id"]
        });
        let mut acknowledged_request = request.clone();
        acknowledged_request.as_object_mut().unwrap().remove("run_id");
        acknowledged_request.as_object_mut().unwrap().remove("target_generation");
        acknowledged_request["source_engine"] = json!("oracle");
        acknowledged_request["mirror"] = json!(false);
        acknowledged_request["row_identity"] = json!("none");
        acknowledged_request["batch_id"] = json!("oracle_oneclick_orders-g10");
        acknowledged_request["arrow_path"] = json!(format!("{source_root}/capture/run/arrow/onboard_orders-gen-10.arrow"));
        acknowledged_request["target"] = json!({
            "host":"oracle-target", "port":5432, "owner":"00000000-0000-4000-8000-000000000001",
            "shard":"oracle_oneclick_orders", "ca_file":"/run/secrets/ca.crt", "database":"previewdb",
            "password_file":"/run/secrets/runtime-password", "runtime_principal":"oracle_runtime"
        });
        acknowledged_request["admission_expect"] = json!({
            "declared_receipt_sha256":admission_sha, "source_engine":"oracle",
            "source_generation":source_generation, "source_id":catalog["source_id"]
        });
        let request_raw = pretty(&acknowledged_request);

        let manifest = json!({
            "protocol":"oo.jobs.native-schedule/v1", "binding":binding,
            "source_id":catalog["source_id"], "method":"C5", "cycle":"pull",
            "registration":{
                "registry_path":registry_path,
                "admission_binary":"/opt/admission", "admission_sha256":"c".repeat(64)
            },
            "producer":{"path":driver_path, "sha256":driver_sha},
            "loader":{"path":"/opt/loader", "sha256":"d".repeat(64)}
        });
        let table = &catalog["tables"][0];
        let types = ["numeric(4,0)", "numeric(12,2)", "timestamp without time zone", "character varying", "bytea"];
        let nullables = [false, true, true, true, true];
        let mut target_columns = vec![json!({
            "name":"_oo_pos", "oid":1700, "typmod":2490372, "nullable":false,
            "type":"numeric(38,0)", "ordinal":1
        })];
        for (index, column) in table["columns"].as_array().unwrap().iter().enumerate() {
            target_columns.push(json!({
                "name":column["stored_physical"], "oid":column["oid"], "typmod":column["typmod"],
                "nullable":nullables[index], "type":types[index], "ordinal":index + 2
            }));
        }
        let mut oracle_columns = Vec::new();
        for (name, data_type, precision, scale, char_length, nullable) in [
            ("KEY_ID", "NUMBER", Some(4), Some(0), None, false),
            ("AMOUNT", "NUMBER", Some(12), Some(2), None, true),
            ("OCCURRED", "DATE", None, None, None, true),
            ("LABEL", "VARCHAR2", None, None, Some(40), true),
            ("PAYLOAD", "RAW", None, None, None, true),
        ] {
            oracle_columns.push(json!({"name":name,"data_type":data_type,"precision":precision,"scale":scale,"char_length":char_length,"nullable":nullable}));
        }
        json!({
            "protocol":"oo.oracle.native-c5-provenance/v1",
            "source_registry_raw_json":String::from_utf8(registry_raw.clone()).unwrap(),
            "source_registry_sha256":crate::canonical::sha256_hex(&registry_raw),
            "source_registry_preimage_sha256":registry_preimage_sha,
            "source_registry_path":registry_path, "source_request_path":source_request_path,
            "source_admission_raw_json":String::from_utf8(admission_raw.clone()).unwrap(),
            "source_admission_sha256":admission_sha, "source_admission_path":admission_path,
            "source_catalog_raw_json":String::from_utf8(source_catalog_raw.clone()).unwrap(),
            "source_catalog_sha256":source_catalog_sha, "source_catalog_path":source_catalog_path,
            "source_request_raw_json":String::from_utf8(request_raw.clone()).unwrap(),
            "source_request_sha256":crate::canonical::sha256_hex(&request_raw),
            "capture_receipt_raw_json":String::from_utf8(capture_raw.clone()).unwrap(),
            "capture_receipt_sha256":capture_sha, "capture_receipt_path":capture_path,
            "source_schema_digest":source_schema, "target_schema_digest":catalog["schema_digest"],
            "acknowledged_request":acknowledged_request,
            "acknowledged_request_sha256":crate::canonical::canonical_sha256(&acknowledged_request),
            "scheduler_manifest":manifest,
            "scheduler_manifest_sha256":crate::canonical::canonical_sha256(&manifest),
            "binding":binding, "pin":pin, "task":task, "task_state":"succeeded", "lease_generation":1,
            "source_generation":source_generation, "target_columns":target_columns,
            "oracle_columns":oracle_columns
        })
    }

    fn fixture(root: &std::path::Path) -> (Value, Value, Value) {
        let catalog: Value = serde_json::from_slice(include_bytes!("../tests/oracle-g3-g5/catalog-g5.json")).unwrap();
        let table = &catalog["tables"][0];
        let schema = decode_oracle_name(&table["logical_schema"]).unwrap();
        let name = decode_oracle_name(&table["logical_table"]).unwrap();
        let dictionary = vec![
            "KEY_ID|NUMBER|4|0|0", "AMOUNT|NUMBER|12|2|0", "OCCURRED|DATE|-|-|0",
            "LABEL|VARCHAR2|-|-|40", "PAYLOAD|RAW|-|-|0",
        ];
        let bound = json!({
            "protocol":"oo.source-semantics.source-bound/v1", "engine":"oracle", "source_id":catalog["source_id"],
            "source_generation":catalog["source_generation"], "target_generation":catalog["target_generation"], "run_id":catalog["run_id"],
            "schema_digest":catalog["schema_digest"], "type_digest":catalog["type_digest"], "config_digest":catalog["config_digest"],
            "input_digest":catalog["input_digest"], "banner":"Oracle AI Database 26ai Free Release 23.26.3.0.0",
            "scn":"8992557", "registry_snapshot":"configured-oracle-scn-8992557", "published_scn":"8992557",
            "consistency_grade":"S0", "delete_detection":"reconcile",
            "dictionary":{"protocol":"oo.source-semantics.type-dictionary/v1","columns":dictionary},
            "bindings":[
                {"sealed_column":table["columns"][0]["logical_name"],"dictionary_column":"KEY_ID"},
                {"sealed_column":table["columns"][1]["logical_name"],"dictionary_column":"AMOUNT"},
                {"sealed_column":table["columns"][2]["logical_name"],"dictionary_column":"OCCURRED"},
                {"sealed_column":table["columns"][3]["logical_name"],"dictionary_column":"LABEL"},
                {"sealed_column":table["columns"][4]["logical_name"],"dictionary_column":"PAYLOAD"}
            ]
        });
        let source_gold = json!([
            {"key":"1","amount":"1.00","occurred":"2026-10-14T08:00:00","label":"a","payload":"00ff"},
            {"key":"3","amount":"3.00","occurred":"2026-10-14T10:00:00","label":"c","payload":"aabb"},
            {"key":"4","amount":"4.00","occurred":"2026-10-16T04:05:06","label":"d","payload":"abcd"}
        ]);
        let target_gold = source_gold.clone();
        let source_bytes = pretty(&source_gold);
        let target_bytes = pretty(&target_gold);
        let source_sha = crate::canonical::sha256_hex(&source_bytes);
        let target_sha = crate::canonical::sha256_hex(&target_bytes);
        let mut request = json!({
            "protocol":"oo.pipeline.native-update/v1","source_engine":"oracle","source_id":catalog["source_id"],
            "source_generation":catalog["source_generation"],"target_generation":catalog["target_generation"],"run_id":catalog["run_id"],
            "config_digest":catalog["config_digest"],"schema_digest":catalog["schema_digest"],"type_digest":catalog["type_digest"],"input_digest":catalog["input_digest"],
            "capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":"S0","cross_table_consistent":false,"full_snapshot":true,
            "parent_source_generation":catalog["source_generation"].as_i64().unwrap()-1,
            "position_codec":"snapshot-zero/v1","position_lineage":"configured.oracle","applied_pos":"0",
            "snapshot":{"kind":"scn","value":"configured-oracle-scn-8992557"},"logical_schema":schema,"logical_table":name,
            "tombstones":[],"columns":[
                {"name":"KEY_ID","logical_name":"KEY_ID","arrow_type":"decimal128","nullable":false,"precision":4,"scale":0},
                {"name":"AMOUNT","logical_name":"AMOUNT","arrow_type":"decimal128","nullable":true,"precision":12,"scale":2},
                {"name":"OCCURRED","logical_name":"OCCURRED","arrow_type":"timestamp_micros","nullable":true},
                {"name":"LABEL","logical_name":"LABEL","arrow_type":"utf8","nullable":true},
                {"name":"PAYLOAD","logical_name":"PAYLOAD","arrow_type":"binary","nullable":true}
            ]
        });
        let load = json!({
            "protocol":"oo.pipeline.native-update-result/v1","source_id":catalog["source_id"],"source_generation":catalog["source_generation"],
            "target_generation":catalog["target_generation"],"run_id":catalog["run_id"],"logical_schema":table["logical_schema"],"logical_table":table["logical_table"],
            "physical_name":table["stored_physical"],"row_count":3,"loader_invoked":true,"source_bound":true,"mirror_committed":true,"load_state":"awaiting_publish"
        });
        let shadow = json!({
            "generation":catalog["source_generation"],"position":"configured-oracle-scn-8992557","method":"C5","consistency_grade":"S0","delete_detection":"reconcile",
            "cells":{"1":{},"3":{},"4":{}},"quality":["DELETES_LAGGED"],"tombstones":["2:5"]
        });
        let db_contract = json!({"protocol":"oo.oracle.database-contract-observation/v1","user":"SYSTEM","log_mode":"NOARCHIVELOG","supplemental_log":"NO"});
        let mut reader = json!({"protocol":"oo.oracle.reader-observation/v1","user":"OO04_READER","container":"FREEPDB1","banner":bound["banner"],
            "dictionary_columns":dictionary,"session_privileges":["CREATE SESSION"],"object_privileges":[
                {"object":"OO04_FIX.ONBOARD_ORDERS","privilege":"SELECT"},{"object":"OO04_FIX.ONBOARD_ORDERS","privilege":"FLASHBACK"}
            ]
        });
        let provenance = source_provenance_fixture(&mut request, &catalog, &load);
        reader["oracle_columns"] = provenance["oracle_columns"].clone();
        let mut typed = json!({"source_id":catalog["source_id"],"source_generation":catalog["source_generation"],"rows":3,"cells":15,"differences":0,
            "keys":["1","3","4"],"all_approved_columns":["AMOUNT","KEY_ID","LABEL","OCCURRED","PAYLOAD"],"reader":"OO04_READER","reader_exit":0,
            "source_gold_sha256":source_sha,"target_gold_sha256":target_sha});
        let records = BTreeMap::from([
            ("capture-request", request), ("db-contract", db_contract), ("load-result", load), ("reader-observation", reader),
            ("source-provenance", provenance), ("shadow", shadow), ("source-bound", bound.clone()),
            ("source-gold", source_gold), ("target-gold", target_gold), ("typed-gold", typed.clone()),
        ]);
        let mut hashes = BTreeMap::new();
        for (name, value) in &records { hashes.insert((*name).to_owned(), crate::canonical::sha256_hex(&pretty(value))); }
        let id = crate::canonical::canonical_sha256(&json!(hashes));
        let evidence_dir = root.join("source-evidence").join(&id);
        std::fs::create_dir_all(&evidence_dir).unwrap();
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(root.join("source-evidence"), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&evidence_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let uid = std::fs::metadata(root).unwrap().uid();
        let mut evidence = serde_json::Map::new();
        for (name, value) in &records {
            let bytes = pretty(value);
            std::fs::write(evidence_dir.join(format!("{name}.json")), &bytes).unwrap();
            std::fs::set_permissions(evidence_dir.join(format!("{name}.json")), std::fs::Permissions::from_mode(0o600)).unwrap();
            let metadata = std::fs::metadata(evidence_dir.join(format!("{name}.json"))).unwrap();
            evidence.insert((*name).to_owned(), json!({"path":format!("source-evidence/{id}/{name}.json"),"sha256":hashes[*name],"size":metadata.len(),"uid":uid,"mode":0o600}));
        }
        let semantics = json!({
            "protocol":"oo.oracle.native-c5-source-semantics/v1","source_id":catalog["source_id"],"executed":true,
            "source_generation":catalog["source_generation"],"target_generation":catalog["target_generation"],"product":"Oracle","capture_method":"C5","c1":"off",
            "consistency_grade":"S0","delete_detection":"reconcile","source_contract":{"reader":"OO04_READER","write_attempts":0,"log_mode":"NOARCHIVELOG","supplemental_log":"NO","empty_string":"null","position":"configured-oracle-scn-8992557"},
            "quality":{"tables":1,"rows":3,"columns":5,"typed_cells":15,"typed_cell_differences":0,"delete_quality":"DELETES_LAGGED","key_2_absent":true},
            "evidence_set_id":id,"evidence":Value::Object(evidence)
        });
        let _ = &mut typed;
        (catalog, bound, semantics)
    }

    #[test]
    fn oracle_current_c5_observation_is_verified_and_semantics_project_to_pack() {
        let base = std::env::temp_dir().join(format!("oo-oracle-c5-observation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let work = base.join("work");
        let row = oracle_row();
        let root = work.join("builds").join(&row.build_id);
        std::fs::create_dir_all(&root).unwrap();
        let (catalog, bound, semantics) = fixture(&root);
        let verified = verify_oracle_c5_observation(&root, &catalog, &semantics).expect("verified C5 evidence");
        assert_eq!(verified["source_bound"], bound);
        let evidence_dir = root.join("source-evidence").join(semantics["evidence_set_id"].as_str().unwrap());
        let read = |name: &str| -> Value { serde_json::from_slice(&std::fs::read(evidence_dir.join(format!("{name}.json"))).unwrap()).unwrap() };
        let provenance = read("source-provenance");
        let request = read("capture-request");
        let reader = read("reader-observation");
        let load = read("load-result");
        let projection = verify_oracle_c5_provenance(&provenance, &request, &bound, &reader, &load, &catalog, &catalog["tables"][0]).unwrap();
        let admission: Value = serde_json::from_str(provenance["source_admission_raw_json"].as_str().unwrap()).unwrap();
        assert!(admission.get("audit_digest").is_none(), "g10 admission receipt binds registry preimage, not capture audit digest");
        assert_ne!(projection["source_schema_digest"], projection["target_schema_digest"]);
        assert_eq!(projection["capture_method"], "c5_diff");
        assert_eq!(projection["consistency_grade"], "S0");
        assert_eq!(projection["delete_detection"], "reconcile");
        assert_eq!(projection["column_mapping"].as_array().unwrap().len(), 5);
        let capture_methods = oracle_c5_capture_methods(&catalog, &semantics, &verified).expect("verified capture projection");
        let dataset = format!("{}.{}", catalog["tables"][0]["logical_schema"].as_str().unwrap(), catalog["tables"][0]["logical_table"].as_str().unwrap());
        assert_eq!(capture_methods[&dataset]["logical_table"], dataset);
        assert_eq!(capture_methods[&dataset]["capture_method"], "c5_diff");
        assert_eq!(capture_methods[&dataset]["consistency"], "S0");
        assert_eq!(capture_methods[&dataset]["delete_detection"], "reconcile");
        assert!(!capture_methods[&dataset]["capture_method"].is_null());
        assert!(!capture_methods[&dataset]["consistency"].is_null());
        assert!(!capture_methods[&dataset]["delete_detection"].is_null());
        for (field, wrong) in [("capture_method", "c1_cdc"), ("consistency_grade", "B"), ("delete_detection", "none")] {
            let mut bad_observation = verified.clone();
            bad_observation["provenance"][field] = json!(wrong);
            assert!(oracle_c5_capture_methods(&catalog, &semantics, &bad_observation).is_err());
        }

        let bundle = base.join("bundle");
        let columns = catalog["tables"][0]["columns"].as_array().unwrap();
        let column_names: Vec<String> = columns.iter().map(|column| column["logical_name"].as_str().unwrap().to_owned()).collect();
        let role_columns: Vec<String> = column_names.iter().map(|column| format!("{dataset}.{column}")).collect();
        let role_column_refs: Vec<&str> = role_columns.iter().map(String::as_str).collect();
        write_oracle_bundle(&bundle, &catalog, &role_column_refs);
        std::fs::write(bundle.join("source_semantics.json"), serde_json::to_vec(&semantics).unwrap()).unwrap();
        std::fs::write(bundle.join("roles.json"), oracle_bytes("roles")).unwrap();
        let mut model: Value = serde_json::from_slice(&std::fs::read(bundle.join("native-model.json")).unwrap()).unwrap();
        model["visibility"]["role"] = json!("oo_r_oracle_orders");
        model["visibility"]["datasets"] = json!([{"columns":column_names,"dataset":dataset}]);
        let model_bytes = serde_json::to_vec(&model).unwrap();
        std::fs::write(bundle.join("native-model.json"), &model_bytes).unwrap();
        crate::sealed::write_manifest(&bundle).unwrap();
        let rules = std::fs::read(bundle.join("rules.json")).unwrap();
        let semantics_bytes = serde_json::to_vec(&semantics).unwrap();
        write_oracle_build(&work, &oracle_value("g3"), &crate::canonical::sha256_hex(&rules), &semantics_bytes, &model_bytes);
        std::fs::write(root.join("native-model-output/command.json"), oracle_bytes("command")).unwrap();
        std::fs::write(root.join("native-model-output/visibility.json"), oracle_bytes("visibility")).unwrap();
        let semantics_dir = root.join("source-semantics");
        std::fs::create_dir_all(&semantics_dir).unwrap();
        std::fs::set_permissions(&semantics_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let semantics_sha = crate::canonical::sha256_hex(&semantics_bytes);
        let semantics_path = semantics_dir.join(format!("{semantics_sha}.json"));
        std::fs::write(&semantics_path, &semantics_bytes).unwrap();
        std::fs::set_permissions(&semantics_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = oracle_config(&work, &bundle);
        config.projects[0].source_protocol.as_mut().unwrap().semantics = "oo.oracle.native-c5-source-semantics/v1".to_owned();
        let catalog_sha = crate::canonical::sha256_hex(&std::fs::read(bundle.join("catalog.json")).unwrap());
        let rebound = native_source_rebind(&config, &row, &catalog_sha, Some(&semantics_sha)).expect("current C5 source rebind");
        assert_eq!(rebound["capture_methods"], capture_methods);
        let docs = Docs {
            generated_sha: "11".repeat(32), generated: empty_model(), curated_sha: String::new(), reviews: Vec::new(),
            effective: empty_model(), effective_yaml: String::new(), effective_sha: "22".repeat(32), feedback: Vec::new(),
            curated_revisions: 0, generated_revisions: 0,
        };
        let maintained = compile_native_maintenance(&config, &row, &docs, Some(&rebound)).expect("compiled current Oracle Pack");
        assert_eq!(maintained["llm_calls"], 0);
        assert_eq!(maintained["source_rebind"]["capture_methods"], capture_methods);
        let artifact = std::path::PathBuf::from(maintained["artifact_directory"].as_str().unwrap());
        let receipt: Value = serde_json::from_slice(&std::fs::read(artifact.join("receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt["source_rebind"]["capture_methods"], capture_methods);
        let effective: Value = serde_json::from_slice(&std::fs::read(artifact.join("effective.json")).unwrap()).unwrap();
        let oos_source = effective["datasets"].as_array().unwrap().iter().find(|item| item["dataset"] == dataset).unwrap().get("source_semantics").unwrap();
        assert_eq!(oos_source["capture"], "c5_diff");
        assert_eq!(oos_source["grade"], "S0");
        assert_eq!(oos_source["delete_detection"], "reconcile");
        let explanation: Value = serde_json::from_slice(&std::fs::read(artifact.join("source.json")).unwrap()).unwrap();
        assert_eq!(explanation["capture_methods"], capture_methods);
        let explained_dataset = explanation["datasets"].as_array().unwrap().iter().find(|item| item["dataset"] == dataset).unwrap();
        assert_eq!(explained_dataset["source_semantics"]["capture"], "c5_diff");
        assert_eq!(explained_dataset["source_semantics"]["grade"], "S0");
        assert_eq!(explained_dataset["source_semantics"]["delete_detection"], "reconcile");
        assert_eq!(explanation["actual_core_effective_sha256"], crate::canonical::canonical_sha256(&effective));
        assert_eq!(maintained["source_json_sha256"], crate::canonical::sha256_hex(&std::fs::read(artifact.join("source.json")).unwrap()));
        let pack: Value = serde_json::from_slice(&std::fs::read(artifact.join("role-pack.json")).unwrap()).unwrap();
        assert_eq!(pack["pack_digest"], maintained["pack_digest"]);
        assert_eq!(receipt["pack_digest"], pack["pack_digest"]);

        let mut wrong_source_audit = provenance.clone();
        let mut source_request: Value = serde_json::from_str(wrong_source_audit["source_request_raw_json"].as_str().unwrap()).unwrap();
        source_request["audit_digest"] = json!("f".repeat(64));
        let source_request_raw = pretty(&source_request);
        wrong_source_audit["source_request_sha256"] = json!(crate::canonical::sha256_hex(&source_request_raw));
        wrong_source_audit["source_request_raw_json"] = json!(String::from_utf8(source_request_raw).unwrap());
        wrong_source_audit["acknowledged_request"] = source_request.clone();
        wrong_source_audit["acknowledged_request_sha256"] = json!(crate::canonical::canonical_sha256(&source_request));
        assert!(verify_oracle_c5_provenance(&wrong_source_audit, &request, &bound, &reader, &load, &catalog, &catalog["tables"][0]).is_err());
        let mut wrong = provenance.clone();
        wrong["source_schema_digest"] = catalog["schema_digest"].clone();
        assert!(verify_oracle_c5_provenance(&wrong, &request, &bound, &reader, &load, &catalog, &catalog["tables"][0]).is_err());
        let mut wrong_reader = reader.clone();
        wrong_reader["oracle_columns"][0]["nullable"] = json!(true);
        assert!(verify_oracle_c5_provenance(&provenance, &request, &bound, &wrong_reader, &load, &catalog, &catalog["tables"][0]).is_err());
        let mut wrong_target = provenance.clone();
        wrong_target["target_columns"][1]["nullable"] = json!(true);
        assert!(verify_oracle_c5_provenance(&wrong_target, &request, &bound, &reader, &load, &catalog, &catalog["tables"][0]).is_err());
        let mut wrong_preimage = provenance.clone();
        wrong_preimage["source_registry_preimage_sha256"] = json!("f".repeat(64));
        assert!(verify_oracle_c5_provenance(&wrong_preimage, &request, &bound, &reader, &load, &catalog, &catalog["tables"][0]).is_err());
        let mut effective = json!({"datasets":[{"dataset":"q_4f4f30345f464958.q_4f4e424f4152445f4f5244455253"}]});
        project_oracle_c5_semantics(&mut effective, &catalog, &bound).expect("derived Oracle pack semantics");
        let source = &effective["datasets"][0]["source_semantics"];
        assert_eq!(source["capture"], "c5_diff");
        assert_eq!(source["grade"], "S0");
        assert_eq!(source["delete_detection"], "reconcile");
        assert_eq!(source["columns"].as_array().unwrap().iter().find(|item| item["column"] == "q_4f43435552524544").unwrap()["native_note"], "oracle_date_includes_time");
        assert_eq!(decode_oracle_name(&catalog["tables"][0]["logical_schema"]).unwrap(), "OO04_FIX");
        let mut wrong = semantics.clone();
        wrong["consistency_grade"] = json!("B");
        assert_ne!(semantic_basis(semantics.clone()), semantic_basis(wrong.clone()));
        assert!(verify_oracle_c5_observation(&root, &catalog, &wrong).is_err());
        let mut runtime = semantics.clone();
        runtime["source_generation"] = json!(6);
        runtime["target_generation"] = json!(6);
        runtime["source_contract"]["position"] = json!("configured-oracle-scn-8992558");
        runtime["quality"]["rows"] = json!(4);
        runtime["quality"]["typed_cells"] = json!(20);
        runtime["quality"]["typed_cell_differences"] = json!(0);
        runtime["evidence_set_id"] = json!("f".repeat(64));
        assert_eq!(semantic_basis(semantics), semantic_basis(runtime));
        let _ = std::fs::remove_dir_all(base);
    }
}
