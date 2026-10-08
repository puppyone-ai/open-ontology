//! Serial per-project worker. A model failure does not approve, pack, or publish.

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_postgres::Client;

use crate::canonical::sha256_hex;
use crate::config::{binaries_ready, project_input_hash, AppConfig, ProjectCfg};
use crate::membership::Member;
use crate::spawn::{run_child, ChildSpec};
use crate::stages::{self, Resume};
use crate::store::{self, BuildRow, StageRow};

pub async fn supervise(
    config: Arc<AppConfig>,
    stop: Arc<AtomicBool>,
    cancels: Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
) {
    let owner = format!("pid-{}", std::process::id());
    while !stop.load(Ordering::SeqCst) {
        let client = match store::connect(&config.postgres).await {
            Ok(client) => client,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let job = match store::claim(&client, &owner).await {
            Ok(Some(job)) => job,
            _ => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let _ = drive(&client, &config, job, &cancels).await;
    }
}

async fn drive(
    client: &Client,
    config: &AppConfig,
    job: BuildRow,
    cancels: &Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
) -> Result<(), ()> {
    let Some(project) = config.projects.iter().find(|item| item.project_id == job.project_id) else {
        let _ = store::set_job(client, &job.build_id, "failed", &job.stage, Some("membership.refused"), &job.input_sha256).await;
        return Err(());
    };
    let input = project_input_hash(project);
    if input != job.input_sha256 || project.config_commit != job.config_commit {
        let _ = store::set_job(client, &job.build_id, "failed", &job.stage, Some("membership.refused"), &job.input_sha256).await;
        return Err(());
    }
    if job.status == "cancel_requested" {
        return settle_cancel(client, &job).await;
    }
    if !binaries_ready(&config.binaries) {
        let _ = store::set_job(client, &job.build_id, "failed", &job.stage, Some("dependency.not_ready"), &job.input_sha256).await;
        return Err(());
    }
    let dir = PathBuf::from(&config.work_dir).join("builds").join(&job.build_id);
    std::fs::create_dir_all(&dir).map_err(|_| ())?;
    loop {
        let stages = store::stage_rows(client, &job.build_id).await.map_err(|_| ())?;
        let Some(row) = next_stage(&stages) else {
            break;
        };
        match stages::resume_action(&row.stage, &row.status, row.output_sha256.as_deref()) {
            Resume::Skip => continue,
            Resume::Await => return Ok(()),
            Resume::Failed => {
                let _ = store::set_job(client, &job.build_id, "failed", &row.stage, row.detail_code.as_deref(), &job.input_sha256).await;
                return Err(());
            }
            Resume::Uncertain => {
                let _ = store::set_stage(client, &job.build_id, &row.stage, "failed", &job.input_sha256, None, Some("resume.uncertain")).await;
                let _ = store::set_job(client, &job.build_id, "failed", &row.stage, Some("resume.uncertain"), &job.input_sha256).await;
                return Err(());
            }
            Resume::Run => {
                let cancel = Arc::new(AtomicBool::new(false));
                if let Ok(mut map) = cancels.lock() {
                    map.insert(job.build_id.clone(), cancel.clone());
                }
                if fresh_cancel(client, &job.build_id).await {
                    cancel.store(true, Ordering::SeqCst);
                }
                if !run_stage(client, config, project, &job, &row.stage, &dir, &cancel).await {
                    if fresh_cancel(client, &job.build_id).await {
                        let _ = store::mark_child_exited(client, &job.build_id).await;
                        let _ = store::finish_canceled(client, &job.build_id).await;
                        return Ok(());
                    }
                    let _ = store::set_job(client, &job.build_id, "failed", &row.stage, Some("stage.failed"), &job.input_sha256).await;
                    return Err(());
                }
            }
        }
    }
    let _ = store::set_job(client, &job.build_id, "succeeded", "S8", None, &job.input_sha256).await;
    Ok(())
}

async fn fresh_cancel(client: &Client, build_id: &str) -> bool {
    let Ok(Some(row)) = client
        .query_opt(
            "SELECT status FROM oo_selfbuild_api.build_job WHERE build_id = $1",
            &[&build_id],
        )
        .await
    else {
        return false;
    };
    let status: String = row.get(0);
    status == "cancel_requested"
}

async fn settle_cancel(client: &Client, job: &BuildRow) -> Result<(), ()> {
    if job.child_pid.is_some() && !job.child_exited {
        let _ = store::heartbeat(client, &job.build_id, job.lease_generation).await;
        return Err(());
    }
    let _ = store::mark_child_exited(client, &job.build_id).await;
    let _ = store::finish_canceled(client, &job.build_id).await;
    Ok(())
}

fn next_stage(stages: &[StageRow]) -> Option<&StageRow> {
    store::STAGES.iter().find_map(|stage| {
        stages.iter().find(|row| row.stage == *stage && !matches!(stages::resume_action(stage, &row.status, row.output_sha256.as_deref()), Resume::Skip))
    })
}

async fn run_stage(
    client: &Client,
    config: &AppConfig,
    project: &ProjectCfg,
    job: &BuildRow,
    stage: &str,
    dir: &Path,
    cancel: &Arc<AtomicBool>,
) -> bool {
    let _ = store::set_stage(client, &job.build_id, stage, "running", &job.input_sha256, None, None).await;
    match stage {
        "S0" => s0(client, config, project, job, dir, cancel.clone()).await,
        "S1" => s1(client, config, project, job, dir, cancel.clone()).await,
        "S2" | "S3" | "S4" => rules(client, config, project, job, dir, cancel.clone()).await,
        "S5" => s5(client, config, project, job, dir, cancel.clone()).await,
        "S6" | "S7" | "S8" => suggestions(client, config, project, job, dir, cancel.clone()).await,
        "S9" | "S10" => {
            let _ = store::set_stage(client, &job.build_id, stage, "awaiting", &job.input_sha256, None, None).await;
            true
        }
        _ => false,
    }
}

async fn s0(
    client: &Client,
    config: &AppConfig,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    cancel: Arc<AtomicBool>,
) -> bool {
    let config_path = dir.join("pg-builder.config.json");
    if write_private(&config_path, &serde_json::to_vec(&stages::builder_config(project, &config.binaries.catalog)).unwrap_or_default()).is_err() {
        return fail(client, job, "S0", "config.write").await;
    }
    let output = spawn(config, &config.binaries.catalog, vec!["catalog-pg".to_owned()], stages::catalog_stdin(project), Duration::from_secs(300), &cancel, project).await;
    if !output.child_exited {
        return false;
    }
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.killed || output.secret_leak || output.exit != Some(0) {
        return fail(client, job, "S0", "catalog.failed").await;
    }
    let catalog_path = dir.join("catalog.json");
    if write_private(&catalog_path, &output.stdout).is_err() {
        return fail(client, job, "S0", "catalog.write").await;
    }
    let comments = dir.join("comments.json");
    let seal = dir.join("seal.json");
    let comments_out = spawn(
        config,
        &config.binaries.pg_builder,
        vec!["comments".to_owned(), "--config".to_owned(), path_str(&config_path), "--out".to_owned(), path_str(&comments)],
        Vec::new(),
        Duration::from_secs(300),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if comments_out.exit != Some(0) || comments_out.secret_leak {
        return fail(client, job, "S0", "comments.failed").await;
    }
    let seal_out = spawn(
        config,
        &config.binaries.pg_builder,
        vec!["seal".to_owned(), "--catalog".to_owned(), path_str(&catalog_path), "--out".to_owned(), path_str(&seal)],
        Vec::new(),
        Duration::from_secs(60),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if seal_out.exit != Some(0) || seal_out.secret_leak {
        return fail(client, job, "S0", "seal.failed").await;
    }
    let hash = sha256_hex(&output.stdout);
    bind_visible(client, project, job, &hash).await;
    store::set_stage(client, &job.build_id, "S0", "succeeded", &job.input_sha256, Some(&hash), None)
        .await
        .unwrap_or(false)
}

async fn s1(
    client: &Client,
    config: &AppConfig,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    cancel: Arc<AtomicBool>,
) -> bool {
    let config_path = dir.join("pg-builder.config.json");
    let profile = dir.join("profile.json");
    let output = spawn(
        config,
        &config.binaries.pg_builder,
        vec!["profile".to_owned(), "--config".to_owned(), path_str(&config_path), "--out".to_owned(), path_str(&profile)],
        Vec::new(),
        Duration::from_secs(300),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.exit != Some(0) || output.secret_leak {
        return fail(client, job, "S1", "profile.failed").await;
    }
    let cards = dir.join("cards.json");
    let output = spawn(
        config,
        &config.binaries.pg_builder,
        vec![
            "cards".to_owned(),
            "--config".to_owned(),
            path_str(&config_path),
            "--catalog".to_owned(),
            path_str(&dir.join("catalog.json")),
            "--profile".to_owned(),
            path_str(&profile),
            "--comments".to_owned(),
            path_str(&dir.join("comments.json")),
            "--seal".to_owned(),
            path_str(&dir.join("seal.json")),
            "--out".to_owned(),
            path_str(&cards),
        ],
        Vec::new(),
        Duration::from_secs(60),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.exit != Some(0) || output.secret_leak {
        return fail(client, job, "S1", "cards.failed").await;
    }
    let bytes = std::fs::read(&cards).unwrap_or_default();
    let hash = sha256_hex(&bytes);
    store::set_stage(client, &job.build_id, "S1", "succeeded", &job.input_sha256, Some(&hash), None)
        .await
        .unwrap_or(false)
}

async fn rules(
    client: &Client,
    config: &AppConfig,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    cancel: Arc<AtomicBool>,
) -> bool {
    let args = stages::rules_args(
        &path_str(&dir.join("catalog.json")),
        &path_str(&dir.join("profile.json")),
        &path_str(&dir.join("cards.json")),
        &project.rules_auto_accept,
    );
    let output = spawn(config, &config.binaries.rules, args, Vec::new(), Duration::from_secs(60), &cancel, project).await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.exit != Some(0) || output.secret_leak {
        return fail(client, job, "S2", "rules.failed").await;
    }
    let path = dir.join("rules.json");
    if write_private(&path, &output.stdout).is_err() {
        return fail(client, job, "S2", "rules.write").await;
    }
    let hash = sha256_hex(&output.stdout);
    for stage in ["S2", "S3", "S4"] {
        if !store::set_stage(client, &job.build_id, stage, "succeeded", &job.input_sha256, Some(&hash), None)
            .await
            .unwrap_or(false)
        {
            return false;
        }
    }
    true
}

async fn s5(
    client: &Client,
    config: &AppConfig,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    cancel: Arc<AtomicBool>,
) -> bool {
    let Some(principal) = config.principals.iter().find(|item| item.ledger_role == "reviewer") else {
        return fail(client, job, "S5", "dependency.not_ready").await;
    };
    let approval = dir.join("card-approval.json");
    let today = utc_date();
    let output = spawn(
        config,
        &config.binaries.pg_builder,
        vec![
            "approve".to_owned(),
            "--cards".to_owned(),
            path_str(&dir.join("cards.json")),
            "--reviewer-label".to_owned(),
            principal.reviewer_label.clone(),
            "--reviewer-at".to_owned(),
            today,
            "--out".to_owned(),
            path_str(&approval),
        ],
        Vec::new(),
        Duration::from_secs(30),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.exit != Some(0) || output.secret_leak {
        return fail(client, job, "S5", "approval.failed").await;
    }
    let cache = dir.join("cache");
    let _ = std::fs::create_dir_all(&cache);
    let generation = dir.join("generation.json");
    let output = spawn(
        config,
        &config.binaries.pg_builder,
        vec![
            "generate".to_owned(),
            "--config".to_owned(),
            path_str(&dir.join("pg-builder.config.json")),
            "--seal".to_owned(),
            path_str(&dir.join("seal.json")),
            "--cards".to_owned(),
            path_str(&dir.join("cards.json")),
            "--approval".to_owned(),
            path_str(&approval),
            "--cache".to_owned(),
            path_str(&cache),
            "--out".to_owned(),
            path_str(&generation),
        ],
        Vec::new(),
        // Three serial model calls share the 300s snapshot budget.
        Duration::from_secs(300),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.secret_leak || output.killed || output.exit != Some(0) {
        note_stderr(dir, "S5", &output);
        return fail(client, job, "S5", "generate.failed").await;
    }
    let bytes = std::fs::read(&generation).unwrap_or_default();
    let parsed = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or(serde_json::Value::Null);
    if parsed.get("outcome").and_then(serde_json::Value::as_str) != Some("validated") {
        return fail(client, job, "S5", "generate.failed").await;
    }
    let hash = sha256_hex(&bytes);
    store::set_stage(client, &job.build_id, "S5", "succeeded", &job.input_sha256, Some(&hash), None)
        .await
        .unwrap_or(false)
}

async fn suggestions(
    client: &Client,
    config: &AppConfig,
    project: &ProjectCfg,
    job: &BuildRow,
    dir: &Path,
    cancel: Arc<AtomicBool>,
) -> bool {
    let catalog = match std::fs::read(dir.join("catalog.json")) {
        Ok(bytes) => bytes,
        Err(_) => return fail(client, job, "S6", "catalog.missing").await,
    };
    let profile = match std::fs::read(dir.join("profile.json")) {
        Ok(bytes) => bytes,
        Err(_) => return fail(client, job, "S6", "profile.missing").await,
    };
    let catalog_value = match serde_json::from_slice(&catalog) {
        Ok(value) => value,
        Err(_) => return fail(client, job, "S6", "catalog.malformed").await,
    };
    let profile_value = match serde_json::from_slice(&profile) {
        Ok(value) => value,
        Err(_) => return fail(client, job, "S6", "profile.malformed").await,
    };
    let command = match crate::suggest::command(&project.project_id, &catalog_value, &profile_value) {
        Ok(value) => value,
        Err(_) => return fail(client, job, "S6", "suggestions.project").await,
    };
    let stdin = serde_json::to_vec(&command).unwrap_or_default();
    let output = spawn(
        config,
        &config.binaries.suggestions,
        stages::suggestions_args(),
        stdin,
        Duration::from_secs(60),
        &cancel,
        project,
    )
    .await;
    let _ = store::mark_child_exited(client, &job.build_id).await;
    if output.exit != Some(0) || output.secret_leak {
        note_stderr(dir, "S6", &output);
        return fail(client, job, "S6", "suggestions.failed").await;
    }
    if write_private(&dir.join("suggestions.json"), &output.stdout).is_err() {
        return fail(client, job, "S6", "suggestions.write").await;
    }
    let hash = sha256_hex(&output.stdout);
    for stage in ["S6", "S7", "S8"] {
        if !store::set_stage(client, &job.build_id, stage, "succeeded", &job.input_sha256, Some(&hash), None)
            .await
            .unwrap_or(false)
        {
            return false;
        }
    }
    let _ = store::set_stage(client, &job.build_id, "S9", "awaiting", &job.input_sha256, None, None).await;
    let _ = store::set_stage(client, &job.build_id, "S10", "awaiting", &job.input_sha256, None, None).await;
    true
}

async fn bind_visible(client: &Client, project: &ProjectCfg, job: &BuildRow, source_sha256: &str) {
    for column in &project.visible_columns {
        let mut parts = column.split('.');
        let Some(schema_name) = parts.next() else { continue };
        let Some(table_name) = parts.next() else { continue };
        let Some(column_name) = parts.next() else { continue };
        if !project.source.tables.iter().any(|(schema, table)| schema == schema_name && table == table_name) {
            continue;
        }
        let record = serde_json::json!({
            "column": column_name,
            "project_id": project.project_id,
            "schema": schema_name,
            "source_sha256": source_sha256,
            "table": table_name
        });
        let candidate = crate::canonical::canonical_sha256(&record);
        let member = Member {
            project_id: project.project_id.clone(),
            object_id: column.clone(),
            object_kind: "column".to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: table_name.to_owned(),
            column_name: Some(column_name.to_owned()),
            candidate_sha256: candidate,
            source_sha256: source_sha256.to_owned(),
            schema_sha256: source_sha256.to_owned(),
            evidence_sha256: source_sha256.to_owned(),
            generation_sha256: source_sha256.to_owned(),
        };
        let _ = store::insert_member(client, &job.build_id, &member).await;
    }
}

async fn spawn(
    _config: &AppConfig,
    binary: &str,
    args: Vec<String>,
    stdin: Vec<u8>,
    deadline: Duration,
    cancel: &Arc<AtomicBool>,
    project: &ProjectCfg,
) -> crate::spawn::ChildOutput {
    let spec = ChildSpec {
        binary: binary.to_owned(),
        args,
        stdin,
        deadline,
        needles: secret_needles(project),
        cancel: cancel.clone(),
        pid_out: Arc::new(AtomicU32::new(0)),
    };
    run_child(spec).await
}

fn secret_needles(project: &ProjectCfg) -> Vec<Vec<u8>> {
    let mut needles = Vec::new();
    for path in [&project.source.password_file, &project.llm.bearer_file] {
        if let Ok(mut bytes) = std::fs::read(path) {
            while bytes.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
                bytes.pop();
            }
            if !bytes.is_empty() {
                needles.push(bytes);
            }
        }
    }
    needles
}

async fn fail(client: &Client, job: &BuildRow, stage: &str, detail: &str) -> bool {
    let _ = store::set_stage(client, &job.build_id, stage, "failed", &job.input_sha256, None, Some(detail)).await;
    false
}

fn note_stderr(dir: &Path, stage: &str, output: &crate::spawn::ChildOutput) {
    if output.secret_leak || output.stderr.is_empty() {
        return;
    }
    let mut text = String::from_utf8_lossy(&output.stderr).replace(['\n', '\r'], " ");
    if text.len() > 240 {
        text.truncate(240);
    }
    let _ = write_private(&dir.join(format!("{stage}.err")), text.as_bytes());
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path)?;
    std::io::Write::write_all(&mut file, bytes)
}

fn path_str(path: &std::path::Path) -> String {
    path.display().to_string()
}

pub(crate) fn utc_date() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

#[allow(dead_code)]
fn _row(row: &StageRow) -> &str {
    row.stage.as_str()
}
