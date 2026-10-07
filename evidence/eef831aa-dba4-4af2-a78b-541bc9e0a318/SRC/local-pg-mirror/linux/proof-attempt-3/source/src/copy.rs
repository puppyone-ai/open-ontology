use std::path::PathBuf;
use std::time::{Duration, Instant};

use oo_connectors::{PgConnector, PgTypePolicy, StreamBudget};
use oo_pipeline::copy_encoder::CopyBudgets;
use oo_pipeline::full_import::{ApplyStep, ImportError, ImportRequest, ImportSession, TableSummary};
use oo_pipeline::target_installer::sha256_hex;

use crate::discover::shapes_from_admitted;
use crate::error::MirrorError;
use crate::manifest::{
    load_manifest, store_manifest, store_target_config, target_config_bytes, ColumnRecord, Manifest,
    Quarantine, TableRecord,
};
use crate::pg::{self, map_source, map_source_text, SessionKind};
use crate::plan::{self, load_plan, Plan};

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
}

pub struct CopyOutcome {
    pub manifest: Manifest,
    pub disposition: &'static str,
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
pub async fn copy(request: CopyRequest) -> Result<CopyOutcome, MirrorError> {
    let plan = load_plan(&request.plan_path)?;
    if let Some(existing) = load_manifest(&request.manifest_path)? {
        if existing.plan_sha256 != plan.plan_sha256 {
            return Err(MirrorError::new("manifest_conflict", "plan_sha256"));
        }
        if existing.mirror_state == "complete" {
            return Ok(CopyOutcome {
                manifest: existing,
                disposition: "replay_complete",
            });
        }
        return Err(MirrorError::new("resume_refused", "resume_refused"));
    }
    if let CopyStop::StopAfterBatches(0) = request.stop {
        return Err(MirrorError::new("budget", "batch_rows"));
    }
    let source_password = plan::read_secret(&request.source_password)?;
    let admin_password = plan::read_secret(&request.admin_password)?;
    let runtime_password = plan::read_secret(&request.runtime_password)?;
    let source_ca = plan::read_ca(&request.source_ca)?;
    let target_ca = plan::read_ca(&request.target_ca)?;
    if sha256_hex(&source_ca) != plan.config.ca_sha256 || sha256_hex(&target_ca) != plan.config.target_ca_sha256 {
        return Err(MirrorError::new("ca_drift", "ca"));
    }
    let connect_timeout = Duration::from_millis(plan.config.connect_timeout_ms);
    {
        let mut admin = pg::dial(
            &plan.config.target_host,
            plan.config.target_port,
            &plan.config.target_admin_user,
            admin_password.as_str(),
            &plan.config.target_database,
            &target_ca,
            connect_timeout,
            SessionKind::Admin,
        )
        .await?;
        if let Err(err) = pg::prepare_target(&mut admin, &plan).await {
            drop(admin);
            return write_closed(&request, &plan, "refused", err.fact, Some(0), "not_opened", 0, &[], 0, "none", None, Vec::new());
        }
    }
    let started = Instant::now();
    let deadline = started + Duration::from_millis(plan.config.snapshot_deadline_ms);
    let source = plan::source_config(&plan.config, source_password.as_str(), &source_ca)?;
    let connector = PgConnector::new(source);
    let prepared = connector
        .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, plan.config.generation, deadline)
        .await;
    let recheck = match &prepared {
        Ok(_) => 1,
        Err(err)
            if matches!(
                err.fact(),
                Some("generation_invalid" | "selection" | "deadline")
            ) && started.elapsed() < Duration::from_millis(plan.config.snapshot_deadline_ms) =>
        {
            0
        }
        Err(_) => 1,
    };
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(err) => {
            let mapped = map_source(&err);
            let state = if matches!(mapped.fact, "deadline" | "statement_timeout" | "lock_timeout" | "connect_timeout") {
                "incomplete"
            } else {
                "refused"
            };
            return write_closed(
                &request,
                &plan,
                state,
                mapped.fact,
                Some(0),
                "not_opened",
                recheck,
                &[],
                0,
                "none",
                None,
                Vec::new(),
            );
        }
    };
    let fresh = shapes_from_admitted(&prepared.admitted);
    let fresh_bytes = plan::admitted_canonical(&fresh)?;
    if sha256_hex(fresh_bytes.as_bytes()) != plan.admitted_sha256 {
        return write_closed(
            &request,
            &plan,
            "refused",
            "catalog_drift",
            Some(0),
            "not_opened",
            1,
            &[],
            0,
            "none",
            None,
            Vec::new(),
        );
    }
    if prepared.expected_principal != plan.expected_principal
        || prepared.expected_database != plan.expected_database
        || prepared.expected_in_recovery != plan.expected_in_recovery
        || prepared.expected_postmaster_start != plan.expected_postmaster
    {
        return write_closed(
            &request,
            &plan,
            "refused",
            "identity_drift",
            Some(0),
            "not_opened",
            1,
            &[],
            0,
            "none",
            None,
            Vec::new(),
        );
    }
    let mut runtime = pg::dial(
        &plan.config.target_host,
        plan.config.target_port,
        &plan.config.target_runtime_user,
        runtime_password.as_str(),
        &plan.config.target_database,
        &target_ca,
        connect_timeout,
        SessionKind::Runtime,
    )
    .await?;
    let (stream, copy_budget) = stream_budget(&plan)?;
    let import = ImportRequest {
        shard: plan.config.shard.clone(),
        owner: plan.config.owner.clone(),
        snapshot: prepared,
        stream,
        copy: copy_budget,
    };
    let mut session = match ImportSession::open(&mut runtime, import).await {
        Ok(session) => session,
        Err(err) => {
            drop(runtime);
            return open_failure(&request, &plan, err);
        }
    };
    if matches!(request.stop, CopyStop::CancelBeforePull) {
        let tables = session.tables();
        let _ = session.cancel(&mut runtime).await;
        drop(runtime);
        return write_closed(
            &request,
            &plan,
            "incomplete",
            "cancel",
            Some(2),
            "verified_reader_guard",
            1,
            &tables,
            0,
            "retired",
            None,
            Vec::new(),
        );
    }
    let mut applied = 0u64;
    loop {
        if let CopyStop::StopAfterBatches(limit) = request.stop {
            if applied >= limit {
                let tables = session.tables();
                let rows = tables.iter().map(|table| table.rows).sum();
                let _ = session.cancel(&mut runtime).await;
                drop(runtime);
                return write_closed(
                    &request,
                    &plan,
                    "incomplete",
                    "stop_after_batches",
                    Some(2),
                    "verified_reader_guard",
                    1,
                    &tables,
                    rows,
                    "retired",
                    None,
                    Vec::new(),
                );
            }
        }
        match session.pull_and_apply(&mut runtime).await {
            Ok(ApplyStep::Applied { .. }) => applied += 1,
            Ok(ApplyStep::Replay { .. } | ApplyStep::TableDone { .. }) => {}
            Ok(ApplyStep::Exhausted) => break,
            Err(err) => {
                let tables = session.tables();
                let rows = tables.iter().map(|table| table.rows).sum();
                let mapped = map_import(&err);
                let _ = session.cancel(&mut runtime).await;
                drop(runtime);
                return write_closed(
                    &request,
                    &plan,
                    "incomplete",
                    mapped.fact,
                    Some(2),
                    "verified_reader_guard",
                    1,
                    &tables,
                    rows,
                    "retired",
                    None,
                    Vec::new(),
                );
            }
        }
    }
    let preview = session.tables();
    let run_id = clean_run_id(session.run_id());
    let summary = match session.finish(&mut runtime).await {
        Ok(summary) => summary,
        Err(err) => {
            let rows = preview.iter().map(|table| table.rows).sum();
            let mapped = map_import(&err);
            drop(runtime);
            return write_closed(
                &request,
                &plan,
                "incomplete",
                mapped.fact,
                Some(2),
                "verified_reader_guard",
                1,
                &preview,
                rows,
                "retired",
                run_id,
                Vec::new(),
            );
        }
    };
    drop(runtime);
    let physical: Vec<String> = summary
        .tables
        .iter()
        .map(|table| table.physical_name.clone())
        .collect();
    let mut admin = pg::dial(
        &plan.config.target_host,
        plan.config.target_port,
        &plan.config.target_admin_user,
        admin_password.as_str(),
        &plan.config.target_database,
        &target_ca,
        connect_timeout,
        SessionKind::Admin,
    )
    .await?;
    if pg::grant_reader(&mut admin, &plan.config.target_reader_user, &physical).await.is_err() {
        drop(admin);
        let rows = summary.tables.iter().map(|table| table.rows).sum();
        return write_closed(
            &request,
            &plan,
            "incomplete",
            "reader_grant",
            Some(2),
            "verified_reader_guard",
            1,
            &summary.tables,
            rows,
            "awaiting_publish",
            clean_run_id(&summary.run_id),
            Vec::new(),
        );
    }
    drop(admin);
    let rows = summary.tables.iter().map(|table| table.rows).sum();
    write_closed(
        &request,
        &plan,
        "complete",
        "none",
        Some(2),
        "verified_reader_guard",
        1,
        &summary.tables,
        rows,
        "awaiting_publish",
        clean_run_id(&summary.run_id),
        Vec::new(),
    )
}

fn open_failure(request: &CopyRequest, plan: &Plan, err: ImportError) -> Result<CopyOutcome, MirrorError> {
    let (peak, fact, quarantine, failure, state) = match &err {
        ImportError::NotReady => (Some(0), "not_opened", Vec::new(), "not_ready", "refused"),
        ImportError::Unsupported { type_name, oid } => (
            Some(2),
            "verified_reader_guard",
            quarantine_for(plan, *oid, type_name),
            "unsupported_column",
            "refused",
        ),
        ImportError::Refused { .. } => (Some(2), "verified_reader_guard", Vec::new(), "refused", "refused"),
        ImportError::Source(text) => {
            let mapped = map_source_text(text);
            let pre_connect = matches!(
                mapped.fact,
                "generation_reused" | "deadline" | "generation_invalid" | "config_identity" | "type_policy_identity" | "selection"
            );
            if pre_connect {
                (Some(0), "not_opened", Vec::new(), mapped.fact, if mapped.fact == "deadline" { "incomplete" } else { "refused" })
            } else {
                (Some(2), "verified_reader_guard", Vec::new(), mapped.fact, "refused")
            }
        }
        ImportError::Database { .. } => (None, "unknown", Vec::new(), "database", "refused"),
        ImportError::Binding(_) | ImportError::Conflict => (None, "unknown", Vec::new(), "binding", "refused"),
    };
    write_closed(
        request,
        plan,
        state,
        failure,
        peak,
        fact,
        1,
        &[],
        0,
        "none",
        None,
        quarantine,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_closed(
    request: &CopyRequest,
    plan: &Plan,
    state: &'static str,
    failure: &'static str,
    peak: Option<u64>,
    socket_fact: &'static str,
    recheck: u64,
    tables: &[TableSummary],
    rows: u64,
    loader_state: &'static str,
    loader_run_id: Option<String>,
    quarantine: Vec<Quarantine>,
) -> Result<CopyOutcome, MirrorError> {
    let records = tables.iter().map(table_record).collect::<Vec<_>>();
    let completed = if state == "complete" { records.clone() } else { Vec::new() };
    let incomplete = if state == "complete" {
        Vec::new()
    } else if records.is_empty() {
        plan.config
            .tables
            .iter()
            .map(|(schema, name)| TableRecord {
                schema: schema.clone(),
                name: name.clone(),
                physical_name: None,
                rows: 0,
                columns: Vec::new(),
            })
            .collect()
    } else {
        records
    };
    let target_bytes = target_config_bytes(plan)?;
    let target_sha = sha256_hex(&target_bytes);
    if state == "complete" {
        store_target_config(&request.target_config_path, &target_bytes)?;
    }
    let mut manifest = Manifest {
        mirror_state: state.to_string(),
        customer_complete: false,
        enterprise: "not_configured".to_string(),
        consistency: if state == "complete" {
            "single_snapshot".to_string()
        } else {
            "not_a_snapshot".to_string()
        },
        plan_sha256: plan.plan_sha256.clone(),
        admitted_sha256: plan.admitted_sha256.clone(),
        failure: failure.to_string(),
        rows,
        source_socket_peak: peak,
        source_socket_fact: socket_fact.to_string(),
        recheck_connections: recheck,
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
        loader_run_id,
        loader_state: loader_state.to_string(),
        mapping_algorithm: "physical-mapping/v1".to_string(),
        system_position: "_oo_pos numeric(38,0)".to_string(),
        target_config_path: request.target_config_path.display().to_string(),
        target_config_sha256: target_sha,
        tables_completed: completed,
        tables_incomplete: incomplete,
        quarantine,
        manifest_sha256: String::new(),
    };
    store_manifest(&request.manifest_path, &mut manifest)?;
    Ok(CopyOutcome {
        manifest,
        disposition: "stored",
    })
}

fn table_record(table: &TableSummary) -> TableRecord {
    let physical_name = oo_pipeline::physical_mapping::quote_physical_name(&table.physical_name)
        .ok()
        .map(|_| table.physical_name.clone());
    let columns = table
        .columns
        .iter()
        .filter_map(|column| {
            oo_pipeline::physical_mapping::quote_physical_name(&column.physical_name).ok()?;
            if column.name.is_empty() || column.name.len() > 63 || column.name.chars().any(|ch| ch.is_control()) {
                return None;
            }
            Some(ColumnRecord {
                name: column.name.clone(),
                attnum: column.attnum,
                oid: column.type_oid,
                physical_name: column.physical_name.clone(),
            })
        })
        .collect();
    TableRecord {
        schema: table.schema.clone(),
        name: table.name.clone(),
        physical_name,
        rows: table.rows,
        columns,
    }
}

fn quarantine_for(plan: &Plan, oid: i64, type_name: &str) -> Vec<Quarantine> {
    let type_name = if type_name.len() <= 63
        && !type_name.is_empty()
        && type_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == ' ')
    {
        type_name.to_string()
    } else {
        "type".to_string()
    };
    let mut rows = Vec::new();
    for table in &plan.admitted {
        for column in &table.columns {
            if column.oid == oid {
                rows.push(Quarantine {
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    column: column.name.clone(),
                    oid,
                    type_name: type_name.clone(),
                    reason: "unsupported_column".to_string(),
                });
            }
        }
    }
    if rows.is_empty() {
        rows.push(Quarantine {
            schema: "unlisted".to_string(),
            table: "unlisted".to_string(),
            column: "unlisted".to_string(),
            oid,
            type_name,
            reason: "unsupported_column".to_string(),
        });
    }
    rows
}

fn stream_budget(plan: &Plan) -> Result<(StreamBudget, CopyBudgets), MirrorError> {
    let max_value = u32::try_from(plan.config.max_bytes).map_err(|_| MirrorError::new("budget", "max_bytes"))?;
    let max_row = max_value.checked_mul(2).ok_or_else(|| MirrorError::new("budget", "max_bytes"))?;
    let max_chunk = max_row.checked_mul(2).ok_or_else(|| MirrorError::new("budget", "max_bytes"))?;
    let max_chunk = max_chunk.max(19);
    let batch_rows = u32::try_from(plan.config.batch_rows).map_err(|_| MirrorError::new("budget", "batch_rows"))?;
    let stream = StreamBudget::try_new(batch_rows, plan.config.max_bytes, max_value, max_row.max(2))
        .map_err(|_| MirrorError::new("budget", "stream"))?;
    let copy = CopyBudgets {
        max_value_bytes: max_value,
        max_row_bytes: max_row.max(2),
        max_chunk_bytes: max_chunk,
        max_rows_per_chunk: batch_rows,
    };
    Ok((stream, copy))
}

fn map_import(err: &ImportError) -> MirrorError {
    match err {
        ImportError::NotReady => MirrorError::new("refused", "not_ready"),
        ImportError::Unsupported { .. } => MirrorError::new("refused", "unsupported_column"),
        ImportError::Refused { .. } => MirrorError::new("refused", "refused"),
        ImportError::Conflict => MirrorError::new("refused", "conflict"),
        ImportError::Binding(_) => MirrorError::new("refused", "binding"),
        ImportError::Source(text) => map_source_text(text),
        ImportError::Database { sqlstate, .. } => {
            let fact = match sqlstate.as_str() {
                "28P01" | "28000" => "authentication_failed",
                "42501" => "insufficient_privilege",
                "3D000" => "database",
                "57014" => "statement_timeout",
                "55P03" => "lock_timeout",
                _ if sqlstate.starts_with("08") => "connection",
                _ => "sql",
            };
            match crate::error::parse_sqlstate(sqlstate) {
                Some(state) => MirrorError::with_sqlstate("database", fact, state),
                None => MirrorError::new("database", fact),
            }
        }
    }
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
