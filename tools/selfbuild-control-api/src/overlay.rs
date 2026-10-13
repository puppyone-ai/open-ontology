//! Generated, curated, and review documents. Curated is insert-only.
//! A compiled role pack is not publication.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
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
    if trigger == "source_generation_advanced" { cache_key = canonical_sha256(&json!({"maintenance":cache_key,"catalog_sha256":value["catalog_sha256"]})); }
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
        let rebound = native_source_rebind(config, row, sha)?;
        if let Some(previous) = latest_native_rebind(&client, row).await? {
            check_generation_progress(&previous["binding_vector"], &rebound["catalog"]["binding_vector"])?;
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
    let keys: &[&str] = match trigger {
        "source_generation_advanced" => &["catalog_sha256", "identity_revision", "idempotency_key", "trigger"],
        "schema_changed" | "source_fact_corrected" => &["fact_edit", "identity_revision", "idempotency_key", "trigger"],
        "feedback" => &["identity_revision", "idempotency_key", "note", "trigger"],
        "table_ready" | "scheduled" | "manual" => &["identity_revision", "idempotency_key", "trigger"],
        _ => return Err(ApiError::malformed()),
    };
    exact(value, keys)?;
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
    }
    value
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

fn native_source_rebind(config: &AppConfig, row: &BuildRow, expected_sha: &str) -> Result<Value, ApiError> {
    let project = config.projects.iter().find(|p| p.project_id == row.project_id).ok_or_else(ApiError::forbidden)?;
    let (raw, hashes) = sealed::native_maintenance_inputs(&project.sealed_bundle, project.source_protocol.as_ref())?;
    if hashes.get("catalog.json").map(String::as_str) != Some(expected_sha) { return Err(ApiError::pack_refused()); }
    let parse = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes).map_err(|_| ApiError::pack_refused());
    let read = |name: &str| raw.get(name).ok_or_else(ApiError::pack_refused).and_then(|b| parse(b));
    let root = Path::new(&config.work_dir).join("builds").join(&row.build_id);
    let original = |name: &str| std::fs::read(root.join(name)).map_err(|_| ApiError::not_ready()).and_then(|b| parse(&b));
    let command = original("native-model-output/command.json")?;
    let catalog = read("catalog.json")?;
    if catalog["protocol"] != oo_builder::SEALED_IMPORT_COLLECTION_PROTOCOL || command["catalog"]["protocol"] != catalog["protocol"] { return Err(ApiError::pack_refused()); }
    let old = oo_builder::compile_sealed_import_collection(&crate::canonical::canonical_json(&command["catalog"])).map_err(|_| ApiError::pack_refused())?;
    let new = oo_builder::compile_sealed_import_collection(&crate::canonical::canonical_json(&catalog)).map_err(|_| ApiError::pack_refused())?;
    if old.columns != new.columns || read("native-model.json")? != original("native-model.json")?
        || hashes.get("rules.json").map(String::as_str) != original("S2.request.json")?["rules_sha256"].as_str()
        || semantic_basis(read("source_semantics.json")?) != semantic_basis(original("S0.response.json")?["source_adapt"]["source_semantics"].clone()) { return Err(ApiError::pack_refused()); }
    let mut allowed: Vec<String> = original("native-model.json")?["visibility"]["datasets"].as_array().ok_or_else(ApiError::pack_refused)?.iter().flat_map(|d| d["columns"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(|c| format!("{}.{}",d["dataset"].as_str().unwrap_or(""),c)))).collect();
    let model = original("native-model.json")?;
    let role = model["visibility"]["role"].as_str().ok_or_else(ApiError::pack_refused)?;
    let mut declared: Vec<String> = read("roles.json")?["roles"][role].as_array().ok_or_else(ApiError::pack_refused)?.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(ApiError::pack_refused)).collect::<Result<_,_>>()?;
    allowed.sort(); declared.sort(); if allowed != declared { return Err(ApiError::pack_refused()); }
    check_generation_progress(&command["catalog"]["binding_vector"], &catalog["binding_vector"])?;
    let before = command["catalog"]["entries"].as_array().ok_or_else(ApiError::pack_refused)?;
    let after = catalog["entries"].as_array().ok_or_else(ApiError::pack_refused)?;
    for old in before {
        let new = after.iter().find(|e| e["tables"][0]["logical_schema"] == old["tables"][0]["logical_schema"] && e["tables"][0]["logical_table"] == old["tables"][0]["logical_table"]).ok_or_else(ApiError::pack_refused)?;
        for key in ["source_id", "engine", "config_digest", "schema_digest", "type_digest", "capture", "state", "hook_checked", "source_introspection", "tool_hook_digest"] {
            if new[key] != old[key] { return Err(ApiError::pack_refused()); }
        }
        let quality = |v: &Value| { let mut q=v["quality"].clone(); if let Some(o)=q.as_object_mut(){o.remove("stored_rows");} q };
        if quality(new) != quality(old) { return Err(ApiError::pack_refused()); }
    }
    Ok(json!({"catalog":catalog,"catalog_sha256":expected_sha,"bundle_files":hashes}))
}

async fn latest_native_rebind(client: &Client, row: &BuildRow) -> Result<Option<Value>, ApiError> {
    let result = client.query_opt("SELECT (response_text::jsonb->'native_maintenance'->'source_rebind')::text FROM oo_selfbuild_api.maintenance_receipt WHERE build_id=$1 AND route='maintenance' AND response_text::jsonb->'native_maintenance'->'source_rebind'->>'catalog_sha256' IS NOT NULL ORDER BY created_at DESC, idempotency_key DESC LIMIT 1", &[&row.build_id]).await.map_err(|_| ApiError::not_ready())?;
    result.map(|r| { let text: String = r.try_get(0).map_err(|_| ApiError::not_ready())?; serde_json::from_str(&text).map_err(|_| ApiError::not_ready()) }).transpose()
}

async fn accepted_native_rebind(config: &AppConfig, client: &Client, row: &BuildRow) -> Result<Option<Value>, ApiError> {
    let Some(receipt) = latest_native_rebind(client, row).await? else { return Ok(None); };
    let sha = receipt["catalog_sha256"].as_str().ok_or_else(ApiError::pack_refused)?;
    native_source_rebind(config, row, sha).map(Some)
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
    if rebound.is_some() {
        let previous = command["settings"]["catalog_version"].clone();
        for decision in command["decisions"].as_array_mut().ok_or_else(ApiError::pack_refused)? {
            if decision["schema_hash"] != previous { return Err(ApiError::pack_refused()); }
            decision["schema_hash"] = json!(compiled.identity_hash);
        }
        command["settings"]["catalog_version"] = json!(compiled.identity_hash);
    }
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
    let source_fact_corrections = project_native_source_money(&mut effective, &docs.effective, &units)?;
    let source_unit_corrections: Vec<Value> = source_fact_corrections.iter().filter_map(|item| {
        let before = item.get("reviewed_candidate")?.get("unit")?.as_str()?;
        let after = item.get("source_declared")?.get("unit")?.as_str()?;
        (before != after).then(|| json!({"column":item["column"],"candidate_unit":before,"declared_unit":after}))
    }).collect();
    let mut visibility = read(&original.join("visibility.json"))?;
    if rebound.is_some() { visibility["catalog_version"] = json!(compiled.identity_hash); }
    let pack = oo_builder::compile_role_pack(&crate::canonical::canonical_json(&effective), &crate::canonical::canonical_json(&visibility)).map_err(|error| { eprintln!("native-maintenance role refused: {}", error.code); ApiError::pack_refused() })?;
    let receipt = json!({"protocol":"oo.selfbuild-control-api/native-maintenance/v1","build_id":row.build_id,"overlay_effective_sha256":docs.effective_sha,"curated_sha256":docs.curated_sha,"generated_sha256":docs.generated_sha,"reviewed_sha256":sha256_hex(reviewed_text.as_bytes()),"source_unit_corrections":source_unit_corrections,"source_fact_corrections":source_fact_corrections,"effective_sha256":canonical_sha256(&effective),"pack_digest":pack.pack_digest,"manifest_sha256":pack.manifest_sha256,"publication":"not_published","llm_calls":0,"source_rebind":rebound.map(|v| json!({"catalog_sha256":v["catalog_sha256"],"bundle_files":v["bundle_files"],"binding_vector":v["catalog"]["binding_vector"],"original_command_sha256":original_command_sha,"original_decisions_sha256":original_decisions_sha,"rebound_decisions_sha256":canonical_sha256(&command["decisions"])}))});
    let digest = canonical_sha256(&receipt);
    let dest = root.join("native-maintenance").join(&digest);
    let mut files = BTreeMap::new();
    files.insert("command.json", crate::canonical::canonical_json(&command));
    files.insert("reviewed.json", reviewed_text);
    files.insert("effective.json", crate::canonical::canonical_json(&effective));
    files.insert("visibility.json", crate::canonical::canonical_json(&visibility));
    files.insert("role-pack.json", pack.artifact);
    files.insert("receipt.json", crate::canonical::canonical_json(&receipt));
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
    use super::{ensure_source_money, project_native_source_money};
    use crate::oosyaml::generated_from_inputs;
    use serde_json::json;
    use std::collections::BTreeMap;

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
}
