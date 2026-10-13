//! Operator commands. A missing W38 commit stops the run before import.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::admit::{identity_ok, session_report, sha256_json, AdmissionInput, SessionFacts};
use crate::audit_local::classify_csv;
use crate::config::{load_config, read_text, Config};
use crate::error::Fail;
use crate::pg::{
    arrow_sample, cancel_sleep, connect, connect_read_only, deadline_sleep, endpoint_from_files, judge_oracle, load_logistics,
    measure, observe_session, read_password, run_oracle, target_cleanup, target_partial,
    target_relations, type_gate, Endpoint,
};
use crate::admit::decision_admitted;
use crate::spawn::{contains_secret, decision_blocks, run_binary};

const STATE_PROTOCOL: &str = "oo.postgres-preview.state/v1";
const RUN_PROTOCOL: &str = "oo.postgres-preview.run/v1";

#[derive(Debug)]
struct Parsed {
    command: String,
    config: String,
    log: Option<String>,
    request: Option<String>,
}

pub fn cli_main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::from(1),
    };
    match runtime.block_on(dispatch()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fail) => {
            eprintln!("{fail}");
            ExitCode::from(fail.exit)
        }
    }
}

async fn dispatch() -> Result<(), Fail> {
    let raw: Vec<String> = env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("model-forward") {
        return model_forward().await;
    }
    if raw.first().map(String::as_str) == Some("builder-bind") {
        return crate::bind::run(&raw[1..]);
    }
    let parsed = parse(raw)?;
    let config = load_config(&read_text(&parsed.config)?)?;
    fs::create_dir_all(&config.out_dir).map_err(|_| Fail::new("config.read", 1))?;
    match parsed.command.as_str() {
        "dba-load" => {
            let dba = endpoint(&config.dba)?;
            load_logistics(&dba, &config.source.username).await
        }
        "dba-load-customer" => {
            let dba = endpoint(&config.dba)?;
            let loaded = crate::pg::load_customer_east(&dba, &config.source.username).await?;
            write_doc(&config, "customer-load.json", &json!({
                "protocol": "oo.postgres-preview.customer-load/v1",
                "account": "dba",
                "schema": "customer_east",
                "table": "delivery_notes",
                "rows": loaded.rows,
                "distinct_note_ids": loaded.distinct_note_ids,
                "hubs": loaded.hubs,
                "shipments": loaded.shipments,
                "tracking_events": loaded.tracking_events,
                "payroll_select": loaded.payroll_select,
                "note_insert": loaded.note_insert
            }), &secrets(&config)?)?;
            Ok(())
        }
        "refuse-unapproved" => cmd_refuse(&config, parsed.request.as_deref()),
        "oracle" => cmd_oracle(&config).await,
        "run" => cmd_run(&config, true).await,
        "customer-run" => cmd_run(&config, false).await,
        "deadline" => cmd_deadline(&config).await,
        "cancel" => cmd_cancel(&config).await,
        "audit" => cmd_audit(&config, parsed.log.as_deref()).await,
        "type-gate" => cmd_type(&config).await,
        "arrow-sample" => cmd_arrow(&config).await,
        "target-partial" => cmd_partial(&config).await,
        "cleanup" => cmd_cleanup(&config).await,
        "write-option-probe" => cmd_write_option(&config).await,
        "consume" => cmd_consume(&config),
        "capture-table-oracle" => crate::consume::capture_table_oracle(Path::new(&config.out_dir), &config.source).await,
        "consume-all" => {
            crate::consume::pack_slug_matches(Path::new(&config.out_dir), &config.pack_role)?;
            crate::consume::attach_all(Path::new(&config.out_dir), Path::new(&config.bins.preview), &config.source, &config.target)
        }
        "replay-role" => crate::consume::replay_installed_role(Path::new(&config.out_dir), Path::new(&config.bins.preview), &config.source, &config.target),
        "rebind" => cmd_rebind(&config),
        "pack-all" => cmd_pack_all(&config),
        "arrow-reseal" => cmd_arrow_reseal(&config),
        "capture-check" => finish_capture(&config, crate::capture::check(&config).await, "capture-check.json"),
        "capture-sync" => finish_capture(&config, crate::capture::sync(&config).await, "capture-sync.json"),
        "capture-compare" => finish_capture(&config, crate::capture::compare(&config).await, "capture-compare.json"),
        "capture-schedule" => finish_capture(&config, crate::capture::schedule(&config).await, "capture-schedule.json"),
        "capture-receipt" => crate::receipt::produce(&config, &read_text(&parsed.config)?).await,
        "capture-receipt-delta" => crate::receipt::produce_delta(&config, &read_text(&parsed.config)?).await,
        _ => Err(Fail::new("usage", 2)),
    }
}

fn parse(args: Vec<String>) -> Result<Parsed, Fail> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        usage();
        return Err(Fail::new("usage", 2));
    };
    let mut config = None;
    let mut log = None;
    let mut request = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match flag.as_str() {
            "--config" => config = value,
            "--log" => log = value,
            "--request" => request = value,
            _ => {
                usage();
                return Err(Fail::new("usage", 2));
            }
        }
    }
    let Some(config) = config else {
        usage();
        return Err(Fail::new("usage", 2));
    };
    if !matches!(
        command.as_str(),
        "dba-load"
            | "dba-load-customer"
            | "refuse-unapproved"
            | "oracle"
            | "run"
            | "customer-run"
            | "deadline"
            | "cancel"
            | "audit"
            | "type-gate"
            | "arrow-sample"
            | "target-partial"
            | "cleanup"
            | "write-option-probe"
            | "consume"
            | "consume-all"
            | "replay-role"
            | "capture-table-oracle"
            | "rebind"
            | "pack-all"
            | "arrow-reseal"
            | "capture-check"
            | "capture-sync"
            | "capture-compare"
            | "capture-schedule"
            | "capture-receipt"
            | "capture-receipt-delta"
    ) {
        usage();
        return Err(Fail::new("usage", 2));
    }
    Ok(Parsed { command, config, log, request })
}

fn cmd_refuse(config: &Config, request: Option<&str>) -> Result<(), Fail> {
    let Some(request) = request else {
        return Err(Fail::new("usage", 2));
    };
    let document = crate::config::unapproved_refusal(config, &serde_json::from_slice(&read_text(request)?).map_err(|_| Fail::new("config.schema", 1))?)?;
    write_doc(config, "unapproved-refusal.json", &document, &secrets(config)?)?;
    Err(Fail::new("unapproved", 6))
}

fn usage() {
    eprintln!(
        "commands: dba-load dba-load-customer refuse-unapproved oracle run customer-run deadline cancel audit type-gate arrow-sample target-partial cleanup write-option-probe consume consume-all replay-role rebind arrow-reseal capture-check capture-sync capture-compare capture-schedule capture-receipt capture-receipt-delta model-forward builder-bind"
    );
}

async fn model_forward() -> Result<(), Fail> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8317").await.map_err(|_| Fail::new("model.forward", 4))?;
    loop {
        let (mut inbound, _) = listener.accept().await.map_err(|_| Fail::new("model.forward", 4))?;
        drop(tokio::spawn(async move {
            if let Ok(mut outbound) = tokio::net::TcpStream::connect("host.docker.internal:8317").await {
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            }
        }));
    }
}

async fn cmd_oracle(config: &Config) -> Result<(), Fail> {
    let session = connect_read_only(&endpoint(&config.source)?).await?;
    let facts = run_oracle(&session).await?;
    write_doc(config, "oracle.json", &json_facts(&facts), &secrets(config)?)?;
    judge_oracle(&facts)
}

async fn cmd_type(config: &Config) -> Result<(), Fail> {
    let session = connect_read_only(&endpoint(&config.source)?).await?;
    let gate = type_gate(&session, &config.approved_objects).await?;
    write_doc(config, "type-gate.json", &json!({"rejected": gate.rejected}), &secrets(config)?)?;
    if gate.rejected.is_empty() { Ok(()) } else { Err(Fail::new("type_gate", 5)) }
}

async fn cmd_arrow(config: &Config) -> Result<(), Fail> {
    let session = connect_read_only(&endpoint(&config.source)?).await?;
    let bytes = arrow_sample(&session).await?;
    let path = Path::new(&config.out_dir).join("arrow-type-sample.arrow");
    fs::write(path, &bytes).map_err(|_| Fail::new("arrow.ipc", 1))?;
    write_doc(
        config,
        "arrow-type-sample.json",
        &json!({
            "protocol": "oo.postgres-preview.arrow-observation/v1",
            "admitted_extract": false,
            "reason": "pre_admission_type_sample",
            "rows": 3,
            "bytes": bytes.len()
        }),
        &secrets(config)?,
    )?;
    Ok(())
}

async fn cmd_deadline(config: &Config) -> Result<(), Fail> {
    let session = connect_read_only(&endpoint(&config.source)?).await?;
    let gate = deadline_sleep(&session).await?;
    write_doc(config, "deadline.json", &json!({"sqlstate": gate.sqlstate, "rows": 0}), &secrets(config)?)?;
    Ok(())
}

async fn cmd_cancel(config: &Config) -> Result<(), Fail> {
    let endpoint = endpoint(&config.source)?;
    let session = connect_read_only(&endpoint).await?;
    let gate = cancel_sleep(&session, &endpoint).await?;
    write_doc(config, "cancel.json", &json!({"sqlstate": gate.sqlstate, "rows": 0, "backend_active": false}), &secrets(config)?)?;
    Ok(())
}

async fn cmd_partial(config: &Config) -> Result<(), Fail> {
    let session = connect(&endpoint(&config.target)?).await?;
    target_partial(&session).await?;
    write_state(config, &fresh_run_id(), "importing", true)?;
    Ok(())
}

async fn cmd_cleanup(config: &Config) -> Result<(), Fail> {
    let state = read_state(config)?;
    if state.as_ref().is_none_or(|item| !item.partial || item.phase != "importing") {
        return Err(Fail::new("cleanup.phase", 9));
    }
    let session = connect(&endpoint(&config.target)?).await?;
    target_cleanup(&session).await?;
    let run_id = state.map(|item| item.run_id).unwrap_or_else(fresh_run_id);
    write_state(config, &run_id, "cleaned", false)?;
    Ok(())
}

async fn cmd_audit(config: &Config, log: Option<&str>) -> Result<(), Fail> {
    let Some(log) = log else {
        usage();
        return Err(Fail::new("usage", 2));
    };
    let secrets = secrets(config)?;
    let text = fs::read(log).map_err(|_| Fail::new("audit.read", 7))?;
    ensure_clean(&text, &secrets)?;
    let path = Path::new(&config.bins.admission);
    if !path.is_file() {
        write_doc(
            config,
            "audit.json",
            &json!({"admission_binary": "not_built", "proven": false, "basis": "binary_missing"}),
            &secrets,
        )?;
        return Err(Fail::new("binary.missing", 3));
    }
    let spawned = run_binary(
        path,
        &[
            "audit-log".to_owned(),
            "--reader".to_owned(),
            config.source.username.clone(),
            "--log".to_owned(),
            log.to_owned(),
        ],
        b"",
    )?;
    ensure_clean(&spawned.stdout, &secrets)?;
    ensure_clean(&spawned.stderr, &secrets)?;
    let value: Value = serde_json::from_slice(&spawned.stdout).map_err(|_| Fail::new("audit.output", 7))?;
    let proven = value.pointer("/zero_write/proven").and_then(Value::as_bool).unwrap_or(false);
    let local = classify_csv(&String::from_utf8_lossy(&text), &config.source.username);
    write_doc(
        config,
        "audit.json",
        &json!({
            "admission_binary": spawned.status,
            "decision": value,
            "local_classifier_not_admission_binary": {
                "proven": local.proven,
                "basis": local.basis,
                "reader_statements": local.reader_statements,
                "forbidden": local.forbidden,
                "other_users_with_forbidden": local.other_users_with_forbidden
            }
        }),
        &secrets,
    )?;
    if local.proven != proven {
        return Err(Fail::new("audit.disagreement", 7));
    }
    if proven { Ok(()) } else { Err(Fail::new("audit.unproven", 7)) }
}

async fn cmd_write_option(config: &Config) -> Result<(), Fail> {
    let secrets = secrets(config)?;
    let dir = Path::new(&config.out_dir).join("producer").join("hubs");
    let evidence = dir.join("evidence.json");
    let access = dir.join("access.json");
    let produced = dir.join("config.json");
    if !evidence.is_file() || !access.is_file() || !produced.is_file() {
        return Err(Fail::new("producer.missing", 3));
    }
    let mut probe: Value = serde_json::from_slice(&fs::read(produced).map_err(|_| Fail::new("config.read", 1))?)
        .map_err(|_| Fail::new("config.schema", 1))?;
    probe["write_options"]["signal_table"] = json!(true);
    let probe_path = Path::new(&config.out_dir).join("negative-config.json");
    write_bytes(probe_path.clone(), &serde_json::to_vec(&probe).map_err(|_| Fail::new("config.read", 1))?, &secrets)?;
    let spawned = run_binary(
        Path::new(&config.bins.admission),
        &[
            "admit-pg-budget".to_owned(),
            "--evidence".to_owned(),
            evidence.display().to_string(),
            "--access".to_owned(),
            access.display().to_string(),
            "--config".to_owned(),
            probe_path.display().to_string(),
        ],
        b"",
    )?;
    ensure_clean(&spawned.stdout, &secrets)?;
    ensure_clean(&spawned.stderr, &secrets)?;
    write_bytes(Path::new(&config.out_dir).join("negative-decision.json"), &spawned.stdout, &secrets)?;
    let (admitted, _, _, _) = decision_admitted(&spawned.stdout)?;
    let blocks = decision_blocks(&spawned.stdout)?;
    write_doc(
        config,
        "negative-write-option.json",
        &json!({"admitted": admitted, "blocks": blocks, "negative_probe": "signal_table"}),
        &secrets,
    )?;
    if admitted || !blocks.iter().any(|code| code == "SIGNAL_WRITE") {
        return Err(Fail::new("admission.bypass", 2));
    }
    Ok(())
}

fn cmd_consume(config: &Config) -> Result<(), Fail> {
    crate::consume::attach(
        Path::new(&config.out_dir),
        Path::new(&config.bins.preview),
        &config.source,
        &config.target,
    )
}

fn cmd_rebind(config: &Config) -> Result<(), Fail> {
    let secrets = secrets(config)?;
    bearer_ok(&config.model.bearer_file)?;
    crate::chain::rebind_catalog(&crate::chain::ChainIn {
        config,
        secrets: &secrets,
        run_id: "rebind",
        rows: 100_000,
        bytes: 0,
        session_sha: "rebind",
        target_tables: 0,
        commit: None,
    })
}

fn cmd_pack_all(config: &Config) -> Result<(), Fail> {
    crate::chain::seal_all_authorities(&crate::chain::ChainIn {
        config, secrets: &secrets(config)?, run_id: "pack-all-existing-capture",
        rows: 0, bytes: 0, session_sha: "existing-capture", target_tables: 0, commit: None,
    })
}

fn cmd_arrow_reseal(config: &Config) -> Result<(), Fail> {
    crate::chain::reseal_arrow(&crate::chain::ChainIn {
        config,
        secrets: &[],
        run_id: "arrow-reseal",
        rows: 100_000,
        bytes: 0,
        session_sha: "arrow-reseal",
        target_tables: 0,
        commit: None,
    })
}

async fn cmd_run(config: &Config, fixture: bool) -> Result<(), Fail> {
    let secrets = secrets(config)?;
    bearer_ok(&config.model.bearer_file)?;
    let run_id = fresh_run_id();
    if let Some(state) = read_state(config)? {
        if state.partial && state.phase != "cleaned" {
            write_doc(config, "partial-refusal.json", &json!({"run_id": run_id, "prior": state.run_id}), &secrets)?;
            return Err(Fail::new("partial_not_cleaned", 9));
        }
        if matches!(state.phase.as_str(), "importing" | "sealed" | "preview" | "detached") {
            write_doc(
                config,
                "foreign-run.json",
                &json!({"run_id": run_id, "prior": state.run_id, "same_run": false}),
                &secrets,
            )?;
            return Err(Fail::new("foreign_run", 9));
        }
    }
    let source = endpoint(&config.source)?;
    let session = connect_read_only(&source).await?;
    let facts = observe_session(&session, &config.approved_objects).await?;
    let input = admission_input(config, &facts);
    let session_doc = session_report(&input);
    let session_sha = sha256_json(&session_doc);
    write_doc(config, "session.json", &session_doc, &secrets)?;
    if !identity_ok(&input) {
        write_run(config, &run_id, "identity", &facts, (0, 0), &session_sha, &secrets)?;
        return Err(Fail::new("identity", 6));
    }
    let gate = type_gate(&session, &config.approved_objects).await?;
    write_doc(config, "type-gate.json", &json!({"rejected": gate.rejected}), &secrets)?;
    if !gate.rejected.is_empty() {
        write_run(config, &run_id, "type_gate", &facts, (0, 0), &session_sha, &secrets)?;
        return Err(Fail::new("type_gate", 5));
    }
    let measured = if fixture { measure(&session).await? } else { crate::pg::measure_objects(&session, &config.approved_objects).await? };
    if measured.rows == 0 {
        write_run(config, &run_id, "no_data", &facts, (0, measured.bytes), &session_sha, &secrets)?;
        return Err(Fail::new("no_data", 8));
    }
    if measured.rows > config.budget.max_rows || measured.bytes > config.budget.max_bytes {
        write_run(config, &run_id, "budget_cap", &facts, (measured.rows, measured.bytes), &session_sha, &secrets)?;
        write_state(config, &run_id, "budget_cap", false)?;
        return Err(Fail::new("budget_cap", 10));
    }
    if fixture {
        let oracle = run_oracle(&session).await?;
        write_doc(config, "oracle.json", &json_facts(&oracle), &secrets)?;
        judge_oracle(&oracle)?;
    }
    let target = connect(&endpoint(&config.target)?).await?;
    let target_tables = target_relations(&target).await?;
    finish_run(RunClose {
        config,
        secrets: &secrets,
        run_id: &run_id,
        facts: &facts,
        rows: measured.rows,
        bytes: measured.bytes,
        session_sha: &session_sha,
        target_tables,
    })
    .await
}

struct RunClose<'a> {
    config: &'a Config,
    secrets: &'a [Zeroizing<String>],
    run_id: &'a str,
    facts: &'a SessionFacts,
    rows: u64,
    bytes: u64,
    session_sha: &'a str,
    target_tables: u64,
}

async fn finish_run(close: RunClose<'_>) -> Result<(), Fail> {
    let RunClose { config, secrets, run_id, facts, rows, bytes, session_sha, target_tables } = close;
    let commit = crate::admit::read_w38_commit(Path::new(&config.coordinator_state))?;
    let result = crate::chain::execute(&crate::chain::ChainIn {
        config,
        secrets,
        run_id,
        rows,
        bytes,
        session_sha,
        target_tables,
        commit: commit.as_deref(),
    })
    .await;
    let token = match &result {
        Ok(()) => {
            write_state(config, run_id, "closed", false)?;
            "closed"
        }
        Err(fail) => fail.token,
    };
    write_run(config, run_id, token, facts, (rows, bytes), session_sha, secrets)?;
    result
}

fn admission_input<'a>(config: &'a Config, facts: &'a SessionFacts) -> AdmissionInput<'a> {
    AdmissionInput {
        source_id: &config.source_id,
        secret_ref: &config.secret_ref,
        expected_user: &config.source.username,
        expected_database: &config.source.database,
        facts,
    }
}

fn endpoint(file: &crate::config::EndpointFile) -> Result<Endpoint, Fail> {
    endpoint_from_files(&file.host, file.port, &file.database, &file.username, &file.password_file, &file.ca_file)
}

fn bearer_ok(path: &str) -> Result<(), Fail> {
    let meta = fs::metadata(path).map_err(|_| Fail::new("model.secret", 1))?;
    if meta.is_file() && meta.len() > 0 && meta.len() <= 512 { Ok(()) } else { Err(Fail::new("model.secret", 1)) }
}

fn finish_capture(config: &Config, report: crate::capture::Report, name: &str) -> Result<(), Fail> {
    let mut scrub = secrets(config)?;
    if let Some(capture) = &config.capture
        && Path::new(&capture.password_file).is_file()
    {
        scrub.push(read_password(&capture.password_file)?);
    }
    write_doc(config, name, &report.body, &scrub)?;
    match report.error {
        Some(fail) => Err(fail),
        None => Ok(()),
    }
}

fn secrets(config: &Config) -> Result<Vec<Zeroizing<String>>, Fail> {
    let mut values = vec![
        read_password(&config.source.password_file)?,
        read_password(&config.dba.password_file)?,
        read_password(&config.target.password_file)?,
    ];
    if let Some(denied) = &config.denied {
        values.push(read_password(&denied.password_file)?);
    }
    Ok(values)
}

fn ensure_clean(bytes: &[u8], secrets: &[Zeroizing<String>]) -> Result<(), Fail> {
    for secret in secrets {
        if contains_secret(bytes, secret) {
            return Err(Fail::new("secret.leak", 1));
        }
    }
    Ok(())
}

fn write_doc(config: &Config, name: &str, value: &Value, secrets: &[Zeroizing<String>]) -> Result<(), Fail> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| Fail::new("config.read", 1))?;
    write_bytes(Path::new(&config.out_dir).join(name), &bytes, secrets)
}

fn write_bytes(path: std::path::PathBuf, bytes: &[u8], secrets: &[Zeroizing<String>]) -> Result<(), Fail> {
    ensure_clean(bytes, secrets)?;
    fs::write(path, bytes).map_err(|_| Fail::new("config.read", 1))
}

fn write_run(
    config: &Config,
    run_id: &str,
    token: &str,
    facts: &SessionFacts,
    measured: (u64, u64),
    session_sha: &str,
    secrets: &[Zeroizing<String>],
) -> Result<(), Fail> {
    write_doc(
        config,
        "run.json",
        &json!({
            "protocol": RUN_PROTOCOL,
            "run_id": run_id,
            "token": token,
            "rows": measured.0,
            "bytes": measured.1,
            "session_report_sha256": session_sha,
            "server_version_num": facts.server_version_num,
            "current_user": facts.current_user,
            "tls": facts.negotiated_tls,
            "model_called": Path::new(&config.out_dir).join("model-called").is_file(),
            "cost": "unknown"
        }),
        secrets,
    )
}

fn json_facts(facts: &[crate::pg::OracleFact]) -> Value {
    json!(facts.iter().map(|fact| json!({"name": fact.name, "got": fact.got})).collect::<Vec<_>>())
}

struct State {
    run_id: String,
    phase: String,
    partial: bool,
}

fn read_state(config: &Config) -> Result<Option<State>, Fail> {
    let path = Path::new(&config.out_dir).join("state.json");
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(|_| Fail::new("config.read", 1))?)
        .map_err(|_| Fail::new("config.read", 1))?;
    if value.get("protocol").and_then(Value::as_str) != Some(STATE_PROTOCOL) {
        return Err(Fail::new("config.schema", 1));
    }
    Ok(Some(State {
        run_id: value.get("run_id").and_then(Value::as_str).unwrap_or("").to_owned(),
        phase: value.get("phase").and_then(Value::as_str).unwrap_or("").to_owned(),
        partial: value.get("partial").and_then(Value::as_bool).unwrap_or(false),
    }))
}

fn write_state(config: &Config, run_id: &str, phase: &str, partial: bool) -> Result<(), Fail> {
    let value = json!({
        "protocol": STATE_PROTOCOL,
        "run_id": run_id,
        "phase": phase,
        "partial": partial
    });
    write_doc(config, "state.json", &value, &secrets(config)?)
}

fn fresh_run_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|item| item.as_nanos()).unwrap_or(0);
    format!("run-{nanos:x}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_command_is_usage() {
        let err = parse(vec!["pack".to_owned(), "--config".to_owned(), "x".to_owned()]).unwrap_err();
        assert_eq!(err.token, "usage");
        assert_eq!(err.exit, 2);
    }
}
