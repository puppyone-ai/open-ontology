//! Read-only diagnostics. A failed source never erases the other probe reports.
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use oo_connectors::engine::{EngineFamily, TableRef};
use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine};
use oo_connectors::postgres::PostgresEngine;
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
}

fn registry() -> BTreeMap<EngineFamily, Adapter> {
    BTreeMap::from([
        (EngineFamily::Postgres, Adapter::Postgres),
        (EngineFamily::Kingbase, Adapter::Postgres),
        (EngineFamily::Mysql, Adapter::Mysql),
        (EngineFamily::Oracle, Adapter::Oracle),
        (EngineFamily::Sqlserver, Adapter::Sqlserver),
        (EngineFamily::Db2, Adapter::Db2),
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

/// One-shot `oo-mysql-doctor` from the pinned lite runtime image. It sees only
/// the owner's secret store, read-only; a loopback source is this host.
fn runtime_mysql_doctor(source: &mut Value) -> Result<Command, &'static str> {
    let image = super::lite::pinned_image();
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
        .args(["--env", "HOME=/home/oo"])
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

fn oracle(
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
                    "oo.lane=a",
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

fn family(source: &Source) -> Result<EngineFamily, &'static str> {
    let name = source.engine.split('-').next().unwrap_or("");
    match name {
        "postgres" | "postgresql" => Ok(EngineFamily::Postgres),
        "mysql" => Ok(EngineFamily::Mysql),
        "oracle" => Ok(EngineFamily::Oracle),
        "sqlserver" => Ok(EngineFamily::Sqlserver),
        "db2" => Ok(EngineFamily::Db2),
        "kingbase" => Ok(EngineFamily::Kingbase),
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

fn tables(source: &Source) -> Result<Vec<TableRef>, &'static str> {
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

fn postgres(
    source: &Source,
    tables: &[TableRef],
    store: &SecretStore,
    tunnel: Option<std::sync::Arc<oo_connectors::transport::ssh::Tunnel>>,
) -> Result<PostgresEngine, String> {
    let ca = source.tls_ca.as_ref().ok_or("SOURCE_TLS_CA_REQUIRED")?;
    let password = store.read(&source.password.secret_ref)?;
    let ca = store.read(&ca.secret_ref)?;
    let config = (|| {
        PgSourceConfig::try_new(
            SourceId::new(&source.id)?,
            PgEndpoint::new(&source.host, source.port, &source.database)?,
            PgCredentials::new(source.user.clone(), password)?,
            SecretRef::new(format!("secret://{}", source.password.secret_ref))?,
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
    let config = match tunnel {
        Some(tunnel) => config.with_tunnel(tunnel).map_err(|e| e.to_string())?,
        None => config,
    };
    Ok(if family(source) == Ok(EngineFamily::Kingbase) {
        PostgresEngine::new_kingbase(config)
    } else {
        PostgresEngine::new(config)
    })
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
                    | Adapter::Db2,
                ) => (),
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
                let tunnel = if family == EngineFamily::Mysql {
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
                            EngineFamily::Mysql => mysql(source, &selected),
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
                            && family != EngineFamily::Mysql
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

pub(super) fn run(stack: &Stack, json_output: bool) -> Result<Invocation, Failure> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Failure::local("DOCTOR_RUNTIME"))?;
    let sources = runtime.block_on(sources(stack));
    let model = model(stack);
    let blocked = sources.iter().any(|s| s["status"] != "ok") || model["status"] != "ok";
    let report = json!({"protocol":"oo.doctor/v1", "read_only":true,
        "status": if blocked {"blocked"} else {"ok"}, "sources":sources, "model":model});
    let output = if json_output {
        report.to_string()
    } else {
        format!(
            "Read-only preflight (DBA statements are advice, never executed).\n{}",
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
        s.engine = "opengauss".into();
        assert!(family(&s).is_err());
        s.engine = "kingbase".into();
        assert_eq!(family(&s).unwrap(), EngineFamily::Kingbase);
        assert!(matches!(
            registry()[&EngineFamily::Kingbase],
            Adapter::Postgres
        ));
        assert!(matches!(registry()[&EngineFamily::Mysql], Adapter::Mysql));
        assert!(matches!(registry()[&EngineFamily::Oracle], Adapter::Oracle));
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
}
