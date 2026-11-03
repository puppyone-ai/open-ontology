//! Read-only diagnostics. A failed source never erases the other probe reports.
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use oo_connectors::engine::{
    Capability, CapabilityState, EngineFamily, ProbeReport, ReadBudget, SourceEngine, TableRef,
};
use oo_connectors::postgres::PostgresEngine;
use oo_connectors::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
};
use oo_stack::{Stack, secret::SecretStore, stack::Source};
use serde_json::{Value, json};

use super::{Failure, Invocation};

#[derive(Clone, Copy)]
enum Adapter {
    Postgres,
    Unavailable,
}

fn registry() -> BTreeMap<EngineFamily, Adapter> {
    BTreeMap::from([
        (EngineFamily::Postgres, Adapter::Postgres),
        (EngineFamily::Mysql, Adapter::Unavailable),
        (EngineFamily::Oracle, Adapter::Unavailable),
    ])
}

fn family(source: &Source) -> Result<EngineFamily, &'static str> {
    let name = source.engine.split('-').next().unwrap_or("");
    match name {
        "postgres" | "postgresql" => Ok(EngineFamily::Postgres),
        "mysql" => Ok(EngineFamily::Mysql),
        "oracle" => Ok(EngineFamily::Oracle),
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
    Ok(PostgresEngine::new(config))
}

async fn inspect<E: SourceEngine>(engine: &mut E, tables: &[TableRef], mode: &str) -> Value {
    let probe = match engine.probe().await {
        Ok(probe) => probe,
        Err(error) => return json!({"status":"blocked","error":error.to_string()}),
    };
    let mut blocked = !available(&probe, Capability::ChunkedSnapshot)
        || (mode == "C1" && !available(&probe, Capability::LogCdc));
    let mut plans = Vec::new();
    for table in tables {
        match engine.plan_chunks(table, &ReadBudget::default()).await {
            Ok(plan) => plans.push(json!({
                "table": table, "strategy": plan.strategy, "key": plan.key,
                "chunks": plan.chunks.len(), "estimated_rows": plan.estimated_rows,
                "estimated_bytes": null, "bytes_basis": "not_exposed_by_source_engine"
            })),
            Err(error) => {
                blocked = true;
                plans.push(json!({"table":table,"error":error.to_string()}));
            }
        }
    }
    json!({
        "status": if blocked { "blocked" } else { "ok" },
        "probe": probe, "dba_requirements": probe.requirements(), "chunk_plans": plans,
    })
}

fn available(probe: &ProbeReport, capability: Capability) -> bool {
    matches!(
        probe.capabilities.get(&capability),
        Some(CapabilityState::Available)
    )
}

async fn sources(stack: &Stack) -> Vec<Value> {
    let registry = registry();
    let mut results = Vec::new();
    for source in &stack.sources {
        let setup = (|| {
            if source.ssh.is_some() {
                return Err("SOURCE_SSH_NOT_AVAILABLE".into());
            }
            let family = family(source)?;
            match registry.get(&family) {
                Some(Adapter::Postgres) => (),
                _ => return Err("engine not available in this build".into()),
            }
            let selection = tables(source);
            let selected = selection.as_ref().cloned().unwrap_or_default();
            let store = SecretStore::from_home()?;
            let engine = postgres(source, &selected, &store)?;
            Ok::<_, String>((engine, selected, selection.err()))
        })();
        let mut result = match setup {
            Ok((mut engine, selected, scope_error)) => {
                let mut report = inspect(&mut engine, &selected, &source.sync.mode).await;
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
    let runtime = tokio::runtime::Builder::new_current_thread()
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
        assert!(matches!(
            registry()[&EngineFamily::Mysql],
            Adapter::Unavailable
        ));
        assert!(matches!(
            registry()[&EngineFamily::Oracle],
            Adapter::Unavailable
        ));
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
