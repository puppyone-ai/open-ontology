//! Authenticate an individual range part, then stage it without publishing.
use super::*;
use crate::native_c5_parts::{self, FileRef, JsonLines, PartScope, PartsIndex, RANGE_PROTOCOL};
use crate::native_c5_parts_stage::{self, Column, Spec};

const PROTOCOL: &str = "oo.pipeline.native-c5-part-stage/v1";

pub(super) async fn stage(document: &Value) -> Result<Value, &'static str> {
    let fields = document.as_object().ok_or("c5_part_request")?;
    if document["protocol"] != PROTOCOL
        || fields.len() != 6
        || fields.keys().any(|key| ![
            "protocol","scope","native_request","registry","lease_id","lease_generation"
        ].contains(&key.as_str()))
    {
        return Err("c5_part_request");
    }
    let task = text_field(document,"lease_id")?;
    let generation = document["lease_generation"].as_i64().filter(|n| *n>0).ok_or("lease_required")?;
    let (scope, plan, registry) = authenticate(document, task, generation)?;
    stage_plan(&scope, &plan, &registry).await
}

fn authenticate(document: &Value, task: &str, generation: i64) -> Result<(PartScope, UpdatePlan, FileRef), &'static str> {
    let scope = PartScope::parse(&document["scope"])?;
    let registry: FileRef = serde_json::from_value(document["registry"].clone())
        .map_err(|_| "c5_part_registry")?;
    if registry.bytes > 1_048_576 {
        return Err("c5_part_registry");
    }
    crate::native_c5_parts::verify_file(&registry)?;
    let bytes = fs::read(&registry.path).map_err(|_| "c5_part_registry")?;
    if bytes.len() as u64 != registry.bytes || hex_bytes(&Sha256::digest(&bytes)) != registry.sha256 {
        return Err("c5_part_registry");
    }
    let registry_value: Value = serde_json::from_slice(&bytes).map_err(|_| "c5_part_registry")?;
    let bindings = registry_value["bindings"].as_array().ok_or("c5_part_registry")?;
    if bindings.len() != 1 || bindings[0]["range_scope"] != document["scope"] {
        return Err("c5_part_registry");
    }
    let mut native = document["native_request"].clone();
    if native["protocol"] != REQUEST_PROTOCOL
        || native["source_engine"] != "mysql"
        || native["source_id"] != scope.range.source_id
        || native["source_generation"] != scope.range.source_generation
        || native["parent_source_generation"] != scope.range.parent_generation
        || native["logical_schema"] != scope.range.schema
        || native["logical_table"] != scope.range.table
        || native["arrow_path"] != scope.part.arrow.path
        || native["input_digest"] != scope.part.arrow.sha256
        || native["snapshot"] != serde_json::to_value(&scope.snapshot).map_err(|_| "c5_part_scope")?
        || native["capture_method"] != "c5_diff"
        || native["delete_detection"] != "reconcile"
        || native["full_snapshot"] != false
        || native["tombstones"] != json!([])
        || native.get("mirror").is_some_and(|value| *value != false)
        || native.get("lease_id").is_some_and(|id| id.as_str() != Some(task))
        || native.get("lease_generation").is_some_and(|value| value.as_i64() != Some(generation))
    {
        return Err("c5_part_scope");
    }
    native["lease_id"] = json!(task);
    native["lease_generation"] = json!(generation);
    native["mirror"] = json!(false);
    let bin = std::env::var("OO_NATIVE_ADMISSION_BIN").map_err(|_| "nonadmitted")?;
    let plan = native_binding::with_operator_env(native_binding::OperatorEnv::Fixed {
        bin:Some(bin),registry:Some(registry.path.clone()),registry_sha256:Some(registry.sha256.clone()),
    }, || classify_inner(&native,true,false,false))?;
    if !plan.source_bound || plan.shape != "unique" || plan.mirror
        || plan.range_scope.is_some() || plan.partition_scope.is_some()
        || plan.transaction_scope.is_some() || plan.gtid_scope.is_some() || plan.source_instance.is_some()
    {
        return Err("c5_part_scope");
    }
    let admission = native_binding::with_operator_env(native_binding::OperatorEnv::Fixed {
        bin:std::env::var("OO_NATIVE_ADMISSION_BIN").ok(),
        registry:Some(registry.path.clone()),registry_sha256:Some(registry.sha256.clone()),
    }, || bind_admission(&native, &plan.arrow))?;
    if admission.range_scope.as_ref() != Some(&document["scope"]) {
        return Err("c5_part_scope");
    }
    if document.get("type_policy").is_some() {
        let binding = &bindings[0];
        let path = text_field(binding,"capture_receipt_path")?;
        let size = fs::symlink_metadata(path).map_err(|_|"c5_parts_notes")?.len();
        if size > 33_554_432 { return Err("c5_parts_notes"); }
        let witness = FileRef {path:path.into(),bytes:size,
            sha256:text_field(binding,"capture_receipt_sha256")?.into()};
        native_c5_parts::verify_file(&witness)?;
        let bytes = fs::read(path).map_err(|_|"c5_parts_notes")?;
        if hex_bytes(&Sha256::digest(&bytes)) != witness.sha256 { return Err("c5_parts_notes"); }
        let capture:Value = serde_json::from_slice(&bytes).map_err(|_|"c5_parts_notes")?;
        if document["type_policy"] != capture["type_policy"]
            || document["normalization_rejects"] != capture["normalization_rejects"] {
            return Err("c5_parts_notes");
        }
    }
    Ok((scope, plan, registry))
}

async fn stage_plan(scope: &PartScope, plan: &UpdatePlan, registry: &FileRef) -> Result<Value, &'static str> {
    let decoded = decode_arrow_stream(&plan.arrow).map_err(arrow_token)?;
    if decoded.rows as u64 != scope.part.row_count {
        return Err("c5_part_rows");
    }
    let prepared = prepare_table(
        &plan.arrow,&plan.source_id,&plan.schema,&plan.table,
        u64::try_from(plan.source_generation).map_err(|_| "stale_run")?,
        &plan.config_digest,decode_hex32(&plan.input_digest).map_err(|_| "digest_mismatch")?,
        &plan.batch_id,&columns_for_prepare(&plan.columns,&plan.column_sources)?,
        Some(&plan.trusted_fields),
    ).map_err(arrow_token)?;
    let spec = Spec {
        columns:prepared.verified_columns.iter().zip(&plan.column_idents).map(|(col,name)| Column {
            name:name.clone(),oid:col.oid,typmod:col.typmod,nullable:!col.not_null,
        }).collect(),
        keys:plan.key_indexes.clone(),
    };
    let mut conn = connect_target(&plan.target).await?;
    let mut receipt = native_c5_parts_stage::stage(
        &mut conn,plan.lease_id.as_deref().ok_or("lease_required")?,
        plan.lease_generation.ok_or("lease_required")?,scope,&spec,&decoded,&registry.sha256,
    ).await?;
    receipt["registry_sha256"]=json!(registry.sha256);
    Ok(receipt)
}

fn reference(value: &Value) -> Result<FileRef, &'static str> {
    serde_json::from_value(value.clone()).map_err(|_| "c5_parts_reference")
}

fn registered(document: &Value) -> Result<(), &'static str> {
    let path = std::env::var("OO_NATIVE_BINDING_REGISTRY").map_err(|_| "nonadmitted")?;
    let sha = std::env::var("OO_NATIVE_BINDING_REGISTRY_SHA256").map_err(|_| "nonadmitted")?;
    let length = fs::symlink_metadata(&path).map_err(|_| "nonadmitted")?.len();
    if length > 65_536 { return Err("nonadmitted"); }
    let file = FileRef {path,sha256:sha,bytes:length};
    native_c5_parts::verify_file(&file)?;
    let bytes = fs::read(&file.path).map_err(|_| "nonadmitted")?;
    if hex_bytes(&Sha256::digest(&bytes)) != file.sha256 { return Err("nonadmitted"); }
    let actual: Value = serde_json::from_slice(&bytes).map_err(|_| "nonadmitted")?;
    let expected = json!({"protocol":"oo.source-admission.c5-parts-index/v1",
        "admitted_index":document["admitted_index"],"parts_index":document["parts_index"],
        "part_count":document["part_count"],"row_count":document["row_count"],
        "source_id":document["source_id"],"source_generation":document["source_generation"]});
    if actual != expected { return Err("c5_parts_registration"); }
    Ok(())
}

fn check_outer(document: &Value, scope: &PartScope, native: &Value) -> Result<(), &'static str> {
    let identity = serde_json::to_value(&scope.range).map_err(|_| "c5_parts_scope")?;
    if identity.as_object().ok_or("c5_parts_scope")?.iter()
        .any(|(key,value)| document.get(key) != Some(value))
        || document["protocol"] != RANGE_PROTOCOL
        || document["parts_index"] != serde_json::to_value(&scope.parts_index).map_err(|_| "c5_parts_scope")?
        || document["part_count"] != scope.part_count || document["row_count"] != scope.total_rows
        || document["input_digest"] != scope.parts_index.sha256
        || document["target"] != native["target"] || document["key_columns"] != native["key_columns"]
        || document["capture_method"] != "c5_diff" || document["delete_detection"] != "reconcile"
        || document["position_codec"] != "snapshot-zero/v1" || document["applied_pos"] != "0"
        || document["consistency_grade"] != "B" || document["cross_table_consistent"] != false {
        return Err("c5_parts_scope");
    }
    Ok(())
}

fn notes(entry: &Value, scope: &PartScope) -> Result<Vec<Value>, &'static str> {
    let policy = &entry["type_policy"];
    if policy.as_object().is_none_or(|p| p.len() != 2
        || !p["zero_date_to_null"].is_boolean() || !p["strip_text_nul"].is_boolean()) {
        return Err("c5_parts_policy");
    }
    let reference = &entry["normalization_rejects"];
    if reference.is_null() { return Ok(Vec::new()); }
    if reference.as_object().is_none_or(|r| r.len() != 2) { return Err("c5_parts_notes"); }
    let path = text_field(reference,"path")?;
    let length = fs::symlink_metadata(path).map_err(|_| "c5_parts_notes")?.len();
    // This is one source part's masked diagnostic page, not the logical range.
    if length > 33_554_432 { return Err("c5_parts_notes"); }
    let file = FileRef {path:path.into(),sha256:hex_field(reference,"sha256").map_err(|_|"c5_parts_notes")?,bytes:length};
    native_c5_parts::verify_file(&file)?;
    let bytes = fs::read(path).map_err(|_| "c5_parts_notes")?;
    if hex_bytes(&Sha256::digest(&bytes)) != file.sha256 { return Err("c5_parts_notes"); }
    let records:Vec<Value> = serde_json::from_slice(&bytes).map_err(|_| "c5_parts_notes")?;
    if records.is_empty() { return Err("c5_parts_notes"); }
    for record in &records {
        let enabled = match record["reason"].as_str() {
            Some("zero_date") => policy["zero_date_to_null"] == true,
            Some("nul_byte") => policy["strip_text_nul"] == true,
            _ => false,
        };
        if !enabled || record["raw_value_recorded"] != false
            || record["input_digest"] != scope.part.arrow.sha256
            || record["run_generation"] != scope.range.source_generation
            || record["row_index"].as_u64().is_none_or(|row| row >= scope.part.row_count) {
            return Err("c5_parts_notes");
        }
    }
    // Validate the existing masked-quality contract in bounded batches.
    for chunk in records.chunks(512) {
        crate::masked_rejects::project(&scope.range.source_id,&scope.range.schema,&scope.range.table,&json!(chunk))?;
    }
    Ok(records)
}

pub(super) async fn execute(document: &Value) -> Result<Value, &'static str> {
    registered(document)?;
    let task = text_field(document,"lease_id")?;
    let generation = document["lease_generation"].as_i64().filter(|g| *g>0).ok_or("lease_required")?;
    let admitted = reference(&document["admitted_index"])?;
    let mut entries = JsonLines::open(admitted.clone())?;
    let mut parts = PartsIndex::open(reference(&document["parts_index"])?,
        document["part_count"].as_u64().ok_or("c5_parts_count")?,
        document["row_count"].as_u64().ok_or("c5_parts_count")?)?;
    let mut first: Option<(PartScope,UpdatePlan)> = None;
    let mut trust = Sha256::new();
    while let Some(part) = parts.next_part()? {
        let entry = entries.next_value()?.ok_or("c5_parts_incomplete")?;
        if entry.as_object().is_none_or(|v| v.len()!=5
            || !["scope","registry","native_request","type_policy","normalization_rejects"].iter().all(|key|v.contains_key(*key))) {
            return Err("c5_parts_entry");
        }
        let (scope, mut plan, registry) = authenticate(&entry,task,generation)?;
        if scope.part != part || entry["type_policy"] != document["type_policy"] {
            return Err("c5_parts_scope");
        }
        check_outer(document,&scope,&entry["native_request"])?;
        notes(&entry,&scope)?;
        if let Some((prior,first_plan)) = &first {
            if !scope.same_capture(prior)
                || plan.config_digest != first_plan.config_digest
                || plan.schema_digest != first_plan.schema_digest
                || plan.type_digest != first_plan.type_digest
                || plan.target != first_plan.target || plan.batch_id != first_plan.batch_id
                || plan.key_indexes != first_plan.key_indexes || plan.column_sources != first_plan.column_sources
                || plan.trusted_fields != first_plan.trusted_fields {
                return Err("c5_parts_schema");
            }
        }
        stage_plan(&scope,&plan,&registry).await?;
        trust.update(registry.sha256.as_bytes());
        trust.update(plan.audit_digest.as_bytes());
        if first.is_none() {
            // Keep only the common schema; staged rows, not the first Arrow, are applied.
            plan.arrow.clear();
            first = Some((scope,plan));
        }
    }
    entries.finish()?;
    parts.finish()?;
    let (scope,mut plan) = first.ok_or("c5_parts_incomplete")?;
    let mut conn = connect_target(&plan.target).await?;
    native_c5_parts_stage::complete(&mut conn,&plan.source_id,task,generation,&scope.parts_index.sha256).await?;
    plan.range_scope = Some(native_c5_parts::range_scope(&scope,task)?);
    plan.parts_index = Some(admitted.clone());
    plan.input_digest = scope.parts_index.sha256.clone();
    plan.audit_digest = hex_bytes(&trust.finalize());
    plan.payload_digest = hex_bytes(&Sha256::digest(serde_json::to_vec(&json!({
        "protocol":RANGE_PROTOCOL,"scope":plan.range_scope,"admitted_index":admitted,
        "target":plan.target,"batch_id":plan.batch_id,"audit":plan.audit_digest,
        "config":plan.config_digest,"schema":plan.schema_digest,"type":plan.type_digest,
    })).map_err(|_|"c5_parts_scope")?));
    plan.producer_trust = json!({"entry":"verify_inputs","trusted":true,"document_bound":true,
        "scope":"complete_c5_range","admissions":scope.part_count,"admitted_index":admitted,
        "parts_index":scope.parts_index,"audit_sha256":plan.audit_digest,
        "note":"Every part passed verify_inputs; no individual part seal is an aggregate seal."});
    plan.mirror = match document.get("mirror") {
        Some(value) => value.as_bool().ok_or("c5_parts_scope")?,
        None => true,
    };
    execute_plan(document,plan).await
}

pub(super) async fn record_quality(
    tx: &mut sqlx::Transaction<'_,sqlx::Postgres>,plan: &UpdatePlan,fence:i64,lease:&Value,
) -> Result<(), &'static str> {
    let mut entries = JsonLines::open(plan.parts_index.clone().ok_or("c5_parts_scope")?)?;
    while let Some(entry) = entries.next_value()? {
        let scope = PartScope::parse(&entry["scope"])?;
        let records = notes(&entry,&scope)?;
        for (index,chunk) in records.chunks(512).enumerate() {
            let masked = crate::masked_rejects::project(&plan.source_id,&plan.schema,&plan.table,&json!(chunk))?;
            let digest = hex_bytes(&Sha256::digest(format!("c5-parts-notes/v1\n{}\n{}\n{}\n{}",
                plan.batch_id,scope.part.ordinal,index,entry["normalization_rejects"]["sha256"]).as_bytes()));
            sqlx::query("SELECT _oo.record_masked_rejects($1,$2::uuid,$3,$4,$5,$6,$7,$8::jsonb)")
                .bind(&plan.shard).bind(&plan.owner).bind(fence).bind(&plan.source_id)
                .bind(&plan.schema).bind(&plan.table).bind(digest).bind(json!(masked).to_string())
                .execute(&mut **tx).await.map_err(sql_token)?;
            baseline_held_heartbeat(tx,lease).await?;
        }
    }
    entries.finish()
}
