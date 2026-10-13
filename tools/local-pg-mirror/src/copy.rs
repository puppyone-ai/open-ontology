//! One source snapshot, then the target generation.
//! The source connection is closed before any target row is written.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use oo_connectors::{config_identity, snapshot_input_digest, type_policy_identity, PgTypePolicy};
use oo_pipeline::copy_encoder::CopyBudgets;
use oo_pipeline::full_import::hook_digest;
use oo_pipeline::target_installer::sha256_hex;

use crate::error::MirrorError;
use crate::ident;
use crate::load;
use crate::manifest::{
    load_manifest, replace_manifest, store_manifest, store_target_config, target_config_bytes, Manifest,
    Quarantine, TableRecord,
};
use crate::pg::{self, SessionKind};
use crate::plan::{self, load_plan, Plan};
use crate::session::{self, Cut, CutKind, CutMode, Digests, Measured};
use crate::spool::{self, Loaded};

pub struct CopyRequest {
    pub plan_path: PathBuf,
    pub source_password: PathBuf,
    pub source_ca: PathBuf,
    pub admin_password: PathBuf,
    pub runtime_password: PathBuf,
    pub manifest_path: PathBuf,
    pub target_config_path: PathBuf,
    pub target_ca: PathBuf,
    pub stop: CopyStop,
}

pub enum CopyStop {
    Finish,
    CancelBeforePull,
    StopAfterBatches(u64),
    DropAfterSnapshot,
}

#[derive(Debug)]
pub struct CopyOutcome {
    pub manifest: Manifest,
    pub disposition: &'static str,
}

#[allow(clippy::too_many_lines)]
pub async fn copy(request: CopyRequest) -> Result<CopyOutcome, MirrorError> {
    let plan = load_plan(&request.plan_path)?;
    let spool_dir = spool::directory(&request.manifest_path)?;
    let existing = load_manifest(&request.manifest_path)?;
    if let Some(found) = &existing {
        if found.plan_sha256 != plan.plan_sha256 {
            return Err(MirrorError::new("manifest_conflict", "plan_sha256"));
        }
        if found.mirror_state == "complete" {
            return Ok(CopyOutcome {
                manifest: found.clone(),
                disposition: "replay_complete",
            });
        }
    }
    match spool::inspect(&spool_dir)? {
        spool::SpoolState::Complete => {
            let Some(loaded) = spool::load_complete(&spool_dir, &plan.plan_sha256, plan.config.generation)? else {
                return Err(MirrorError::new("manifest_conflict", "spool"));
            };
            return resume(request, plan, loaded, existing).await;
        }
        spool::SpoolState::Partial => {
            if existing.is_some() {
                return Err(MirrorError::new("resume_refused", "resume_refused"));
            }
            spool::discard(&spool_dir)?;
        }
        spool::SpoolState::Absent => {
            if existing.is_some() {
                return Err(MirrorError::new("resume_refused", "resume_refused"));
            }
        }
    }
    if let CopyStop::StopAfterBatches(0) = request.stop {
        return Err(MirrorError::new("budget", "batch_rows"));
    }
    fresh(request, plan, spool_dir).await
}

#[allow(clippy::too_many_lines)]
async fn fresh(request: CopyRequest, plan: Plan, spool_dir: PathBuf) -> Result<CopyOutcome, MirrorError> {
    let source_password = plan::read_secret(&request.source_password)?;
    let admin_password = plan::read_secret(&request.admin_password)?;
    let runtime_password = plan::read_secret(&request.runtime_password)?;
    let source_ca = plan::read_ca(&request.source_ca)?;
    let target_ca = plan::read_ca(&request.target_ca)?;
    if sha256_hex(&source_ca) != plan.config.ca_sha256 || sha256_hex(&target_ca) != plan.config.target_ca_sha256 {
        return Err(MirrorError::new("ca_drift", "ca"));
    }
    let digests = identities(&plan, source_password.as_str(), &source_ca)?;
    let connect_timeout = Duration::from_millis(plan.config.connect_timeout_ms);
    {
        let mut admin = dial_admin(&plan, admin_password.as_str(), &target_ca, connect_timeout).await?;
        if let Err(err) = pg::prepare_target(&mut admin, &plan).await {
            drop(admin);
            return store_draft(&request, &plan, pre_source(err.fact));
        }
    }
    let deadline = Instant::now() + Duration::from_millis(plan.config.snapshot_deadline_ms);
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return store_draft(&request, &plan, pre_source("deadline"));
    }
    let source = match tokio::time::timeout(
        remaining,
        pg::dial_source(&plan, source_password.as_str(), &source_ca, connect_timeout),
    )
    .await
    {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return store_draft(&request, &plan, pre_source(err.fact)),
        Err(_) => return store_draft(&request, &plan, pre_source("deadline")),
    };
    drop(source_password);
    let mode = match request.stop {
        CopyStop::Finish => CutMode::Finish,
        CopyStop::CancelBeforePull => CutMode::Cancel,
        CopyStop::StopAfterBatches(limit) => CutMode::StopAfter(limit),
        CopyStop::DropAfterSnapshot => CutMode::Drop,
    };
    let budget = copy_budget(&plan)?;
    let cut = session::read_cut(
        source,
        &plan,
        deadline,
        Duration::from_millis(plan.config.snapshot_deadline_ms),
        mode,
        spool_dir,
        &digests,
        budget,
    )
    .await;
    match cut.kind {
        CutKind::Failed | CutKind::Dropped => store_draft(&request, &plan, draft_from_cut(&cut)),
        CutKind::Ready | CutKind::Cancelled | CutKind::Stopped => {
            land(&request, &plan, &digests, &cut, admin_password.as_str(), runtime_password.as_str(), &target_ca, false).await
        }
    }
}

async fn resume(
    request: CopyRequest,
    plan: Plan,
    loaded: Loaded,
    existing: Option<Manifest>,
) -> Result<CopyOutcome, MirrorError> {
    let source_ca = plan::read_ca(&request.source_ca)?;
    let target_ca = plan::read_ca(&request.target_ca)?;
    if sha256_hex(&source_ca) != plan.config.ca_sha256 || sha256_hex(&target_ca) != plan.config.target_ca_sha256 {
        return Err(MirrorError::new("ca_drift", "ca"));
    }
    let admin_password = plan::read_secret(&request.admin_password)?;
    let runtime_password = plan::read_secret(&request.runtime_password)?;
    let digests = identities(&plan, "x", &source_ca)?;
    if loaded.header.config_digest != digests.config_digest
        || loaded.header.type_digest != digests.type_digest
        || loaded.header.input_digest != digests.input_digest
        || loaded.header.schema_digest != digests.schema_digest
        || loaded.header.snapshot_id.is_empty()
    {
        return Err(MirrorError::new("manifest_conflict", "spool"));
    }
    if let Some(found) = &existing
        && found
            .snapshot_id
            .as_ref()
            .is_some_and(|id| id != &loaded.header.snapshot_id)
    {
        return Err(MirrorError::new("manifest_conflict", "snapshot_id"));
    }
    let measured = preserved(&existing, &loaded.header.snapshot_id);
    let connect_timeout = Duration::from_millis(plan.config.connect_timeout_ms);
    let mut admin = dial_admin(&plan, admin_password.as_str(), &target_ca, connect_timeout).await?;
    pg::prepare_target(&mut admin, &plan).await?;
    let mut runtime = dial_runtime(&plan, runtime_password.as_str(), &target_ca, connect_timeout).await?;
    let outcome = match load::apply(&mut runtime, &mut admin, &plan, &digests, &loaded.batches, true).await {
        Ok(outcome) => outcome,
        Err(err) => return Err(err),
    };
    drop(runtime);
    finish_land(&request, &plan, &measured, outcome, admin, "none", existing.is_some()).await
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn land(
    request: &CopyRequest,
    plan: &Plan,
    digests: &Digests,
    cut: &Cut,
    admin_password: &str,
    runtime_password: &str,
    target_ca: &[u8],
    replace: bool,
) -> Result<CopyOutcome, MirrorError> {
    let connect_timeout = Duration::from_millis(plan.config.connect_timeout_ms);
    let mut runtime = match dial_runtime(plan, runtime_password, target_ca, connect_timeout).await {
        Ok(conn) => conn,
        Err(err) => return store_draft(request, plan, failed_land(cut, err.fact)),
    };
    let mut admin = match dial_admin(plan, admin_password, target_ca, connect_timeout).await {
        Ok(conn) => conn,
        Err(err) => {
            drop(runtime);
            return store_draft(request, plan, failed_land(cut, err.fact));
        }
    };
    let seal = matches!(cut.kind, CutKind::Ready);
    let outcome = match load::apply(&mut runtime, &mut admin, plan, digests, &cut.batches, seal).await {
        Ok(outcome) => outcome,
        Err(err) => {
            drop(runtime);
            drop(admin);
            return store_draft(request, plan, failed_land(cut, err.fact));
        }
    };
    drop(runtime);
    finish_land(request, plan, &cut.measured, outcome, admin, cut.failure, replace).await
}

async fn finish_land(
    request: &CopyRequest,
    plan: &Plan,
    measured: &Measured,
    outcome: load::TargetOutcome,
    mut admin: sqlx::postgres::PgConnection,
    unsealed_failure: &'static str,
    replace: bool,
) -> Result<CopyOutcome, MirrorError> {
    if !outcome.sealed {
        drop(admin);
        let rows = outcome.tables.iter().map(|table| table.rows).sum();
        return store_draft(
            request,
            plan,
            Draft {
                state: "incomplete",
                failure: unsealed_failure,
                peak: Some(measured.peak),
                fact: measured.fact,
                rows,
                loader_state: "retired",
                loader_run_id: clean_run_id(&outcome.run_id),
                completed: Vec::new(),
                incomplete: outcome.tables,
                quarantine: Vec::new(),
                snapshot_id: measured.snapshot_id.clone(),
                backend_pid: measured.backend_pid,
                read_only: measured.read_only,
                write_tuples: measured.write_tuples,
                import_generation: None,
                spool_state: "none",
                index_count: 0,
                analyze_tables: 0,
                replace,
            },
        );
    }
    let physical: Vec<String> = outcome.tables.iter().filter_map(|table| table.physical_name.clone()).collect();
    let rows = outcome.tables.iter().map(|table| table.rows).sum();
    if pg::grant_reader(&mut admin, &plan.config.target_reader_user, &physical).await.is_err() {
        drop(admin);
        return store_draft(
            request,
            plan,
            Draft {
                state: "incomplete",
                failure: "reader_grant",
                peak: Some(measured.peak),
                fact: measured.fact,
                rows,
                loader_state: "awaiting_publish",
                loader_run_id: clean_run_id(&outcome.run_id),
                completed: Vec::new(),
                incomplete: outcome.tables,
                quarantine: Vec::new(),
                snapshot_id: measured.snapshot_id.clone(),
                backend_pid: measured.backend_pid,
                read_only: measured.read_only,
                write_tuples: measured.write_tuples,
                import_generation: None,
                spool_state: "complete",
                index_count: outcome.index_count,
                analyze_tables: outcome.analyze_tables,
                replace,
            },
        );
    }
    drop(admin);
    store_draft(
        request,
        plan,
        Draft {
            state: "complete",
            failure: "none",
            peak: Some(measured.peak),
            fact: measured.fact,
            rows,
            loader_state: "awaiting_publish",
            loader_run_id: clean_run_id(&outcome.run_id),
            completed: outcome.tables,
            incomplete: Vec::new(),
            quarantine: Vec::new(),
            snapshot_id: measured.snapshot_id.clone(),
            backend_pid: measured.backend_pid,
            read_only: measured.read_only,
            write_tuples: measured.write_tuples,
            import_generation: Some(outcome.generation),
            spool_state: "complete",
            index_count: outcome.index_count,
            analyze_tables: outcome.analyze_tables,
            replace,
        },
    )
}

struct Draft {
    state: &'static str,
    failure: &'static str,
    peak: Option<u64>,
    fact: &'static str,
    rows: u64,
    loader_state: &'static str,
    loader_run_id: Option<String>,
    completed: Vec<TableRecord>,
    incomplete: Vec<TableRecord>,
    quarantine: Vec<Quarantine>,
    snapshot_id: Option<String>,
    backend_pid: Option<i64>,
    read_only: bool,
    write_tuples: Option<i64>,
    import_generation: Option<i64>,
    spool_state: &'static str,
    index_count: u64,
    analyze_tables: u64,
    replace: bool,
}

fn pre_source(failure: &'static str) -> Draft {
    Draft {
        state: if matches!(
            failure,
            "deadline" | "statement_timeout" | "lock_timeout" | "connect_timeout" | "connection" | "io"
        ) {
            "incomplete"
        } else {
            "refused"
        },
        failure,
        peak: Some(0),
        fact: "not_opened",
        rows: 0,
        loader_state: "none",
        loader_run_id: None,
        completed: Vec::new(),
        incomplete: Vec::new(),
        quarantine: Vec::new(),
        snapshot_id: None,
        backend_pid: None,
        read_only: false,
        write_tuples: None,
        import_generation: None,
        spool_state: "none",
        index_count: 0,
        analyze_tables: 0,
        replace: false,
    }
}

fn draft_from_cut(cut: &Cut) -> Draft {
    Draft {
        state: cut.state,
        failure: cut.failure,
        peak: Some(cut.measured.peak),
        fact: cut.measured.fact,
        rows: 0,
        loader_state: "none",
        loader_run_id: None,
        completed: Vec::new(),
        incomplete: Vec::new(),
        quarantine: cut.quarantine.clone(),
        snapshot_id: cut.measured.snapshot_id.clone(),
        backend_pid: cut.measured.backend_pid,
        read_only: cut.measured.read_only,
        write_tuples: cut.measured.write_tuples,
        import_generation: None,
        spool_state: cut.spool,
        index_count: 0,
        analyze_tables: 0,
        replace: false,
    }
}

fn failed_land(cut: &Cut, failure: &'static str) -> Draft {
    let mut draft = draft_from_cut(cut);
    draft.failure = failure;
    draft.state = if matches!(
        failure,
        "deadline" | "statement_timeout" | "lock_timeout" | "connect_timeout" | "connection" | "io"
    ) || cut.state == "incomplete"
    {
        "incomplete"
    } else {
        "refused"
    };
    draft
}

fn preserved(existing: &Option<Manifest>, snapshot_id: &str) -> Measured {
    if let Some(found) = existing {
        return Measured {
            peak: found.source_socket_peak.unwrap_or(0),
            fact: if found.source_socket_fact == "repeatable_read_read_only" {
                "repeatable_read_read_only"
            } else {
                "not_opened"
            },
            snapshot_id: found.snapshot_id.clone().or_else(|| Some(snapshot_id.to_string())),
            backend_pid: found.source_backend_pid,
            read_only: found.source_read_only,
            write_tuples: found.source_write_tuples,
        };
    }
    Measured {
        peak: 0,
        fact: "repeatable_read_read_only",
        snapshot_id: Some(snapshot_id.to_string()),
        backend_pid: None,
        read_only: true,
        write_tuples: None,
    }
}

fn store_draft(request: &CopyRequest, plan: &Plan, mut draft: Draft) -> Result<CopyOutcome, MirrorError> {
    if draft.incomplete.is_empty() && draft.state != "complete" {
        draft.incomplete = plan
            .config
            .tables
            .iter()
            .map(|(schema, name)| TableRecord {
                schema: schema.clone(),
                name: name.clone(),
                physical_name: None,
                rows: 0,
                canonical_digest: String::new(),
                columns: Vec::new(),
            })
            .collect();
    }
    let target_bytes = target_config_bytes(plan)?;
    let target_sha = sha256_hex(&target_bytes);
    if draft.state == "complete" {
        store_target_config(&request.target_config_path, &target_bytes)?;
    }
    let mut manifest = Manifest {
        mirror_state: draft.state.to_string(),
        customer_complete: false,
        enterprise: "not_configured".to_string(),
        consistency: if draft.state == "complete" {
            "single_snapshot".to_string()
        } else {
            "not_a_snapshot".to_string()
        },
        plan_sha256: plan.plan_sha256.clone(),
        admitted_sha256: plan.admitted_sha256.clone(),
        failure: draft.failure.to_string(),
        rows: draft.rows,
        source_socket_peak: draft.peak,
        source_socket_fact: draft.fact.to_string(),
        recheck_connections: 0,
        source_count_star: false,
        source_explain: false,
        source_ddl: false,
        connect_timeout_ms: plan.config.connect_timeout_ms,
        statement_timeout_ms: plan.config.statement_timeout_ms,
        lock_timeout_ms: plan.config.lock_timeout_ms,
        max_rows: plan.config.max_rows,
        max_bytes: plan.config.max_bytes,
        batch_rows: plan.config.batch_rows,
        snapshot_deadline_ms: plan.config.snapshot_deadline_ms,
        loader_protocol: "full-import/v1".to_string(),
        loader_run_id: draft.loader_run_id,
        loader_state: draft.loader_state.to_string(),
        mapping_algorithm: "physical-mapping/v1".to_string(),
        system_position: "_oo_pos numeric(38,0)".to_string(),
        target_config_path: request.target_config_path.display().to_string(),
        target_config_sha256: target_sha,
        tables_completed: draft.completed,
        tables_incomplete: draft.incomplete,
        quarantine: draft.quarantine,
        snapshot_id: draft.snapshot_id,
        source_backend_pid: draft.backend_pid,
        source_read_only: draft.read_only,
        source_write_tuples: draft.write_tuples,
        import_generation: draft.import_generation,
        import_hash: None,
        spool_state: draft.spool_state.to_string(),
        index_count: draft.index_count,
        analyze_tables: draft.analyze_tables,
        manifest_sha256: String::new(),
    };
    if draft.replace {
        replace_manifest(&request.manifest_path, &mut manifest)?;
    } else {
        store_manifest(&request.manifest_path, &mut manifest)?;
    }
    Ok(CopyOutcome {
        manifest,
        disposition: "stored",
    })
}

fn identities(plan: &Plan, password: &str, ca_pem: &[u8]) -> Result<Digests, MirrorError> {
    let source = plan::source_config(&plan.config, password, ca_pem)?;
    let config = config_identity(&source);
    let type_id = type_policy_identity(PgTypePolicy::PRESERVE_ROW);
    let input = snapshot_input_digest(&config, &type_id, &plan.admitted_sha256, plan.config.generation)
        .map_err(|_| MirrorError::new("config", "input_digest"))?;
    Ok(Digests {
        config_digest: ident::hex_encode(&config),
        type_digest: ident::hex_encode(&type_id),
        input_digest: ident::hex_encode(&input),
        input_bytes: input,
        schema_digest: plan.admitted_sha256.clone(),
        hook_digest: hook_digest(),
    })
}

fn copy_budget(plan: &Plan) -> Result<CopyBudgets, MirrorError> {
    let max_value = u32::try_from(plan.config.max_bytes).map_err(|_| MirrorError::new("budget", "max_bytes"))?;
    let max_row = max_value.checked_mul(2).ok_or_else(|| MirrorError::new("budget", "max_bytes"))?;
    let max_chunk = max_row.checked_mul(2).ok_or_else(|| MirrorError::new("budget", "max_bytes"))?;
    let batch_rows = u32::try_from(plan.config.batch_rows).map_err(|_| MirrorError::new("budget", "batch_rows"))?;
    Ok(CopyBudgets {
        max_value_bytes: max_value,
        max_row_bytes: max_row.max(2),
        max_chunk_bytes: max_chunk.max(19),
        max_rows_per_chunk: batch_rows,
    })
}

async fn dial_admin(
    plan: &Plan,
    password: &str,
    ca: &[u8],
    timeout: Duration,
) -> Result<sqlx::postgres::PgConnection, MirrorError> {
    pg::dial(
        &plan.config.target_host,
        plan.config.target_port,
        &plan.config.target_admin_user,
        password,
        &plan.config.target_database,
        ca,
        timeout,
        SessionKind::Admin,
    )
    .await
}

async fn dial_runtime(
    plan: &Plan,
    password: &str,
    ca: &[u8],
    timeout: Duration,
) -> Result<sqlx::postgres::PgConnection, MirrorError> {
    pg::dial(
        &plan.config.target_host,
        plan.config.target_port,
        &plan.config.target_runtime_user,
        password,
        &plan.config.target_database,
        ca,
        timeout,
        SessionKind::Runtime,
    )
    .await
}

fn clean_run_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 64 {
        return None;
    }
    if value.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-') {
        Some(value.to_ascii_lowercase())
    } else {
        None
    }
}
