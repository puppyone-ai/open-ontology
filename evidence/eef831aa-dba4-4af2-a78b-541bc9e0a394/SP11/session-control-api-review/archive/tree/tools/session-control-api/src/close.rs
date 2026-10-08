//! S1–S11 close. session-release confirms S1–S4 and S8. This crate records the rest.
//! `production_signed` stays false. A releasing fixture is not a close.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use session_release::{run, ReleaseReceipt as FixtureReceipt, ReleaseRequest, CONTRACT as RELEASE_CONTRACT};
use session_transition::{
    is_hex64, CancelTrigger, Event, Phase, ReleaseSteps, SessionState, StepId, StepRecord, StepStatus,
};

use gateway_token_store::{
    lookup, CurrentTuple, DeclaredTransport, LookupCache, LookupRequest, RevokeScope, StoreError, TrustedControlAdapter,
};

use crate::config::{Config, CONTRACT, DEPLOYMENT, OWNER_UUID, PRODUCTION_SIGNED};
use crate::error::Fault;
use crate::lifecycle::{self, endpoint_of, identity_from_resource, load_head, phase_label, runtime_db, token_endpoint, tree, Tree};
use crate::sha256::{hex, sha256};
use crate::sql;
use crate::store::{self, ResourceRow};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StepOut {
    pub id: String,
    pub status: String,
    pub actual_absence: bool,
    pub evidence_sha256: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CloseReceipt {
    pub contract: &'static str,
    pub disposition: String,
    pub reason: String,
    pub phase: String,
    pub production_signed: bool,
    pub deployment: &'static str,
    pub retryable: bool,
    pub reducer_physical_release_certified: bool,
    pub steps: Vec<StepOut>,
}

impl CloseReceipt {
    pub fn acceptable(&self) -> bool {
        if self.production_signed || self.deployment != DEPLOYMENT || self.phase != "closed" {
            return false;
        }
        if self.disposition != "closed" && self.disposition != "replay" {
            return false;
        }
        if self.retryable || self.reducer_physical_release_certified || self.steps.len() != 11 {
            return false;
        }
        const IDS: [&str; 11] = ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"];
        for id in IDS {
            let Some(step) = self.steps.iter().find(|step| step.id == id) else {
                return false;
            };
            if !is_hex64(&step.evidence_sha256) {
                return false;
            }
            match id {
                "s8" if step.status == "confirmed" && step.actual_absence => {}
                "s5" if step.status == "skipped" && !step.actual_absence => {}
                "s5" if step.status == "confirmed" && !step.actual_absence => {}
                "s1" | "s2" | "s3" | "s4" | "s6" | "s7" | "s9" | "s10" | "s11"
                    if step.status == "confirmed" && !step.actual_absence => {}
                _ => return false,
            }
        }
        true
    }
}

pub async fn cancel_session(
    cfg: &Config,
    conversation_id: &str,
    caller_principal: &str,
    caller_project: &str,
    allow_admin: bool,
) -> Result<CloseReceipt, Fault> {
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let Some(conversation) = store::load_conversation(&db, conversation_id).await? else {
        return Err(Fault::NotFound);
    };
    let owner = conversation.principal_id == caller_principal && conversation.project_id == caller_project;
    let admin = allow_admin && conversation.project_id == caller_project;
    if !owner && !admin {
        return Err(Fault::Forbidden("not_owner"));
    }
    let trigger = if owner { CancelTrigger::User } else { CancelTrigger::Admin };
    let resource = store::load_resource(&db, &conversation.project_id, &conversation.session_id).await?;
    if resource.stage == "running" {
        let fixture = fixture_stage(cfg, &resource.project_id, &resource.session_id, trigger).await?;
        if fixture.disposition != "fixture_persisted" {
            return Ok(fixture);
        }
    } else if resource.stage == "ready"
        || (resource.backend_pid.is_none() && resource.stage != "fixture_done" && resource.stage != "closed")
    {
        let head = load_head(cfg, &resource.project_id, &resource.session_id).await.ok();
        return Ok(incomplete("backend_absent", "ready", head.as_ref()));
    }
    let receipt = finish_stage(cfg, &resource.project_id, &resource.session_id).await?;
    if receipt.acceptable() && receipt.disposition == "closed" {
        let seq = store::next_seq(&db, &resource.project_id, conversation_id).await?;
        let _ = store::insert_event(
            &db,
            &resource.project_id,
            conversation_id,
            seq,
            &format!("{}-closed", resource.event_prefix),
            "closed",
            "closed",
        )
        .await;
    }
    Ok(receipt)
}

pub async fn fixture_stage(cfg: &Config, project_id: &str, session_id: &str, trigger: CancelTrigger) -> Result<CloseReceipt, Fault> {
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let resource = store::load_resource(&db, project_id, session_id).await?;
    let paths = tree(&cfg.state_dir, &resource.pool_slot, &resource.session_id);
    if denied(&paths) {
        return Ok(incomplete("unexpected_certification", "releasing", None));
    }
    let head = load_head(cfg, project_id, session_id).await?;
    if resource.stage == "fixture_done" {
        return Ok(persisted(&head));
    }
    if head.phase == Phase::Releasing && fixture_confirmed(&head) && workspace_absent(&paths) {
        let outputs = store::load_output(&db, project_id, session_id, "outputs").await?;
        let transcript = store::load_output(&db, project_id, session_id, "transcript").await?;
        expect_collect(&head, StepId::S3, "outputs", &outputs.0, outputs.1.len())?;
        expect_collect(&head, StepId::S4, "transcript", &transcript.0, transcript.1.len())?;
        let pid = resource.harness_pid.or_else(|| read_harness_pid(&paths.checkpoint));
        store::set_stage(&db, project_id, session_id, "fixture_done", pid).await?;
        return Ok(persisted(&head));
    }
    if head.phase == Phase::Releasing {
        return Ok(incomplete("unproven_resume", "releasing", Some(&head)));
    }
    if head.phase != Phase::Running || resource.backend_pid.is_none() {
        return Ok(incomplete("backend_absent", phase_label(head.phase).as_str(), Some(&head)));
    }
    let outputs = store::load_output(&db, project_id, session_id, "outputs").await?;
    let transcript = store::load_output(&db, project_id, session_id, "transcript").await?;
    if hash_file(&paths.outputs)? != outputs.0 || hash_file(&paths.transcript)? != transcript.0 {
        return Ok(incomplete("evidence_mismatch", "running", Some(&head)));
    }
    revoke_token(cfg, &resource).await?;
    prove_revoked(cfg, &resource).await?;
    store::release_credentials(&db, &resource).await?;
    let pid = resource.backend_pid.ok_or(Fault::Incomplete("backend_absent"))?;
    let request = ReleaseRequest {
        contract: RELEASE_CONTRACT.to_owned(),
        host: cfg.pg.host.clone(),
        port: cfg.pg.port,
        database: cfg.pg.database.clone(),
        runtime_user: sql::SESSION_RUNTIME.to_owned(),
        runtime_password: cfg.pg.session_runtime_password.clone(),
        cancel_user: sql::CANCEL_LOGIN.to_owned(),
        cancel_password: cfg.pg.cancel_password.clone(),
        ca_pem: cfg.pg.ca_pem.clone(),
        project_id: project_id.to_owned(),
        session_id: session_id.to_owned(),
        identity: identity_from_resource(&resource)?,
        expected_revision: head.revision,
        expected_owner_generation: head.owner_generation,
        now_ms: head.last_now_ms,
        cancel_trigger: trigger,
        event_prefix: resource.event_prefix.clone(),
        owner_uuid: OWNER_UUID.to_owned(),
        workspace_root: paths.root.display().to_string(),
        workspace_path: paths.workspace.display().to_string(),
        checkpoint_dir: paths.checkpoint.display().to_string(),
        transcript_path: paths.transcript.display().to_string(),
        outputs_path: paths.outputs.display().to_string(),
        backend_pid: pid,
        application_name: resource.application_name.clone(),
        query_role: sql::QUERY_ROLE.to_owned(),
        harness_argv: harness_argv(),
    };
    let result = run(request).await;
    if result.receipt.disposition == "store_unavailable" {
        let reason = if result.receipt.reason == "commit_unknown" { "commit_unknown" } else { "store" };
        return Err(Fault::Unavailable(reason));
    }
    let after = load_head(cfg, project_id, session_id).await?;
    if result.exit_code != 0 || !fixture_ok(&result.receipt, &outputs.0, &transcript.0) || !fixture_confirmed(&after) {
        return Ok(incomplete(safe_reason(&result.receipt.reason).as_str(), phase_label(after.phase).as_str(), Some(&after)));
    }
    expect_collect(&after, StepId::S3, "outputs", &outputs.0, outputs.1.len())?;
    expect_collect(&after, StepId::S4, "transcript", &transcript.0, transcript.1.len())?;
    if !workspace_absent(&paths) {
        return Ok(incomplete("workspace_present", "releasing", Some(&after)));
    }
    let harness_pid = result.receipt.harness_pid.map(i64::from);
    write_marker(&paths.checkpoint, "api-fixture-done")?;
    store::set_stage(&db, project_id, session_id, "fixture_done", harness_pid).await?;
    Ok(persisted(&load_head(cfg, project_id, session_id).await?))
}

pub async fn finish_stage(cfg: &Config, project_id: &str, session_id: &str) -> Result<CloseReceipt, Fault> {
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let resource = store::load_resource(&db, project_id, session_id).await?;
    let paths = tree(&cfg.state_dir, &resource.pool_slot, &resource.session_id);
    if denied(&paths) {
        return Ok(incomplete("unexpected_certification", "releasing", None));
    }
    let mut head = load_head(cfg, project_id, session_id).await?;
    if head.phase == Phase::Closed {
        return accept_closed(cfg, &db, &resource, &paths, &head, "replay").await;
    }
    if resource.stage != "fixture_done" || !fixture_confirmed(&head) {
        return Ok(incomplete("fixture_required", phase_label(head.phase).as_str(), Some(&head)));
    }
    if let Err(reason) = live_recheck(&db, &resource, &paths, true).await {
        return Ok(incomplete(reason, "releasing", Some(&head)));
    }
    prove_revoked(cfg, &resource).await?;
    let identity = identity_from_resource(&resource)?;
    let outputs = store::load_output(&db, project_id, session_id, "outputs").await?;
    let transcript = store::load_output(&db, project_id, session_id, "transcript").await?;
    expect_collect(&head, StepId::S3, "outputs", &outputs.0, outputs.1.len())?;
    expect_collect(&head, StepId::S4, "transcript", &transcript.0, transcript.1.len())?;
    let snapshot_hash = evidence_hash("snapshot_absent", "not_found")?;
    ensure_step(cfg, &identity, &resource, &mut head, "s5", StepId::S5, StepStatus::Skipped, false, &snapshot_hash).await?;
    let token_hash = evidence_hash("gateway_token_store", "revoked")?;
    ensure_step(cfg, &identity, &resource, &mut head, "s6", StepId::S6, StepStatus::Confirmed, false, &token_hash).await?;
    let mapping_hash = evidence_hash("proxy_mapping", "present_count_0")?;
    ensure_step(cfg, &identity, &resource, &mut head, "s7", StepId::S7, StepStatus::Confirmed, false, &mapping_hash).await?;
    let replacement = ensure_replacement(&db, &resource, &paths).await?;
    let slot_hash = evidence_hash("pool_slot", &format!("replaced:{replacement}"))?;
    ensure_step(cfg, &identity, &resource, &mut head, "s9", StepId::S9, StepStatus::Confirmed, false, &slot_hash).await?;
    let permit_hash = evidence_hash("permit", "held_0")?;
    ensure_step(cfg, &identity, &resource, &mut head, "s10", StepId::S10, StepStatus::Confirmed, false, &permit_hash).await?;
    let audit_hash = evidence_hash("release_audit", "inserted")?;
    store::insert_audit(&db, project_id, session_id, "s11", &audit_hash).await?;
    ensure_step(cfg, &identity, &resource, &mut head, "s11", StepId::S11, StepStatus::Confirmed, false, &audit_hash).await?;
    if let Err(reason) = live_recheck(&db, &resource, &paths, true).await {
        return Ok(incomplete(reason, "releasing", Some(&head)));
    }
    prove_revoked(cfg, &resource).await?;
    let steps = release_steps(&head)?;
    let now = head.last_now_ms.saturating_add(1);
    let applied = lifecycle::apply(
        cfg,
        &endpoint,
        &identity,
        session_id,
        head.revision,
        head.owner_generation,
        &format!("{}-close", resource.event_prefix),
        now,
        &Event::CompleteRelease { steps: Box::new(steps) },
    )
    .await;
    if let Err(Fault::Incomplete("unexpected_certification")) = &applied {
        let _ = write_marker(&paths.checkpoint, "certification_denied");
    }
    applied?;
    let closed = load_head(cfg, project_id, session_id).await?;
    if closed.phase != Phase::Closed {
        return Ok(incomplete("not_closed", phase_label(closed.phase).as_str(), Some(&closed)));
    }
    accept_closed(cfg, &db, &resource, &paths, &closed, "closed").await
}

async fn accept_closed(
    cfg: &Config,
    db: &store::Db<'_>,
    resource: &ResourceRow,
    paths: &Tree,
    head: &SessionState,
    disposition: &str,
) -> Result<CloseReceipt, Fault> {
    if denied(paths) {
        return Ok(incomplete("unexpected_certification", "closed", Some(head)));
    }
    let token_required = disposition != "replay" || paths.token_file.is_file();
    if let Err(reason) = live_recheck(db, resource, paths, false).await {
        return Ok(incomplete(reason, "closed", Some(head)));
    }
    if paths.token_file.is_file() {
        prove_revoked(cfg, resource).await?;
        let _ = fs::remove_file(&paths.token_file);
    } else if token_required && disposition == "closed" {
        return Ok(incomplete("token_missing", "closed", Some(head)));
    }
    store::set_stage(db, &resource.project_id, &resource.session_id, "closed", resource.harness_pid).await?;
    let mut receipt = from_head(disposition, "closed", false, head);
    receipt.disposition = disposition.to_owned();
    receipt.phase = "closed".to_owned();
    receipt.retryable = false;
    if !receipt.acceptable() {
        return Ok(incomplete("not_closed", "closed", Some(head)));
    }
    Ok(receipt)
}

async fn live_recheck(db: &store::Db<'_>, resource: &ResourceRow, paths: &Tree, require_token_file: bool) -> Result<(), &'static str> {
    if !workspace_absent(paths) {
        return Err("workspace_present");
    }
    match fs::symlink_metadata(&paths.snapshot) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err("snapshot_present_uncollected"),
        Err(_) => return Err("snapshot_unreadable"),
    }
    let pid = resource.harness_pid.or_else(|| read_harness_pid(&paths.checkpoint)).ok_or("harness_unproven")?;
    proc_absent(pid)?;
    if store::mapping_present(db, &resource.project_id, &resource.session_id).await.map_err(|_| "mapping_unproven")? != 0 {
        return Err("mapping_present");
    }
    if store::permits_held_by_session(db, &resource.session_id).await.map_err(|_| "permit_unproven")? != 0 {
        return Err("permit_held");
    }
    if require_token_file && !paths.token_file.is_file() {
        return Err("token_missing");
    }
    Ok(())
}

async fn ensure_replacement(db: &store::Db<'_>, resource: &ResourceRow, paths: &Tree) -> Result<String, Fault> {
    if !workspace_absent(paths) {
        return Err(Fault::Incomplete("workspace_present"));
    }
    store::mark_slot_destroyed(db, &resource.pool_slot).await?;
    if let Some(existing) = store::replacement_slot(db, &resource.pool_slot).await? {
        let dir = paths.root.join("pool").join(&existing);
        fs::create_dir_all(&dir).map_err(|_| Fault::Unavailable("pool"))?;
        return Ok(existing);
    }
    let slot = lifecycle::random_token("p", 8)?;
    let dir = paths.root.join("pool").join(&slot);
    fs::create_dir_all(&dir).map_err(|_| Fault::Unavailable("pool"))?;
    store::insert_pool_empty(db, &slot, &resource.pool_slot).await?;
    Ok(slot)
}

async fn ensure_step(
    cfg: &Config,
    identity: &session_transition::IdentityTuple,
    resource: &ResourceRow,
    head: &mut SessionState,
    suffix: &str,
    step: StepId,
    status: StepStatus,
    actual_absence: bool,
    evidence_hash: &str,
) -> Result<(), Fault> {
    if let Some(existing) = head.release.get(step) {
        if existing.status != status || existing.actual_absence != actual_absence || existing.evidence_hash != evidence_hash {
            return Err(Fault::Incomplete("step_mismatch"));
        }
        return Ok(());
    }
    let endpoint = endpoint_of(cfg);
    let now = head.last_now_ms.saturating_add(1);
    lifecycle::apply(
        cfg,
        &endpoint,
        identity,
        &resource.session_id,
        head.revision,
        head.owner_generation,
        &format!("{}-{suffix}", resource.event_prefix),
        now,
        &Event::RecordReleaseStep {
            step,
            status,
            evidence_hash: evidence_hash.to_owned(),
            actual_absence,
        },
    )
    .await?;
    *head = load_head(cfg, &resource.project_id, &resource.session_id).await?;
    Ok(())
}

async fn revoke_token(cfg: &Config, resource: &ResourceRow) -> Result<(), Fault> {
    let adapter = token_admin(cfg)?;
    adapter
        .revoke(RevokeScope::Session {
            project_id: resource.project_id.clone(),
            session_id: resource.session_id.clone(),
        })
        .await
        .map_err(token_fault)?;
    Ok(())
}

async fn prove_revoked(cfg: &Config, resource: &ResourceRow) -> Result<(), Fault> {
    let plain = fs::read_to_string(&tree(&cfg.state_dir, &resource.pool_slot, &resource.session_id).token_file)
        .map_err(|_| Fault::Incomplete("token_missing"))?;
    let presented = plain.trim();
    if presented.is_empty() {
        return Err(Fault::Incomplete("token_missing"));
    }
    let claims = serde_json::from_str(&resource.claims_json).map_err(|_| Fault::Incomplete("claims"))?;
    let cache = LookupCache::new();
    let endpoint = token_endpoint(&endpoint_of(cfg));
    let current = CurrentTuple {
        project_id: resource.project_id.clone(),
        principal_id: resource.principal_id.clone(),
        role: resource.role_name.clone(),
        claims,
        policy_version: resource.policy_version.clone(),
        knowledge_version: resource.knowledge_version.clone(),
        audience_url: cfg.audience_url.clone(),
        sandbox_id: Some(resource.workspace_id.clone()),
        session_id: Some(resource.session_id.clone()),
    };
    let transport = DeclaredTransport::EgressProxy {
        sandbox_id: resource.workspace_id.clone(),
        session_id: resource.session_id.clone(),
    };
    let request = LookupRequest {
        endpoint: &endpoint,
        user: sql::TOKEN_RUNTIME,
        password: &cfg.pg.token_runtime_password,
        ca_pem: cfg.pg.ca_pem.as_bytes(),
        cache: &cache,
        presented_token: presented,
        current: &current,
        gateway_audience: &cfg.audience_url,
        transport: &transport,
    };
    match lookup(request).await {
        Err(StoreError::Revoked) => Ok(()),
        Err(err) => Err(token_fault(err)),
        Ok(_) => Err(Fault::Incomplete("token_live")),
    }
}

fn token_admin(cfg: &Config) -> Result<TrustedControlAdapter, Fault> {
    TrustedControlAdapter::new(
        token_endpoint(&endpoint_of(cfg)),
        sql::TOKEN_ADMIN,
        cfg.pg.token_admin_password.clone(),
        cfg.pg.ca_pem.as_bytes().to_vec(),
    )
    .map_err(|_| Fault::Unavailable("token_login"))
}

fn token_fault(err: StoreError) -> Fault {
    match err {
        StoreError::CommitUnknown => Fault::Unavailable("commit_unknown"),
        StoreError::Connect | StoreError::Tls => Fault::Unavailable("store"),
        StoreError::Revoked => Fault::Incomplete("revoked"),
        _ => Fault::Incomplete("token_lookup"),
    }
}

fn harness_argv() -> Vec<String> {
    vec![
        "/bin/bash".to_owned(),
        "-c".to_owned(),
        "trap 'sleep 6; exit 0' TERM; while true; do sleep 1; done".to_owned(),
        "_".to_owned(),
    ]
}

fn fixture_ok(receipt: &FixtureReceipt, outputs_sha: &str, transcript_sha: &str) -> bool {
    receipt.disposition == "released_unclosed"
        && receipt.phase.as_deref() == Some("releasing")
        && !receipt.physical_release_certified
        && !receipt.production_sandbox
        && !receipt.complete_release_called
        && receipt.local_fixture
        && contains_hash(receipt, "s3", outputs_sha)
        && contains_hash(receipt, "s4", transcript_sha)
        && receipt.steps.iter().any(|step| step.id == "s8" && step.reducer == "confirmed")
}

fn contains_hash(receipt: &FixtureReceipt, id: &str, sha: &str) -> bool {
    let needle = format!("\"content_sha256\":\"{sha}\"");
    receipt.steps.iter().any(|step| step.id == id && step.evidence_json.as_ref().is_some_and(|json| json.contains(&needle)))
}

fn fixture_confirmed(state: &SessionState) -> bool {
    confirmed(state, StepId::S1, false)
        && confirmed(state, StepId::S2, false)
        && confirmed(state, StepId::S3, false)
        && confirmed(state, StepId::S4, false)
        && confirmed(state, StepId::S8, true)
}

fn confirmed(state: &SessionState, step: StepId, absence: bool) -> bool {
    state.release.get(step).is_some_and(|record| {
        record.status == StepStatus::Confirmed && record.actual_absence == absence && is_hex64(&record.evidence_hash)
    })
}

fn expect_collect(state: &SessionState, step: StepId, kind: &str, sha: &str, len: usize) -> Result<(), Fault> {
    let record = state.release.get(step).ok_or(Fault::Incomplete("evidence_mismatch"))?;
    let hash = collect_evidence_hash(kind, sha, u64::try_from(len).unwrap_or(u64::MAX))?;
    if record.evidence_hash != hash {
        return Err(Fault::Incomplete("evidence_mismatch"));
    }
    Ok(())
}

fn collect_evidence_hash(kind: &str, sha: &str, len: u64) -> Result<String, Fault> {
    #[derive(Serialize)]
    struct CollectEvidence<'a> {
        adapter: &'static str,
        kind: &'a str,
        content_sha256: &'a str,
        len: u64,
    }
    let bytes = serde_json::to_vec(&CollectEvidence {
        adapter: "fixture_bounded_collect",
        kind,
        content_sha256: sha,
        len,
    })
    .map_err(|_| Fault::Incomplete("evidence_mismatch"))?;
    hash_bytes(&bytes)
}

fn evidence_hash(adapter: &str, observation: &str) -> Result<String, Fault> {
    #[derive(Serialize)]
    struct Evidence<'a> {
        adapter: &'a str,
        observation: &'a str,
    }
    let bytes = serde_json::to_vec(&Evidence { adapter, observation }).map_err(|_| Fault::Malformed("evidence"))?;
    hash_bytes(&bytes)
}

fn hash_bytes(bytes: &[u8]) -> Result<String, Fault> {
    Ok(hex(&sha256(bytes).ok_or(Fault::Malformed("hash"))?))
}

fn hash_file(path: &Path) -> Result<String, Fault> {
    let bytes = fs::read(path).map_err(|_| Fault::Incomplete("artifact_missing"))?;
    hash_bytes(&bytes)
}

fn release_steps(state: &SessionState) -> Result<ReleaseSteps, Fault> {
    let steps = ReleaseSteps {
        s1: state.release.s1.clone(),
        s2: state.release.s2.clone(),
        s3: state.release.s3.clone(),
        s4: state.release.s4.clone(),
        s5: state.release.s5.clone(),
        s6: state.release.s6.clone(),
        s7: state.release.s7.clone(),
        s8: state.release.s8.clone(),
        s9: state.release.s9.clone(),
        s10: state.release.s10.clone(),
        s11: state.release.s11.clone(),
    };
    if steps.s1.is_none()
        || steps.s2.is_none()
        || steps.s3.is_none()
        || steps.s4.is_none()
        || steps.s5.is_none()
        || steps.s6.is_none()
        || steps.s7.is_none()
        || steps.s8.is_none()
        || steps.s9.is_none()
        || steps.s10.is_none()
        || steps.s11.is_none()
    {
        return Err(Fault::Incomplete("release_incomplete"));
    }
    Ok(steps)
}

fn persisted(head: &SessionState) -> CloseReceipt {
    from_head("fixture_persisted", phase_label(head.phase).as_str(), true, head)
}

fn incomplete(reason: &str, phase: &str, head: Option<&SessionState>) -> CloseReceipt {
    let steps = head.map(steps_from).unwrap_or_default();
    CloseReceipt {
        contract: CONTRACT,
        disposition: "incomplete".to_owned(),
        reason: reason.to_owned(),
        phase: phase.to_owned(),
        production_signed: PRODUCTION_SIGNED,
        deployment: DEPLOYMENT,
        retryable: true,
        reducer_physical_release_certified: false,
        steps,
    }
}

fn from_head(disposition: &str, phase: &str, retryable: bool, head: &SessionState) -> CloseReceipt {
    CloseReceipt {
        contract: CONTRACT,
        disposition: disposition.to_owned(),
        reason: disposition.to_owned(),
        phase: phase.to_owned(),
        production_signed: PRODUCTION_SIGNED,
        deployment: DEPLOYMENT,
        retryable,
        reducer_physical_release_certified: false,
        steps: steps_from(head),
    }
}

fn steps_from(head: &SessionState) -> Vec<StepOut> {
    let pairs = [
        ("s1", head.release.s1.as_ref()),
        ("s2", head.release.s2.as_ref()),
        ("s3", head.release.s3.as_ref()),
        ("s4", head.release.s4.as_ref()),
        ("s5", head.release.s5.as_ref()),
        ("s6", head.release.s6.as_ref()),
        ("s7", head.release.s7.as_ref()),
        ("s8", head.release.s8.as_ref()),
        ("s9", head.release.s9.as_ref()),
        ("s10", head.release.s10.as_ref()),
        ("s11", head.release.s11.as_ref()),
    ];
    pairs.into_iter().map(|(id, record)| step_out(id, record)).collect()
}

fn step_out(id: &str, record: Option<&StepRecord>) -> StepOut {
    match record {
        Some(record) => StepOut {
            id: id.to_owned(),
            status: match record.status {
                StepStatus::Confirmed => "confirmed",
                StepStatus::Skipped => "skipped",
            }
            .to_owned(),
            actual_absence: record.actual_absence,
            evidence_sha256: record.evidence_hash.clone(),
        },
        None => StepOut {
            id: id.to_owned(),
            status: "absent".to_owned(),
            actual_absence: false,
            evidence_sha256: String::new(),
        },
    }
}

fn workspace_absent(paths: &Tree) -> bool {
    matches!(fs::symlink_metadata(&paths.workspace), Err(err) if err.kind() == std::io::ErrorKind::NotFound)
}

fn proc_absent(pid: i64) -> Result<(), &'static str> {
    let Ok(pid) = u32::try_from(pid) else {
        return Err("harness_unproven");
    };
    if pid == 0 || fs::read_dir("/proc").is_err() {
        return Err(if pid == 0 { "harness_unproven" } else { "proc_unreadable" });
    }
    match fs::symlink_metadata(format!("/proc/{pid}")) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("proc_unreadable"),
        Ok(_) => Err("harness_alive"),
    }
}

fn read_harness_pid(dir: &Path) -> Option<i64> {
    let bytes = fs::read(dir.join("checkpoint.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    i64::try_from(value.get("harness")?.get("pid")?.as_u64()?).ok()
}

fn denied(paths: &Tree) -> bool {
    paths.checkpoint.join("certification_denied").is_file()
}

fn write_marker(dir: &Path, name: &str) -> Result<(), Fault> {
    fs::create_dir_all(dir).map_err(|_| Fault::Unavailable("checkpoint"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join(name))
        .map_err(|_| Fault::Unavailable("checkpoint"))?;
    file.write_all(b"1\n").map_err(|_| Fault::Unavailable("checkpoint"))?;
    file.sync_all().map_err(|_| Fault::Unavailable("checkpoint"))?;
    Ok(())
}

fn safe_reason(reason: &str) -> String {
    if (1..=64).contains(&reason.len()) && reason.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
        reason.to_owned()
    } else {
        "release_incomplete".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{CloseReceipt, StepOut};
    use crate::config::{DEPLOYMENT, PRODUCTION_SIGNED};

    fn step(id: &str, status: &str, absence: bool) -> StepOut {
        StepOut {
            id: id.to_owned(),
            status: status.to_owned(),
            actual_absence: absence,
            evidence_sha256: "ab".repeat(32),
        }
    }

    fn closed_steps() -> Vec<StepOut> {
        ["s1", "s2", "s3", "s4", "s6", "s7", "s8", "s9", "s10", "s11"]
            .into_iter()
            .map(|id| step(id, "confirmed", id == "s8"))
            .chain(std::iter::once(step("s5", "skipped", false)))
            .collect()
    }

    fn receipt(disposition: &str, phase: &str, steps: Vec<StepOut>) -> CloseReceipt {
        CloseReceipt {
            contract: crate::config::CONTRACT,
            disposition: disposition.to_owned(),
            reason: disposition.to_owned(),
            phase: phase.to_owned(),
            production_signed: PRODUCTION_SIGNED,
            deployment: DEPLOYMENT,
            retryable: false,
            reducer_physical_release_certified: false,
            steps,
        }
    }

    #[test]
    fn releasing_and_unproven_absence_are_not_closed() {
        let mut steps = closed_steps();
        assert!(receipt("closed", "closed", steps.clone()).acceptable());
        assert!(!receipt("fixture_persisted", "releasing", steps.clone()).acceptable());
        assert!(!receipt("incomplete", "releasing", Vec::new()).acceptable());
        if let Some(s8) = steps.iter_mut().find(|step| step.id == "s8") {
            s8.actual_absence = false;
        }
        assert!(!receipt("closed", "closed", steps).acceptable());
        let mut signed = receipt("closed", "closed", closed_steps());
        signed.production_signed = true;
        assert!(!signed.acceptable());
        signed.production_signed = false;
        signed.reducer_physical_release_certified = true;
        assert!(!signed.acceptable());
    }
}
