use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::time::Duration;

use serde_json::{Value, json};
use tokio_postgres::Client;

use crate::canonical::{canonical_sha256, sha256_hex};
use crate::config::{AppConfig, ProjectCfg};
use crate::error::ApiError;
use crate::membership::Member;
use crate::oosyaml;
use crate::spawn::{ChildSpec, run_child};
use crate::store::{self, BuildRow};

fn read(path: &Path, max: usize) -> Result<Value, ApiError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ApiError::not_ready())?;
    if !meta.file_type().is_file() || meta.len() > max as u64 {
        return Err(ApiError::malformed());
    }
    let bytes = std::fs::read(path).map_err(|_| ApiError::not_ready())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ApiError::malformed())?;
    oo_core::json_scan::parse_json_value_with_limits(
        text,
        "oo.selfbuild.library/v1",
        &oo_core::json_scan::JsonScanLimits {
            max_bytes: max,
            max_nodes: 4_000_000,
            max_depth: 32,
        },
    )
    .map_err(|_| ApiError::malformed())
}

fn pinned(path: &str, sha: &str, max: usize) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.file_type().is_file() && meta.len() <= max as u64)
        && std::fs::read(path).is_ok_and(|bytes| sha256_hex(&bytes) == sha)
}

pub fn ready(project: &ProjectCfg) -> bool {
    let Some(library) = &project.library else {
        return false;
    };
    crate::config::regular_file(&library.python)
        && pinned(
            &library.script,
            &library.script_sha256,
            project.budgets.max_document_bytes,
        )
        && pinned(
            &library.config,
            &library.config_sha256,
            project.budgets.max_document_bytes,
        )
        && pinned(
            &library.manifest,
            &library.manifest_sha256,
            project.budgets.max_document_bytes,
        )
}

fn write(path: &Path, value: &Value) -> Result<(), ApiError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ApiError::not_ready())?;
    file.write_all(crate::canonical::canonical_json(value).as_bytes())
        .map_err(|_| ApiError::not_ready())
}

async fn invoke(
    project: &ProjectCfg,
    dir: &Path,
    stage: &str,
    cancel: Arc<AtomicBool>,
) -> Result<Value, ApiError> {
    if !ready(project) {
        return Err(ApiError::not_ready());
    }
    let library = project.library.as_ref().ok_or_else(ApiError::not_ready)?;
    let mut request = json!({"stage": stage, "config": library.config, "manifest": library.manifest,
        "work_dir": dir, "max_tables": project.budgets.max_tables,
        "datasets": project.source.tables.iter().map(|(s,t)| format!("{s}.{t}")).collect::<Vec<_>>(),
        "llm": {"model": project.llm.model_id, "bearer_file": project.llm.bearer_file,
                  "endpoint": format!("{}/v1", project.llm.origin)}});
    if stage == "S5" {
        let policy = read(
            Path::new(&project.role_policy_file),
            project.budgets.max_document_bytes,
        )?;
        let library_config = read(Path::new(&library.config), project.budgets.max_document_bytes)?;
        request["llm"]["semantic_scope"] = semantic_scope(
            &project.project_id,
            &policy,
            &json!({"visible_columns": project.visible_columns,
                "hidden_columns": project.hidden_columns, "source_ref": project.source_ref,
                "allowance_id": project.allowance_id, "sample_policy": project.sample_policy,
                "rules_auto_accept": project.rules_auto_accept,
                "column_review_input_sha256": library_config["column_review_input"]["sha256"]}),
        )?;
    }
    let pid = Arc::new(AtomicU32::new(0));
    let result = run_child(ChildSpec {
        binary: library.python.clone(),
        args: vec![library.script.clone()],
        stdin: serde_json::to_vec(&request).map_err(|_| ApiError::malformed())?,
        deadline: Duration::from_secs(if stage == "S5" { 43_200 } else { 7_200 }),
        needles: Vec::new(),
        cancel,
        pid_out: pid,
    })
    .await;
    std::fs::write(dir.join(format!("{stage}-child.stdout")), &result.stdout)
        .map_err(|_| ApiError::not_ready())?;
    std::fs::write(dir.join(format!("{stage}-child.stderr")), &result.stderr)
        .map_err(|_| ApiError::not_ready())?;
    if result.exit != Some(0) || result.killed || result.secret_leak || !result.child_exited {
        return Err(ApiError::not_ready());
    }
    let receipt: Value =
        serde_json::from_slice(&result.stdout).map_err(|_| ApiError::not_ready())?;
    if receipt["protocol"] != "oo.selfbuild.library-stage/v1"
        || receipt["stage"] != stage
        || receipt["published"] != false
    {
        return Err(ApiError::membership());
    }
    let artifact = receipt["artifact"]
        .as_str()
        .filter(|name| !name.contains('/') && !name.contains('\\') && !name.starts_with('.'))
        .ok_or_else(ApiError::malformed)?;
    let sha = receipt["artifact_sha256"]
        .as_str()
        .ok_or_else(ApiError::malformed)?;
    if !pinned(
        dir.join(artifact)
            .to_str()
            .ok_or_else(ApiError::malformed)?,
        sha,
        project.budgets.max_document_bytes,
    ) {
        return Err(ApiError::membership());
    }
    Ok(receipt)
}

fn semantic_scope(project: &str, policy: &Value, permissions: &Value) -> Result<Value, ApiError> {
    let identifier = |key: &str| {
        policy[key]
            .as_str()
            .filter(|value| crate::ids::ident_ok(value))
            .ok_or_else(ApiError::malformed)
    };
    Ok(json!({
        "project": project, "role": identifier("role")?, "principal": identifier("principal")?,
        "permissions_sha256": canonical_sha256(&json!({"policy":policy,"permissions":permissions}))
    }))
}

#[cfg(test)]
mod semantic_scope_tests {
    use super::*;

    #[test]
    fn scope_binds_policy_and_membership_without_generation_or_credentials() {
        let policy = json!({"role":"oo_r_reader","principal":"reader","datasets":["app.orders"]});
        let scope = semantic_scope("project", &policy, &json!({"hidden_columns":[]})).unwrap();
        assert_eq!(scope["role"], "oo_r_reader");
        for (p, permissions) in [
            (json!({"role":"oo_r_other","principal":"reader"}), json!({"hidden_columns":[]})),
            (policy.clone(), json!({"hidden_columns":["app.orders.phone"]})),
            (policy.clone(), json!({"hidden_columns":[], "column_review_input_sha256":"a".repeat(64)})),
        ] {
            assert_ne!(scope, semantic_scope("project", &p, &permissions).unwrap());
        }
        assert!(semantic_scope("project", &json!({"role":"oo_r_reader"}), &json!({})).is_err());
    }
}

pub async fn run(
    client: &Client,
    project: &ProjectCfg,
    job: &BuildRow,
    stage: &str,
    dir: &Path,
    cancel: &Arc<AtomicBool>,
) -> bool {
    match run_inner(client, project, job, stage, dir, cancel).await {
        Ok(done) => done,
        Err(_) => {
            let _ = store::set_stage(
                client,
                &job.build_id,
                stage,
                "failed",
                &job.input_sha256,
                None,
                Some("library.stage_refused"),
            )
            .await;
            false
        }
    }
}

async fn run_inner(
    client: &Client,
    project: &ProjectCfg,
    job: &BuildRow,
    stage: &str,
    dir: &Path,
    cancel: &Arc<AtomicBool>,
) -> Result<bool, ApiError> {
    if matches!(stage, "S9" | "S10") {
        return store::set_stage(
            client,
            &job.build_id,
            stage,
            "awaiting",
            &job.input_sha256,
            None,
            None,
        )
        .await;
    }
    for checkpoint in store::stage_rows(client, &job.build_id).await? {
        if checkpoint.status == "succeeded" && checkpoint.stage.as_str() < "S9" {
            let receipt = read(
                &dir.join(format!("{}-receipt.json", checkpoint.stage)),
                project.budgets.max_document_bytes,
            )?;
            let artifact = receipt["artifact"]
                .as_str()
                .filter(|s| !s.contains('/') && !s.contains('\\'))
                .ok_or_else(ApiError::membership)?;
            let sha = checkpoint
                .output_sha256
                .as_deref()
                .ok_or_else(ApiError::membership)?;
            if receipt["artifact_sha256"] != sha
                || !pinned(
                    dir.join(artifact)
                        .to_str()
                        .ok_or_else(ApiError::membership)?,
                    sha,
                    project.budgets.max_document_bytes,
                )
            {
                return Err(ApiError::membership());
            }
        }
    }
    let receipt = invoke(project, dir, stage, cancel.clone()).await?;
    if stage == "S6" {
        bind_candidates(client, project, job, dir).await?;
    }
    store::set_stage(
        client,
        &job.build_id,
        stage,
        "succeeded",
        &job.input_sha256,
        receipt["artifact_sha256"].as_str(),
        None,
    )
    .await
}

pub fn projection(facts: &Value) -> Result<(Value, Value), ApiError> {
    let mut tables = Vec::new();
    let mut columns = Vec::new();
    for table in facts["tables"].as_array().ok_or_else(ApiError::malformed)? {
        let dataset = table["dataset"].as_str().ok_or_else(ApiError::malformed)?;
        let (schema, name) = dataset.split_once('.').ok_or_else(ApiError::malformed)?;
        let mut listed = Vec::new();
        for column in table["columns"]
            .as_array()
            .ok_or_else(ApiError::malformed)?
        {
            let key = column["name"].as_str().ok_or_else(ApiError::malformed)?;
            listed.push(json!({"name": key, "data_type": column["data_type"]}));
            let class = &column["classification_candidate"];
            let semantic = if class["semantic_type"] == "money" {
                json!("quantity")
            } else {
                class["semantic_type"].clone()
            };
            columns.push(json!({"column":format!("{dataset}.{key}"), "semantic_type":semantic, "pii":class["pii"], "confidence":"high"}));
        }
        tables.push(json!({"schema":schema,"table":name,"columns":listed}));
    }
    let relationships = facts["relationship_candidates"].as_array().ok_or_else(ApiError::malformed)?.iter().map(|r| json!({
        "name":r["name"], "from_column":format!("{}.{}",r["from"]["dataset"].as_str().unwrap_or(""),r["from"]["columns"][0].as_str().unwrap_or("")),
        "to_column":format!("{}.{}",r["to"]["dataset"].as_str().unwrap_or(""),r["to"]["columns"][0].as_str().unwrap_or("")),
        "cardinality":r["cardinality"],"confidence":if r["confidence"]=="0.99" {"high"} else {"low"}
    })).collect::<Vec<_>>();
    Ok((
        json!({"tables":tables}),
        json!({"columns":columns,"relationships":relationships}),
    ))
}

async fn bind_candidates(
    client: &Client,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
) -> Result<(), ApiError> {
    let facts = read(&dir.join("merged.json"), project.budgets.max_document_bytes)?;
    let (catalog, rules) = projection(&facts)?;
    let (mut model, _) =
        oosyaml::generated_from_inputs(&catalog, &rules, &project.rules_auto_accept)?;
    let source_sha = canonical_sha256(&facts["binding_vector"]);
    let generation_sha = canonical_sha256(&facts);
    let schema_sha = canonical_sha256(&catalog);
    let mut curated = serde_json::Map::new();
    for table in facts["tables"].as_array().ok_or_else(ApiError::malformed)? {
        let dataset = table["dataset"].as_str().ok_or_else(ApiError::malformed)?;
        curated.insert(dataset.to_owned(), json!({"columns":[]}));
        let (schema, name) = dataset.split_once('.').ok_or_else(ApiError::malformed)?;
        for column in std::iter::once(None).chain(
            table["columns"]
                .as_array()
                .ok_or_else(ApiError::malformed)?
                .iter()
                .map(Some),
        ) {
            let key = column.and_then(|c| c["name"].as_str());
            let id = key
                .map(|c| format!("{dataset}.{c}"))
                .unwrap_or_else(|| dataset.to_owned());
            if key.is_some()
                && (project.visible_columns.contains(&id) == project.hidden_columns.contains(&id))
            {
                return Err(ApiError::membership());
            }
            let member = Member {
                project_id: project.project_id.clone(),
                object_id: id,
                object_kind: if key.is_some() { "column" } else { "dataset" }.into(),
                schema_name: schema.into(),
                table_name: name.into(),
                column_name: key.map(str::to_owned),
                candidate_sha256: canonical_sha256(
                    &json!({"table":table,"column":column,"input":job.input_sha256}),
                ),
                source_sha256: source_sha.clone(),
                schema_sha256: schema_sha.clone(),
                evidence_sha256: generation_sha.clone(),
                generation_sha256: generation_sha.clone(),
            };
            store::insert_member(client, &job.build_id, &member).await?;
            if let Some(column) = column
                && let Some(item) = key.and_then(|k| {
                    model
                        .datasets
                        .get_mut(dataset)
                        .and_then(|d| d.columns.get_mut(k))
                })
            {
                item.label = column["label_candidate"].as_str().unwrap_or("").to_owned();
                item.label_from = "generated".into();
            }
        }
    }
    let yaml = oosyaml::canonical_doc(&oosyaml::model_to_y(&model, "generated"))?;
    crate::overlay::insert_generated_rev1(client, &job.build_id, &yaml).await?;
    write(&dir.join("curated.json"), &Value::Object(curated))?;
    write(&dir.join("catalog.json"), &catalog)?;
    write(
        &dir.join("generation.json"),
        &json!({"outcome":"validated","facts_sha256":generation_sha,"business_owner_signed":false}),
    )
}

async fn prepare_review(
    config: &AppConfig,
    project: &ProjectCfg,
    build: &BuildRow,
) -> Result<std::path::PathBuf, ApiError> {
    if !ready(project) || crate::config::project_input_hash(project) != build.input_sha256
        || project.project_id != build.project_id || project.config_commit != build.config_commit
    {
        return Err(ApiError::not_ready());
    }
    let dir = Path::new(&config.work_dir)
        .join("builds")
        .join(&build.build_id);
    let members = store::list_members(&config.postgres, &build.build_id).await?;
    let facts = read(&dir.join("merged.json"), project.budgets.max_document_bytes)?;
    check_member_inputs(&facts, &members, build)?;
    let ours = store::our_decisions(&config.postgres, &build.build_id).await?;
    let ledger = store::connect(&config.ledger).await?;
    let control = store::connect(&config.postgres).await?;
    let decision_rows:Vec<(String,String)>=control.query("SELECT object_id,idempotency_key FROM oo_selfbuild_api.decision_record WHERE build_id=$1 ORDER BY created_at,idempotency_key",&[&build.build_id]).await.map_err(|_|ApiError::not_ready())?.iter().map(|r|(r.get(0),r.get(1))).collect();
    let mut columns = serde_json::Map::new();
    let mut datasets = serde_json::Map::new();
    let model = crate::overlay::effective_model(config, build).await?;
    if !model.stale.is_empty() {
        return Err(ApiError::pack_refused());
    }
    for member in &members {
        let decision_keys = decision_keys_for(&decision_rows, &member.object_id);
        let row=ledger.query_opt("SELECT revision,candidate_sha256,input_sha256,evidence_sha256,schema_sha256,curated_sha256,source_sha256 FROM oo_review_ledger.queue_head WHERE project_id=$1 AND object_id=$2",&[&build.project_id,&member.object_id]).await.map_err(|_|ApiError::not_ready())?.ok_or_else(ApiError::membership)?;
        let head = store::QueueHead {
            revision: row.get(0),
            candidate_sha256: row.get(1),
            input_sha256: row.get(2),
            evidence_sha256: row.get(3),
            schema_sha256: row.get(4),
            curated_sha256: row.get(5),
            source_sha256: row.get(6),
        };
        if head.input_sha256 != build.input_sha256
            || head.candidate_sha256 != member.candidate_sha256
            || head.evidence_sha256 != member.evidence_sha256
            || head.schema_sha256 != member.schema_sha256
            || head.source_sha256 != member.source_sha256
            || head.curated_sha256 != member.generation_sha256
        {
            return Err(ApiError::membership());
        }
        let decisions = ledger.query("SELECT decision_kind, choice, fields_json, reviewer_label FROM oo_review_ledger.decision WHERE project_id=$1 AND object_id=$2 AND revision=$3 AND candidate_sha256=$4 AND input_sha256=$5 AND idempotency_key=ANY($6) AND evidence_sha256=$7 AND schema_sha256=$8 AND curated_sha256=$9 AND source_sha256=$10 ORDER BY array_position($6::text[],idempotency_key)",
            &[&build.project_id,&member.object_id,&head.revision,&member.candidate_sha256,&build.input_sha256,&decision_keys,&member.evidence_sha256,&member.schema_sha256,&member.generation_sha256,&member.source_sha256]).await.map_err(|_| ApiError::not_ready())?;
        let mut latest = BTreeMap::new();
        for decision in decisions {
            latest.insert(
                decision.get::<_, String>(0),
                (
                    decision.get::<_, String>(1),
                    decision.get::<_, String>(2),
                    decision.get::<_, String>(3),
                ),
            );
        }
        let label_choice = latest
            .get("label_description")
            .map(|(choice, _, _)| choice.as_str())
            .ok_or_else(ApiError::membership)?;
        let hidden = project.hidden_columns.contains(&member.object_id);
        let approved = label_choice == "approve";
        if !ours
            .iter()
            .any(|d| d.object_id == member.object_id && d.decision_kind == "label_description")
            || !review_admitted(hidden, label_choice)
        {
            return Err(ApiError::membership());
        }
        if hidden {
            continue;
        }
        let label_fields: Value = serde_json::from_str(
            &latest
                .get("label_description")
                .ok_or_else(ApiError::membership)?
                .1,
        )
        .map_err(|_| ApiError::malformed())?;
        let approved_fields = descriptive_approvals(&label_fields, &member.object_kind);
        if member.object_kind == "dataset" {
            let mut fields = approved_fields.clone();
            fields["reviewer"] = json!(
                latest
                    .get("label_description")
                    .ok_or_else(ApiError::membership)?
                    .2
            );
            datasets.insert(member.object_id.clone(), fields);
        }
        if member.object_kind == "column"
            && project.visible_columns.contains(&member.object_id)
            && !project.hidden_columns.contains(&member.object_id)
        {
            let (choice, fields, reviewer) = latest
                .get("classification")
                .ok_or_else(ApiError::classification_incomplete)?;
            if choice != "confirm"
                || !ours
                    .iter()
                    .any(|d| d.object_id == member.object_id && d.decision_kind == "classification")
            {
                return Err(ApiError::classification_incomplete());
            }
            let fields: Value = serde_json::from_str(fields).map_err(|_| ApiError::malformed())?;
            // Classification alone grants no read access. The shared Python
            // adapter must intersect this with the pinned operator column policy.
            columns.insert(member.object_id.clone(),json!({"pii":fields["pii"],"semantic_type":fields["semantic_type"],"reviewer":reviewer,"label_approved":approved,"approved_fields":approved_fields}));
        }
    }
    let policy = read(
        Path::new(&project.role_policy_file),
        project.budgets.max_document_bytes,
    )?;
    let role = policy["role"]
        .as_str()
        .filter(|s| crate::ids::ident_ok(s))
        .ok_or_else(ApiError::malformed)?;
    let principal = policy["principal"]
        .as_str()
        .filter(|s| crate::ids::ident_ok(s))
        .ok_or_else(ApiError::malformed)?;
    let labels: BTreeMap<String, String> = model
        .datasets
        .iter()
        .flat_map(|(name, t)| {
            t.columns
                .iter()
                .filter(|(_, c)| c.label_from == "curated")
                .map(move |(key, c)| (format!("{name}.{key}"), c.label.clone()))
        })
        .collect();
    let relationships: Vec<String> = if project.rules_auto_accept == "rule_high_confidence" {
        facts["relationships"]
            .as_array()
            .ok_or_else(ApiError::malformed)?
            .iter()
            .filter(|r| measured_relationship_admissible(r))
            .filter_map(|r| r["name"].as_str().map(str::to_owned))
            .collect()
    } else {
        Vec::new()
    };
    write(
        &dir.join("ledger-review.json"),
        &json!({"project":project.project_id,"role":role,"principal":principal,"columns":columns,"datasets":datasets,"hidden_columns":project.hidden_columns,"labels":labels,"relationships":relationships,"reviewer":"rule_high_confidence","review_origin":"review_ledger","business_owner_signed":false}),
    )?;
    invoke(project, &dir, "compile", Arc::new(AtomicBool::new(false))).await?;
    if crate::config::project_input_hash(project) != build.input_sha256 {
        return Err(ApiError::membership());
    }
    Ok(dir)
}

fn check_member_inputs(facts: &Value, members: &[Member], build: &BuildRow) -> Result<(), ApiError> {
    let (catalog, _) = projection(facts)?;
    let generation = canonical_sha256(facts);
    let schema = canonical_sha256(&catalog);
    let source = canonical_sha256(&facts["binding_vector"]);
    let mut expected = BTreeMap::new();
    for table in facts["tables"].as_array().ok_or_else(ApiError::membership)? {
        let dataset = table["dataset"].as_str().ok_or_else(ApiError::membership)?;
        for column in std::iter::once(None).chain(
            table["columns"].as_array().ok_or_else(ApiError::membership)?.iter().map(Some)
        ) {
            let id = match column {
                Some(c) => format!("{dataset}.{}", c["name"].as_str().ok_or_else(ApiError::membership)?),
                None => dataset.to_owned(),
            };
            let hash = canonical_sha256(&json!({"table":table,"column":column,"input":build.input_sha256}));
            if expected.insert(id, hash).is_some() { return Err(ApiError::membership()); }
        }
    }
    if expected.len() != members.len() || members.iter().any(|m|
        m.project_id != build.project_id || expected.get(&m.object_id) != Some(&m.candidate_sha256)
            || m.generation_sha256 != generation || m.evidence_sha256 != generation
            || m.schema_sha256 != schema || m.source_sha256 != source
    ) {
        return Err(ApiError::membership());
    }
    Ok(())
}

pub async fn validate_stored(
    config: &AppConfig, project: &ProjectCfg, build: &BuildRow, producer: &Value,
) -> Result<(), ApiError> {
    // Cached GET and compile replay must not outlive their operator grants or
    // bypass current ledger/input bindings. Reprojection never calls a model.
    let dir = prepare_review(config, project, build).await?;
    for (key, file) in [
        ("effective_sha256", "effective-oos.json"),
        ("visibility_sha256", "visibility.json"),
        ("review_sha256", "effective-review-input.json"),
    ] {
        if producer[key] != canonical_sha256(&read(&dir.join(file), project.budgets.max_document_bytes)?) {
            return Err(ApiError::pack_refused());
        }
    }
    Ok(())
}

pub async fn compile(
    config: &AppConfig,
    project: &ProjectCfg,
    build: &BuildRow,
    subject: &str,
    body_sha: &str,
) -> Result<Value, ApiError> {
    let dir = prepare_review(config, project, build).await?;
    let request = read(
        &dir.join("compiler-request.json"),
        project.budgets.max_document_bytes,
    )?;
    let tree_binding = read(
        &dir.join("oos-tree-binding.json"),
        project.budgets.max_document_bytes,
    )?;
    let tree = oo_builder::compile_oos_tree(
        &dir.join("ontology"),
        &tree_binding.to_string(),
        &request["visibility"].to_string(),
        &project.budgets.role_pack,
    )
    .map_err(|_| ApiError::pack_refused())?;
    if tree.effective_oos != request["effective_oos"] {
        return Err(ApiError::pack_refused());
    }
    let pack = tree.pack;
    let artifact: Value =
        serde_json::from_str(&pack.artifact).map_err(|_| ApiError::pack_refused())?;
    let required = read(&dir.join("required-metrics.json"), project.budgets.max_document_bytes)?;
    require_metrics(&artifact, &required)?;
    let required_assets = read(&dir.join("required-assets.json"), project.budgets.max_document_bytes)?;
    require_assets(&artifact, &required_assets)?;
    let producer = json!({"protocol":"oo.selfbuild.library-pack/v1","business_owner_signed":false,"published":false,"pack_digest":pack.pack_digest,"artifact":artifact,"effective_sha256":canonical_sha256(&request["effective_oos"]),"visibility_sha256":canonical_sha256(&request["visibility"]),"review_sha256":canonical_sha256(&read(&dir.join("effective-review-input.json"),project.budgets.max_document_bytes)?)});
    write(&dir.join("pack.json"), &producer)?;
    let bytes = std::fs::read(dir.join("pack.json")).map_err(|_| ApiError::not_ready())?;
    store::insert_pack(
        &config.postgres,
        build,
        subject,
        &sha256_hex(&bytes),
        None,
        body_sha,
    )
    .await?;
    if !store::finish_reviewed(&config.postgres, build).await? {
        return Err(ApiError::not_ready());
    }
    Ok(crate::pack::envelope(
        "compileBuildPack",
        &build.build_id,
        &producer,
    ))
}

fn require_metrics(artifact: &Value, required: &Value) -> Result<(), ApiError> {
    let names = required.as_array().ok_or_else(ApiError::pack_refused)?;
    if names.len() > 4096 || names.iter().any(|name| !name.as_str().is_some_and(crate::ids::ident_ok)) {
        return Err(ApiError::pack_refused());
    }
    if names.is_empty() {
        return Ok(());
    }
    let files = artifact["files"].as_array().ok_or_else(ApiError::pack_refused)?;
    let body = files.iter().find(|file| file["path"] == "metrics/availability.json")
        .and_then(|file| file["text"].as_str()).ok_or_else(ApiError::pack_refused)?;
    let availability: Value = serde_json::from_str(body).map_err(|_| ApiError::pack_refused())?;
    if availability["protocol"] != "oo.knowledge.metric-availability/v1"
        || !availability["certified_metrics"].as_array()
            .is_some_and(|metrics| names.iter().all(|name| metrics.contains(name)))
    {
        return Err(ApiError::pack_refused());
    }
    Ok(())
}

fn require_assets(artifact: &Value, required: &Value) -> Result<(), ApiError> {
    let collections = required.as_object().ok_or_else(ApiError::pack_refused)?;
    if collections.is_empty() { return Ok(()); }
    let files = artifact["files"].as_array().ok_or_else(ApiError::pack_refused)?;
    let body = files.iter().find(|file| file["path"] == "availability.json")
        .and_then(|file| file["text"].as_str()).ok_or_else(ApiError::pack_refused)?;
    let status: Value = serde_json::from_str(body).map_err(|_| ApiError::pack_refused())?;
    if status["protocol"] != "oo.knowledge.asset-availability/v1" {
        return Err(ApiError::pack_refused());
    }
    for (kind, names) in collections {
        if !["datasets", "relationships", "metrics", "glossary", "examples", "enums", "entities"]
            .contains(&kind.as_str()) {
            return Err(ApiError::pack_refused());
        }
        let names = names.as_array().ok_or_else(ApiError::pack_refused)?;
        if names.is_empty() || names.len() > 32768
            || names.iter().any(|n| n.as_str().is_none_or(|s| s.is_empty() || s.len() > 1024))
            || !status["available"][kind].as_array()
                .is_some_and(|available| names.iter().all(|name| available.contains(name))) {
            return Err(ApiError::pack_refused());
        }
    }
    Ok(())
}

fn review_admitted(hidden: bool, choice: &str) -> bool {
    if hidden {
        matches!(choice, "reject" | "defer")
    } else {
        choice == "approve"
    }
}

fn descriptive_approvals(fields: &Value, object_kind: &str) -> Value {
    let mut approved = json!({
        "label": fields["label"]["overwrite"] == true,
        "description": fields["description"]["overwrite"] == true,
    });
    if object_kind == "dataset" {
        approved["grain"] = json!(fields["grain"]["overwrite"] == true);
    }
    approved
}

fn measured_relationship_admissible(relation: &Value) -> bool {
    relation["evidence"]["parent_unique"] == true
        && relation["evidence"]["row_containment"]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|ratio| (0.98..=1.0).contains(&ratio))
}

#[cfg(test)]
mod relationship_admission_tests {
    use super::*;

    #[test]
    fn automatic_review_requires_the_same_measurement_as_publication() {
        let mut relation = json!({"evidence":{
            "parent_unique":true,"containment":"0.999","row_containment":"0.03"
        }});
        assert!(!measured_relationship_admissible(&relation));
        relation["evidence"]["row_containment"] = json!("0.99");
        assert!(measured_relationship_admissible(&relation));
        relation["evidence"]["parent_unique"] = json!(false);
        assert!(!measured_relationship_admissible(&relation));
        assert!(!measured_relationship_admissible(&json!({"confidence":"0.99"})));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn required_metrics_use_compiled_role_visibility_not_input_certification() {
        let artifact = |certified: serde_json::Value| serde_json::json!({"files":[{
            "path":"metrics/availability.json",
            "text":serde_json::json!({
                "protocol":"oo.knowledge.metric-availability/v1",
                "certified_metrics":certified, "candidate_metrics":["shipment_weight"]
            }).to_string()
        }]});
        let required = serde_json::json!(["shipment_weight"]);
        assert!(super::require_metrics(&artifact(serde_json::json!(["shipment_weight"])), &required).is_ok());
        assert!(super::require_metrics(&artifact(serde_json::json!([])), &required).is_err());
        assert!(super::require_metrics(&serde_json::json!({"files":[]}), &required).is_err());
        assert!(super::require_metrics(&artifact(serde_json::json!([])), &serde_json::json!([])).is_ok());
        assert!(super::require_metrics(&artifact(serde_json::json!([])), &serde_json::json!(["bad.name"])).is_err());
    }

    #[test]
    fn grain_requires_its_own_bound_dataset_approval() {
        let fields = serde_json::json!({
            "label": {"overwrite": true},
            "description": {"overwrite": true},
            "grain": {"overwrite": false}
        });
        assert_eq!(super::descriptive_approvals(&fields, "dataset")["grain"], false);
        let mut approved = fields;
        approved["grain"]["overwrite"] = serde_json::json!(true);
        assert_eq!(super::descriptive_approvals(&approved, "dataset")["grain"], true);
        assert!(super::descriptive_approvals(&approved, "column").get("grain").is_none());
        assert_eq!(super::descriptive_approvals(&serde_json::json!({}), "dataset")["grain"], false);
    }

    #[test]
    fn complete_assets_gate_uses_compiled_role_inventory() {
        let artifact = serde_json::json!({"files":[{"path":"availability.json","text":
            serde_json::json!({"protocol":"oo.knowledge.asset-availability/v1",
                "available":{"metrics":["receipts"],"examples":["checked"],"enums":[]}}).to_string()}]});
        assert!(super::require_assets(&artifact, &serde_json::json!({"metrics":["receipts"],"examples":["checked"]})).is_ok());
        assert!(super::require_assets(&artifact, &serde_json::json!({"enums":["unexplained"]})).is_err());
        assert!(super::require_assets(&artifact, &serde_json::json!({"unknown":["anything"]})).is_err());
        assert!(super::require_assets(&artifact, &serde_json::json!({"examples":[]})).is_err());
    }

    #[test]
    fn hidden_rejections_do_not_block_the_visible_library_pack() {
        assert!(super::review_admitted(true, "reject"));
        assert!(super::review_admitted(true, "defer"));
        assert!(!super::review_admitted(true, "approve"));
        assert!(super::review_admitted(false, "approve"));
        assert!(!super::review_admitted(false, "defer"));
    }
}

#[cfg(test)]
mod compile_decision_scope_tests {
    #[test]
    fn only_this_objects_build_decision_keys_are_forwarded_to_the_ledger() {
        let rows = vec![
            ("app.a.id".to_owned(), "key-a".to_owned()),
            ("app.b.id".to_owned(), "key-b".to_owned()),
            ("app.a.id".to_owned(), "class-a".to_owned()),
        ];
        assert_eq!(
            super::decision_keys_for(&rows, "app.a.id"),
            vec!["key-a".to_owned(), "class-a".to_owned()]
        );
        assert!(super::decision_keys_for(&rows, "app.unknown.id").is_empty());
    }
}

fn decision_keys_for(rows: &[(String, String)], object: &str) -> Vec<String> {
    rows.iter()
        .filter(|(id, _)| id == object)
        .map(|(_, key)| key.clone())
        .collect()
}
