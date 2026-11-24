//! Read-only diagnostics. A failed source never erases the other probe reports.
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use oo_connectors::engine::{EngineFamily, TableRef};
use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine};
use oo_connectors::postgres::PostgresEngine;
use oo_connectors::postgres::opengauss::OpenGaussEngine;
use oo_connectors::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
};
use oo_stack::{Stack, secret::SecretStore, stack::Source};
use serde_json::{Value, json};

use super::{Failure, Invocation};
#[path = "inspect.rs"]
mod inspection;
use inspection::inspect;

#[derive(Clone, Copy)]
enum Adapter {
    Postgres,
    Oracle,
    Mysql,
    Sqlserver,
    Db2,
    Dm8,
    Opengauss,
}

/// Families the feature-isolated `oo-mysql-doctor` helper probes: the MySQL
/// wire family and the analytic engines on the same wire (no second driver).
fn helper_family(family: EngineFamily) -> bool {
    matches!(
        family,
        EngineFamily::Mysql
            | EngineFamily::Mariadb
            | EngineFamily::Tidb
            | EngineFamily::Oceanbase
            | EngineFamily::Clickhouse
            | EngineFamily::Doris
            | EngineFamily::Starrocks
    )
}

fn registry() -> BTreeMap<EngineFamily, Adapter> {
    BTreeMap::from([
        (EngineFamily::Postgres, Adapter::Postgres),
        (EngineFamily::Kingbase, Adapter::Postgres),
        (EngineFamily::Mysql, Adapter::Mysql),
        (EngineFamily::Mariadb, Adapter::Mysql),
        (EngineFamily::Tidb, Adapter::Mysql),
        (EngineFamily::Oceanbase, Adapter::Mysql),
        (EngineFamily::Clickhouse, Adapter::Mysql),
        (EngineFamily::Doris, Adapter::Mysql),
        (EngineFamily::Starrocks, Adapter::Mysql),
        (EngineFamily::Opengauss, Adapter::Opengauss),
        (EngineFamily::Oracle, Adapter::Oracle),
        (EngineFamily::Sqlserver, Adapter::Sqlserver),
        (EngineFamily::Db2, Adapter::Db2),
        (EngineFamily::Dameng, Adapter::Dm8),
    ])
}

fn mysql(source: &Source, tables: &[TableRef]) -> Value {
    let result = (|| {
        // Resolve only beside this installation, never from cwd or PATH. A
        // clean install has none (D16): use the pinned runtime image instead.
        let helper = std::env::current_exe()
            .map_err(|_| "MYSQL_DOCTOR_INSTALL")?
            .with_file_name("oo-mysql-doctor");
        let mut source = json!(source);
        let mut command = if helper.exists() {
            Command::new(helper)
        } else {
            runtime_mysql_doctor(&mut source)?
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "MYSQL_DOCTOR_INSTALL")?;
        let body = json!({"source":source,"tables":tables}).to_string();
        if child
            .stdin
            .take()
            .ok_or("MYSQL_DOCTOR_STDIN")?
            .write_all(body.as_bytes())
            .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("MYSQL_DOCTOR_STDIN");
        }
        let output = child.wait_with_output().map_err(|_| "MYSQL_DOCTOR_WAIT")?;
        if !output.status.success() {
            return Err("MYSQL_DOCTOR_FAILED");
        }
        let report: Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "MYSQL_DOCTOR_PROTOCOL")?;
        if !matches!(report["status"].as_str(), Some("ok" | "blocked")) {
            return Err("MYSQL_DOCTOR_PROTOCOL");
        }
        Ok(report)
    })();
    result.unwrap_or_else(|error| json!({"status":"blocked","error":error}))
}

/// Absolute `file:` secret paths named by a source: the helper in the image sees only the
/// owner's secret store, so each of these files is bound read-only at the same path.
fn file_secret_paths(source: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let mut refs = vec![&source["password"], &source["tls_ca"]];
    if source["ssh"].is_object() {
        refs.extend([&source["ssh"]["key"], &source["ssh"]["password"]]);
    }
    for reference in refs {
        if let Some(path) = reference["secret_ref"]
            .as_str()
            .and_then(|r| r.strip_prefix("file:"))
            && path.starts_with('/')
            && !path.contains([',', ':', '\n'])
            && !found.iter().any(|f| f == path)
        {
            found.push(path.to_owned());
        }
    }
    found
}

/// One-shot `oo-mysql-doctor` from the pinned lite runtime image. It sees only
/// the owner's secret store, read-only; a loopback source is this host.
fn runtime_mysql_doctor(source: &mut Value) -> Result<Command, &'static str> {
    let image = super::lite::runtime_image();
    if !super::lite::digest(&image) {
        return Err("MYSQL_DOCTOR_INSTALL");
    }
    let home = std::env::var_os("HOME").ok_or("SECRET_HOME")?;
    let store = std::path::Path::new(&home).join(".config/open-ontology/secrets");
    let store = std::fs::canonicalize(store).map_err(|_| "SECRET_HOME")?;
    let user = super::lite::user(&store).map_err(|_| "SECRET_HOME")?;
    if source["host"]
        .as_str()
        .is_some_and(|h| matches!(h, "localhost" | "127.0.0.1" | "::1"))
    {
        source["host"] = json!("host.docker.internal");
    }
    let mut command = Command::new("docker");
    command
        .args([
            "run",
            "--rm",
            "--interactive",
            "--read-only",
            "--tmpfs",
            "/tmp",
            "--cap-drop",
            "ALL",
        ])
        .args(["--security-opt", "no-new-privileges:true", "--user", &user])
        .arg("--label")
        .arg(format!(
            "oo.lane={}",
            super::lite::lane(std::path::Path::new("/nonexistent"))
        ))
        .args(["--env", "HOME=/home/oo"]);
    for path in file_secret_paths(source) {
        command.arg("--volume").arg(format!("{path}:{path}:ro"));
    }
    command
        .arg("--volume")
        .arg(format!(
            "{}:/home/oo/.config/open-ontology/secrets:ro",
            store.to_str().ok_or("SECRET_HOME")?
        ))
        .args(["--entrypoint", "oo-mysql-doctor", &image]);
    Ok(command)
}

fn db2(
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
) -> Result<oo_connectors::db2::Db2Engine, String> {
    if source.ssh.is_some() {
        return Err("SSH_EXTERNAL_WORKER_NOT_AVAILABLE".into());
    }
    // Operator-controlled executable, like oo-load --db2-worker. Its wrapper
    // owns the pinned image and the source-only network/CA mount.
    let executable = std::env::var_os("OO_DB2_WORKER").ok_or("DB2_WORKER_REQUIRED")?;
    let mut config = oo_connectors::db2::ConnectConfig::new(
        &source.host,
        source.port,
        &source.database,
        &source.user,
        store.read(&source.password.secret_ref)?.into(),
    );
    config.scope = tables.to_vec();
    if source.tls_ca.is_some() {
        config.tls_ca = Some("/run/oo-source/ca.pem".into());
    }
    oo_connectors::db2::Db2Engine::connect(&mut Command::new(executable), &config)
        .map_err(|e| e.to_string())
}

fn dm8(
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
) -> Result<oo_connectors::dm8::Dm8Engine, String> {
    if source.ssh.is_some() {
        return Err("SSH_EXTERNAL_WORKER_NOT_AVAILABLE".into());
    }
    // The DM worker has no TLS yet; never silently drop a trust anchor.
    if source.tls_ca.is_some() {
        return Err("DM8_TLS_NOT_AVAILABLE".into());
    }
    // Operator-controlled executable, like oo-load --dm8-worker. Its wrapper
    // owns the pinned image and the source-only network namespace.
    let executable = std::env::var_os("OO_DM8_WORKER").ok_or("DM8_WORKER_REQUIRED")?;
    let mut config = oo_connectors::dm8::ConnectConfig::new(
        &source.host,
        source.port,
        &source.user,
        store.read(&source.password.secret_ref)?.into(),
    );
    config.scope = tables.to_vec();
    oo_connectors::dm8::Dm8Engine::connect(&mut Command::new(executable), &config)
        .map_err(|e| e.to_string())
}

pub(super) fn oracle(
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
) -> Result<OracleEngine, String> {
    // The current Oracle transport does not implement the stack TLS contract.
    // Never silently discard an operator-specified trust anchor.
    if source.tls_ca.is_some() {
        return Err("ORACLE_TLS_NOT_AVAILABLE".into());
    }
    let mut config = OracleConnectConfig::new(
        &source.host,
        source.port,
        &source.database,
        &source.user,
        store.read(&source.password.secret_ref)?.into(),
    );
    config.scope = tables.to_vec();
    let engine = match std::env::var("OO_ORACLE_WORKER_IMAGE") {
        Ok(image) => {
            if source.ssh.is_some() {
                return Err("SSH_EXTERNAL_WORKER_NOT_AVAILABLE".into());
            }
            if !pinned_worker(&image) {
                return Err("ORACLE_WORKER_DIGEST_REQUIRED".into());
            }
            OracleEngine::connect_worker(
                config,
                Command::new("docker").args([
                    "run",
                    "--rm",
                    "-i",
                    "--platform",
                    "linux/amd64",
                    "--label",
                    &format!(
                        "oo.lane={}",
                        super::lite::lane(std::path::Path::new("/nonexistent"))
                    ),
                    "--cap-drop",
                    "ALL",
                    "--security-opt",
                    "no-new-privileges",
                    &image,
                ]),
            )
        }
        Err(std::env::VarError::NotPresent) => OracleEngine::connect(config),
        Err(_) => return Err("ORACLE_WORKER_DIGEST_REQUIRED".into()),
    };
    engine.map_err(|e| e.to_string())
}

fn pinned_worker(image: &str) -> bool {
    image
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(super) fn family(source: &Source) -> Result<EngineFamily, &'static str> {
    let name = source.engine.split('-').next().unwrap_or("");
    match name {
        "postgres" | "postgresql" => Ok(EngineFamily::Postgres),
        // PolarDB MySQL is a deployment variant (`mysql-polardb`), not a new family.
        "mysql" | "polardb" => Ok(EngineFamily::Mysql),
        "mariadb" => Ok(EngineFamily::Mariadb),
        "tidb" => Ok(EngineFamily::Tidb),
        "oceanbase" => Ok(EngineFamily::Oceanbase),
        "opengauss" => Ok(EngineFamily::Opengauss),
        "clickhouse" => Ok(EngineFamily::Clickhouse),
        "doris" => Ok(EngineFamily::Doris),
        "starrocks" => Ok(EngineFamily::Starrocks),
        "mongodb" => Ok(EngineFamily::Mongodb),
        "opensearch" => Ok(EngineFamily::Opensearch),
        "oracle" => Ok(EngineFamily::Oracle),
        "sqlserver" => Ok(EngineFamily::Sqlserver),
        "db2" => Ok(EngineFamily::Db2),
        "kingbase" => Ok(EngineFamily::Kingbase),
        "dm8" | "dameng" => Ok(EngineFamily::Dameng),
        // Port selects a candidate only. The adapter must verify server identity.
        "auto" => match source.port {
            5432 => Ok(EngineFamily::Postgres),
            3306 => Ok(EngineFamily::Mysql),
            1521 => Ok(EngineFamily::Oracle),
            _ => Err("ENGINE_REQUIRED_FOR_NONSTANDARD_PORT"),
        },
        _ => Err("ENGINE_NOT_AVAILABLE"),
    }
}

pub(super) fn tables(source: &Source) -> Result<Vec<TableRef>, &'static str> {
    if source.scope.exclude.iter().any(|s| {
        s.contains('?')
            || (s.contains('*') && s != "*" && !(s.ends_with(".*") && s.matches('*').count() == 1))
    }) {
        return Err("SCOPE_PATTERN_NOT_AVAILABLE");
    }
    let mut out = Vec::new();
    for name in &source.scope.include {
        if name.contains(['*', '?']) {
            return Err("SCOPE_DISCOVERY_NOT_AVAILABLE: use explicit schema.table entries");
        }
        let (schema, table) = name
            .split_once('.')
            .filter(|(s, t)| !s.is_empty() && !t.is_empty())
            .ok_or("SCOPE_REQUIRES_QUALIFIED_TABLE")?;
        if source.scope.exclude.iter().any(|excluded| {
            excluded == "*"
                || excluded == name
                || excluded == schema
                || excluded == &format!("{schema}.*")
        }) {
            continue;
        }
        let table = TableRef {
            schema: schema.into(),
            name: table.into(),
        };
        if !out.contains(&table) {
            out.push(table);
        }
    }
    Ok(out)
}

pub(super) fn postgres(
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
    tunnel: Option<std::sync::Arc<oo_connectors::transport::ssh::Tunnel>>,
) -> Result<PostgresEngine, String> {
    let config = pg_config(source, tables, store, tunnel)?;
    Ok(if family(source) == Ok(EngineFamily::Kingbase) {
        PostgresEngine::new_kingbase(config)
    } else {
        PostgresEngine::new(config)
    })
}

fn pg_config(
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
    tunnel: Option<std::sync::Arc<oo_connectors::transport::ssh::Tunnel>>,
) -> Result<PgSourceConfig, String> {
    let ca = source.tls_ca.as_ref().ok_or("SOURCE_TLS_CA_REQUIRED")?;
    let password = store.read(&source.password.secret_ref)?;
    let ca = store.read(&ca.secret_ref)?;
    let config = (|| {
        PgSourceConfig::try_new(
            SourceId::new(&source.id)?,
            PgEndpoint::new(&source.host, source.port, &source.database)?,
            PgCredentials::new(source.user.clone(), password)?,
            SecretRef::new("secret://source-password")?,
            PgTlsConfig::from_ca_pem(ca)?,
            PgBudget::new(
                Duration::from_secs(10),
                Duration::from_secs(30),
                Duration::from_secs(1),
                100_000,
                32 * 1024 * 1024,
            )?,
            TableSelection::new(
                tables
                    .iter()
                    .map(|t| SelectedTable::new(&t.schema, &t.name))
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
        )
    })()
    .map_err(|e: oo_connectors::SourceError| e.to_string())?;
    match tunnel {
        Some(tunnel) => config.with_tunnel(tunnel).map_err(|e| e.to_string()),
        None => Ok(config),
    }
}

async fn sources(stack: &Stack) -> Vec<Value> {
    let registry = registry();
    let mut results = Vec::new();
    for source in &stack.sources {
        let setup = (|| {
            let family = family(source)?;
            match registry.get(&family) {
                Some(
                    Adapter::Postgres
                    | Adapter::Oracle
                    | Adapter::Mysql
                    | Adapter::Sqlserver
                    | Adapter::Db2
                    | Adapter::Dm8
                    | Adapter::Opengauss,
                ) => (),
                // Document sources need a driver the lite helper does not link yet.
                None if matches!(family, EngineFamily::Mongodb | EngineFamily::Opensearch) => {
                    return Err("DOCUMENT_SOURCE_DOCTOR_NOT_AVAILABLE: connect and import run in the pipeline; the read-only probe needs the document driver in the helper".into());
                }
                _ => return Err("engine not available in this build".into()),
            }
            let selection = tables(source);
            let selected = selection.as_ref().cloned().unwrap_or_default();
            let store = SecretStore::from_home()?;
            Ok::<_, String>((family, store, selected, selection.err()))
        })();
        let mut result = match setup {
            Ok((family, store, selected, scope_error)) => {
                // MySQL's helper owns its runtime and tunnel. Never block this
                // runtime waiting for a child that needs its forwarding tasks.
                let tunnel = if helper_family(family) {
                    Ok(None)
                } else {
                    oo_connectors::transport::ssh::connect(source, &store).await
                };
                let mut report = match tunnel {
                    Err(error) => json!({"status":"blocked","error":error,
                        "tunnel":{"status":"blocked","error":error}}),
                    Ok(tunnel) => {
                        let mut routed = source.clone();
                        if let Some(tunnel) = &tunnel
                            && family == EngineFamily::Oracle
                        {
                            routed.host = tunnel.address().ip().to_string();
                            routed.port = tunnel.address().port();
                        }
                        let mut report = match family {
                            EngineFamily::Postgres | EngineFamily::Kingbase => {
                                match postgres(source, &selected, &store, tunnel.clone()) {
                                    Ok(mut engine) => {
                                        inspect(&mut engine, &selected, &source.sync.mode).await
                                    }
                                    Err(error) => json!({"status":"blocked","error":error}),
                                }
                            }
                            EngineFamily::Oracle => match oracle(&routed, &selected, &store) {
                                Ok(mut engine) => {
                                    inspect(&mut engine, &selected, &source.sync.mode).await
                                }
                                Err(error) => json!({"status":"blocked","error":error}),
                            },
                            EngineFamily::Db2 => match db2(source, &selected, &store) {
                                Ok(mut engine) => {
                                    inspect(&mut engine, &selected, &source.sync.mode).await
                                }
                                Err(error) => json!({"status":"blocked","error":error}),
                            },
                            EngineFamily::Dameng => match dm8(source, &selected, &store) {
                                Ok(mut engine) => {
                                    inspect(&mut engine, &selected, &source.sync.mode).await
                                }
                                Err(error) => json!({"status":"blocked","error":error}),
                            },
                            f if helper_family(f) => mysql(source, &selected),
                            EngineFamily::Opengauss => {
                                match pg_config(source, &selected, &store, tunnel.clone())
                                    .map(OpenGaussEngine::new)
                                {
                                    Ok(mut engine) => {
                                        inspect(&mut engine, &selected, &source.sync.mode).await
                                    }
                                    Err(error) => json!({"status":"blocked","error":error}),
                                }
                            }
                            EngineFamily::Sqlserver => {
                                let config = (|| {
                                    let ca =
                                        source.tls_ca.as_ref().ok_or("SQLSERVER_TLS_REQUIRED")?;
                                    Ok::<_, String>(oo_connectors::sqlserver::ConnectConfig {
                                        host: source.host.clone(),
                                        port: source.port,
                                        database: source.database.clone(),
                                        user: source.user.clone(),
                                        password: store.read(&source.password.secret_ref)?.into(),
                                        ca_pem: store.read(&ca.secret_ref)?.into_bytes(),
                                        tables: selected.clone(),
                                        mode: None,
                                    })
                                })();
                                match config {
                                    Ok(config) => {
                                        match oo_connectors::sqlserver::SqlServerEngine::connect(
                                            config,
                                            oo_connectors::engine::ReadBudget::default(),
                                        )
                                        .await
                                        {
                                            Ok(mut engine) => {
                                                inspect(&mut engine, &selected, &source.sync.mode)
                                                    .await
                                            }
                                            Err(error) => {
                                                json!({"status":"blocked","error":error.to_string()})
                                            }
                                        }
                                    }
                                    Err(error) => json!({"status":"blocked","error":error}),
                                }
                            }
                            _ => unreachable!("registry checked above"),
                        };
                        if let Some(ssh) = &source.ssh
                            && !helper_family(family)
                        {
                            report["tunnel"] = json!({"status":"ok","host_key_sha256":ssh.host_key_sha256,
                            "transport":"ssh-direct-tcpip"});
                        }
                        report
                    }
                };
                if let Some(error) = scope_error {
                    report["status"] = json!("blocked");
                    report["scope_error"] = json!(error);
                }
                report
            }
            Err(error) => json!({"status":"blocked","error":error}),
        };
        result["source_id"] = json!(source.id);
        results.push(result);
    }
    results
}

fn model(stack: &Stack) -> Value {
    // Embed the reviewed egress implementation, not a script resolved from cwd/PATH.
    // Config is piped; argv never contains credentials or operator configuration.
    let result = (|| {
        let mut child = Command::new("python3")
            .args([
                "-c",
                include_str!("../../../../tools/model-egress/model_egress.py"),
                "--config",
                "-",
                "--probe",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "MODEL_EGRESS_START")?;
        let written = child
            .stdin
            .take()
            .ok_or("MODEL_EGRESS_STDIN")
            .and_then(|mut pipe| {
                let bytes = serde_json::to_vec(stack).map_err(|_| "MODEL_CONFIG")?;
                pipe.write_all(&bytes).map_err(|_| "MODEL_EGRESS_STDIN")
            });
        if written.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("MODEL_EGRESS_STDIN");
        }
        let output = child.wait_with_output().map_err(|_| "MODEL_EGRESS_WAIT")?;
        let report: Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "MODEL_EGRESS_PROTOCOL")?;
        if !output.status.success() || report.get("error").is_some() {
            return Ok(json!({"status":"blocked","report":report}));
        }
        Ok::<_, &'static str>(json!({"status":"ok","report":report}))
    })();
    result.unwrap_or_else(|error| json!({"status":"blocked","error":error}))
}

/// What a DBA must grant before any build, from the probe each source report already
/// carries. Only the capabilities the source's sync mode needs count (the snapshot
/// path always; log CDC for C1, range digests for C5); the rest stays advice in
/// `dba_requirements`. Builds that skip this fail later, at load.
fn required_grants(stack: &Stack, sources: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for (source, report) in stack.sources.iter().zip(sources) {
        let mut needed = vec!["catalog", "chunked_snapshot"];
        match source.sync.mode.as_str() {
            "C1" => needed.push("log_cdc"),
            "C5" => needed.push("digest_diff"),
            _ => (),
        }
        let mut seen = std::collections::BTreeSet::new();
        for capability in needed {
            let state = &report["probe"]["capabilities"][capability];
            if state["state"] != "needs_setup" {
                continue;
            }
            for need in state["requirements"].as_array().into_iter().flatten() {
                let name = need["name"].as_str().unwrap_or_default();
                if seen.insert(name.to_owned()) {
                    out.push(json!({"source_id":source.id,"capability":capability,
                        "requirement":name,"observed":need["observed"],"required":need["required"],
                        "dba_statement":need["dba_statement"]}));
                }
            }
        }
    }
    out
}

fn grants_text(grants: &[Value]) -> String {
    let mut text = String::from(
        "A DBA must grant this before oo build (run as an administrator, then re-run oo doctor):\n",
    );
    for grant in grants {
        let line = match grant["dba_statement"].as_str() {
            Some(statement) => statement.to_owned(),
            None => format!(
                "-- no automatic statement: {} ({})",
                grant["requirement"].as_str().unwrap_or(""),
                grant["required"].as_str().unwrap_or("")
            ),
        };
        text.push_str(&format!(
            "  {line}   -- source {}\n",
            grant["source_id"].as_str().unwrap_or("")
        ));
    }
    text
}

pub(super) fn run(stack: &Stack, json_output: bool) -> Result<Invocation, Failure> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Failure::local("DOCTOR_RUNTIME"))?;
    let sources = runtime.block_on(sources(stack));
    let model = model(stack);
    let grants = required_grants(stack, &sources);
    #[allow(unused_mut)]
    let mut blocked = sources.iter().any(|s| s["status"] != "ok")
        || model["status"] != "ok"
        || !grants.is_empty();
    #[allow(unused_mut)]
    let mut report = json!({"protocol":"oo.doctor/v1", "read_only":true,
        "status": if blocked {"blocked"} else {"ok"}, "sources":sources, "model":model});
    if !grants.is_empty() {
        report["required_grants"] = json!(grants);
    }
    // Super Drive: an added field, only in builds with the docs feature.
    #[cfg(feature = "docs")]
    {
        let docs = runtime.block_on(super::doctor_docs::section());
        if docs["status"] == "blocked" {
            blocked = true;
            report["status"] = json!("blocked");
        }
        report["docs"] = docs;
    }
    let output = if json_output {
        report.to_string()
    } else {
        format!(
            "Read-only preflight (DBA statements are advice, never executed).\n{}{}",
            if grants.is_empty() {
                String::new()
            } else {
                grants_text(&grants)
            },
            serde_json::to_string_pretty(&report).map_err(|_| Failure::local("DOCTOR_REPORT"))?
        )
    };
    Ok(Invocation::AdminReport(output, if blocked { 1 } else { 0 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        oo_stack::parse(include_str!("../../../oo-stack/example.stack.json"))
            .unwrap()
            .sources
            .remove(0)
    }

    #[test]
    fn registry_does_not_misidentify_compatible_families() {
        let mut s = source();
        s.engine = "postgres-17".into();
        assert_eq!(family(&s).unwrap(), EngineFamily::Postgres);
        // Compatible products keep their own family: never aliased to Postgres.
        s.engine = "opengauss".into();
        assert_eq!(family(&s).unwrap(), EngineFamily::Opengauss);
        for (engine, expected) in [
            ("mariadb", EngineFamily::Mariadb),
            ("tidb", EngineFamily::Tidb),
            ("oceanbase", EngineFamily::Oceanbase),
            ("mysql-polardb", EngineFamily::Mysql),
            ("clickhouse", EngineFamily::Clickhouse),
            ("doris", EngineFamily::Doris),
            ("starrocks", EngineFamily::Starrocks),
            ("mongodb", EngineFamily::Mongodb),
            ("opensearch", EngineFamily::Opensearch),
        ] {
            s.engine = engine.into();
            assert_eq!(family(&s).unwrap(), expected, "{engine}");
        }
        s.engine = "kingbase".into();
        assert_eq!(family(&s).unwrap(), EngineFamily::Kingbase);
        assert!(matches!(
            registry()[&EngineFamily::Kingbase],
            Adapter::Postgres
        ));
        assert!(matches!(registry()[&EngineFamily::Mysql], Adapter::Mysql));
        assert!(matches!(registry()[&EngineFamily::Oracle], Adapter::Oracle));
        s.engine = "dm8".into();
        assert_eq!(family(&s).unwrap(), EngineFamily::Dameng);
        assert!(matches!(registry()[&EngineFamily::Dameng], Adapter::Dm8));
    }

    #[test]
    fn helper_container_gets_exactly_the_file_secrets_the_source_names() {
        let source = json!({
            "password":{"secret_ref":"file:/private/tmp/a/pw"},
            "tls_ca":{"secret_ref":"file:/private/tmp/a/ca"},
            "ssh":{"key":{"secret_ref":"file:/private/tmp/a/pw"},"password":{"secret_ref":"named-secret"}},
        });
        assert_eq!(
            file_secret_paths(&source),
            vec!["/private/tmp/a/pw", "/private/tmp/a/ca"]
        );
        for refused in ["file:relative/x", "file:/a,b/x", "env:X", "name"] {
            let one = json!({"password":{"secret_ref":refused},"tls_ca":null});
            assert!(file_secret_paths(&one).is_empty(), "{refused}");
        }
    }

    #[test]
    fn worker_requires_content_id_not_tag_or_command() {
        assert!(pinned_worker(&format!("sha256:{}", "a".repeat(64))));
        for image in ["latest", "sha256:abc", "sh -c anything", "repo:tag"] {
            assert!(!pinned_worker(image));
        }
    }

    #[test]
    fn scope_is_explicit_and_exclusions_win() {
        let mut s = source();
        assert!(tables(&s).is_err());
        s.scope.include = vec!["public.a".into(), "public.b".into(), "public.a".into()];
        s.scope.exclude = vec!["public.b".into()];
        assert_eq!(
            tables(&s).unwrap(),
            vec![TableRef {
                schema: "public".into(),
                name: "a".into()
            }]
        );
        s.scope.exclude = vec!["public.*".into()];
        assert!(tables(&s).unwrap().is_empty());
        for unsupported in ["pub*.*", "*.*", "public.?", "public.a*"] {
            s.scope.exclude = vec![unsupported.into()];
            assert!(tables(&s).is_err());
        }
    }

    #[test]
    fn grants_the_sync_mode_needs_are_reported_before_any_build() {
        let mut stack =
            oo_stack::parse(include_str!("../../../oo-stack/example.stack.json")).unwrap();
        stack.sources[0].sync.mode = "C5".into();
        let need = |name: &str, statement: Value| {
            json!({"name":name,"observed":"missing",
            "required":"visibility of all active sessions","dba_statement":statement})
        };
        let stats = need(
            "pg_read_all_stats",
            json!("GRANT pg_read_all_stats TO oo_reader;"),
        );
        let report = json!({"status":"blocked","probe":{"capabilities":{
            "catalog":{"state":"needs_setup","requirements":[stats.clone()]},
            "chunked_snapshot":{"state":"needs_setup","requirements":[stats.clone()]},
            "digest_diff":{"state":"needs_setup","requirements":[stats]},
            "log_cdc":{"state":"needs_setup","requirements":[
                need("REPLICATION", json!("ALTER ROLE oo_reader REPLICATION;"))]}}}});
        let grants = required_grants(&stack, std::slice::from_ref(&report));
        // One statement, named once, for the capabilities this mode needs; C1's is not asked.
        assert_eq!(grants.len(), 1);
        assert_eq!(
            grants[0]["dba_statement"],
            "GRANT pg_read_all_stats TO oo_reader;"
        );
        assert_eq!(grants[0]["source_id"], json!(stack.sources[0].id));
        assert!(grants_text(&grants).contains("GRANT pg_read_all_stats TO oo_reader;"));
        stack.sources[0].sync.mode = "C1".into();
        let names: Vec<_> = required_grants(&stack, &[report])
            .iter()
            .map(|g| g["requirement"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["pg_read_all_stats", "REPLICATION"]);
        let ready = json!({"probe":{"capabilities":{"catalog":{"state":"available"}}}});
        assert!(required_grants(&stack, &[ready]).is_empty());
    }
}
