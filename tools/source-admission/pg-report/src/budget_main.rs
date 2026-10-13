//! Live budget phases against one configured PostgreSQL session.
//! Passwords stay in files. This binary does not retry a refused read.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy,
    RunBinding, SecretRef, SelectedTable, SnapshotOwner, SourceError, SourceErrorKind, SourceId,
    StreamBudget, TableSelection, WindowRequest,
};
use serde_json::json;
use zeroize::Zeroizing;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let need = |name: &str| -> Option<String> {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let Some(phase) = need("--phase") else {
        eprintln!("phase required");
        return ExitCode::from(1);
    };
    let Ok(built) = build(&args) else {
        eprintln!("config rejected");
        return ExitCode::from(1);
    };
    match phase.as_str() {
        "gate-read" => gate_read(built, &args).await,
        "bounded-read" => bounded_read(built, &args).await,
        "exhaust" => expect_budget(built, "max_rows").await,
        "timeout" => expect_budget(built, "statement_timeout").await,
        "cancel" => cancel(built).await,
        _ => {
            eprintln!("unknown phase");
            ExitCode::from(1)
        }
    }
}

struct Built {
    config: PgSourceConfig,
    source_id: String,
    username: String,
    database: String,
    secret_ref: String,
    schema: String,
    table: String,
    statement_ms: u64,
    max_rows: u64,
    max_bytes: u64,
}

async fn gate_read(built: Built, args: &[String]) -> ExitCode {
    let need = |name: &str| -> Option<String> {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let Some(admit) = need("--admit-bin") else {
        return ExitCode::from(1);
    };
    let Some(access_out) = need("--access-out") else {
        return ExitCode::from(1);
    };
    let Some(evidence_out) = need("--evidence-out") else {
        return ExitCode::from(1);
    };
    let Some(config_out) = need("--config-out") else {
        return ExitCode::from(1);
    };
    let Some(decision_out) = need("--decision-out") else {
        return ExitCode::from(1);
    };
    let expect_rows = need("--expect-rows")
        .and_then(|text| text.parse::<usize>().ok())
        .unwrap_or(2);
    let config_bytes = match config_bytes(&built) {
        Ok(bytes) => bytes,
        Err(()) => return ExitCode::from(1),
    };
    let connector = PgConnector::new(built.config);
    let mut session = match connector.connect().await {
        Ok(session) => session,
        Err(err) => return fail_source("connect", &err),
    };
    let inspection = match session.inspect_budget().await {
        Ok(inspection) => inspection,
        Err(err) => return fail_source("inspect", &err),
    };
    if fs::write(&access_out, &inspection.access_bytes).is_err()
        || fs::write(&evidence_out, &inspection.evidence_bytes).is_err()
    {
        return ExitCode::from(1);
    }
    if fs::write(&config_out, &config_bytes).is_err() {
        return ExitCode::from(1);
    }
    let output = match Command::new(&admit)
        .args([
            "admit-pg-budget",
            "--evidence",
            &evidence_out,
            "--access",
            &access_out,
            "--config",
            &config_out,
        ])
        .output()
    {
        Ok(output) => output,
        Err(_) => return ExitCode::from(1),
    };
    let _ = fs::write(&decision_out, &output.stdout);
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        println!("admit_exit={code}");
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    let batch = match session
        .read_typed(
            &RunBinding {
                source_id: built.source_id,
                schema: built.schema,
                table: built.table,
                run_generation: 1,
                input_digest: [38; 32],
            },
            PgTypePolicy::PRESERVE_ROW,
        )
        .await
    {
        Ok(batch) => batch,
        Err(err) => return fail_source("read", &err),
    };
    println!("rows={}", batch.rows.len());
    if batch.rows.len() == expect_rows {
        ExitCode::from(0)
    } else {
        ExitCode::from(2)
    }
}

async fn bounded_read(built: Built, args: &[String]) -> ExitCode {
    let need = |name: &str| -> Option<String> {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let Some(admit) = need("--admit-bin") else {
        return ExitCode::from(1);
    };
    let Some(access_out) = need("--access-out") else {
        return ExitCode::from(1);
    };
    let Some(evidence_out) = need("--evidence-out") else {
        return ExitCode::from(1);
    };
    let Some(config_out) = need("--config-out") else {
        return ExitCode::from(1);
    };
    let Some(decision_out) = need("--decision-out") else {
        return ExitCode::from(1);
    };
    let Some(arrow_out) = need("--arrow-out") else {
        return ExitCode::from(1);
    };
    let Some(expect_rows) = need("--expect-rows").and_then(|text| text.parse::<u64>().ok()) else {
        return ExitCode::from(1);
    };
    let page_rows = need("--page-rows")
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(2_048);
    let deadline_ms = need("--deadline-ms")
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(300_000);
    let cancel_after_ms = need("--cancel-after-ms").and_then(|text| text.parse::<u64>().ok());
    let snapshot_file = need("--snapshot-file");
    let config_bytes = match config_bytes(&built) {
        Ok(bytes) => bytes,
        Err(()) => return ExitCode::from(1),
    };
    let guard_config = built.config.clone();
    let connector = PgConnector::new(built.config);
    let mut session = match connector.connect().await {
        Ok(session) => session,
        Err(err) => return fail_source("connect", &err),
    };
    let inspection = match session.inspect_budget().await {
        Ok(inspection) => inspection,
        Err(err) => return fail_source("inspect", &err),
    };
    if fs::write(&access_out, &inspection.access_bytes).is_err()
        || fs::write(&evidence_out, &inspection.evidence_bytes).is_err()
        || fs::write(&config_out, &config_bytes).is_err()
    {
        return ExitCode::from(1);
    }
    let output = match Command::new(&admit)
        .args([
            "admit-pg-budget",
            "--evidence",
            &evidence_out,
            "--access",
            &access_out,
            "--config",
            &config_out,
        ])
        .output()
    {
        Ok(output) => output,
        Err(_) => return ExitCode::from(1),
    };
    let _ = fs::write(&decision_out, &output.stdout);
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        println!("admit_exit={code}");
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    let mut guard = match PgConnector::new(guard_config).connect().await {
        Ok(guard) => guard,
        Err(err) => return fail_source("guard", &err),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(ms) = cancel_after_ms {
        let flag = Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            flag.store(true, Ordering::Relaxed);
        });
    }
    let deadline = Instant::now() + Duration::from_millis(deadline_ms);
    match session
        .read_window(
            &mut guard,
            WindowRequest {
                binding: &RunBinding {
                    source_id: built.source_id,
                    schema: built.schema,
                    table: built.table,
                    run_generation: 1,
                    input_digest: [38; 32],
                },
                policy: PgTypePolicy::PRESERVE_ROW,
                deadline,
                cancel: &cancel,
                arrow_out: Path::new(&arrow_out),
                page_rows,
                snapshot_file: snapshot_file.as_deref().map(Path::new),
            },
        )
        .await
    {
        Ok(receipt) => {
            println!(
                "rows={} pages={} bytes={} windows_used={} max_rows_in_window={} rate_waits={} snapshot={} arrow_sha256={}",
                receipt.rows,
                receipt.pages,
                receipt.payload_bytes,
                receipt.windows_used,
                receipt.max_rows_in_window,
                receipt.rate_waits,
                receipt.snapshot,
                receipt.arrow_sha256
            );
            if receipt.rows == expect_rows {
                ExitCode::from(0)
            } else {
                ExitCode::from(2)
            }
        }
        Err(err) => fail_source("read", &err),
    }
}

async fn expect_budget(built: Built, fact: &str) -> ExitCode {
    let connector = PgConnector::new(built.config);
    let mut session = match connector.connect().await {
        Ok(session) => session,
        Err(err) => return fail_source("connect", &err),
    };
    match session
        .read_typed(
            &RunBinding {
                source_id: built.source_id,
                schema: built.schema,
                table: built.table,
                run_generation: 1,
                input_digest: [38; 32],
            },
            PgTypePolicy::PRESERVE_ROW,
        )
        .await
    {
        Ok(_) => {
            println!("fact=read_succeeded");
            ExitCode::from(2)
        }
        Err(err) => {
            println!("fact={}", err.fact().unwrap_or(""));
            if err.kind() == SourceErrorKind::Budget && err.fact() == Some(fact) {
                ExitCode::from(0)
            } else {
                ExitCode::from(2)
            }
        }
    }
}

async fn cancel(built: Built) -> ExitCode {
    let connector = PgConnector::new(built.config);
    let deadline = Instant::now() + Duration::from_secs(60);
    let request = match connector
        .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 38, deadline)
        .await
    {
        Ok(request) => request,
        Err(err) => return fail_source("prepare", &err),
    };
    let mut owner = match SnapshotOwner::open(request).await {
        Ok(owner) => owner,
        Err(err) => return fail_source("open", &err),
    };
    let budget = match StreamBudget::try_new(1, 1_024, 64, 128) {
        Ok(budget) => budget,
        Err(err) => return fail_source("stream_budget", &err),
    };
    if let Err(err) = owner.open_stream(budget).await {
        return fail_source("open_stream", &err);
    }
    match owner.cancel().await {
        Ok(cancelled) => {
            println!("cancelled={}", cancelled.generation);
            ExitCode::from(0)
        }
        Err(err) => fail_source("cancel", &err),
    }
}

fn fail_source(step: &str, err: &SourceError) -> ExitCode {
    match err.sqlstate() {
        Some(state) => println!(
            "step={step} fact={} sqlstate={state}",
            err.fact().unwrap_or("")
        ),
        None => println!("step={step} fact={}", err.fact().unwrap_or("")),
    }
    ExitCode::from(1)
}

fn config_bytes(built: &Built) -> Result<Vec<u8>, ()> {
    let mut bytes = serde_json::to_vec(&json!({
        "protocol": "oo.source-admission.pg-budget/v1",
        "target_id": "postgres-17-native",
        "source_id": built.source_id,
        "secret_ref": built.secret_ref,
        "compat_mode": "postgres-native",
        "current_user": built.username,
        "database": built.database,
        "hard_caps": [
            {"unit": "Rows", "hard_limit": built.max_rows, "client_cap": built.max_rows, "confidence": "unknown"},
            {"unit": "Bytes", "hard_limit": built.max_bytes, "client_cap": built.max_bytes, "confidence": "unknown"},
            {"unit": "WallClock", "hard_limit": built.statement_ms, "client_cap": built.statement_ms, "confidence": "unknown"}
        ],
        "write_options": {
            "signal_table": false,
            "heartbeat_write": false,
            "create_stream": false,
            "create_trigger": false
        },
        "debezium": {
            "signal_data_collection": null,
            "heartbeat_action_query": null,
            "snapshot_mode": "never",
            "create_stream": false,
            "create_trigger": false
        },
        "requested_write_class": "R0",
        "r1w": null
    }))
    .map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn build(args: &[String]) -> Result<Built, ()> {
    let need = |name: &str| -> Option<String> {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let source_id = need("--source-id").ok_or(())?;
    let host = need("--host").ok_or(())?;
    let port = need("--port").ok_or(())?.parse::<u16>().map_err(|_| ())?;
    let database = need("--database").ok_or(())?;
    let username = need("--username").ok_or(())?;
    let password_file = need("--password-file").ok_or(())?;
    let ca_file = need("--ca-file").ok_or(())?;
    let secret_ref = need("--secret-ref").ok_or(())?;
    let schema = need("--schema").ok_or(())?;
    let table = need("--table").ok_or(())?;
    let statement_ms = parse_u64(args, "--statement-ms", 30_000)?;
    let lock_ms = parse_u64(args, "--lock-ms", 5_000)?;
    let connect_ms = parse_u64(args, "--connect-ms", 30_000)?;
    let max_rows = parse_u64(args, "--max-rows", 1_000)?;
    let max_bytes = parse_u64(args, "--max-bytes", 1_048_576)?;
    let password_raw = fs::read_to_string(password_file).map_err(|_| ())?;
    let password = Zeroizing::new(password_raw.trim_end_matches(['\n', '\r']).to_string());
    let ca = fs::read(ca_file).map_err(|_| ())?;
    let rows = u32::try_from(max_rows).map_err(|_| ())?;
    let config = PgSourceConfig::try_new(
        SourceId::new(source_id.clone()).map_err(|_| ())?,
        PgEndpoint::new(host, port, database.clone()).map_err(|_| ())?,
        PgCredentials::new(username.clone(), password.as_str()).map_err(|_| ())?,
        SecretRef::new(secret_ref.clone()).map_err(|_| ())?,
        PgTlsConfig::from_ca_pem(ca).map_err(|_| ())?,
        PgBudget::new(
            Duration::from_millis(connect_ms),
            Duration::from_millis(statement_ms),
            Duration::from_millis(lock_ms),
            rows,
            max_bytes,
        )
        .map_err(|_| ())?,
        TableSelection::new(vec![
            SelectedTable::new(schema.clone(), table.clone()).map_err(|_| ())?,
        ])
        .map_err(|_| ())?,
    )
    .map_err(|_| ())?;
    Ok(Built {
        config,
        source_id,
        username,
        database,
        secret_ref,
        schema,
        table,
        statement_ms,
        max_rows,
        max_bytes,
    })
}

fn parse_u64(args: &[String], flag: &str, default: u64) -> Result<u64, ()> {
    match args.windows(2).find(|pair| pair[0] == flag) {
        Some(pair) => pair[1].parse().map_err(|_| ()),
        None => Ok(default),
    }
}
