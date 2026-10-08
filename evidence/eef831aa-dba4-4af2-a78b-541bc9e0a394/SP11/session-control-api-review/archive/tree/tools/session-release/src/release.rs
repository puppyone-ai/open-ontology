//! Bounded local-fixture release. Cancel is persisted before any signal.
//! `complete_release` is never sent. Production actions stay unsupported.

use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use session_store::Endpoint;
use session_transition::{is_token, IdentityTuple, Phase, SessionState, StepId, StepStatus};

use crate::backend::{self, BackendReport};
use crate::checkpoint::{self, Checkpoint, HarnessCheckpoint, CHECKPOINT_CONTRACT};
use crate::clock::{self, T0};
use crate::drive::{self, Cursor, DriveFail};
use crate::harness::{self, StopReport};
use crate::model::{blank, ReleaseReceipt, ReleaseRequest, StepView, CONTRACT};
use crate::paths::{self, Class, ReadBound};
use crate::sha256_hex;

const DENY_QUERY: &[&str] = &[
    "postgres",
    "oo_fixture_admin",
    "oo_session_admin",
    "oo_session_runtime",
    "oo_session_cancel",
];

const STEP_IDS: [(&str, StepId); 11] = [
    ("s1", StepId::S1),
    ("s2", StepId::S2),
    ("s3", StepId::S3),
    ("s4", StepId::S4),
    ("s5", StepId::S5),
    ("s6", StepId::S6),
    ("s7", StepId::S7),
    ("s8", StepId::S8),
    ("s9", StepId::S9),
    ("s10", StepId::S10),
    ("s11", StepId::S11),
];

pub struct RunResult {
    pub exit_code: u8,
    pub receipt: ReleaseReceipt,
}

struct Secret(String);

impl Secret {
    fn take(slot: &mut String) -> Self {
        Self(std::mem::take(slot))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        bytes.fill(0);
        std::hint::black_box(bytes);
    }
}

struct Mark {
    orchestrator: String,
    hash: Option<String>,
    json: Option<String>,
}

struct StoreCtx<'a> {
    endpoint: &'a Endpoint,
    runtime_user: &'a str,
    runtime_password: &'a str,
    ca_pem: &'a [u8],
    project_id: &'a str,
    session_id: &'a str,
    identity: &'a IdentityTuple,
    prefix: &'a str,
}

pub async fn run(mut request: ReleaseRequest) -> RunResult {
    let runtime_password = Secret::take(&mut request.runtime_password);
    let cancel_password = Secret::take(&mut request.cancel_password);
    run_inner(request, runtime_password, cancel_password).await
}

async fn run_inner(request: ReleaseRequest, runtime_password: Secret, cancel_password: Secret) -> RunResult {
    if let Err(reason) = validate(&request, runtime_password.as_str(), cancel_password.as_str()) {
        return refuse("malformed", reason);
    }
    if let Err(reason) = gate_paths(&request) {
        return refuse(reason, reason);
    }
    let endpoint = Endpoint {
        host: request.host.clone(),
        port: request.port,
        database: request.database.clone(),
    };
    let ca_pem = request.ca_pem.clone();
    let cursor = match drive::load(
        &endpoint,
        &request.runtime_user,
        runtime_password.as_str(),
        ca_pem.as_bytes(),
        &request.project_id,
        &request.session_id,
    )
    .await
    {
        Ok(cursor) => cursor,
        Err(err) => return from_drive(err),
    };
    if !same_identity(&cursor.state, &request) {
        return stamp(refuse("identity_mismatch", "identity_mismatch"), &cursor);
    }
    if cursor.phase == Phase::Closed {
        return stamp(refuse("closed", "closed"), &cursor);
    }
    if cursor.phase == Phase::Releasing {
        return resume(&request, &endpoint, &ca_pem, runtime_password.as_str(), cancel_password.as_str(), cursor).await;
    }
    if cursor.revision != request.expected_revision || cursor.generation != request.expected_owner_generation {
        return stamp(refuse("revision_conflict", "revision_conflict"), &cursor);
    }
    fresh(
        &request,
        &endpoint,
        &ca_pem,
        runtime_password.as_str(),
        cancel_password.as_str(),
        cursor,
    )
    .await
}

async fn fresh(
    request: &ReleaseRequest,
    endpoint: &Endpoint,
    ca_pem: &str,
    runtime_password: &str,
    cancel_password: &str,
    mut cursor: Cursor,
) -> RunResult {
    let mut receipt = blank("incomplete", "incomplete");
    receipt.database_used = true;
    receipt.deadline_basis = "monotonic_and_wall";
    receipt.resumed = false;
    let mut marks = initial_marks();
    let ctx = StoreCtx {
        endpoint,
        runtime_user: &request.runtime_user,
        runtime_password,
        ca_pem: ca_pem.as_bytes(),
        project_id: &request.project_id,
        session_id: &request.session_id,
        identity: &request.identity,
        prefix: &request.event_prefix,
    };
    let cancel_now = request.now_ms.max(cursor.now_ms);
    cursor = match drive::apply_cancel(
        ctx.endpoint,
        ctx.runtime_user,
        ctx.runtime_password,
        ctx.ca_pem,
        ctx.project_id,
        ctx.session_id,
        ctx.identity,
        &cursor,
        &event_id(ctx.prefix, "cancel"),
        cancel_now,
        request.cancel_trigger,
    )
    .await
    {
        Ok(next) => next,
        Err(err) => return stamp(from_drive(err), &cursor),
    };
    if cursor.phase != Phase::Releasing {
        weaken(&mut receipt, "unexpected_phase");
        return conclude(receipt, &cursor, &marks, None, false).await;
    }
    stamp_receipt(&mut receipt, &cursor);
    let Some(t0) = clock::capture() else {
        weaken(&mut receipt, "clock_backward");
        receipt.slo_miss = true;
        return conclude(receipt, &cursor, &marks, None, false).await;
    };
    receipt.t0_wall_epoch_ms = Some(t0.wall_epoch_ms);
    let checkpoint_dir = PathBuf::from(&request.checkpoint_dir);
    let mut checkpoint = Checkpoint {
        contract: CHECKPOINT_CONTRACT.to_owned(),
        project_id: request.project_id.clone(),
        session_id: request.session_id.clone(),
        owner_generation: cursor.generation,
        pre_revision: request.expected_revision,
        durable_revision_after_cancel: cursor.revision,
        t0_wall_epoch_ms: t0.wall_epoch_ms,
        cancel_event_id: event_id(&request.event_prefix, "cancel"),
        backend_pid: request.backend_pid,
        application_name: request.application_name.clone(),
        query_role: request.query_role.clone(),
        harness: None,
    };
    if checkpoint::save(&checkpoint_dir, &checkpoint).is_err() || checkpoint::marker(&checkpoint_dir, "cancel_persisted").is_err()
    {
        weaken(&mut receipt, "checkpoint_io");
        return conclude(receipt, &cursor, &marks, Some(t0), false).await;
    }
    let mut armed = match harness::spawn(&request.harness_argv, &checkpoint_dir) {
        Ok(armed) => armed,
        Err(reason) => {
            weaken(&mut receipt, reason);
            return conclude(receipt, &cursor, &marks, Some(t0), false).await;
        }
    };
    receipt.harness_pid = Some(armed.pid);
    checkpoint.harness = Some(HarnessCheckpoint {
        pid: armed.pid,
        pgid: armed.pgid,
        starttime: armed.starttime,
        signaled: false,
    });
    if checkpoint::save(&checkpoint_dir, &checkpoint).is_err() || checkpoint::marker(&checkpoint_dir, "harness_armed").is_err() {
        let budget = std::time::Duration::from_secs(10);
        let _ = tokio::task::spawn_blocking(move || harness::stop(&mut armed, budget)).await;
        weaken(&mut receipt, "checkpoint_io");
        return conclude(receipt, &cursor, &marks, Some(t0), false).await;
    }
    let Some(elapsed) = clock::elapsed_ms(t0, false) else {
        let budget = std::time::Duration::from_secs(10);
        let _ = tokio::task::spawn_blocking(move || harness::stop(&mut armed, budget)).await;
        weaken(&mut receipt, "clock_backward");
        receipt.slo_miss = true;
        return conclude(receipt, &cursor, &marks, Some(t0), false).await;
    };
    let already_missed = clock::missed(elapsed, clock::ADMISSION_MS);
    if already_missed {
        receipt.slo_miss = true;
    }
    let backend_deadline = Instant::now() + clock::remaining(elapsed, clock::ADMISSION_MS);
    let harness_budget = clock::remaining(elapsed, clock::HARNESS_MS);
    let harness_t0 = t0;
    let stop_handle = tokio::task::spawn_blocking(move || {
        let report = harness::stop(&mut armed, harness_budget);
        let elapsed = clock::elapsed_ms(harness_t0, false);
        (report, elapsed)
    });
    let report = backend::observe_and_cancel(
        endpoint,
        &request.cancel_user,
        cancel_password,
        ca_pem.as_bytes(),
        request.backend_pid,
        &request.application_name,
        &request.query_role,
        &request.database,
        backend_deadline,
        already_missed,
    )
    .await;
    receipt.backend_elapsed_ms = clock::elapsed_ms(t0, false);
    receipt.backend_basis = Some(report.basis.to_owned());
    if report.slo_miss {
        receipt.slo_miss = true;
    }
    if backend::confirmable(&report) {
        let evidence = backend_evidence(request, &report, "monotonic_and_wall");
        if let Err(err) = persist(&ctx, &mut cursor, &mut marks, StepId::S2, &evidence, false).await {
            let _ = stop_handle.await;
            return stamp(carry(from_drive(err), &receipt), &cursor);
        }
    } else {
        remember_evidence(&mut marks, StepId::S2, report.basis, &backend_evidence(request, &report, "monotonic_and_wall"));
        weaken(&mut receipt, report.basis);
    }
    let (stop, harness_elapsed) = match stop_handle.await {
        Ok(pair) => pair,
        Err(_) => {
            note(&mut marks, StepId::S1, "unavailable");
            weaken(&mut receipt, "harness_unavailable");
            return conclude(receipt, &cursor, &marks, Some(t0), false).await;
        }
    };
    receipt.harness_elapsed_ms = harness_elapsed;
    if stop.basis == "timeout" {
        receipt.slo_miss = true;
    }
    if stop.basis == "stopped" {
        let evidence = harness_evidence(&stop, receipt.harness_elapsed_ms.unwrap_or(0));
        if let Err(err) = persist(&ctx, &mut cursor, &mut marks, StepId::S1, &evidence, false).await {
            return stamp(carry(from_drive(err), &receipt), &cursor);
        }
    } else {
        remember_evidence(&mut marks, StepId::S1, stop.basis, &harness_evidence(&stop, receipt.harness_elapsed_ms.unwrap_or(0)));
        weaken(&mut receipt, stop.basis);
    }
    if stop.basis == "stopped"
        && let Err(err) = collect_and_destroy(request, &ctx, &mut cursor, &mut marks, &mut receipt, t0, false).await
    {
        apply_drive_err(&mut receipt, &err);
    }
    match reload(&ctx).await {
        Ok(durable) => conclude(receipt, &durable, &marks, Some(t0), false).await,
        Err(err) => {
            apply_drive_err(&mut receipt, &err);
            conclude(receipt, &cursor, &marks, Some(t0), false).await
        }
    }
}

async fn resume(
    request: &ReleaseRequest,
    endpoint: &Endpoint,
    ca_pem: &str,
    runtime_password: &str,
    cancel_password: &str,
    mut cursor: Cursor,
) -> RunResult {
    let mut receipt = blank("incomplete", "incomplete");
    receipt.database_used = true;
    receipt.resumed = true;
    receipt.deadline_basis = "wall_epoch";
    stamp_receipt(&mut receipt, &cursor);
    let mut marks = initial_marks();
    let checkpoint_dir = PathBuf::from(&request.checkpoint_dir);
    let checkpoint = match checkpoint::load(&checkpoint_dir) {
        Ok(checkpoint) => checkpoint,
        Err(reason) => {
            weaken(&mut receipt, reason);
            return conclude(receipt, &cursor, &marks, None, true).await;
        }
    };
    if !checkpoint_matches(&checkpoint, request, &cursor) {
        weaken(&mut receipt, "checkpoint_mismatch");
        return conclude(receipt, &cursor, &marks, None, true).await;
    }
    let t0 = T0 {
        mono: Instant::now(),
        wall_epoch_ms: checkpoint.t0_wall_epoch_ms,
    };
    receipt.t0_wall_epoch_ms = Some(checkpoint.t0_wall_epoch_ms);
    let Some(elapsed) = clock::elapsed_ms(t0, true) else {
        weaken(&mut receipt, "clock_backward");
        receipt.slo_miss = true;
        return conclude(receipt, &cursor, &marks, Some(t0), true).await;
    };
    let already_missed = clock::missed(elapsed, clock::ADMISSION_MS);
    if already_missed {
        receipt.slo_miss = true;
    }
    match checkpoint.harness {
        Some(harness) => {
            receipt.harness_pid = Some(harness.pid);
            let observed = harness::observe_recorded(harness.pid, harness.starttime);
            let status = if observed == "alive" { "harness_not_owned" } else if observed == "absent" { "unproven_absence" } else { observed };
            note(&mut marks, StepId::S1, status);
            weaken(&mut receipt, status);
        }
        None => {
            note(&mut marks, StepId::S1, "harness_unarmed");
            weaken(&mut receipt, "harness_unarmed");
        }
    }
    let deadline = Instant::now() + clock::remaining(elapsed, clock::ADMISSION_MS);
    let report = backend::observe_and_cancel(
        endpoint,
        &request.cancel_user,
        cancel_password,
        ca_pem.as_bytes(),
        request.backend_pid,
        &request.application_name,
        &request.query_role,
        &request.database,
        deadline,
        already_missed,
    )
    .await;
    receipt.backend_elapsed_ms = clock::elapsed_ms(t0, true);
    receipt.backend_basis = Some(report.basis.to_owned());
    if report.slo_miss {
        receipt.slo_miss = true;
    }
    let ctx = StoreCtx {
        endpoint,
        runtime_user: &request.runtime_user,
        runtime_password,
        ca_pem: ca_pem.as_bytes(),
        project_id: &request.project_id,
        session_id: &request.session_id,
        identity: &request.identity,
        prefix: &request.event_prefix,
    };
    let evidence = backend_evidence(request, &report, "wall_epoch");
    if backend::confirmable(&report) && cursor.state.release.get(StepId::S2).is_none() {
        if let Err(err) = persist(&ctx, &mut cursor, &mut marks, StepId::S2, &evidence, false).await {
            return stamp(carry(from_drive(err), &receipt), &cursor);
        }
    } else if backend::confirmable(&report) {
        remember_evidence(&mut marks, StepId::S2, "confirmed", &evidence);
    } else {
        remember_evidence(&mut marks, StepId::S2, report.basis, &evidence);
        weaken(&mut receipt, report.basis);
    }
    match reload(&ctx).await {
        Ok(durable) => conclude(receipt, &durable, &marks, Some(t0), true).await,
        Err(err) => {
            apply_drive_err(&mut receipt, &err);
            conclude(receipt, &cursor, &marks, Some(t0), true).await
        }
    }
}

async fn reload(ctx: &StoreCtx<'_>) -> Result<Cursor, DriveFail> {
    drive::load(
        ctx.endpoint,
        ctx.runtime_user,
        ctx.runtime_password,
        ctx.ca_pem,
        ctx.project_id,
        ctx.session_id,
    )
    .await
}

fn apply_drive_err(receipt: &mut ReleaseReceipt, err: &DriveFail) {
    let unavailable = matches!(
        err,
        DriveFail::Unavailable | DriveFail::UnknownCommit | DriveFail::Store("tls" | "connect" | "commit_unknown")
    );
    if unavailable {
        receipt.disposition = "store_unavailable";
        receipt.reason = err_reason(err).to_owned();
    } else {
        weaken(receipt, err_reason(err));
    }
}

async fn collect_and_destroy(
    request: &ReleaseRequest,
    ctx: &StoreCtx<'_>,
    cursor: &mut Cursor,
    marks: &mut [Mark; 11],
    receipt: &mut ReleaseReceipt,
    t0: T0,
    resumed: bool,
) -> Result<(), DriveFail> {
    let Some(elapsed) = clock::elapsed_ms(t0, resumed) else {
        weaken(receipt, "clock_backward");
        receipt.slo_miss = true;
        return Ok(());
    };
    if clock::missed(elapsed, clock::RESOURCE_MS) {
        receipt.slo_miss = true;
        weaken(receipt, "resource_deadline");
        note(marks, StepId::S3, "resource_deadline");
        note(marks, StepId::S4, "resource_deadline");
        note(marks, StepId::S8, "resource_deadline");
        return Ok(());
    }
    let root = PathBuf::from(&request.workspace_root);
    let workspace = PathBuf::from(&request.workspace_path);
    let outputs = PathBuf::from(&request.outputs_path);
    let transcript = PathBuf::from(&request.transcript_path);
    let outputs_ok = match read_artifact(&workspace, &outputs, &request.owner_uuid) {
        Ok(bytes) => persist_collect(ctx, cursor, marks, StepId::S3, "outputs", &bytes).await?,
        Err(reason) => {
            note(marks, StepId::S3, reason);
            weaken(receipt, reason);
            false
        }
    };
    let transcript_ok = match read_artifact(&workspace, &transcript, &request.owner_uuid) {
        Ok(bytes) => persist_collect(ctx, cursor, marks, StepId::S4, "transcript", &bytes).await?,
        Err(reason) => {
            note(marks, StepId::S4, reason);
            weaken(receipt, reason);
            false
        }
    };
    if !outputs_ok || !transcript_ok {
        note(marks, StepId::S8, "not_destroyed");
        return Ok(());
    }
    let Some(after) = clock::elapsed_ms(t0, resumed) else {
        weaken(receipt, "clock_backward");
        receipt.slo_miss = true;
        return Ok(());
    };
    if clock::missed(after, clock::RESOURCE_MS) {
        receipt.slo_miss = true;
        weaken(receipt, "resource_deadline");
        note(marks, StepId::S8, "resource_deadline");
        return Ok(());
    }
    if paths::destroy_owned(&root, &workspace, &request.owner_uuid).is_err() {
        note(marks, StepId::S8, "destroy_failed");
        weaken(receipt, "destroy_failed");
        return Ok(());
    }
    if !matches!(std::fs::symlink_metadata(&workspace), Err(err) if err.kind() == std::io::ErrorKind::NotFound) {
        note(marks, StepId::S8, "still_present");
        weaken(receipt, "still_present");
        return Ok(());
    }
    let evidence = WorkspaceEvidence {
        adapter: "local_fixture_directory",
        actual_absence: true,
        verified: "not_found",
    };
    persist(ctx, cursor, marks, StepId::S8, &evidence, true).await
}

async fn persist_collect(
    ctx: &StoreCtx<'_>,
    cursor: &mut Cursor,
    marks: &mut [Mark; 11],
    step: StepId,
    kind: &str,
    bytes: &[u8],
) -> Result<bool, DriveFail> {
    let Some(content_sha256) = sha256_hex(bytes) else {
        note(marks, step, "evidence_hash");
        return Ok(false);
    };
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let evidence = CollectEvidence {
        adapter: "fixture_bounded_collect",
        kind,
        content_sha256: &content_sha256,
        len,
    };
    persist(ctx, cursor, marks, step, &evidence, false).await?;
    Ok(true)
}

fn read_artifact(workspace: &Path, artifact: &Path, uuid: &str) -> Result<Vec<u8>, &'static str> {
    match paths::read_bounded(workspace, artifact, uuid) {
        ReadBound::Ready(bytes) => Ok(bytes),
        ReadBound::TooBig => Err("too_big"),
        ReadBound::Missing => Err("missing"),
        ReadBound::Foreign => Err("artifact_foreign"),
    }
}

async fn persist(
    ctx: &StoreCtx<'_>,
    cursor: &mut Cursor,
    marks: &mut [Mark; 11],
    step: StepId,
    evidence: &impl Serialize,
    actual_absence: bool,
) -> Result<(), DriveFail> {
    let (hash, json) = hashed(evidence).map_err(|_| DriveFail::Store("evidence"))?;
    let now = cursor.now_ms.saturating_add(1);
    let next = drive::apply_step(
        ctx.endpoint,
        ctx.runtime_user,
        ctx.runtime_password,
        ctx.ca_pem,
        ctx.project_id,
        ctx.session_id,
        ctx.identity,
        cursor,
        &event_id(ctx.prefix, step_suffix(step)),
        now,
        step,
        &hash,
        actual_absence,
    )
    .await?;
    *cursor = next;
    let mark = &mut marks[idx(step)];
    mark.orchestrator = "confirmed".to_owned();
    mark.hash = Some(hash);
    mark.json = Some(json);
    Ok(())
}

async fn conclude(
    mut receipt: ReleaseReceipt,
    cursor: &Cursor,
    marks: &[Mark; 11],
    t0: Option<T0>,
    resumed: bool,
) -> RunResult {
    if let Some(t0) = t0 {
        match clock::elapsed_ms(t0, resumed) {
            Some(elapsed) => receipt.elapsed_ms = Some(elapsed),
            None => {
                weaken(&mut receipt, "clock_backward");
                receipt.slo_miss = true;
            }
        }
    }
    stamp_receipt(&mut receipt, cursor);
    push_steps(&mut receipt, marks, &cursor.state);
    if receipt.disposition != "store_unavailable" && success_ready(&receipt) {
        receipt.disposition = "released_unclosed";
        receipt.reason = "local_fixture_not_certified".to_owned();
    }
    let exit_code = exit_for(&receipt);
    RunResult { exit_code, receipt }
}

fn push_steps(receipt: &mut ReleaseReceipt, marks: &[Mark; 11], state: &SessionState) {
    receipt.steps = STEP_IDS
        .into_iter()
        .enumerate()
        .map(|(index, (id, step))| {
            let reducer = match state.release.get(step) {
                Some(record) if record.status == StepStatus::Confirmed && (step != StepId::S8 || record.actual_absence) => "confirmed",
                Some(record) if record.status == StepStatus::Confirmed => "present",
                Some(_) => "skipped",
                None => "absent",
            };
            StepView {
                id,
                orchestrator: marks[index].orchestrator.clone(),
                reducer: reducer.to_owned(),
                evidence_sha256: marks[index].hash.clone(),
                evidence_json: marks[index].json.clone(),
            }
        })
        .collect();
}

fn success_ready(receipt: &ReleaseReceipt) -> bool {
    if receipt.physical_release_certified
        || receipt.production_sandbox
        || receipt.complete_release_called
        || receipt.cancel_return_was_sufficient
        || receipt.auth_provider_used
        || receipt.runtime_used
        || !receipt.local_fixture
        || !receipt.database_used
        || receipt.slo_miss
        || receipt.containment != "local_fixture"
        || receipt.phase.as_deref() != Some("releasing")
        || receipt.steps.len() != 11
        || receipt.unsupported.iter().any(|item| item.status != "unsupported")
    {
        return false;
    }
    for step in &receipt.steps {
        let runnable = matches!(step.id, "s1" | "s2" | "s3" | "s4" | "s8");
        if runnable {
            if step.orchestrator != "confirmed" || step.reducer != "confirmed" || step.evidence_sha256.is_none() {
                return false;
            }
        } else if step.orchestrator != "unsupported" || step.reducer != "absent" {
            return false;
        }
    }
    let Some(workspace) = receipt.steps.iter().find(|step| step.id == "s8") else {
        return false;
    };
    let Some(json) = &workspace.evidence_json else {
        return false;
    };
    json.contains("\"adapter\":\"local_fixture_directory\"") && json.contains("\"actual_absence\":true")
}

fn remember_evidence(marks: &mut [Mark; 11], step: StepId, status: &str, evidence: &impl Serialize) {
    match hashed(evidence) {
        Ok((hash, json)) => {
            let mark = &mut marks[idx(step)];
            mark.orchestrator = status.to_owned();
            mark.hash = Some(hash);
            mark.json = Some(json);
        }
        Err(()) => note(marks, step, status),
    }
}

fn hashed(value: &impl Serialize) -> Result<(String, String), ()> {
    let bytes = serde_json::to_vec(value).map_err(|_| ())?;
    let hash = sha256_hex(&bytes).ok_or(())?;
    let text = String::from_utf8(bytes).map_err(|_| ())?;
    Ok((hash, text))
}

fn note(marks: &mut [Mark; 11], step: StepId, status: &str) {
    marks[idx(step)].orchestrator = status.to_owned();
}

fn initial_marks() -> [Mark; 11] {
    std::array::from_fn(|index| Mark {
        orchestrator: if matches!(index, 0 | 1 | 2 | 3 | 7) {
            "not_run".to_owned()
        } else {
            "unsupported".to_owned()
        },
        hash: None,
        json: None,
    })
}

fn idx(step: StepId) -> usize {
    match step {
        StepId::S1 => 0,
        StepId::S2 => 1,
        StepId::S3 => 2,
        StepId::S4 => 3,
        StepId::S5 => 4,
        StepId::S6 => 5,
        StepId::S7 => 6,
        StepId::S8 => 7,
        StepId::S9 => 8,
        StepId::S10 => 9,
        StepId::S11 => 10,
    }
}

fn step_suffix(step: StepId) -> &'static str {
    match step {
        StepId::S1 => "s1",
        StepId::S2 => "s2",
        StepId::S3 => "s3",
        StepId::S4 => "s4",
        StepId::S8 => "s8",
        StepId::S5 | StepId::S6 | StepId::S7 | StepId::S9 | StepId::S10 | StepId::S11 => "unsupported",
    }
}

fn event_id(prefix: &str, suffix: &str) -> String {
    format!("{prefix}-{suffix}")
}

fn backend_evidence<'a>(request: &'a ReleaseRequest, report: &'a BackendReport, deadline_basis: &'a str) -> BackendEvidence<'a> {
    BackendEvidence {
        adapter: "fixture_pg_backend_cancel",
        basis: report.basis,
        cancel_invoked: report.cancel_invoked,
        cancel_returned: report.cancel_returned,
        cancel_return_was_sufficient: false,
        observed_state: &report.observed_state,
        slo_miss: report.slo_miss,
        pid: request.backend_pid,
        application_name: &request.application_name,
        query_role: &request.query_role,
        deadline_basis,
    }
}

fn harness_evidence(report: &StopReport, elapsed_ms: u64) -> HarnessEvidence {
    HarnessEvidence {
        adapter: "direct_child_harness",
        basis: report.basis,
        signaled: report.signaled,
        force_killed: report.force_killed,
        escaped: report.escaped.len(),
        within_deadline: !clock::missed(elapsed_ms, clock::HARNESS_MS),
    }
}

#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct BackendEvidence<'a> {
    adapter: &'static str,
    basis: &'a str,
    cancel_invoked: bool,
    cancel_returned: Option<bool>,
    cancel_return_was_sufficient: bool,
    observed_state: &'a str,
    slo_miss: bool,
    pid: i32,
    application_name: &'a str,
    query_role: &'a str,
    deadline_basis: &'a str,
}

#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct HarnessEvidence {
    adapter: &'static str,
    basis: &'static str,
    signaled: bool,
    force_killed: bool,
    escaped: usize,
    within_deadline: bool,
}

#[derive(Serialize)]
struct CollectEvidence<'a> {
    adapter: &'static str,
    kind: &'a str,
    content_sha256: &'a str,
    len: u64,
}

#[derive(Serialize)]
struct WorkspaceEvidence {
    adapter: &'static str,
    actual_absence: bool,
    verified: &'static str,
}

fn validate(request: &ReleaseRequest, runtime_password: &str, cancel_password: &str) -> Result<(), &'static str> {
    if request.contract != CONTRACT {
        return Err("contract");
    }
    if request.runtime_user != crate::model::RUNTIME_LOGIN || request.cancel_user != crate::model::CANCEL_LOGIN {
        return Err("login");
    }
    if runtime_password.is_empty() || cancel_password.is_empty() || runtime_password.contains('\0') || cancel_password.contains('\0')
    {
        return Err("password");
    }
    if request.host.is_empty() || request.host.contains('\0') || request.port == 0 || !is_token(&request.database) {
        return Err("endpoint");
    }
    if !is_token(&request.project_id) || !is_token(&request.session_id) || !session_transition::identity_ok(&request.identity) {
        return Err("identity");
    }
    if request.identity.project_id != request.project_id {
        return Err("identity");
    }
    if !is_token(&request.event_prefix) || !ids_ok(&request.event_prefix) {
        return Err("event_prefix");
    }
    if !uuid_ok(&request.owner_uuid) {
        return Err("owner_uuid");
    }
    if !is_token(&request.query_role) || DENY_QUERY.contains(&request.query_role.as_str()) {
        return Err("query_role");
    }
    if !is_token(&request.application_name) || request.application_name.len() > 63 {
        return Err("application_name");
    }
    if request.backend_pid <= 0 {
        return Err("backend_pid");
    }
    if !request.ca_pem.contains("BEGIN CERTIFICATE") || request.ca_pem.len() > 65_536 {
        return Err("ca_pem");
    }
    if !harness::argv_ok(&request.harness_argv) {
        return Err("argv");
    }
    Ok(())
}

fn ids_ok(prefix: &str) -> bool {
    ["cancel", "s1", "s2", "s3", "s4", "s8"]
        .iter()
        .all(|suffix| is_token(&event_id(prefix, suffix)))
}

fn uuid_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let groups = [8usize, 4, 4, 4, 12];
    let mut index = 0usize;
    for (group, len) in groups.iter().enumerate() {
        if group > 0 {
            if bytes.get(index) != Some(&b'-') {
                return false;
            }
            index += 1;
        }
        for _ in 0..*len {
            let Some(byte) = bytes.get(index) else {
                return false;
            };
            if !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase() {
                return false;
            }
            index += 1;
        }
    }
    index == bytes.len()
}

fn gate_paths(request: &ReleaseRequest) -> Result<(), &'static str> {
    let root = PathBuf::from(&request.workspace_root);
    let workspace = PathBuf::from(&request.workspace_path);
    let checkpoint = PathBuf::from(&request.checkpoint_dir);
    let transcript = PathBuf::from(&request.transcript_path);
    let outputs = PathBuf::from(&request.outputs_path);
    match paths::classify(&root, &workspace, &request.owner_uuid) {
        Class::Owned if workspace.is_dir() => {}
        Class::Missing => return Err("workspace_missing"),
        Class::Owned | Class::Foreign => return Err("workspace_foreign"),
    }
    let Some(anchor) = anchor(&checkpoint, &request.owner_uuid) else {
        return Err("checkpoint_foreign");
    };
    match paths::classify(&anchor, &checkpoint, &request.owner_uuid) {
        Class::Owned if checkpoint.is_dir() => {}
        Class::Missing => return Err("checkpoint_missing"),
        Class::Owned | Class::Foreign => return Err("checkpoint_foreign"),
    }
    if paths::overlaps(&checkpoint, &workspace) {
        return Err("checkpoint_overlap");
    }
    match paths::artifact_class(&workspace, &transcript, &request.owner_uuid) {
        Class::Foreign => return Err("artifact_foreign"),
        Class::Missing | Class::Owned => {}
    }
    match paths::artifact_class(&workspace, &outputs, &request.owner_uuid) {
        Class::Foreign => return Err("artifact_foreign"),
        Class::Missing | Class::Owned => {}
    }
    Ok(())
}

fn anchor(path: &Path, uuid: &str) -> Option<PathBuf> {
    let mut acc = PathBuf::new();
    for component in path.components() {
        acc.push(component);
        if let Component::Normal(value) = component
            && value == std::ffi::OsStr::new(uuid)
        {
            return Some(acc);
        }
    }
    None
}

fn same_identity(state: &SessionState, request: &ReleaseRequest) -> bool {
    state.session_id == request.session_id && request.identity.project_id == request.project_id && state.bound.as_ref() == Some(&request.identity)
}

fn checkpoint_matches(checkpoint: &Checkpoint, request: &ReleaseRequest, cursor: &Cursor) -> bool {
    checkpoint.contract == CHECKPOINT_CONTRACT
        && checkpoint.project_id == request.project_id
        && checkpoint.session_id == request.session_id
        && checkpoint.owner_generation == cursor.generation
        && checkpoint.backend_pid == request.backend_pid
        && checkpoint.application_name == request.application_name
        && checkpoint.query_role == request.query_role
        && checkpoint.cancel_event_id == event_id(&request.event_prefix, "cancel")
}

fn from_drive(err: DriveFail) -> RunResult {
    let (disposition, reason) = match err {
        DriveFail::Unavailable => ("store_unavailable", "pg_unavailable".to_owned()),
        DriveFail::UnknownCommit => ("store_unavailable", "commit_unknown".to_owned()),
        DriveFail::Store(reason) if matches!(reason, "tls" | "connect" | "commit_unknown") => ("store_unavailable", reason.to_owned()),
        DriveFail::Store(reason) => ("incomplete", reason.to_owned()),
        DriveFail::Reason(reason) => ("incomplete", reason),
    };
    let receipt = blank(disposition, reason);
    let exit_code = exit_for(&receipt);
    RunResult { exit_code, receipt }
}

fn carry(mut result: RunResult, src: &ReleaseReceipt) -> RunResult {
    result.receipt.slo_miss = src.slo_miss;
    result.receipt.resumed = src.resumed;
    result.receipt.deadline_basis = src.deadline_basis;
    result.receipt.t0_wall_epoch_ms = src.t0_wall_epoch_ms;
    result.receipt.elapsed_ms = src.elapsed_ms;
    result.receipt.backend_elapsed_ms = src.backend_elapsed_ms;
    result.receipt.harness_elapsed_ms = src.harness_elapsed_ms;
    result.receipt.harness_pid = src.harness_pid;
    result.receipt.backend_basis = src.backend_basis.clone();
    result.receipt.database_used = true;
    result
}

fn refuse(disposition: &'static str, reason: &'static str) -> RunResult {
    let receipt = blank(disposition, reason);
    let exit_code = exit_for(&receipt);
    RunResult { exit_code, receipt }
}

fn stamp(mut result: RunResult, cursor: &Cursor) -> RunResult {
    stamp_receipt(&mut result.receipt, cursor);
    result.receipt.database_used = true;
    result
}

fn stamp_receipt(receipt: &mut ReleaseReceipt, cursor: &Cursor) {
    receipt.phase = Some(phase_name(cursor.phase));
    receipt.revision = Some(cursor.revision);
    receipt.database_used = true;
}

fn weaken(receipt: &mut ReleaseReceipt, reason: &str) {
    if receipt.reason == "incomplete" {
        receipt.reason = reason.to_owned();
    }
}

fn err_reason(err: &DriveFail) -> &'static str {
    match err {
        DriveFail::Unavailable => "pg_unavailable",
        DriveFail::UnknownCommit => "commit_unknown",
        DriveFail::Store(reason) => reason,
        DriveFail::Reason(_) => "refused",
    }
}

fn exit_for(receipt: &ReleaseReceipt) -> u8 {
    if receipt.disposition == "released_unclosed" {
        0
    } else if receipt.disposition == "malformed" {
        1
    } else if receipt.disposition == "store_unavailable" {
        3
    } else {
        2
    }
}

fn phase_name(phase: Phase) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{success_ready, uuid_ok, validate};
    use crate::model::{blank, unsupported_matrix, ReleaseReceipt, ReleaseRequest, StepView};
    use session_transition::{CancelTrigger, IdentityTuple};

    fn sample() -> ReleaseRequest {
        ReleaseRequest {
            contract: crate::model::CONTRACT.to_owned(),
            host: "oo-a295-pg".to_owned(),
            port: 5432,
            database: "oo_a295".to_owned(),
            runtime_user: "oo_session_runtime".to_owned(),
            runtime_password: "runtime-secret".to_owned(),
            cancel_user: "oo_session_cancel".to_owned(),
            cancel_password: "cancel-secret".to_owned(),
            ca_pem: "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n".to_owned(),
            project_id: "project-a".to_owned(),
            session_id: "session-a".to_owned(),
            identity: IdentityTuple {
                project_id: "project-a".to_owned(),
                principal_id: "alice".to_owned(),
                role: "analyst".to_owned(),
                claims_hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
                policy_version: "pol-1".to_owned(),
                knowledge_version: "know-1".to_owned(),
                config_version: "cfg-1".to_owned(),
            },
            expected_revision: 3,
            expected_owner_generation: 1,
            now_ms: 2_000,
            cancel_trigger: CancelTrigger::User,
            event_prefix: "rel-a".to_owned(),
            owner_uuid: "eef831aa-dba4-4af2-a78b-541bc9e0a295".to_owned(),
            workspace_root: "/tmp/eef831aa-dba4-4af2-a78b-541bc9e0a295".to_owned(),
            workspace_path: "/tmp/eef831aa-dba4-4af2-a78b-541bc9e0a295/workspace".to_owned(),
            checkpoint_dir: "/tmp/eef831aa-dba4-4af2-a78b-541bc9e0a295/checkpoint".to_owned(),
            transcript_path: "/tmp/eef831aa-dba4-4af2-a78b-541bc9e0a295/workspace/transcript.txt".to_owned(),
            outputs_path: "/tmp/eef831aa-dba4-4af2-a78b-541bc9e0a295/workspace/outputs.txt".to_owned(),
            backend_pid: 7,
            application_name: "fixture-query".to_owned(),
            query_role: "oo_fixture_query".to_owned(),
            harness_argv: vec!["/bin/bash".to_owned(), "-c".to_owned(), "true".to_owned()],
        }
    }

    #[test]
    fn lowercase_uuid_and_denied_roles_fail_closed() {
        assert!(uuid_ok("eef831aa-dba4-4af2-a78b-541bc9e0a295"));
        assert!(!uuid_ok("EEF831AA-DBA4-4AF2-A78B-541BC9E0A295"));
        assert!(validate(&sample(), "runtime-secret", "cancel-secret").is_ok());
        let mut denied = sample();
        denied.query_role = "oo_session_runtime".to_owned();
        assert_eq!(validate(&denied, "runtime-secret", "cancel-secret"), Err("query_role"));
        let mut login = sample();
        login.cancel_user = "postgres".to_owned();
        assert_eq!(validate(&login, "runtime-secret", "cancel-secret"), Err("login"));
    }

    #[test]
    fn success_requires_five_fixture_steps_and_leaves_the_session_open() {
        assert!(!success_ready(&blank("incomplete", "probe")));
        let mut receipt = ready_receipt();
        assert!(success_ready(&receipt));
        receipt.physical_release_certified = true;
        assert!(!success_ready(&receipt));
        let mut closed = ready_receipt();
        closed.phase = Some("closed".to_owned());
        assert!(!success_ready(&closed));
        let mut skipped = ready_receipt();
        skipped.steps[4].reducer = "skipped".to_owned();
        assert!(!success_ready(&skipped));
        let mut certified = ready_receipt();
        certified.complete_release_called = true;
        assert!(!success_ready(&certified));
    }

    fn ready_receipt() -> ReleaseReceipt {
        let mut receipt = blank("incomplete", "incomplete");
        receipt.database_used = true;
        receipt.phase = Some("releasing".to_owned());
        receipt.unsupported = unsupported_matrix();
        let evidence = "{\"adapter\":\"local_fixture_directory\",\"actual_absence\":true}";
        receipt.steps = ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"]
            .into_iter()
            .map(|id| {
                let runnable = matches!(id, "s1" | "s2" | "s3" | "s4" | "s8");
                StepView {
                    id,
                    orchestrator: if runnable { "confirmed".to_owned() } else { "unsupported".to_owned() },
                    reducer: if runnable { "confirmed".to_owned() } else { "absent".to_owned() },
                    evidence_sha256: if runnable { Some("ab".repeat(32)) } else { None },
                    evidence_json: if id == "s8" { Some(evidence.to_owned()) } else if runnable { Some("{}".to_owned()) } else { None },
                }
            })
            .collect();
        receipt
    }
}
