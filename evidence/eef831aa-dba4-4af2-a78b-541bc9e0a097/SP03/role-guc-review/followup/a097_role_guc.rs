//! Independent a097 oracle. Production `PgConnector` only.
//! Hostile libpq variables are applied to a child process, never via `set_var`.

use std::fs;
use std::time::Duration;

use oo_connectors::{
    AccessReport, EMPTY_SCHEMA_HASH, PgBudget, PgConnector, PgCredentials, PgEndpoint,
    PgSourceConfig, PgTlsConfig, SecretRef, SourceErrorKind, SourceId, TableSelection, TlsReport,
};
use serde::Deserialize;
use sqlx::{ConnectOptions, Row};
use zeroize::Zeroizing;

const LOG: &str = "/pglog/postgresql.log";
const NOTES: &str = "/phases/oracle-notes.txt";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    inherit_user: String,
    setonly_user: String,
    definer_user: String,
    side_user: String,
    noset_user: String,
    super_user: String,
    bypass_user: String,
    createrole_user: String,
    createdb_user: String,
    oponly_user: String,
    castonly_user: String,
    guc_user: String,
    password_dir: String,
    ca_file: String,
    wrong_ca_file: String,
    wrong_host: String,
}

struct Obs {
    session_user: String,
    current_user: String,
    ssl: Option<bool>,
    version: Option<String>,
    role_setting: String,
    extra: String,
}

fn note(key: &str, value: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(NOTES)
        .expect("notes");
    writeln!(file, "{key}={value}").expect("notes write");
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
    database: &str,
    source_id: &str,
) -> Result<oo_connectors::PgSession, oo_connectors::SourceError> {
    let config = PgSourceConfig::try_new(
        SourceId::new(source_id).unwrap(),
        PgEndpoint::new(&pointer.host, pointer.port, database).unwrap(),
        PgCredentials::new(user, password).unwrap(),
        SecretRef::new("secret://a097/review").unwrap(),
        PgTlsConfig::from_ca_pem(ca).unwrap(),
        budget(),
        empty_selection(),
    )
    .unwrap();
    PgConnector::new(config).connect().await
}

fn source_exec_count() -> usize {
    let raw = fs::read_to_string(LOG).unwrap_or_default();
    raw.lines()
        .filter(|line| {
            line.contains("app=oo-pg-source")
                && (line.contains("LOG:  statement:") || line.contains("LOG:  execute"))
        })
        .count()
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(200)).await;
}

fn has_block(report: &AccessReport, code: &str, fact: &str) -> bool {
    report
        .block_reasons
        .iter()
        .any(|item| item.code == code && item.fact == fact)
}

fn blocks_of(report: &AccessReport) -> String {
    report
        .block_reasons
        .iter()
        .map(|item| format!("{}/{}", item.code, item.fact))
        .collect::<Vec<_>>()
        .join(",")
}

async fn observe(pointer: &Pointer, ca: &[u8], user: &str, password: &str, database: &str) -> Obs {
    let options = sqlx::postgres::PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(database)
        .application_name("oo-a097-observe")
        .ssl_mode(sqlx::postgres::PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    let mut conn = options.connect().await.expect("observe connect");
    let row = sqlx::query(
        "SELECT SESSION_USER::text, CURRENT_USER::text, \
         (SELECT s.ssl FROM pg_catalog.pg_stat_ssl s WHERE s.pid = pg_catalog.pg_backend_pid()), \
         (SELECT s.version FROM pg_catalog.pg_stat_ssl s WHERE s.pid = pg_catalog.pg_backend_pid()), \
         pg_catalog.current_setting('role'), \
         COALESCE((SELECT pg_catalog.string_agg(extra.name, ',' ORDER BY extra.name) FROM ( \
           SELECT s.name FROM pg_catalog.pg_settings s \
           WHERE s.source IN ('client', 'session', 'startup', 'user', 'database', 'database user') \
           AND s.name <> ALL($1::text[]) \
           UNION SELECT 'role'::text WHERE pg_catalog.current_setting('role') IS DISTINCT FROM 'none' \
         ) AS extra), '')",
    )
    .bind(vec![
        "DateStyle".to_string(),
        "client_encoding".to_string(),
        "TimeZone".to_string(),
        "extra_float_digits".to_string(),
        "application_name".to_string(),
    ])
    .fetch_one(&mut conn)
    .await
    .expect("observe identity");
    Obs {
        session_user: row.try_get::<String, _>(0).expect("session_user"),
        current_user: row.try_get::<String, _>(1).expect("current_user"),
        ssl: row.try_get::<Option<bool>, _>(2).expect("ssl"),
        version: row.try_get::<Option<String>, _>(3).expect("version"),
        role_setting: row.try_get::<String, _>(4).expect("role"),
        extra: row.try_get::<String, _>(5).expect("extra"),
    }
}

fn record_obs(label: &str, obs: &Obs, report: &AccessReport) {
    note(&format!("{label}_session_user"), &obs.session_user);
    note(&format!("{label}_current_user"), &obs.current_user);
    note(
        &format!("{label}_ssl"),
        &obs.ssl.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()),
    );
    note(
        &format!("{label}_version"),
        obs.version.as_deref().unwrap_or("null"),
    );
    note(&format!("{label}_role_setting"), &obs.role_setting);
    note(&format!("{label}_extra"), &obs.extra);
    note(&format!("{label}_blocks"), &blocks_of(report));
    note(&format!("{label}_allowed"), &report.registration_allowed.to_string());
    note(&format!("{label}_tls_mode"), &report.tls.mode);
    note(&format!("{label}_tls_trust"), &report.tls.trust);
    note(
        &format!("{label}_tls_hostcheck"),
        &report.tls.hostname_verification.to_string(),
    );
    note(&format!("{label}_tls_version"), &report.tls.negotiated_version);
    note(&format!("{label}_tls_obs"), &report.tls.server_observation);
    note(&format!("{label}_admission"), &report.version.admission);
    note(&format!("{label}_write"), &report.write.state);
    if let Some(principal) = &report.principal.value {
        note(&format!("{label}_report_session"), &principal.session_user);
        note(&format!("{label}_report_current"), &principal.current_user);
        note(&format!("{label}_report_db"), &principal.database);
    }
}

fn expect_consistent(label: &str, obs: &Obs, report: &AccessReport, failures: &mut Vec<String>) {
    let Some(principal) = &report.principal.value else {
        failures.push(format!("{label}: principal missing"));
        return;
    };
    if principal.session_user != obs.session_user || principal.current_user != obs.current_user {
        failures.push(format!(
            "{label}: report users {} / {} vs observed {} / {}",
            principal.session_user, principal.current_user, obs.session_user, obs.current_user
        ));
    }
    if report.tls.mode != "verify-full"
        || report.tls.trust != "caller-ca-only"
        || !report.tls.hostname_verification
    {
        failures.push(format!("{label}: tls request flags changed"));
    }
    if report.version.admission != "tested" {
        failures.push(format!("{label}: admission {}", report.version.admission));
    }
    let aligned = obs.session_user == obs.current_user;
    if aligned {
        if has_block(report, "role_mismatch", "session_user") {
            failures.push(format!("{label}: role_mismatch on aligned users"));
        }
        match (&obs.ssl, &obs.version) {
            (Some(true), Some(version)) if version.starts_with("TLSv1.") => {
                if report.tls.server_observation != "observed"
                    || report.tls.negotiated_version != *version
                {
                    failures.push(format!(
                        "{label}: expected observed {version}, got {} {}",
                        report.tls.server_observation, report.tls.negotiated_version
                    ));
                }
            }
            (None, None) => {
                if report.tls.server_observation != "unknown"
                    || report.tls.negotiated_version != "unknown"
                {
                    failures.push(format!("{label}: missing row was not unknown"));
                }
                if report.registration_allowed {
                    failures.push(format!("{label}: missing stats admitted the session"));
                }
            }
            _ => failures.push(format!(
                "{label}: unexpected ssl/version pair {:?} {:?}",
                obs.ssl, obs.version
            )),
        }
    } else if !has_block(report, "role_mismatch", "session_user") {
        failures.push(format!("{label}: missing role_mismatch"));
    } else if obs.ssl.is_some() || obs.version.is_some() {
        failures.push(format!("{label}: hidden stats row was visible"));
    } else if report.tls.negotiated_version != "unknown"
        || report.tls.server_observation != "unknown"
    {
        failures.push(format!("{label}: mismatch fabricated a tls version"));
    }
    if obs.extra.is_empty() {
        if has_block(report, "startup_option", "startup_options") {
            failures.push(format!("{label}: startup block without extra settings"));
        }
    } else if !has_block(report, "startup_option", "startup_options") {
        failures.push(format!("{label}: extra [{}] did not block", obs.extra));
    }
    let should_admit = aligned && obs.extra.is_empty() && obs.ssl == Some(true);
    if report.registration_allowed != should_admit {
        failures.push(format!(
            "{label}: allowed={} expected={should_admit}",
            report.registration_allowed
        ));
    }
    if !should_admit && report.write.state != "unknown" {
        failures.push(format!("{label}: blocked write state {}", report.write.state));
    }
    if should_admit && report.write.state != "read_only" {
        failures.push(format!("{label}: admitted write state {}", report.write.state));
    }
    if report.tls.negotiated_version == "unknown" {
        if report.tls.server_observation != "unknown" {
            failures.push(format!("{label}: unknown version marked observed"));
        }
    }
}

async fn catalog_stops(label: &str, session: &mut oo_connectors::PgSession, fact: &str, failures: &mut Vec<String>) {
    settle().await;
    let before = source_exec_count();
    match session.catalog().await {
        Ok(_) => failures.push(format!("{label}: catalog was admitted")),
        Err(err) => {
            if err.kind() != SourceErrorKind::Config || err.fact() != Some(fact) {
                failures.push(format!(
                    "{label}: catalog {:?} {:?}",
                    err.kind(),
                    err.fact()
                ));
            }
        }
    }
    settle().await;
    let after = source_exec_count();
    note(
        &format!("{label}_catalog_delta"),
        &(after.saturating_sub(before)).to_string(),
    );
    if after != before {
        failures.push(format!("{label}: catalog issued {after} vs {before} statements"));
    }
}

async fn inspect_adds_no_sql(
    label: &str,
    session: &mut oo_connectors::PgSession,
    failures: &mut Vec<String>,
) -> AccessReport {
    settle().await;
    let before = source_exec_count();
    let report = session.inspect_access().await.expect("inspect");
    settle().await;
    let after = source_exec_count();
    note(
        &format!("{label}_inspect_delta"),
        &(after.saturating_sub(before)).to_string(),
    );
    if !report.registration_allowed && after != before {
        failures.push(format!("{label}: blocked inspect issued sql"));
    }
    report
}

async fn run_probe(pointer: &Pointer, ca: &[u8], reader_pass: &str) {
    assert_eq!(std::env::var("PGSSLMODE").as_deref(), Ok("disable"));
    assert_eq!(std::env::var("PGUSER").as_deref(), Ok("forged-user"));
    assert_eq!(std::env::var("PGPASSWORD").as_deref(), Ok("forged-secret"));
    assert_eq!(
        std::env::var("PGOPTIONS").as_deref(),
        Ok("-c role=oo_src_reader")
    );
    let mut session = open(
        pointer,
        ca,
        &pointer.reader_user,
        reader_pass,
        &pointer.database,
        "a097_probe",
    )
    .await
    .expect("probe connect");
    let report = session.inspect_access().await.expect("probe inspect");
    let principal = report.principal.value.as_ref().expect("principal");
    assert_eq!(principal.session_user, pointer.reader_user);
    assert_eq!(principal.current_user, pointer.reader_user);
    assert_eq!(report.tls.mode, "verify-full");
    assert!(report.tls.negotiated_version.starts_with("TLSv1."));
    assert_eq!(report.tls.server_observation, "observed");
    assert!(report.registration_allowed, "{}", blocks_of(&report));
}

fn spawn_probe() {
    let exe = std::env::current_exe().expect("test exe");
    let output = std::process::Command::new(exe)
        .args(["--exact", "independent_role_guc", "--nocapture"])
        .env("OO_A097_PROBE", "poison")
        .env("PGOPTIONS", "-c role=oo_src_reader")
        .env("PGSSLMODE", "disable")
        .env("PGSSLCERT", "/tmp/oo-a097-not-a-cert")
        .env("PGSSLKEY", "/tmp/oo-a097-not-a-key")
        .env("PGSSLROOTCERT", "/tmp/oo-a097-not-a-ca")
        .env("PGUSER", "forged-user")
        .env("PGPASSWORD", "forged-secret")
        .env("PGDATABASE", "forged-db")
        .env("PGAPPNAME", "forged-app")
        .env("PGHOST", "127.0.0.1")
        .env("PGPORT", "1")
        .output()
        .expect("probe process");
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let redacted: String = err
            .lines()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                !lower.contains("pass")
                    && !lower.contains("certificate")
                    && !lower.contains("private key")
                    && !line_has_long_token(line)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = fs::write("/phases/probe-stderr.txt", &redacted);
        panic!("child probe failed status {}", output.status);
    }
}

fn line_has_long_token(line: &str) -> bool {
    let mut run = 0_usize;
    for byte in line.bytes() {
        if byte.is_ascii_hexdigit() {
            run += 1;
            if run >= 12 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

#[tokio::test]
async fn independent_role_guc() {
    let path = std::env::var("OO_PG_FIXTURE").expect("OO_PG_FIXTURE is required");
    let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer")).unwrap();
    let reader_pass = secret(&pointer.password_dir, "reader.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    if std::env::var("OO_A097_PROBE").ok().as_deref() == Some("poison") {
        run_probe(&pointer, &ca, reader_pass.as_str()).await;
        return;
    }
    let parent_options = std::env::var_os("PGOPTIONS");
    let parent_sslmode = std::env::var_os("PGSSLMODE");
    spawn_probe();
    assert_eq!(std::env::var_os("PGOPTIONS"), parent_options);
    assert_eq!(std::env::var_os("PGSSLMODE"), parent_sslmode);
    assert!(std::env::var_os("PGPASSWORD").is_none());
    note("child_env", "isolated");

    let mut failures = Vec::new();
    let cases = [
        ("reader", pointer.reader_user.as_str(), "reader.pass", pointer.database.as_str()),
        ("guc", pointer.guc_user.as_str(), "guc.pass", pointer.database.as_str()),
        ("plain", "oo_a097_plain", "a097-plain.pass", pointer.database.as_str()),
        ("setnone", "oo_a097_setnone", "a097-setnone.pass", pointer.database.as_str()),
        ("self", "oo_a097_self", "a097-self.pass", pointer.database.as_str()),
        ("userguc", "oo_a097_userguc", "a097-userguc.pass", pointer.database.as_str()),
        ("allow", "oo_a097_allow", "a097-allow.pass", pointer.database.as_str()),
        ("dbuser", "oo_a097_dbuser", "a097-dbuser.pass", pointer.database.as_str()),
        ("case", "oo_a097_case_login", "a097-case.pass", pointer.database.as_str()),
        ("odd", "oo_a097_odd", "a097-odd.pass", pointer.database.as_str()),
        ("db_work_mem", pointer.reader_user.as_str(), "reader.pass", "oo_a097_db"),
        ("plain_db", "oo_a097_plain", "a097-plain.pass", "oo_a097_db"),
    ];

    for (label, user, file, database) in cases {
        let pass = secret(&pointer.password_dir, file);
        let obs = observe(&pointer, &ca, user, pass.as_str(), database).await;
        let mut session = open(&pointer, &ca, user, pass.as_str(), database, &format!("a097_{label}"))
            .await
            .unwrap_or_else(|err| panic!("{label} connect {:?} {:?}", err.kind(), err.fact()));
        let report = inspect_adds_no_sql(label, &mut session, &mut failures).await;
        record_obs(label, &obs, &report);
        expect_consistent(label, &obs, &report, &mut failures);
        match label {
            "allow" if !obs.extra.is_empty() => {
                failures.push(format!("allowlist leaked {}", obs.extra));
            }
            "userguc" if !obs.extra.split(',').any(|item| item == "work_mem") => {
                failures.push(format!("user source missing work_mem [{}]", obs.extra));
            }
            "dbuser" if !obs.extra.split(',').any(|item| item == "temp_buffers") => {
                failures.push(format!("database-user source missing temp_buffers [{}]", obs.extra));
            }
            "db_work_mem" | "plain_db" if !obs.extra.split(',').any(|item| item == "work_mem") => {
                failures.push(format!("{label} database source missing work_mem [{}]", obs.extra));
            }
            "self" if obs.extra.split(',').all(|item| item != "role") || obs.session_user != obs.current_user => {
                failures.push("self SET role did not stay aligned with a role startup finding".to_string());
            }
            "case" if obs.current_user != "Oo_A097_Case" || obs.session_user == obs.current_user => {
                failures.push(format!("case role collapsed to {}", obs.current_user));
            }
            "guc" if obs.current_user != "oo_src_side" => {
                failures.push(format!("guc current_user {}", obs.current_user));
            }
            "setnone" if obs.role_setting != "none" => {
                failures.push(format!("role none setting is {}", obs.role_setting));
            }
            _ => {}
        }
        if obs.session_user != obs.current_user {
            catalog_stops(label, &mut session, "session_user_mismatch", &mut failures).await;
        } else if !obs.extra.is_empty() {
            catalog_stops(label, &mut session, "startup_options", &mut failures).await;
        }
        drop(session);
    }

    let guc_pass = secret(&pointer.password_dir, "guc.pass");
    let mut guc_again = open(
        &pointer,
        &ca,
        &pointer.guc_user,
        guc_pass.as_str(),
        &pointer.database,
        "a097_guc_fresh",
    )
    .await
    .expect("fresh guc");
    let again = guc_again.inspect_access().await.expect("fresh guc inspect");
    note("guc_fresh_blocks", &blocks_of(&again));
    note("guc_fresh_tls", &again.tls.negotiated_version);
    if again.registration_allowed
        || !has_block(&again, "role_mismatch", "session_user")
        || !has_block(&again, "startup_option", "startup_options")
        || again.tls.negotiated_version != "unknown"
    {
        failures.push("fresh guc connection was not still blocked as unknown".to_string());
    }
    drop(guc_again);
    let mut reader_again = open(
        &pointer,
        &ca,
        &pointer.reader_user,
        reader_pass.as_str(),
        &pointer.database,
        "a097_reader_fresh",
    )
    .await
    .expect("fresh reader");
    let reader_report = reader_again.inspect_access().await.expect("fresh reader inspect");
    note("reader_fresh_allowed", &reader_report.registration_allowed.to_string());
    note("reader_fresh_tls", &reader_report.tls.negotiated_version);
    if !reader_report.registration_allowed
        || !reader_report.tls.negotiated_version.starts_with("TLSv1.")
        || reader_report.write.state != "read_only"
    {
        failures.push(format!(
            "fresh reader allowed={} write={} tls={}",
            reader_report.registration_allowed, reader_report.write.state, reader_report.tls.negotiated_version
        ));
    }
    let catalog = reader_again.catalog().await.expect("empty catalog");
    note("empty_catalog_hash", &catalog.schema_hash);
    note("empty_catalog_alg", &catalog.hash_algorithm);
    if catalog.schema_hash != EMPTY_SCHEMA_HASH || catalog.hash_algorithm != "pg-catalog-hash/v1" {
        failures.push("empty catalog hash changed".to_string());
    }
    drop(reader_again);

    let wrong_ca = fs::read(&pointer.wrong_ca_file).expect("wrong ca");
    settle().await;
    let before_ca = source_exec_count();
    match open(
        &pointer,
        &wrong_ca,
        &pointer.reader_user,
        reader_pass.as_str(),
        &pointer.database,
        "a097_wrong_ca",
    )
    .await
    {
        Ok(_) => failures.push("wrong ca connected".to_string()),
        Err(err) => {
            note("wrong_ca_kind", &format!("{:?}", err.kind()));
            note("wrong_ca_fact", err.fact().unwrap_or(""));
            if err.kind() != SourceErrorKind::Tls || err.fact() != Some("tls_handshake") {
                failures.push(format!("wrong ca {:?} {:?}", err.kind(), err.fact()));
            }
        }
    }
    settle().await;
    let after_ca = source_exec_count();
    note("wrong_ca_delta", &(after_ca - before_ca).to_string());
    if after_ca != before_ca {
        failures.push("wrong ca issued connector sql".to_string());
    }

    settle().await;
    let before_host = source_exec_count();
    let host_config = PgSourceConfig::try_new(
        SourceId::new("a097_wrong_host").unwrap(),
        PgEndpoint::new(&pointer.wrong_host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(&pointer.reader_user, reader_pass.as_str()).unwrap(),
        SecretRef::new("secret://a097/review").unwrap(),
        PgTlsConfig::from_ca_pem(&ca).unwrap(),
        budget(),
        empty_selection(),
    )
    .unwrap();
    match PgConnector::new(host_config).connect().await {
        Ok(_) => failures.push("wrong host connected".to_string()),
        Err(err) => {
            note("wrong_host_kind", &format!("{:?}", err.kind()));
            note("wrong_host_fact", err.fact().unwrap_or(""));
            if err.kind() != SourceErrorKind::Tls || err.fact() != Some("tls_handshake") {
                failures.push(format!("wrong host {:?} {:?}", err.kind(), err.fact()));
            }
        }
    }
    settle().await;
    let after_host = source_exec_count();
    note("wrong_host_delta", &(after_host - before_host).to_string());
    if after_host != before_host {
        failures.push("wrong host issued connector sql".to_string());
    }

    let plain_pass = secret(&pointer.password_dir, "a097-plain.pass");
    let disabled = sqlx::postgres::PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username("oo_a097_plain")
        .password(plain_pass.as_str())
        .database(&pointer.database)
        .application_name("oo-a097-observe")
        .ssl_mode(sqlx::postgres::PgSslMode::Disable);
    match tokio::time::timeout(Duration::from_secs(15), disabled.connect()).await {
        Ok(Ok(mut conn)) => {
            let ssl = sqlx::query_scalar::<_, Option<bool>>(
                "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()",
            )
            .fetch_optional(&mut conn)
            .await;
            let shown = match ssl {
                Ok(Some(Some(true))) => "true",
                Ok(Some(Some(false))) => "false",
                Ok(Some(None)) | Ok(None) => "null",
                Err(_) => "query-failed",
            };
            note("plaintext_fixture", "accepted");
            note("plaintext_ssl", shown);
        }
        Ok(Err(_)) => note("plaintext_fixture", "refused"),
        Err(_) => note("plaintext_fixture", "timeout"),
    }

    check_regressions(&pointer, &ca, &mut failures).await;

    if !failures.is_empty() {
        note("failures", &failures.len().to_string());
        for (index, item) in failures.iter().enumerate() {
            note(&format!("failure_{index}"), item);
        }
        panic!("{}", failures.join(" | "));
    }
    note("oracle", "pass");
}

async fn check_regressions(pointer: &Pointer, ca: &[u8], failures: &mut Vec<String>) {
    let specs = [
        ("side", pointer.side_user.as_str(), "side.pass"),
        ("inherit", pointer.inherit_user.as_str(), "inherit.pass"),
        ("setonly", pointer.setonly_user.as_str(), "setonly.pass"),
        ("noset", pointer.noset_user.as_str(), "noset.pass"),
        ("definer", pointer.definer_user.as_str(), "definer.pass"),
        ("super", pointer.super_user.as_str(), "login-super.pass"),
        ("bypass", pointer.bypass_user.as_str(), "bypass.pass"),
        ("createrole", pointer.createrole_user.as_str(), "createrole.pass"),
        ("createdb", pointer.createdb_user.as_str(), "createdb.pass"),
        ("oponly", pointer.oponly_user.as_str(), "oponly.pass"),
        ("castonly", pointer.castonly_user.as_str(), "castonly.pass"),
    ];
    for (label, user, file) in specs {
        let pass = secret(&pointer.password_dir, file);
        let mut session = match open(
            pointer,
            ca,
            user,
            pass.as_str(),
            &pointer.database,
            &format!("a097_{label}"),
        )
        .await
        {
            Ok(session) => session,
            Err(err) => {
                failures.push(format!("{label} connect {:?} {:?}", err.kind(), err.fact()));
                continue;
            }
        };
        let report = match session.inspect_access().await {
            Ok(report) => report,
            Err(err) => {
                failures.push(format!("{label} inspect {:?} {:?}", err.kind(), err.fact()));
                continue;
            }
        };
        note(&format!("regr_{label}_allowed"), &report.registration_allowed.to_string());
        note(&format!("regr_{label}_write"), &report.write.state);
        note(&format!("regr_{label}_blocks"), &blocks_of(&report));
        match label {
            "noset" => {
                if !report.registration_allowed || report.write.state != "read_only" {
                    failures.push(format!("noset allowed={} write={}", report.registration_allowed, report.write.state));
                }
            }
            "side" => {
                if report.registration_allowed
                    || report.write.state == "read_only"
                    || !report.write.findings.iter().any(|item| item.path == "direct")
                {
                    failures.push("side direct write was not reported".to_string());
                }
            }
            "setonly" => {
                if report.registration_allowed
                    || !report.write.findings.iter().any(|item| item.path == "set_role")
                    || !report.memberships.iter().any(|item| item.set_option && !item.inherit_option)
                {
                    failures.push("setonly SET path was not reported".to_string());
                }
            }
            "inherit" => {
                if report.registration_allowed
                    || !report.write.findings.iter().any(|item| item.path == "inherited" || item.path == "set_role")
                {
                    failures.push("inherit path was not reported".to_string());
                }
            }
            "definer" => {
                if report.registration_allowed
                    || !report.block_reasons.iter().any(|item| item.code == "security_definer")
                    || !report.security_definers.iter().any(|item| item.name == "public_definer")
                {
                    failures.push("definer was not reported".to_string());
                }
            }
            "super" | "bypass" | "createrole" | "createdb" => {
                if report.registration_allowed
                    || !report.block_reasons.iter().any(|item| item.code == "privileged_role")
                    || report.write.state == "read_only"
                {
                    failures.push(format!("{label} privileged role was not blocked"));
                }
            }
            "oponly" => {
                if report.registration_allowed
                    || !report.security_definers.iter().any(|item| item.name == "boom_op")
                {
                    failures.push("operator definer was not reported".to_string());
                }
            }
            "castonly" => {
                if report.registration_allowed
                    || !report.security_definers.iter().any(|item| item.name == "boom_cast")
                {
                    failures.push("cast definer was not reported".to_string());
                }
            }
            _ => {}
        }
        drop(session);
    }
}

#[test]
fn serde_contract_and_callers() {
    assert_eq!(oo_connectors::PROTOCOL, "oo.source.pgsql/v1");
    assert_eq!(oo_connectors::schema_hash(&[]), EMPTY_SCHEMA_HASH);
    assert_eq!(
        EMPTY_SCHEMA_HASH,
        "cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc"
    );
    let old = r#"{"mode":"verify-full","trust":"caller-ca-only","hostname_verification":true,"negotiated_version":"TLSv1.3"}"#;
    assert!(serde_json::from_str::<TlsReport>(old).is_err());
    let new = r#"{"mode":"verify-full","trust":"caller-ca-only","hostname_verification":true,"negotiated_version":"unknown","server_observation":"unknown"}"#;
    let parsed: TlsReport = serde_json::from_str(new).unwrap();
    assert_eq!(parsed.server_observation, "unknown");
    assert_eq!(parsed.negotiated_version, "unknown");
    assert!(!parsed.negotiated_version.starts_with("TLSv1."));
    assert_eq!(parsed.mode, "verify-full");
    let again: TlsReport = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
    assert_eq!(again, parsed);

    #[derive(Deserialize)]
    struct OldTls {
        mode: String,
        trust: String,
        hostname_verification: bool,
        negotiated_version: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OldTlsStrict {
        #[allow(dead_code)]
        mode: String,
        #[allow(dead_code)]
        trust: String,
        #[allow(dead_code)]
        hostname_verification: bool,
        #[allow(dead_code)]
        negotiated_version: String,
    }
    let old_reader: OldTls = serde_json::from_str(new).unwrap();
    assert_eq!(old_reader.negotiated_version, "unknown");
    assert_eq!(old_reader.mode, "verify-full");
    assert!(old_reader.hostname_verification);
    assert_eq!(old_reader.trust, "caller-ca-only");
    assert!(serde_json::from_str::<OldTlsStrict>(new).is_err());
    assert!(SourceId::new("Bad").is_err());
    assert!(PgCredentials::new("oo_src_reader", "").is_err());
    assert!(PgEndpoint::new("/tmp/.s.PGSQL.5432", 5432, "source_lab").is_err());
    assert!(PgEndpoint::new("oo-source-pg", 0, "source_lab").is_err());
    note("serde_old_rejected", "1");
    note("serde_new_roundtrip", "1");
    note("serde_default_reader_ignores_new_field", "1");
    note("serde_deny_unknown_rejects_new_field", "1");
    note("protocol", oo_connectors::PROTOCOL);
}
