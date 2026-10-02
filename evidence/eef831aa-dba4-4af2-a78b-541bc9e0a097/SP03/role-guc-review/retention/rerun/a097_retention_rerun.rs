//! NEW attempt a097-retention. Not the original oracle.
//! The original file remains followup/a097_role_guc.rs.
//! This run keeps role-GUC, wrong CA, wrong host, admission, a reader
//! control, and the catalog refusal count. It does not repeat the full matrix.

use std::fs;
use std::time::Duration;

use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef,
    SourceErrorKind, SourceId, TableSelection,
};
use serde::Deserialize;
use zeroize::Zeroizing;

const LOG: &str = "/pglog/postgresql.log";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    guc_user: String,
    side_user: String,
    password_dir: String,
    ca_file: String,
    wrong_ca_file: String,
    wrong_host: String,
}

fn secret(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("secret file");
    Zeroizing::new(text.trim().to_string())
}

fn budget() -> PgBudget {
    PgBudget::new(
        Duration::from_secs(30),
        Duration::from_secs(30),
        Duration::from_secs(5),
        1_000,
        1024 * 1024,
    )
    .unwrap()
}

fn empty_selection() -> TableSelection {
    TableSelection::new(Vec::new()).unwrap()
}

async fn open(
    pointer: &Pointer,
    ca: &[u8],
    user: &str,
    password: &str,
    source_id: &str,
) -> Result<oo_connectors::PgSession, oo_connectors::SourceError> {
    let config = PgSourceConfig::try_new(
        SourceId::new(source_id).unwrap(),
        PgEndpoint::new(&pointer.host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(user, password).unwrap(),
        SecretRef::new("secret://a097/retention").unwrap(),
        PgTlsConfig::from_ca_pem(ca).unwrap(),
        budget(),
        empty_selection(),
    )
    .unwrap();
    PgConnector::new(config).connect().await
}

fn source_exec_count() -> usize {
    fs::read_to_string(LOG)
        .unwrap_or_default()
        .lines()
        .filter(|line| {
            line.contains("app=oo-pg-source")
                && (line.contains("LOG:  statement:") || line.contains("LOG:  execute"))
        })
        .count()
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(200)).await;
}

fn blocks(report: &oo_connectors::AccessReport) -> String {
    report
        .block_reasons
        .iter()
        .map(|item| format!("{}/{}", item.code, item.fact))
        .collect::<Vec<_>>()
        .join(",")
}

async fn run_probe(pointer: &Pointer, ca: &[u8], reader_pass: &str) {
    assert_eq!(std::env::var("PGSSLMODE").as_deref(), Ok("disable"));
    assert_eq!(std::env::var("PGUSER").as_deref(), Ok("forged-user"));
    assert_eq!(std::env::var("PGOPTIONS").as_deref(), Ok("-c role=oo_src_reader"));
    let mut session = open(
        pointer,
        ca,
        &pointer.reader_user,
        reader_pass,
        "a097_ret_probe",
    )
    .await
    .expect("probe connect");
    let report = session.inspect_access().await.expect("probe inspect");
    let principal = report.principal.value.as_ref().expect("principal");
    assert_eq!(principal.session_user, pointer.reader_user);
    assert_eq!(principal.current_user, pointer.reader_user);
    assert_eq!(report.tls.mode, "verify-full");
    assert!(report.tls.negotiated_version.starts_with("TLSv1."));
    assert!(report.registration_allowed, "{}", blocks(&report));
    println!(
        "a097_rerun child_reader_allowed=true tls={} admission={}",
        report.tls.negotiated_version, report.version.admission
    );
}

fn spawn_probe() {
    let exe = std::env::current_exe().expect("test exe");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "retention_role_guc_ca_host_admission",
            "--nocapture",
        ])
        .env("OO_A097_PROBE", "poison")
        .env("PGOPTIONS", "-c role=oo_src_reader")
        .env("PGSSLMODE", "disable")
        .env("PGUSER", "forged-user")
        .env("PGPASSWORD", "forged-secret")
        .env("PGDATABASE", "forged-db")
        .env("PGAPPNAME", "forged-app")
        .env("PGHOST", "127.0.0.1")
        .env("PGPORT", "1")
        .output()
        .expect("probe process");
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.starts_with("a097_rerun ") {
            println!("{line}");
        }
    }
    assert!(
        output.status.success(),
        "child probe failed status {}",
        output.status
    );
}

#[tokio::test]
async fn retention_role_guc_ca_host_admission() {
    let path = std::env::var("OO_PG_FIXTURE").expect("OO_PG_FIXTURE is required");
    let pointer: Pointer =
        serde_json::from_str(&fs::read_to_string(&path).expect("pointer")).unwrap();
    let reader_pass = secret(&pointer.password_dir, "reader.pass");
    let guc_pass = secret(&pointer.password_dir, "guc.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    if std::env::var("OO_A097_PROBE").ok().as_deref() == Some("poison") {
        run_probe(&pointer, &ca, reader_pass.as_str()).await;
        return;
    }
    let parent_sslmode = std::env::var_os("PGSSLMODE");
    let parent_options = std::env::var_os("PGOPTIONS");
    spawn_probe();
    assert_eq!(std::env::var_os("PGSSLMODE"), parent_sslmode);
    assert_eq!(std::env::var_os("PGOPTIONS"), parent_options);
    println!("a097_rerun child_env=isolated");

    let mut reader = open(
        &pointer,
        &ca,
        &pointer.reader_user,
        reader_pass.as_str(),
        "a097_ret_reader",
    )
    .await
    .expect("reader connect");
    let reader_report = reader.inspect_access().await.expect("reader inspect");
    let reader_principal = reader_report.principal.value.as_ref().expect("reader principal");
    println!(
        "a097_rerun reader_allowed={} session={} current={} tls_mode={} tls={} obs={} admission={} write={}",
        reader_report.registration_allowed,
        reader_principal.session_user,
        reader_principal.current_user,
        reader_report.tls.mode,
        reader_report.tls.negotiated_version,
        reader_report.tls.server_observation,
        reader_report.version.admission,
        reader_report.write.state
    );
    assert!(reader_report.registration_allowed, "{}", blocks(&reader_report));
    assert_eq!(reader_principal.session_user, pointer.reader_user);
    assert_eq!(reader_principal.current_user, pointer.reader_user);
    assert_eq!(reader_report.tls.mode, "verify-full");
    assert_eq!(reader_report.tls.trust, "caller-ca-only");
    assert!(reader_report.tls.hostname_verification);
    assert!(reader_report.tls.negotiated_version.starts_with("TLSv1."));
    assert_eq!(reader_report.tls.server_observation, "observed");
    assert_eq!(reader_report.version.admission, "tested");
    assert_eq!(reader_report.write.state, "read_only");
    drop(reader);

    let mut guc = open(
        &pointer,
        &ca,
        &pointer.guc_user,
        guc_pass.as_str(),
        "a097_ret_guc",
    )
    .await
    .expect("guc connect");
    settle().await;
    let before_inspect = source_exec_count();
    let guc_report = guc.inspect_access().await.expect("guc inspect");
    settle().await;
    let after_inspect = source_exec_count();
    let guc_principal = guc_report.principal.value.as_ref().expect("guc principal");
    println!(
        "a097_rerun guc_allowed={} session={} current={} blocks={} tls={} obs={} admission={} write={} inspect_delta={}",
        guc_report.registration_allowed,
        guc_principal.session_user,
        guc_principal.current_user,
        blocks(&guc_report),
        guc_report.tls.negotiated_version,
        guc_report.tls.server_observation,
        guc_report.version.admission,
        guc_report.write.state,
        after_inspect.saturating_sub(before_inspect)
    );
    assert!(!guc_report.registration_allowed);
    assert_eq!(guc_principal.session_user, pointer.guc_user);
    assert_eq!(guc_principal.current_user, pointer.side_user);
    assert!(guc_report.block_reasons.iter().any(|item| {
        item.code == "role_mismatch" && item.fact == "session_user"
    }));
    assert!(guc_report.block_reasons.iter().any(|item| {
        item.code == "startup_option" && item.fact == "startup_options"
    }));
    assert_eq!(guc_report.tls.mode, "verify-full");
    assert_eq!(guc_report.tls.negotiated_version, "unknown");
    assert_eq!(guc_report.tls.server_observation, "unknown");
    assert!(!guc_report.tls.negotiated_version.starts_with("TLSv1."));
    assert_eq!(guc_report.version.admission, "tested");
    assert_eq!(guc_report.version.major, 17);
    assert_eq!(guc_report.version.minor, 10);
    assert_eq!(guc_report.write.state, "unknown");
    assert_eq!(after_inspect, before_inspect, "blocked inspect issued sql");
    settle().await;
    let before_catalog = source_exec_count();
    match guc.catalog().await {
        Ok(_) => panic!("guc catalog was admitted"),
        Err(err) => {
            println!(
                "a097_rerun guc_catalog_kind={:?} fact={}",
                err.kind(),
                err.fact().unwrap_or("")
            );
            assert_eq!(err.kind(), SourceErrorKind::Config);
            assert_eq!(err.fact(), Some("session_user_mismatch"));
        }
    }
    settle().await;
    let after_catalog = source_exec_count();
    println!(
        "a097_rerun guc_catalog_delta={}",
        after_catalog.saturating_sub(before_catalog)
    );
    assert_eq!(after_catalog, before_catalog, "catalog refusal issued sql");
    drop(guc);

    let mut guc_again = open(
        &pointer,
        &ca,
        &pointer.guc_user,
        guc_pass.as_str(),
        "a097_ret_guc_fresh",
    )
    .await
    .expect("fresh guc");
    let again = guc_again.inspect_access().await.expect("fresh guc inspect");
    println!(
        "a097_rerun guc_fresh_blocks={} tls={}",
        blocks(&again),
        again.tls.negotiated_version
    );
    assert!(!again.registration_allowed);
    assert_eq!(again.tls.negotiated_version, "unknown");
    assert_eq!(again.tls.server_observation, "unknown");
    assert_eq!(again.version.admission, "tested");
    drop(guc_again);

    let mut reader_again = open(
        &pointer,
        &ca,
        &pointer.reader_user,
        reader_pass.as_str(),
        "a097_ret_reader_fresh",
    )
    .await
    .expect("fresh reader");
    let fresh = reader_again.inspect_access().await.expect("fresh reader inspect");
    println!(
        "a097_rerun reader_fresh_allowed={} tls={} write={}",
        fresh.registration_allowed, fresh.tls.negotiated_version, fresh.write.state
    );
    assert!(fresh.registration_allowed, "{}", blocks(&fresh));
    assert!(fresh.tls.negotiated_version.starts_with("TLSv1."));
    assert_eq!(fresh.write.state, "read_only");
    drop(reader_again);

    let wrong_ca = fs::read(&pointer.wrong_ca_file).expect("wrong ca");
    settle().await;
    let before_ca = source_exec_count();
    match open(
        &pointer,
        &wrong_ca,
        &pointer.reader_user,
        reader_pass.as_str(),
        "a097_ret_wrong_ca",
    )
    .await
    {
        Ok(_) => panic!("wrong ca connected"),
        Err(err) => {
            println!(
                "a097_rerun wrong_ca_kind={:?} fact={}",
                err.kind(),
                err.fact().unwrap_or("")
            );
            assert_eq!(err.kind(), SourceErrorKind::Tls);
            assert_eq!(err.fact(), Some("tls_handshake"));
        }
    }
    settle().await;
    let after_ca = source_exec_count();
    println!(
        "a097_rerun wrong_ca_delta={}",
        after_ca.saturating_sub(before_ca)
    );
    assert_eq!(after_ca, before_ca, "wrong ca issued sql");

    settle().await;
    let before_host = source_exec_count();
    let host_config = PgSourceConfig::try_new(
        SourceId::new("a097_ret_wrong_host").unwrap(),
        PgEndpoint::new(&pointer.wrong_host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(&pointer.reader_user, reader_pass.as_str()).unwrap(),
        SecretRef::new("secret://a097/retention").unwrap(),
        PgTlsConfig::from_ca_pem(&ca).unwrap(),
        budget(),
        empty_selection(),
    )
    .unwrap();
    match PgConnector::new(host_config).connect().await {
        Ok(_) => panic!("wrong host connected"),
        Err(err) => {
            println!(
                "a097_rerun wrong_host_kind={:?} fact={}",
                err.kind(),
                err.fact().unwrap_or("")
            );
            assert_eq!(err.kind(), SourceErrorKind::Tls);
            assert_eq!(err.fact(), Some("tls_handshake"));
        }
    }
    settle().await;
    let after_host = source_exec_count();
    println!(
        "a097_rerun wrong_host_delta={}",
        after_host.saturating_sub(before_host)
    );
    assert_eq!(after_host, before_host, "wrong host issued sql");
    println!("a097_rerun result=pass");
}
