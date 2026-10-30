//! One publish attempt: applicability, measured gates, fence, then the existing CAS.

use serde_json::{Value, json};

use crate::apply::{self, Measured};
use crate::error::{Phase, StoreError, query_error};
use crate::sql::{self, ADMIN_ROLE, MAINTAIN_LOCK};
use crate::store::{
    self, CheckpointRequest, Endpoint, GateRequest, PlanRequest, Receipt, RecoverRequest, begin,
    checkpoint, commit, gate_record, open, plan, record_server_gate, recover, refused, require_tls,
    rollback, status,
};
use crate::target::{self, FenceRef, TargetConn};

#[path = "data_only.rs"]
mod data_only;
pub use data_only::{DataOnlyParent, maintain_data_only};

#[derive(Clone, Copy)]
pub struct ControlConn<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
}

pub struct MaintainRequest<'a> {
    pub quality_observer: Option<TargetConn<'a>>,
    pub deployment_mode: crate::DeploymentMode,
    pub control: ControlConn<'a>,
    pub target: TargetConn<'a>,
    pub replica: Option<TargetConn<'a>>,
    pub reader_user: &'a str,
    pub reader_password: &'a str,
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub source_hash: &'a str,
    pub expected_version: Option<&'a str>,
    pub schema_name: &'a str,
    pub relation_name: &'a str,
    pub role_name: &'a str,
    pub native_mapping: Option<&'a Value>,
}

struct CandidateRow {
    state: String,
    source_hash: String,
    pack_digest: String,
    version: String,
    effective: Value,
    visibility: Value,
}

struct PointerRow {
    version: Option<String>,
    candidate_id: Option<String>,
    pack_digest: Option<String>,
}

pub fn maintenance_sha256() -> String {
    crate::sha256::sha256_hex(sql::MAINTENANCE_SQL.as_bytes())
}

pub async fn maintain_install(
    conn: &ControlConn<'_>,
    project_id: &str,
) -> Result<Receipt, StoreError> {
    if conn.user != ADMIN_ROLE || !crate::seal::token_ok(project_id) {
        return Err(StoreError::Input);
    }
    let mut session = open(conn.endpoint, conn.user, conn.password, conn.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match install_body(&tx, project_id).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn maintain_activate(request: &MaintainRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request)?;
    let mut session = open(
        request.control.endpoint,
        request.control.user,
        request.control.password,
        request.control.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    session
        .client
        .query_one(sql::LOCK_SESSION, &[&MAINTAIN_LOCK])
        .await
        .map_err(query_error)?;
    let result = activate_bound(&mut session, request).await;
    let _ = session
        .client
        .query_one(sql::UNLOCK_SESSION, &[&MAINTAIN_LOCK])
        .await;
    result
}

pub async fn maintain_recover(request: &MaintainRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request)?;
    let mut session = open(
        request.control.endpoint,
        request.control.user,
        request.control.password,
        request.control.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    session
        .client
        .query_one(sql::LOCK_SESSION, &[&MAINTAIN_LOCK])
        .await
        .map_err(query_error)?;
    let result = recover_bound(&mut session, request).await;
    let _ = session
        .client
        .query_one(sql::UNLOCK_SESSION, &[&MAINTAIN_LOCK])
        .await;
    result
}

async fn install_body(
    tx: &tokio_postgres::Transaction<'_>,
    project_id: &str,
) -> Result<Receipt, StoreError> {
    let deployment = tx
        .query_opt(
            "SELECT project_id FROM oo_release_control.deployment WHERE singleton",
            &[],
        )
        .await
        .map_err(query_error)?;
    let Some(deployment) = deployment else {
        return Ok(refused(
            "activation",
            json!({"check": "deployment", "database_used": true}),
        ));
    };
    let project: String = deployment
        .try_get(0)
        .map_err(query_error)?;
    if project != project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let hash = maintenance_sha256();
    let exists = tx
        .query_opt(sql::MAINTENANCE_TABLE, &[])
        .await
        .map_err(query_error)?
        .is_some();
    if !exists {
        tx.batch_execute(sql::MAINTENANCE_SQL)
            .await
            .map_err(query_error)?;
        tx.execute(sql::INSERT_MAINTENANCE_HASH, &[&hash])
            .await
            .map_err(query_error)?;
        return Ok(store::stamped(
            "installed",
            None,
            json!({
                "sql_sha256": hash,
                "compiled_pack": "not_published",
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    let stored: String = tx
        .query_one(sql::READ_MAINTENANCE_HASH, &[])
        .await
        .map_err(query_error)?
        .try_get(0)
        .map_err(query_error)?;
    if stored != hash {
        return Ok(refused(
            "install_drift",
            json!({"check": "maintenance_hash", "database_used": true}),
        ));
    }
    Ok(store::stamped(
        "replay",
        None,
        json!({
            "sql_sha256": stored,
            "compiled_pack": "not_published",
            "database_used": true,
        }),
        "not_issued",
    ))
}

async fn activate_locked(
    session: &mut store::Session,
    request: &MaintainRequest<'_>,
) -> Result<Receipt, StoreError> {
    let installed = maintain_install(&request.control, request.project_id).await?;
    if installed.code != 0 {
        return Ok(installed);
    }
    let pointer = read_pointer(session).await.map_err(|e|e.at(Phase::ReadPointer))?;
    if pointer.candidate_id.as_deref() == Some(request.candidate_id) {
        return finish_from_status(session, request, &pointer).await;
    }
    if pointer.version.as_deref() != request.expected_version {
        return Ok(refused(
            "cas_conflict",
            json!({
                "pointer_version": pointer.version,
                "fence_touched": false,
                "database_used": true,
            }),
        ));
    }
    let candidate = read_candidate(session, request.candidate_id).await.map_err(|e|e.at(Phase::ReadCandidate))?;
    let Some(candidate) = candidate else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if candidate.source_hash != request.source_hash {
        return Ok(refused("stale", json!({})));
    }
    let logical_bindings =
        installed_visibility_bindings(request, &candidate.visibility).map_err(|_| StoreError::Input)?;
    let validated_mapping=request.native_mapping.map(target::ValidatedNativeContext::new).transpose()?;
    let publication=validated_mapping.as_ref().and_then(target::ValidatedNativeContext::publication);
    let library_mapping=publication.is_some_and(|p|crate::native_library::is_library(&p.receipt));
    let resolved = std::sync::Mutex::new(std::collections::BTreeMap::new());
    binding_work(&logical_bindings, if library_mapping { 8 } else { 1 }, |b| async {
        let physical = resolve_validated_binding(&request.target, validated_mapping.as_ref(), b).await?;
        let key = (b.0.clone(), b.1.clone(), b.2.clone());
        if resolved.lock().map_err(|_| StoreError::Input)?.insert(key, physical).is_some() {
            return Err(StoreError::Input);
        }
        Ok(None)
    }).await?;
    let mut resolved = resolved.into_inner().map_err(|_| StoreError::Input)?;
    // Keep exact logical/physical alignment regardless of completion order.
    let bindings = logical_bindings.iter().map(|b|
        resolved.remove(&(b.0.clone(), b.1.clone(), b.2.clone())).ok_or(StoreError::Input)
    ).collect::<Result<Vec<_>, _>>()?;
    if publication.as_ref().is_some_and(|p|p.receipt.get("run_ids").is_none() && !crate::native_library::is_library(&p.receipt)) && bindings.len() != 1 {
        return refuse_before_fence(request, "native_publication_requires_atomic_single_fence")
            .await;
    }
    let (schema, relation, role, _columns) = match selected_binding(&candidate.visibility, request)
    {
        Ok(binding) => binding,
        Err(_) => return Ok(refused("visibility", json!({}))),
    };
    if schema != request.schema_name
        || relation != request.relation_name
        || role != request.role_name
    {
        return Ok(refused("target_binding", json!({})));
    }
    let mut preconditions = match apply::preconditions(&candidate.effective) {
        Ok(rows) => rows,
        Err(_) => return Ok(refused("effective", json!({}))),
    };
    write_preconditions(
        session,
        request.candidate_id,
        &candidate.source_hash,
        &preconditions,
    )
    .await?;
    if request.target.endpoint == request.control.endpoint
        && (request.native_mapping.is_some() || logical_bindings == bindings)
        && let Some(proof) = crate::business_positive::preflight(
            session, request.candidate_id, &candidate.source_hash, &candidate.effective, &candidate.visibility,
        ).await?
    {
        for row in &mut preconditions {
            if proof.matches_native(request.native_mapping) && row.disposition == "blocked" && proof.covers(row.gate_id) {
                row.disposition = "admissible";
                row.detail = "authenticated_test_business_receipt";
            }
        }
    }
    if preconditions.iter().any(|row| row.disposition == "blocked") {
        return refuse_before_fence(request, "blocked").await;
    }
    if blocking_external(session, request.candidate_id)
        .await?
        .is_some()
    {
        return refuse_before_fence(request, "gate_failed").await;
    }
    let mut types = Vec::new();
    for (logical, physical) in logical_bindings.iter().zip(&bindings) {
        let actual = if publication.as_ref().is_some_and(|p|crate::native_library::is_library(&p.receipt)) {target::library_column_types(&request.target, &physical.0, &physical.1).await?} else {target::column_types(&request.target, &physical.0, &physical.1).await?};
        for (logical_column, physical_column) in logical.3.iter().zip(&physical.3) {
            if let Some((_, typ)) = actual.iter().find(|(name, _)| name == physical_column) {
                types.push((
                    format!("{}.{}.{}", logical.0, logical.1, logical_column),
                    typ.clone(),
                ));
            }
        }
    }
    bind_mapping(session, request).await?;
    let lengths = read_lengths(session, request.candidate_id).await?;
    let authority = read_authority(session).await?;
    let compiled = crate::seal::compiled_pack_with_ceilings(&candidate.effective, &candidate.visibility, request.control.endpoint.pack_ceilings.as_ref().unwrap_or(&oo_builder::RolePackCeilings::default()));
    if let Ok(pack) = &compiled {
        match pack.accessible_pack_digest() {
            Ok(digest) if digest == candidate.pack_digest => {}
            _ => return refuse_before_fence(request, "pack_mismatch").await,
        }
    }
    let compiled_pairs: Vec<(&str, &str)> = compiled
        .as_ref()
        .map(|pack| {
            pack.files
                .iter()
                .map(|file| (file.path.as_str(), file.text.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let compiled_files = compiled.is_ok().then_some(compiled_pairs.as_slice());
    let semantic = if publication.as_ref().is_some_and(|p|crate::native_library::is_library(&p.receipt)) {
        let maximum=request.control.endpoint.pack_ceilings.as_ref()
            .map_or_else(||oo_builder::RolePackCeilings::default().max_relationships,|c|c.max_relationships);
        Some(target::library_semantic_observations(request.quality_observer.as_ref().unwrap_or(&request.target),&candidate.effective,&logical_bindings,&bindings,maximum).await?)
    } else {None};
    let (g3,g4)=semantic.unwrap_or_else(||(apply::measure_g3(&candidate.effective),apply::measure_g4(&candidate.effective)));
    // Measurement uses the installed ceiling, without changing the staged,
    // hashed visibility document or its column permissions.
    let mut measured_visibility = candidate.visibility.clone();
    if let Some(ceilings) = &request.control.endpoint.pack_ceilings {
        measured_visibility["limits"]["max_datasets"] = json!(ceilings.max_datasets);
    }
    let measured = vec![
        apply::measure_g1(&candidate.effective),
        apply::measure_g2(&candidate.effective, &measured_visibility, &types),
        g3,
        g4,
        apply::measure_g9(
            &candidate.effective,
            &candidate.visibility,
            &lengths,
            compiled_files,
        ),
        apply::measure_g13(&candidate.effective, &measured_visibility),
    ];
    for item in &measured {
        let receipt = record_measured(request, &authority, item).await?;
        if let Some(refusal)=activation_gate_refusal(request.candidate_id,item,receipt,item.blocking) {
            return Ok(refusal);
        }
    }
    let g8 = Measured {
        gate_id: "G8",
        status: "not_run",
        blocking: false,
        detail: String::new(),
    };
    let g8_receipt = record_component(request, &g8).await?;
    if let Some(refusal)=activation_gate_refusal(request.candidate_id,&g8,g8_receipt,true) {
        return Ok(refusal);
    }
    // Diagnose immutable gate failures before retiring the currently readable cut.
    // Fence/replica checkpoints are still verified by the final plan after preparation.
    if let Some(refusal) = preflight_gate_refusal(
        request.candidate_id, status(&scope_request(request)).await?,
    )? {
        return Ok(refusal);
    }
    if let Some(previous_id) = pointer.candidate_id.as_deref()
        && let Some(previous) = read_candidate(session, previous_id).await.map_err(|e|e.at(Phase::PreviousCandidate))?
    {
        let previous_mapping=known_mapping(session,previous_id).await.map_err(|e|e.at(Phase::PreviousBinding))?;
        let previous_context=previous_mapping.as_ref().filter(|v|!v.is_null())
            .map(target::ValidatedNativeContext::new).transpose().map_err(|e|e.at(Phase::PreviousBinding))?;
        for old in
            installed_visibility_bindings(request, &previous.visibility).map_err(|_| StoreError::Input)?
        {
            let old = resolve_validated_binding(&request.target, previous_context.as_ref(), &old)
                .await.map_err(|e|e.at(Phase::PreviousBinding))?;
            if !bindings
                .iter()
                .any(|b| b.0 == old.0 && b.1 == old.1 && b.2 == old.2)
            {
                let f = FenceRef {
                    schema_name: &old.0,
                    relation_name: &old.1,
                    role_name: &old.2,
                };
                let retired = target::retire_target(&request.target, &f, &candidate.version)
                    .await.map_err(|e|e.at(Phase::RetirePrevious))?;
                if retired.code != 0 {
                    return Ok(retired);
                }
            }
        }
    }
    let library = publication.as_ref().is_some_and(|p| crate::native_library::is_library(&p.receipt));
    // Bootstrap shared installation state once, before concurrent table DDL.
    if let Some(first) = bindings.first() {
        if let Some(refused) = prepare_binding(request, first, library).await.map_err(|e|e.at(Phase::PrepareTarget))? {
            return Ok(refused);
        }
    }
    // Native views lock shared underlying relations. Parallel DDL can deadlock
    // even when the logical table names differ; data capture has its own pool.
    let parallelism = 1;
    // Finish closing every table before granting any candidate permissions.
    if let Some(refused) = binding_work(
        &bindings[bindings.len().min(1)..], parallelism,
        |b| prepare_binding(request, b, library),
    ).await.map_err(|e|e.at(Phase::PrepareTarget))? {
        return Ok(refused);
    }
    match binding_work(&bindings, parallelism, |b| grant_and_observe_binding(request, b)).await {
        Ok(Some(refused)) => {
            revoke_all(request, &bindings).await?;
            return Ok(refused);
        }
        Err(error) => {
            revoke_all(request, &bindings).await?;
            return Err(error);
        }
        Ok(None) => {}
    }
    if request.deployment_mode == crate::DeploymentMode::AllInOne || publication.as_ref().is_some_and(|p|crate::native_library::is_library(&p.receipt)) {
        target::install_visibility_reader(&request.target, request.reader_user).await?;
    }
    let epoch = format!("e{}", &candidate.source_hash[..16]);
    let mut replica_observation = if let Some(replica) = &request.replica {
        match target::replica_barrier(&request.target, replica).await {
            Ok(observation) => Some(observation),
            Err(err) => { revoke_all(request, &bindings).await?; return Err(err); }
        }
    } else { None };
    let replica_state = if replica_observation.is_some() { "replayed" } else { "missing" };
    for (kind, barrier) in [("fence", "closed"), ("replica", replica_state)] {
        let receipt = record_checkpoint(request, &authority, kind, barrier, &epoch).await?;
        let disposition = receipt
            .body
            .get("disposition")
            .and_then(Value::as_str)
            .unwrap_or("");
        let replay = receipt.body.get("outcome").and_then(Value::as_str) == Some("replay");
        let accepted = receipt.code == 0
            && (replay
                || (kind == "fence" && disposition == "fresh")
                || (kind == "replica" && disposition == if replica_state == "replayed" { "fresh" } else { "not_fresh" }));
        if !accepted {
            revoke_all(request, &bindings).await?;
            return Ok(receipt);
        }
    }
    let planned = plan(&scope_request(request)).await?;
    if planned.code != 0 {
        revoke_all(request, &bindings).await?;
        return Ok(planned);
    }
    let entered = store::enter_applying(&scope_request(request)).await?;
    if entered.code != 0 {
        revoke_all(request, &bindings).await?;
        return Ok(entered);
    }
    let committed = store::commit_activation(&scope_request(request)).await?;
    if committed.code != 0 {
        revoke_all(request, &bindings).await?;
        return Ok(committed);
    }
    let mut opened_rows = Vec::new();
    if let Some(publication)=publication.as_ref().filter(|p|p.receipt.get("run_ids").is_some() || crate::native_library::is_library(&p.receipt)) {
        match target::open_native_collection(&request.target,&bindings,&candidate.pack_digest,&candidate.version,request.candidate_id,publication).await {
            Ok(rows) if rows.iter().all(|r|r.code==0)=>opened_rows=rows,
            result=>{
                if crate::native_library::is_library(&publication.receipt) {target::library_state(&request.target,publication,false).await?;}
                revoke_all(request,&bindings).await?;
                let (schema,relation,role,_)=&bindings[0];
                let fence=FenceRef {schema_name:schema,relation_name:relation,role_name:role};
                let reason=match &result {Ok(_)=>"open_failed",Err(error)=>error.reason()};
                return close_after_open_failure(request,&fence,&candidate.version,reason).await;
            }
        }
    } else {
    for (schema, relation, role, columns) in &bindings {
        let f = FenceRef {
            schema_name: schema,
            relation_name: relation,
            role_name: role,
        };
        let opened = if let Some(publication) = publication.as_ref() {
            target::open_native_target(
                &request.target,
                &f,
                columns,
                &candidate.pack_digest,
                &candidate.version,
                request.candidate_id,
                publication,
            )
            .await
        } else {
            target::open_target(
                &request.target,
                &f,
                columns,
                &candidate.pack_digest,
                &candidate.version,
                request.candidate_id,
            )
            .await
        };
        match opened {
            Ok(receipt) if receipt.code == 0 => opened_rows.push(receipt),
            Ok(receipt) => {
                revoke_all(request, &bindings).await?;
                return close_after_open_failure(
                    request,
                    &f,
                    &candidate.version,
                    receipt.body["reason"].as_str().unwrap_or("open_failed"),
                )
                .await;
            }
            Err(err) => {
                revoke_all(request, &bindings).await?;
                return close_after_open_failure(
                    request,
                    &f,
                    &candidate.version,
                    match &err {
                        StoreError::Query {
                            sqlstate: Some(code),
                        } => code.as_str(),
                        other => other.reason(),
                    },
                )
                .await;
            }
        }
    }
    }
    let opened = aggregate_opened(opened_rows);
    if let Some(replica)=&request.replica {
        let after=match target::replica_barrier(&request.target,replica).await {
            Ok(observation)=>observation,
            Err(err)=>{
                if publication.as_ref().is_some_and(|p|crate::native_library::is_library(&p.receipt)) {return visibility_failure(request,&bindings,&candidate,&err).await;}
                revoke_all(request,&bindings).await?;return Err(err);
            }
        };
        replica_observation=Some(json!({"before_activation":replica_observation,"after_open":after}));
    }
    let visibility_observation = if request.deployment_mode == crate::DeploymentMode::AllInOne || publication.as_ref().is_some_and(|p|crate::native_library::is_library(&p.receipt)) {
        let observation_target=request.replica.as_ref().unwrap_or(&request.target);
        let expected=target::VisibilityRef { bindings: &bindings, version: &candidate.version, pack: &candidate.pack_digest, candidate: request.candidate_id, mapping: request.native_mapping };
        let observed=if let Some(publication)=publication.as_ref().filter(|p|crate::native_library::is_library(&p.receipt)) {
            target::verify_library_visibility(&request.target,observation_target,request.reader_user,request.reader_password,expected,publication,request.replica.is_some()).await
        } else if request.replica.is_some() {
            target::replica_visibility_barrier(observation_target,request.reader_user,request.reader_password,expected).await
        } else { target::primary_visibility_barrier(observation_target,request.reader_user,request.reader_password,expected).await };
        match observed {
            Ok(observation) => Some(observation),
            Err(error) => {
                return visibility_failure(request, &bindings, &candidate, &error).await;
            }
        }
    } else { None };
    if let Some(publication)=publication.as_ref() {
        if let Err(error)=target::library_state(&request.target,publication,true).await {
            return visibility_failure(request,&bindings,&candidate,&error).await;
        }
    }
    if open_matches(&opened) {
        let g7 = Measured {
            gate_id: "G7",
            status: "pass",
            blocking: false,
            detail: if let Some(observation)=&replica_observation {
                format!("open_grant_matches_pointer_columns;replica={}",crate::canon::canonical(observation))
            } else { "open_grant_matches_pointer_columns".to_owned() },
        };
        let g7_receipt = record_server_gate(
            &gate_request(request, &authority, &g7),
            &g7.detail,
        )
        .await?;
        if let Some(refusal)=activation_gate_refusal(request.candidate_id,&g7,g7_receipt,true) {
            if let Some(publication)=publication.as_ref() {target::library_state(&request.target,publication,false).await?;}
            return Ok(refusal);
        }
    }
    let mut finished = finish(request, &opened, &preconditions).await?;
    if let Some(observation) = replica_observation { finished.body["replica_observation"] = observation; }
    if let Some(observation) = visibility_observation {
        finished.body["deployment_mode"] = json!(if request.deployment_mode==crate::DeploymentMode::AllInOne {"all_in_one"} else {"primary_replica"});
        finished.body["readonly_replica"] = json!(request.replica.is_some());
        finished.body["visibility_observation"] = observation;
    }
    Ok(finished)
}

fn activation_gate_refusal(candidate:&str,gate:&Measured,recorded:Receipt,require_admissible:bool)->Option<Receipt> {
    if recorded.code==0 && (!require_admissible || recorded.body.get("test_admissible")==Some(&Value::Bool(true))) {
        return None;
    }
    Some(refused("gate_failed",json!({
        "candidate_id":candidate,"gate_id":gate.gate_id,"gate_status":gate.status,
        "blocking":gate.blocking,"detail":gate.detail,"activation_completed":false,
        "gate_record_code":recorded.code,"gate_receipt":recorded.body,"database_used":true
    })))
}

fn preflight_gate_refusal(candidate: &str, observed: Receipt) -> Result<Option<Receipt>, StoreError> {
    if observed.code != 0 {
        return Ok(Some(observed));
    }
    let reasons = observed.body["test_reasons"].as_array().ok_or(StoreError::Input)?;
    let mut blocked = Vec::new();
    for reason in reasons {
        let reason = reason.as_str().ok_or(StoreError::Input)?;
        if !matches!(reason, "missing_fence" | "stale_fence" | "missing_replica" | "stale_replica") {
            blocked.push(reason);
        }
    }
    if blocked.is_empty() {
        return Ok(None);
    }
    Ok(Some(refused("gate_preflight_not_ready", json!({
        "candidate_id":candidate,"reasons":blocked,"gates":observed.body["gates"],
        "fence_touched":false,"activation_completed":false,"database_used":true
    }))))
}

async fn recover_locked(
    session: &mut store::Session,
    request: &MaintainRequest<'_>,
) -> Result<Receipt, StoreError> {
    let candidate = read_candidate(session, request.candidate_id).await?;
    if let Some(candidate) = candidate.as_ref()
        && candidate.state == "applying"
    {
        let receipt = recover(&RecoverRequest {
            endpoint: request.control.endpoint,
            user: request.control.user,
            password: request.control.password,
            ca_pem: request.control.ca_pem,
            action: "recover",
            project_id: request.project_id,
            candidate_id: request.candidate_id,
            expected_version: None,
        })
        .await?;
        if receipt.code != 0 {
            return Ok(receipt);
        }
    }
    let pointer = read_pointer(session).await?;
    let lookup_candidate = if let Some(id) = pointer.candidate_id.as_deref() {
        read_candidate(session, id).await?
    } else {
        None
    };
    let logical = lookup_candidate
        .as_ref()
        .and_then(|c| selected_binding(&c.visibility, request).ok())
        .or_else(|| {
            candidate
                .as_ref()
                .and_then(|c| selected_binding(&c.visibility, request).ok())
        })
        .ok_or(StoreError::Input)?;
    let binding_id = if lookup_candidate
        .as_ref()
        .is_some_and(|c| selected_binding(&c.visibility, request).is_ok())
    {
        pointer.candidate_id.as_deref().ok_or(StoreError::Input)?
    } else {
        request.candidate_id
    };
    let physical = resolve_candidate_binding(session, request, binding_id, &logical).await?;
    let fence = FenceRef {
        schema_name: &physical.0,
        relation_name: &physical.1,
        role_name: &physical.2,
    };
    let visible = pointer_columns(session, &pointer, request).await?;
    let current = target::status_target(
        &request.target,
        &fence,
        pointer.pack_digest.as_deref(),
        visible.as_deref(),
    )
    .await?;
    if current.code != 0 {
        return Ok(current);
    }
    let state = current
        .body
        .get("fence_state")
        .and_then(Value::as_str)
        .unwrap_or("");
    let opened_pack = current
        .body
        .get("opened_pack_digest")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if state == "granted" {
        if pointer.candidate_id.as_deref() == Some(request.candidate_id) {
            let Some(candidate) = read_candidate(session, request.candidate_id).await? else {
                return Ok(refused("activation", json!({"check": "candidate"})));
            };
            if candidate.state == "active" {
                let logical = selected_binding(&candidate.visibility, request)
                    .map_err(|_| StoreError::Input)?;
                let (_, _, _, columns) =
                    resolve_candidate_binding(session, request, request.candidate_id, &logical)
                        .await?;
                return target::open_target(
                    &request.target,
                    &fence,
                    &columns,
                    &candidate.pack_digest,
                    &candidate.version,
                    request.candidate_id,
                )
                .await;
            }
        }
        return target::revoke_target(&request.target, &fence).await;
    }
    if state == "open" && opened_pack.as_deref() != pointer.pack_digest.as_deref() {
        return reopen_pointer(session, request, &fence, &pointer).await;
    }
    Ok(current)
}

async fn reopen_pointer(
    session: &mut store::Session,
    request: &MaintainRequest<'_>,
    fence: &FenceRef<'_>,
    pointer: &PointerRow,
) -> Result<Receipt, StoreError> {
    let Some(candidate_id) = pointer.candidate_id.clone() else {
        return target::close_target(&request.target, fence).await;
    };
    let Some(candidate) = read_candidate(session, &candidate_id).await? else {
        return Ok(refused("activation", json!({"check": "pointer"})));
    };
    let Ok(logical) = selected_binding(&candidate.visibility, request) else {
        return target::revoke_target(&request.target, fence).await;
    };
    let (_schema, _relation, _role, columns) =
        resolve_candidate_binding(session, request, &candidate_id, &logical).await?;
    target::open_target(
        &request.target,
        fence,
        &columns,
        &candidate.pack_digest,
        &candidate.version,
        &candidate_id,
    )
    .await
}

async fn finish(
    request: &MaintainRequest<'_>,
    opened: &Receipt,
    preconditions: &[apply::Precondition],
) -> Result<Receipt, StoreError> {
    let current = status(&scope_request(request)).await?;
    if current.code != 0 {
        return Ok(current);
    }
    let mut body = current.body;
    body["outcome"] = json!("activated");
    body["reason"] = Value::Null;
    body["fence_state"] = opened
        .body
        .get("fence_state")
        .cloned()
        .unwrap_or(Value::Null);
    body["opened_columns"] = opened
        .body
        .get("opened_columns")
        .cloned()
        .unwrap_or(json!([]));
    body["revoked_columns"] = opened
        .body
        .get("revoked_columns")
        .cloned()
        .unwrap_or(json!([]));
    body["compiled_pack"] = json!("not_published");
    body["datasets"] = opened.body["datasets"].clone();
    if let Some(publication)=opened.body.get("native_publication") {
        body["native_publication"]=publication.clone();
        if publication.get("library_id").is_some() {body["native_publication"]["state"]=json!("ready");body["native_publication"]["ready"]=json!(true);}
    }
    copy_publication(&mut body, opened);
    body["streams_stopped"] = opened
        .body
        .get("streams_stopped")
        .cloned()
        .unwrap_or(json!(0));
    body["preconditions"] = json!(
        preconditions
            .iter()
            .map(|row| json!({
                "gate_id": row.gate_id,
                "disposition": row.disposition,
                "detail": row.detail,
            }))
            .collect::<Vec<_>>()
    );
    Ok(Receipt { code: 0, body })
}

async fn finish_from_status(
    session: &mut store::Session,
    request: &MaintainRequest<'_>,
    pointer: &PointerRow,
) -> Result<Receipt, StoreError> {
    let mut rows = Vec::new();
    let mut bindings = Vec::new();
    if let Some(id) = pointer.candidate_id.as_deref() {
        let candidate = read_candidate(session, id)
            .await?
            .ok_or(StoreError::Input)?;
        let mapping=known_mapping(session,id).await?;
        let context=mapping.as_ref().filter(|v|!v.is_null())
            .map(target::ValidatedNativeContext::new).transpose()?;
        for (schema, relation, role, columns) in
            installed_visibility_bindings(request, &candidate.visibility).map_err(|_| StoreError::Input)?
        {
            let logical = (schema, relation, role, columns);
            let (schema, relation, role, columns) =
                resolve_validated_binding(&request.target, context.as_ref(), &logical).await?;
            let f = FenceRef {
                schema_name: &schema,
                relation_name: &relation,
                role_name: &role,
            };
            let row = target::status_target(
                &request.target,
                &f,
                pointer.pack_digest.as_deref(),
                Some(&columns),
            )
            .await?;
            if row.code != 0 {
                return Ok(row);
            }
            bindings.push((schema, relation, role, columns));
            rows.push(row);
        }
    }
    let library_publication=request.native_mapping.map(crate::native_publication::from_context).transpose()?.flatten().filter(|p|crate::native_library::is_library(&p.receipt));
    let observation = if request.deployment_mode == crate::DeploymentMode::AllInOne || library_publication.is_some() {
        let candidate = read_candidate(session, request.candidate_id).await?.ok_or(StoreError::Input)?;
        let expected=target::VisibilityRef { bindings: &bindings, version: &candidate.version, pack: &candidate.pack_digest, candidate: request.candidate_id, mapping: request.native_mapping };
        let result=if let Some(publication)=library_publication.as_ref() {
            if let Some(replica)=request.replica.as_ref() {
                if let Err(error)=target::replica_barrier(&request.target,replica).await {return visibility_failure(request,&bindings,&candidate,&error).await;}
            }
            target::verify_library_visibility(&request.target,request.replica.as_ref().unwrap_or(&request.target),request.reader_user,request.reader_password,expected,publication,request.replica.is_some()).await
        } else {target::primary_visibility_barrier(&request.target,request.reader_user,request.reader_password,expected).await};
        match result {Ok(observation)=>Some(observation),Err(error)=>return visibility_failure(request,&bindings,&candidate,&error).await}
    } else {None};
    let mut opened = aggregate_opened(rows);
    if let Some(mapping)=request.native_mapping {
        if let Some(publication)=crate::native_publication::from_context(mapping)? {
            if crate::native_library::is_library(&publication.receipt) {
                opened.body["native_publication"]=json!({"library_id":publication.receipt["library"]["library_id"],"receipt_sha256":publication.receipt_sha256,"outcome":"replay"});
            }
        }
    }
    let current = status(&scope_request(request)).await?;
    if current.code != 0 {
        return Ok(current);
    }
    let mut body = current.body;
    if let Some(publication)=opened.body.get("native_publication") {
        body["native_publication"]=publication.clone();
        if publication.get("library_id").is_some() {body["native_publication"]["state"]=json!("ready");body["native_publication"]["ready"]=json!(true);}
    }
    body["outcome"] = json!("replay");
    body["fence_state"] = opened
        .body
        .get("fence_state")
        .cloned()
        .unwrap_or(Value::Null);
    body["opened_columns"] = opened
        .body
        .get("opened_columns")
        .cloned()
        .unwrap_or(json!([]));
    body["revoked_columns"] = opened
        .body
        .get("revoked_columns")
        .cloned()
        .unwrap_or(json!([]));
    body["compiled_pack"] = json!("not_published");
    body["datasets"] = opened.body["datasets"].clone();
    copy_publication(&mut body, &opened);
    if let Some(observation) = observation {
        body["deployment_mode"] = json!(if request.deployment_mode==crate::DeploymentMode::AllInOne {"all_in_one"} else {"primary_replica"});
        body["readonly_replica"] = json!(request.replica.is_some());
        body["visibility_observation"] = observation;
    }
    Ok(Receipt { code: 0, body })
}

async fn visibility_failure(
    request: &MaintainRequest<'_>,
    bindings: &[apply::VisibilityBinding],
    candidate: &CandidateRow,
    error: &StoreError,
) -> Result<Receipt, StoreError> {
    if let Some(mapping)=request.native_mapping {
        if let Some(publication)=crate::native_publication::from_context(mapping)? {
            target::library_state(&request.target,&publication,false).await?;
        }
    }
    for (schema, relation, role, _) in bindings {
        let fence = FenceRef {
            schema_name: schema,
            relation_name: relation,
            role_name: role,
        };
        let closed = target::close_target(&request.target, &fence).await?;
        if closed.code != 0 {
            return Err(StoreError::Input);
        }
    }
    Ok(refused("post_commit_visibility_failed", json!({"failure_detail":error.to_string(),"deployment_mode":if request.deployment_mode==crate::DeploymentMode::AllInOne {"all_in_one"} else {"primary_replica"},"readonly_replica":request.replica.is_some(),"fail_closed":true,"half_published":true,"knowledge_published":false,"candidate_id":request.candidate_id,"version":candidate.version,"pack_digest":candidate.pack_digest})))
}
fn copy_publication(body: &mut Value, opened: &Receipt) {
    let fence_open = opened.body.get("fence_state").and_then(Value::as_str) == Some("open");
    let pointed = body.get("pointer_is_candidate") == Some(&Value::Bool(true));
    let columns_match = opened.body.get("knowledge_published") == Some(&Value::Bool(true));
    let published = fence_open && pointed && columns_match;
    body["knowledge_published"] = json!(published);
    body["certificate_class"] = json!(if published {
        "test_scope_not_production"
    } else {
        "not_issued"
    });
}

fn open_matches(opened: &Receipt) -> bool {
    opened.body.get("fence_state").and_then(Value::as_str) == Some("open")
        && opened.body.get("knowledge_published") == Some(&Value::Bool(true))
        && matches!(
            opened.body.get("policy").and_then(Value::as_str),
            Some("true" | "(true)")
        )
}

async fn refuse_before_fence(
    request: &MaintainRequest<'_>,
    reason: &'static str,
) -> Result<Receipt, StoreError> {
    let mut current = status(&scope_request(request)).await?;
    current.code = 2;
    current.body["outcome"] = json!("refused");
    current.body["reason"] = json!(reason);
    current.body["fence_touched"] = json!(false);
    current.body["compiled_pack"] = json!("not_published");
    current.body["knowledge_published"] = json!(false);
    current.body["certificate_class"] = json!("not_issued");
    Ok(current)
}

async fn close_after_open_failure(
    request: &MaintainRequest<'_>,
    fence: &FenceRef<'_>,
    version: &str,
    open_sqlstate: &str,
) -> Result<Receipt, StoreError> {
    let revoked = target::revoke_target(&request.target, fence).await?;
    let rolled = recover(&RecoverRequest {
        endpoint: request.control.endpoint,
        user: request.control.user,
        password: request.control.password,
        ca_pem: request.control.ca_pem,
        action: "rollback",
        project_id: request.project_id,
        candidate_id: request.candidate_id,
        expected_version: Some(version),
    })
    .await?;
    Ok(Receipt {
        code: 2,
        body: json!({
            "outcome": "refused",
            "reason": "open_failed",
            "fence_state": revoked.body.get("fence_state").cloned().unwrap_or(Value::Null),
            "fence_touched": true,
            "pointer_version": rolled.body.get("pointer_version").cloned().unwrap_or(Value::Null),
            "open_sqlstate": open_sqlstate,
            "revoke_code": revoked.code,
            "rollback_code": rolled.code,
            "rollback_outcome": rolled.body.get("outcome").cloned().unwrap_or(Value::Null),
            "compiled_pack": "not_published",
            "knowledge_published": false,
            "certificate_class": "not_issued",
            "contract": crate::CONTRACT,
            "full_sp08_accepted": false,
            "public_deployment_claimed": false,
            "customer_ownership_claimed": false,
            "standby_configured": false,
            "production_ready_certificate": false,
            "owner_authenticated": false,
            "credential_verified": false,
            "integrity_verified": false,
            "customer_gate_verified": false,
            "runsc_verified": false,
            "sessions_terminated_by_tool": false,
            "database_used": true,
        }),
    })
}

async fn blocking_external(
    session: &mut store::Session,
    candidate_id: &str,
) -> Result<Option<String>, StoreError> {
    let tx = begin(session).await?;
    let row = match tx
        .query_opt(sql::READ_BLOCKING_EXTERNAL, &[&candidate_id])
        .await
    {
        Ok(row) => row,
        Err(err) => return Err(rollback(tx, query_error(err)).await),
    };
    let gate = match row {
        Some(row) => Some(
            row.try_get(0)
                .map_err(query_error)?,
        ),
        None => None,
    };
    commit(
        tx,
        store::stamped(
            "external",
            None,
            json!({"database_used": true}),
            "not_issued",
        ),
    )
    .await?;
    Ok(gate)
}

async fn pointer_columns(
    session: &mut store::Session,
    pointer: &PointerRow,
    request: &MaintainRequest<'_>,
) -> Result<Option<Vec<String>>, StoreError> {
    let Some(candidate_id) = pointer.candidate_id.as_deref() else {
        return Ok(None);
    };
    let Some(candidate) = read_candidate(session, candidate_id).await? else {
        return Ok(None);
    };
    let Ok(logical) = selected_binding(&candidate.visibility, request) else {
        return Ok(None);
    };
    let (_, _, _, columns) =
        resolve_candidate_binding(session, request, candidate_id, &logical).await?;
    Ok(Some(columns))
}

async fn write_preconditions(
    session: &mut store::Session,
    candidate_id: &str,
    source_hash: &str,
    rows: &[apply::Precondition],
) -> Result<(), StoreError> {
    let tx = begin(session).await?;
    for row in rows {
        let evidence = apply::evidence_sha(source_hash, row.gate_id, row.disposition, row.detail);
        if let Err(err) = tx
            .execute(
                sql::UPSERT_MAINTENANCE_GATE,
                &[
                    &candidate_id,
                    &row.gate_id,
                    &row.disposition,
                    &row.detail,
                    &evidence,
                ],
            )
            .await
        {
            return Err(rollback(tx, query_error(err)).await);
        }
    }
    commit(
        tx,
        store::stamped(
            "preconditions",
            None,
            json!({"database_used": true}),
            "not_issued",
        ),
    )
    .await?;
    Ok(())
}

async fn read_candidate(
    session: &mut store::Session,
    candidate_id: &str,
) -> Result<Option<CandidateRow>, StoreError> {
    let tx = begin(session).await?;
    let row = match tx
        .query_opt(sql::READ_CANDIDATE_MAINTAIN, &[&candidate_id])
        .await
    {
        Ok(row) => row,
        Err(err) => return Err(rollback(tx, query_error(err)).await),
    };
    let Some(row) = row else {
        commit(tx, store::stamped("absent", None, json!({}), "not_issued")).await?;
        return Ok(None);
    };
    let effective_text: String = row
        .try_get(5)
        .map_err(query_error)?;
    let visibility_text: String = row
        .try_get(6)
        .map_err(query_error)?;
    let parsed = CandidateRow {
        state: row
            .try_get(0)
            .map_err(query_error)?,
        source_hash: row
            .try_get(1)
            .map_err(query_error)?,
        pack_digest: row
            .try_get(2)
            .map_err(query_error)?,
        version: row
            .try_get(3)
            .map_err(query_error)?,
        effective: serde_json::from_str(&effective_text).map_err(|_| StoreError::Input)?,
        visibility: serde_json::from_str(&visibility_text).map_err(|_| StoreError::Input)?,
    };
    commit(
        tx,
        store::stamped("candidate", None, json!({}), "not_issued"),
    )
    .await?;
    Ok(Some(parsed))
}

async fn read_pointer(session: &mut store::Session) -> Result<PointerRow, StoreError> {
    let tx = begin(session).await?;
    let row = match tx.query_one(sql::READ_POINTER_PACK, &[]).await {
        Ok(row) => row,
        Err(err) => return Err(rollback(tx, query_error(err)).await),
    };
    let parsed = PointerRow {
        version: row
            .try_get(0)
            .map_err(query_error)?,
        candidate_id: row
            .try_get(1)
            .map_err(query_error)?,
        pack_digest: row
            .try_get(2)
            .map_err(query_error)?,
    };
    commit(tx, store::stamped("pointer", None, json!({}), "not_issued")).await?;
    Ok(parsed)
}

async fn read_lengths(
    session: &mut store::Session,
    candidate_id: &str,
) -> Result<Vec<i32>, StoreError> {
    let tx = begin(session).await?;
    let rows = match tx.query(sql::READ_FILE_LENGTHS, &[&candidate_id]).await {
        Ok(rows) => rows,
        Err(err) => return Err(rollback(tx, query_error(err)).await),
    };
    let mut lengths = Vec::new();
    for row in rows {
        lengths.push(
            row.try_get(0)
                .map_err(query_error)?,
        );
    }
    commit(tx, store::stamped("files", None, json!({}), "not_issued")).await?;
    Ok(lengths)
}

async fn read_authority(session: &mut store::Session) -> Result<String, StoreError> {
    let tx = begin(session).await?;
    let row = match tx.query_one(sql::READ_AUTHORITY_SHA, &[]).await {
        Ok(row) => row,
        Err(err) => return Err(rollback(tx, query_error(err)).await),
    };
    let hash: String = row
        .try_get(0)
        .map_err(query_error)?;
    commit(
        tx,
        store::stamped("authority", None, json!({}), "not_issued"),
    )
    .await?;
    Ok(hash)
}

fn scope_request<'a>(request: &'a MaintainRequest<'_>) -> PlanRequest<'a> {
    PlanRequest {
        endpoint: request.control.endpoint,
        user: request.control.user,
        password: request.control.password,
        ca_pem: request.control.ca_pem,
        project_id: request.project_id,
        candidate_id: request.candidate_id,
        source_hash: request.source_hash,
        scope: "test_scope",
        expected_version: request.expected_version,
    }
}

fn measurement_detail(measured: &Measured) -> &str {
    &measured.detail
}

fn gate_request<'a>(
    request: &'a MaintainRequest<'_>,
    authority: &'a str,
    measured: &'a Measured,
) -> GateRequest<'a> {
    GateRequest {
        endpoint: request.control.endpoint,
        user: request.control.user,
        password: request.control.password,
        ca_pem: request.control.ca_pem,
        project_id: request.project_id,
        candidate_id: request.candidate_id,
        source_hash: request.source_hash,
        gate_id: measured.gate_id,
        issuer_class: "fixture_authority",
        status: measured.status,
        blocking: measured.blocking,
        report_type: "target_measurement",
        authority_statement_sha256: Some(authority),
    }
}

async fn record_measured(
    request: &MaintainRequest<'_>,
    authority: &str,
    measured: &Measured,
) -> Result<Receipt, StoreError> {
    record_server_gate(
        &gate_request(request, authority, measured),
        measurement_detail(measured),
    )
    .await
}

async fn record_component(
    request: &MaintainRequest<'_>,
    measured: &Measured,
) -> Result<Receipt, StoreError> {
    gate_record(&GateRequest {
        endpoint: request.control.endpoint,
        user: request.control.user,
        password: request.control.password,
        ca_pem: request.control.ca_pem,
        project_id: request.project_id,
        candidate_id: request.candidate_id,
        source_hash: request.source_hash,
        gate_id: measured.gate_id,
        issuer_class: "component_authority",
        status: measured.status,
        blocking: measured.blocking,
        report_type: "component_recompile",
        authority_statement_sha256: None,
    })
    .await
}

async fn record_checkpoint(
    request: &MaintainRequest<'_>,
    authority: &str,
    kind: &str,
    barrier: &str,
    epoch: &str,
) -> Result<Receipt, StoreError> {
    checkpoint(&CheckpointRequest {
        endpoint: request.control.endpoint,
        user: request.control.user,
        password: request.control.password,
        ca_pem: request.control.ca_pem,
        project_id: request.project_id,
        candidate_id: request.candidate_id,
        kind,
        source_hash: request.source_hash,
        epoch,
        barrier,
        sessions_terminated: false,
        issuer_class: "fixture_authority",
        authority_statement_sha256: Some(authority),
    })
    .await
}

fn checked(request: &MaintainRequest<'_>) -> Result<(), StoreError> {
    if request.quality_observer.as_ref().is_some_and(|q|q.endpoint!=request.target.endpoint) {return Err(StoreError::Input);}
    if request.deployment_mode == crate::DeploymentMode::AllInOne && request.replica.is_some() {
        return Err(StoreError::Input);
    }
    if let Some(mapping) = request.native_mapping {
        target::validate_native_context(mapping)?;
    }
    if request.control.user != ADMIN_ROLE || request.target.user != sql::TARGET_ROLE {
        return Err(StoreError::Input);
    }
    if !crate::seal::token_ok(request.project_id)
        || !crate::seal::token_ok(request.candidate_id)
        || !crate::seal::hex64(request.source_hash)
    {
        return Err(StoreError::Input);
    }
    if let Some(version) = request.expected_version
        && !crate::seal::token_ok(version)
    {
        return Err(StoreError::Input);
    }
    Ok(())
}

async fn binding_work<'a, F, Fut>(
    bindings: &'a [apply::VisibilityBinding],
    parallelism: usize,
    work: F,
) -> Result<Option<Receipt>, StoreError>
where
    F: Fn(&'a apply::VisibilityBinding) -> Fut,
    Fut: std::future::Future<Output = Result<Option<Receipt>, StoreError>>,
{
    for chunk in bindings.chunks(parallelism.clamp(1, 8)) {
        let run = |index| {
            let work = &work;
            async move {
                match chunk.get(index) {
                    Some(binding) => work(binding).await,
                    None => Ok(None),
                }
            }
        };
        let (a,b,c,d,e,f,g,h) = tokio::join!(run(0),run(1),run(2),run(3),run(4),run(5),run(6),run(7));
        // Join the entire batch before returning, so no grant task survives a
        // failed batch and races the caller's fail-closed cleanup.
        for result in [a,b,c,d,e,f,g,h] {
            if let Some(refused) = result? {
                return Ok(Some(refused));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod bounded_binding_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    fn bindings() -> Vec<apply::VisibilityBinding> {
        (0..17).map(|i| ("sample".into(),format!("t{i}"),"reader".into(),vec!["id".into()])).collect()
    }

    #[tokio::test]
    async fn table_work_is_bounded_and_all_tasks_finish() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let finished = AtomicUsize::new(0);
        let rows = bindings();
        binding_work(&rows, 8, |_| async {
            let count = active.fetch_add(1, SeqCst) + 1;
            peak.fetch_max(count, SeqCst);
            tokio::task::yield_now().await;
            active.fetch_sub(1, SeqCst);
            finished.fetch_add(1, SeqCst);
            Ok(None)
        }).await.unwrap();
        assert_eq!((active.load(SeqCst), peak.load(SeqCst), finished.load(SeqCst)), (0, 8, 17));
    }

    #[tokio::test]
    async fn failure_waits_for_current_batch_and_never_starts_next_batch() {
        let finished = AtomicUsize::new(0);
        let rows = bindings();
        let result = binding_work(&rows, 8, |binding| {
            let fail = binding.1 == "t2";
            let finished = &finished;
            async move {
                tokio::task::yield_now().await;
                finished.fetch_add(1, SeqCst);
                if fail { Err(StoreError::Input) } else { Ok(None) }
            }
        }).await;
        assert!(matches!(result, Err(StoreError::Input)));
        assert_eq!(finished.load(SeqCst), 8);
    }
}

async fn prepare_binding(
    request: &MaintainRequest<'_>, binding: &apply::VisibilityBinding, library: bool,
) -> Result<Option<Receipt>, StoreError> {
    let f = FenceRef {schema_name:&binding.0, relation_name:&binding.1, role_name:&binding.2};
    let installed = if library {
        target::prepare_library_target(&request.target, &f).await?
    } else {
        target::install_target(&request.target, &f).await?
    };
    if installed.code != 0 { return Ok(Some(installed)); }
    let closed = target::close_target(&request.target, &f).await?;
    Ok((closed.code != 0).then_some(closed))
}

async fn grant_and_observe_binding(
    request: &MaintainRequest<'_>, binding: &apply::VisibilityBinding,
) -> Result<Option<Receipt>, StoreError> {
    let f = FenceRef {schema_name:&binding.0, relation_name:&binding.1, role_name:&binding.2};
    let granted = target::grant_target(&request.target, &f, &binding.3).await?;
    if granted.code != 0 { return Ok(Some(granted)); }
    let observed = target::observe_target(&request.target, &f, &binding.3, request.reader_user, request.reader_password).await?;
    Ok((observed.code != 0).then_some(observed))
}

fn installed_visibility_bindings(
    request: &MaintainRequest<'_>,
    visibility: &Value,
) -> Result<Vec<apply::VisibilityBinding>, &'static str> {
    let ceiling = request.control.endpoint.pack_ceilings.as_ref()
        .map_or(256, |c| c.max_datasets);
    apply::visibility_bindings_with_ceiling(visibility, ceiling)
}

fn selected_binding(
    visibility: &Value,
    request: &MaintainRequest<'_>,
) -> Result<apply::VisibilityBinding, &'static str> {
    installed_visibility_bindings(request, visibility)?
        .into_iter()
        .find(|(s, r, role, _)| {
            s == request.schema_name && r == request.relation_name && role == request.role_name
        })
        .ok_or("target_binding")
}
async fn revoke_all(
    request: &MaintainRequest<'_>,
    bindings: &[apply::VisibilityBinding],
) -> Result<(), StoreError> {
    for (s, r, role, _) in bindings {
        let f = FenceRef {
            schema_name: s,
            relation_name: r,
            role_name: role,
        };
        target::revoke_target(&request.target, &f).await?;
    }
    Ok(())
}
fn aggregate_opened(rows: Vec<Receipt>) -> Receipt {
    let all = rows.iter().all(|r| {
        r.code == 0 && r.body["knowledge_published"] == true && r.body["fence_state"] == "open"
    });
    let mut body = rows.first().map(|r| r.body.clone()).unwrap_or(json!({}));
    body["knowledge_published"] = json!(all);
    body["certificate_class"] = json!(if all {
        "test_scope_not_production"
    } else {
        "not_issued"
    });
    body["datasets"] = json!(rows.into_iter().map(|r| r.body).collect::<Vec<_>>());
    Receipt { code: 0, body }
}

fn rebind<'a>(r: &MaintainRequest<'a>, b: &'a apply::VisibilityBinding) -> MaintainRequest<'a> {
    MaintainRequest {
        quality_observer: r.quality_observer,
        deployment_mode: r.deployment_mode,
        replica: r.replica,
        native_mapping: r.native_mapping,
        control: r.control,
        target: TargetConn {
            endpoint: r.target.endpoint,
            user: r.target.user,
            password: r.target.password,
            ca_pem: r.target.ca_pem,
        },
        reader_user: r.reader_user,
        reader_password: r.reader_password,
        project_id: r.project_id,
        candidate_id: r.candidate_id,
        source_hash: r.source_hash,
        expected_version: r.expected_version,
        schema_name: &b.0,
        relation_name: &b.1,
        role_name: &b.2,
    }
}
async fn recover_all(
    session: &mut store::Session,
    request: &MaintainRequest<'_>,
) -> Result<Receipt, StoreError> {
    let pointer = read_pointer(session).await?;
    let mut bindings = Vec::new();
    for id in [Some(request.candidate_id), pointer.candidate_id.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Some(c) = read_candidate(session, id).await? {
            for b in installed_visibility_bindings(request, &c.visibility).map_err(|_| StoreError::Input)? {
                if !bindings.iter().any(|old: &apply::VisibilityBinding| {
                    old.0 == b.0 && old.1 == b.1 && old.2 == b.2
                }) {
                    bindings.push(b);
                }
            }
        }
    }
    if bindings.is_empty() {
        return recover_locked(session, request).await;
    }
    let mut rows = Vec::new();
    for b in &bindings {
        let scoped = rebind(request, b);
        let row = recover_locked(session, &scoped).await?;
        if row.code != 0 {
            return Ok(row);
        }
        rows.push(row);
    }
    Ok(aggregate_opened(rows))
}

async fn resolve_binding(
    request: &MaintainRequest<'_>,
    logical: &apply::VisibilityBinding,
) -> Result<apply::VisibilityBinding, StoreError> {
    if let Some(mapping) = request.native_mapping {
        target::resolve_native_binding(&request.target, mapping, logical).await
    } else {
        Ok(logical.clone())
    }
}

async fn resolve_validated_binding(
    target:&TargetConn<'_>,
    context:Option<&target::ValidatedNativeContext<'_>>,
    logical:&apply::VisibilityBinding,
)->Result<apply::VisibilityBinding,StoreError> {
    match context {
        Some(context)=>target::resolve_validated_native_binding(target,context,logical).await,
        None=>Ok(logical.clone()),
    }
}

fn with_mapping<'a>(r: &MaintainRequest<'a>, mapping: Option<&'a Value>) -> MaintainRequest<'a> {
    MaintainRequest {
        quality_observer: r.quality_observer,
        deployment_mode: r.deployment_mode,
        replica: r.replica,
        native_mapping: mapping,
        control: r.control,
        target: TargetConn {
            endpoint: r.target.endpoint,
            user: r.target.user,
            password: r.target.password,
            ca_pem: r.target.ca_pem,
        },
        reader_user: r.reader_user,
        reader_password: r.reader_password,
        project_id: r.project_id,
        candidate_id: r.candidate_id,
        source_hash: r.source_hash,
        expected_version: r.expected_version,
        schema_name: r.schema_name,
        relation_name: r.relation_name,
        role_name: r.role_name,
    }
}
async fn known_mapping(
    session: &mut store::Session,
    id: &str,
) -> Result<Option<Value>, StoreError> {
    let row=session.client.query_opt("SELECT b.mapping_json,b.mapping_sha256,b.source_hash,c.source_hash FROM oo_release_control.candidate_target_binding b JOIN oo_release_control.candidate c USING(candidate_id) WHERE b.candidate_id=$1",&[&id]).await.map_err(|e|query_error(e).at(Phase::KnownMappingQuery))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let bound_source: String = row.try_get(2).map_err(|e|query_error(e).at(Phase::KnownMappingDecode))?;
    let candidate_source: String = row.try_get(3).map_err(|e|query_error(e).at(Phase::KnownMappingDecode))?;
    if bound_source != candidate_source {
        return Err(StoreError::Input);
    }
    let text: String = row.try_get(0).map_err(|e|query_error(e).at(Phase::KnownMappingDecode))?;
    let hash: String = row.try_get(1).map_err(|e|query_error(e).at(Phase::KnownMappingDecode))?;
    if crate::sha256::sha256_hex(text.as_bytes()) != hash {
        return Err(StoreError::Input);
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| StoreError::Input)?;
    if !value.is_null() {
        target::validate_native_context(&value)?;
    }
    Ok(Some(value))
}
async fn activate_bound(
    session: &mut store::Session,
    r: &MaintainRequest<'_>,
) -> Result<Receipt, StoreError> {
    let installed = maintain_install(&r.control, r.project_id).await?;
    if installed.code != 0 {
        return Ok(installed);
    }
    let known = known_mapping(session, r.candidate_id).await?;
    if let (Some(existing), Some(requested)) = (&known, r.native_mapping)
        && existing != requested
    {
        return Ok(refused(
            "target_binding_changed",
            json!({"fence_touched":false}),
        ));
    }
    let mapping = known.as_ref().filter(|v| !v.is_null()).or_else(|| {
        if known.is_none() {
            r.native_mapping
        } else {
            None
        }
    });
    let request = with_mapping(r, mapping);
    activate_locked(session, &request).await
}
async fn bind_mapping(
    session: &mut store::Session,
    r: &MaintainRequest<'_>,
) -> Result<(), StoreError> {
    let text = crate::canon::canonical(r.native_mapping.unwrap_or(&Value::Null));
    let hash = crate::sha256::sha256_hex(text.as_bytes());
    session.client.execute("INSERT INTO oo_release_control.candidate_target_binding(candidate_id,source_hash,mapping_json,mapping_sha256) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",&[&r.candidate_id,&r.source_hash,&text,&hash]).await.map_err(query_error)?;
    let row=session.client.query_one("SELECT source_hash,mapping_json FROM oo_release_control.candidate_target_binding WHERE candidate_id=$1",&[&r.candidate_id]).await.map_err(query_error)?;
    let source: String = row.try_get(0).map_err(|_| StoreError::Input)?;
    let stored: String = row.try_get(1).map_err(|_| StoreError::Input)?;
    if source != r.source_hash || stored != text {
        return Err(StoreError::Input);
    }
    Ok(())
}
async fn resolve_candidate_binding(
    session: &mut store::Session,
    r: &MaintainRequest<'_>,
    id: &str,
    logical: &apply::VisibilityBinding,
) -> Result<apply::VisibilityBinding, StoreError> {
    let known = known_mapping(session, id).await?;
    let request = with_mapping(r, known.as_ref().filter(|v| !v.is_null()));
    resolve_binding(&request, logical).await
}

async fn recover_bound(
    session: &mut store::Session,
    r: &MaintainRequest<'_>,
) -> Result<Receipt, StoreError> {
    let known = known_mapping(session, r.candidate_id).await?;
    if let (Some(existing), Some(requested)) = (&known, r.native_mapping)
        && existing != requested
    {
        return Ok(refused(
            "target_binding_changed",
            json!({"fence_touched":false}),
        ));
    }
    if let Some(mapping)=known.as_ref().filter(|v|!v.is_null()) {
        let context=target::ValidatedNativeContext::new(mapping)?;
        if let Some(publication)=context.publication() {
            if crate::native_library::is_library(&publication.receipt) {
                let pointer=read_pointer(session).await?;
                let candidate=read_candidate(session,r.candidate_id).await?.ok_or(StoreError::Input)?;
                if pointer.candidate_id.as_deref()!=Some(r.candidate_id) || candidate.state!="active" {return Err(StoreError::Input);}
                target::library_state(&r.target,&publication,false).await?;
                let bound=with_mapping(r,Some(mapping));
                let mut bindings=Vec::new();
                for logical in installed_visibility_bindings(r, &candidate.visibility).map_err(|_|StoreError::Input)? {
                    bindings.push(resolve_validated_binding(&r.target,Some(&context),&logical).await?);
                }
                for (schema,relation,role,columns) in &bindings {
                    let fence=FenceRef{schema_name:schema,relation_name:relation,role_name:role};
                    target::close_target(&r.target,&fence).await?;
                    let granted=target::grant_target(&r.target,&fence,columns).await?;
                    if granted.code!=0 {return Ok(granted);}
                }
                let opened=target::open_native_collection(&r.target,&bindings,&candidate.pack_digest,&candidate.version,r.candidate_id,&publication).await;
                if opened.as_ref().map_or(true,|rows|rows.iter().any(|row|row.code!=0)) {
                    target::library_state(&r.target,&publication,false).await?; return Err(StoreError::Input);
                }
                if let Some(replica)=r.replica.as_ref() {
                    if let Err(error)=target::replica_barrier(&r.target,replica).await {target::library_state(&r.target,&publication,false).await?;return Err(error);}
                }
                let expected=target::VisibilityRef{bindings:&bindings,version:&candidate.version,pack:&candidate.pack_digest,candidate:r.candidate_id,mapping:Some(mapping)};
                target::verify_library_visibility(&r.target,r.replica.as_ref().unwrap_or(&r.target),r.reader_user,r.reader_password,expected,&publication,r.replica.is_some()).await?;
                return finish_from_status(session,&bound,&pointer).await;
            }
        }
    }
    recover_all(session, r).await
}

#[cfg(test)]
mod observer_binding_tests {
    use super::*;
    #[test]
    fn mapping_and_table_rebinding_preserve_the_read_only_quality_observer() {
        let endpoint=Endpoint {pack_ceilings:None,host:"127.0.0.1".into(),port:1,database:"fixture".into()};
        let hash="a".repeat(64);
        let request=MaintainRequest {quality_observer:Some(TargetConn {endpoint:&endpoint,user:"postgres",password:"fixture",ca_pem:b"fixture"}),deployment_mode:crate::DeploymentMode::AllInOne,control:ControlConn {endpoint:&endpoint,user:"oo_release_admin",password:"fixture",ca_pem:b"fixture"},target:TargetConn {endpoint:&endpoint,user:"oo_target_admin",password:"fixture",ca_pem:b"fixture"},replica:None,reader_user:"oo_reader",reader_password:"fixture",project_id:"fixture",candidate_id:"library",source_hash:&hash,expected_version:None,schema_name:"app",relation_name:"items",role_name:"oo_r_reader",native_mapping:None};
        assert_eq!(with_mapping(&request,None).quality_observer.unwrap().user,"postgres");
        let binding=("app".into(),"items".into(),"oo_r_reader".into(),vec!["id".into()]);
        assert_eq!(rebind(&request,&binding).quality_observer.unwrap().user,"postgres");
    }
}

#[cfg(test)]
mod activation_exit_tests {
    use super::*;

    #[test]
    fn preflight_defers_only_target_checkpoints_never_a_gate_or_pointer_conflict() {
        let receipt = |reasons: Value| Receipt { code: 0, body: json!({"test_reasons":reasons,"gates":[]}) };
        assert!(preflight_gate_refusal("new", receipt(json!([
            "missing_fence","stale_fence","missing_replica","stale_replica"
        ]))).unwrap().is_none());
        for reason in ["G4_conflict","G1_failed","G9_not_run","G7_failed",
                       "conflict","cas_conflict","blocked","unrecognized_reason"] {
            let refused = preflight_gate_refusal("new", receipt(json!(["missing_fence",reason])))
                .unwrap().unwrap();
            assert_eq!(refused.code, 2);
            assert_eq!(refused.body["fence_touched"], false);
            assert_eq!(refused.body["reasons"], json!([reason]));
        }
        assert!(preflight_gate_refusal("new", receipt(json!([1]))).is_err());
        assert!(preflight_gate_refusal("new", Receipt { code: 0, body: json!({}) }).is_err());
        let error = preflight_gate_refusal("new", Receipt { code: 2, body: json!({"reason":"stale"}) })
            .unwrap().unwrap();
        assert_eq!(error.body["reason"], "stale");
    }

    #[test]
    fn recorded_blocking_gates_and_nonadmissible_component_gates_refuse_activation() {
        for gate_id in ["G4","G8","G7"] {
            let gate=Measured{gate_id,status:"not_run",blocking:gate_id=="G4",detail:"evidence retained".into()};
            let body=json!({"outcome":"recorded","test_admissible":false,"gate_id":gate_id,"report_sha256":"abc"});
            let recorded=Receipt{code:0,body:body.clone()};
            let result=activation_gate_refusal("candidate-v3",&gate,recorded,true).unwrap();
            assert_eq!(result.code,2);
            assert_eq!(result.body["outcome"],"refused");
            assert_eq!(result.body["reason"],"gate_failed");
            assert_eq!(result.body["gate_id"],gate_id);
            assert_eq!(result.body["detail"],gate.detail);
            assert_eq!(result.body["gate_receipt"],body);
            assert_eq!(result.body["gate_record_code"],0);
            assert_eq!(result.body["activation_completed"],false);
        }
    }

    #[test]
    fn activation_preserves_admissible_gates_and_nonblocking_warnings_not_record_errors() {
        let gate=Measured{gate_id:"G4",status:"warn",blocking:false,detail:"undefined coverage".into()};
        for body in [json!({"test_admissible":false}),json!({})] {
            assert!(activation_gate_refusal("candidate",&gate,Receipt{code:0,body:body.clone()},false).is_none());
            assert_eq!(activation_gate_refusal("candidate",&gate,Receipt{code:0,body},true).unwrap().code,2);
        }
        assert!(activation_gate_refusal("candidate",&gate,Receipt{code:0,body:json!({"test_admissible":true})},true).is_none());
        assert_eq!(activation_gate_refusal("candidate",&gate,Receipt{code:2,body:json!({"test_admissible":true})},false).unwrap().code,2);
    }
}
