use oo_connectors::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
    engine::{ReadBudget, SourceEngine, TableRef},
    postgres::PostgresEngine,
};
use oo_pipeline::{engine_load, engine_load_pg::PgTarget};
use oo_stack::{secret::SecretStore, stack::Source};
use serde::Deserialize;
use sqlx::{
    ConnectOptions, Connection,
    postgres::{PgConnectOptions, PgConnection, PgSslMode},
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

mod c5;
mod sync;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetConfig {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: oo_stack::stack::SecretRef,
    tls_ca: Option<oo_stack::stack::SecretRef>,
    /// Explicit permission for a loopback-only synthetic target without TLS.
    #[serde(default)]
    local_fixture: bool,
    run_id: String,
}

fn read(path: &str) -> Result<Vec<u8>, &'static str> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "config_open")?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| "config_read")?;
    if bytes.len() > 1_048_576 {
        return Err("config_budget");
    }
    Ok(bytes)
}

fn binding(source: &Source) -> Result<String, &'static str> {
    serde_json::to_string(&(
        &source.id,
        &source.engine,
        &source.host,
        source.port,
        &source.database,
        &source.user,
    ))
    .map_err(|_| "source_binding")
}

async fn source_control_connection(
    source: &Source,
    store: &SecretStore,
) -> Result<PgConnection, String> {
    let options = PgConnectOptions::new()
        .host(&source.host)
        .port(source.port)
        .database(&source.database)
        .username(&source.user)
        .password(&store.read(&source.password.secret_ref)?)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(
            store
                .read(
                    &source
                        .tls_ca
                        .as_ref()
                        .ok_or("source_tls_required")?
                        .secret_ref,
                )?
                .into_bytes(),
        )
        .options([
            ("statement_timeout", "30000"),
            ("default_transaction_read_only", "on"),
        ])
        .disable_statement_logging();
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| "sync_source_connect".into())
}

async fn target_connection(
    target: &TargetConfig,
    store: &SecretStore,
) -> Result<PgConnection, String> {
    let password = store.read(&target.password.secret_ref)?;
    let mut options = PgConnectOptions::new()
        .host(&target.host)
        .port(target.port)
        .database(&target.database)
        .username(&target.user)
        .password(&password)
        .disable_statement_logging();
    if let Some(ca) = &target.tls_ca {
        options = options
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(store.read(&ca.secret_ref)?.into_bytes());
    } else if target.local_fixture && matches!(target.host.as_str(), "127.0.0.1" | "::1") {
        options = options.ssl_mode(PgSslMode::Disable);
    } else {
        return Err("target_tls_required".into());
    }
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| "target_connect".into())
}

fn selected(source: &Source, requested: Option<&String>) -> Result<Vec<TableRef>, &'static str> {
    let names: Vec<_> = requested
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|| source.scope.include.clone());
    let matches = |pattern: &str, name: &str| {
        pattern == "*"
            || pattern == name
            || pattern
                .strip_suffix(".*")
                .is_some_and(|p| name.starts_with(&format!("{p}.")))
    };
    let mut tables = Vec::new();
    for name in names {
        if name.contains(['*', '?']) {
            return Err("explicit_tables_required");
        }
        if !source.scope.include.iter().any(|s| matches(s, &name))
            || source.scope.exclude.iter().any(|s| matches(s, &name))
        {
            return Err("table_outside_scope");
        }
        let (schema, name) = name
            .split_once('.')
            .filter(|(s, t)| !s.is_empty() && !t.is_empty())
            .ok_or("qualified_table_required")?;
        let table = TableRef {
            schema: schema.into(),
            name: name.into(),
        };
        if tables.contains(&table) {
            return Err("duplicate_table");
        }
        tables.push(table);
    }
    if tables.is_empty() {
        return Err("tables_required");
    }
    Ok(tables)
}

async fn run_engine<E: SourceEngine>(
    engine: &mut E,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
) -> Result<(), String> {
    let duty = u32::from(budget.duty_cycle_percent);
    if !(1..=100).contains(&duty) {
        return Err("invalid_probe_duty_cycle".into());
    }
    let probe_started = std::time::Instant::now();
    let probe = engine.probe().await.map_err(|e| e.to_string())?;
    // Metadata reads also consume source budget on no-rewrite resumes.
    tokio::time::sleep(probe_started.elapsed() * (100 - duty) / duty).await;
    for table in tables {
        let mut conn = target_connection(target, store).await?;
        // Bind resume to the endpoint, not just a user-reusable display ID.
        let binding = binding(source)?;
        let mut sink = PgTarget::new(&mut conn, &binding, &target.run_id, table).await?;
        let (receipts, replay) = match sink.completed().await? {
            Some(receipts) => (receipts, true),
            None => (
                engine_load::load(engine, table, budget, oo_load::IpcSink::new, &mut sink).await?,
                false,
            ),
        };
        let bytes = sink.write_bytes().await?;
        println!(
            "{}",
            serde_json::json!({
                "protocol":"oo.load/v1", "source":source.id, "family":probe.family, "version":probe.version,
                "table":table, "target":sink.relation(), "run_id":target.run_id,
                "receipts":receipts, "replay":replay, "target_write_bytes":bytes,
                "read_budget":budget,
                "write_basis":"committed_logical_stage_tuples_including_replaced_plans",
                "publication":"local_table_view", "gateway_published":false,
                "consistency":"B", "source_time":"unmeasured"
            })
        );
        drop(sink);
        conn.close().await.map_err(|_| "target_close")?;
    }
    Ok(())
}

async fn execute() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut opts = BTreeMap::new();
    while let Some(key) = args.next() {
        if key == "--help" {
            println!(
                "oo-load --stack FILE --source ID [--tables schema.table,...] --target FILE\n\
                [--secrets-dir DIR] [--chunk-rows N] [--oracle-worker EXECUTABLE]\n\
                [--sync FILE] PostgreSQL C1 from an explicitly aligned baseline; see README.\n\
                [--c5 FILE] PostgreSQL C5 range cycle over an existing snapshot.\n\
                Target JSON requires run_id and password.secret_ref. Same run_id resumes; new run_id refreshes.\n\
                Local table publication only; not Gateway publication. See tools/oo-load/README.md."
            );
            return Ok(());
        }
        if ![
            "--stack",
            "--source",
            "--tables",
            "--target",
            "--secrets-dir",
            "--chunk-rows",
            "--oracle-worker",
            "--sync",
            "--c5",
        ]
        .contains(&key.as_str())
            || opts.contains_key(&key)
        {
            return Err("arguments".into());
        }
        opts.insert(key, args.next().ok_or("argument_value")?);
    }
    let stack = oo_stack::parse(
        std::str::from_utf8(&read(opts.get("--stack").ok_or("stack_required")?)?)
            .map_err(|_| "stack_utf8")?,
    )?;
    let source = stack
        .sources
        .iter()
        .find(|s| Some(&s.id) == opts.get("--source"))
        .ok_or("source_required")?;
    if opts.contains_key("--sync") && opts.contains_key("--c5") {
        return Err("sync_modes_conflict".into());
    }
    if (opts.contains_key("--sync") || opts.contains_key("--c5")) && source.engine != "postgres" {
        return Err("sync_engine_not_integrated".into());
    }
    if source.ssh.is_some() {
        return Err("ssh_not_supported".into());
    }
    let tables = selected(source, opts.get("--tables"))?;
    let target: TargetConfig =
        serde_json::from_slice(&read(opts.get("--target").ok_or("target_required")?)?)
            .map_err(|_| "target_config")?;
    let store = match opts.get("--secrets-dir") {
        Some(dir) => SecretStore::open(PathBuf::from(dir))?,
        None => SecretStore::from_home()?,
    };
    let mut budget = ReadBudget::default();
    if let Some(rows) = opts.get("--chunk-rows") {
        budget.max_chunk_rows = rows.parse().map_err(|_| "chunk_rows")?;
        if budget.max_chunk_rows == 0 || budget.max_chunk_rows > 100_000 {
            return Err("chunk_rows".into());
        }
    }
    let password = store.read(&source.password.secret_ref)?;
    match source.engine.as_str() {
        "postgres" => {
            let config = (|| {
                PgSourceConfig::try_new(
                    SourceId::new(&source.id)?,
                    PgEndpoint::new(&source.host, source.port, &source.database)?,
                    PgCredentials::new(source.user.clone(), password)?,
                    SecretRef::new(format!("secret://{}", source.password.secret_ref))?,
                    PgTlsConfig::from_ca_pem(
                        store
                            .read(
                                &source
                                    .tls_ca
                                    .as_ref()
                                    .ok_or_else(|| {
                                        oo_connectors::SourceError::protocol("tls_ca_required")
                                    })?
                                    .secret_ref,
                            )
                            .map_err(|_| oo_connectors::SourceError::protocol("tls_ca_read"))?,
                    )?,
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
            if let Some(path) = opts.get("--sync") {
                return sync::run(
                    config,
                    source,
                    &tables,
                    &target,
                    &store,
                    &budget,
                    serde_json::from_slice(&read(path)?).map_err(|_| "sync_config")?,
                )
                .await;
            }
            if let Some(path) = opts.get("--c5") {
                return c5::run(
                    PostgresEngine::new(config),
                    source,
                    &tables,
                    &target,
                    &store,
                    &budget,
                    serde_json::from_slice(&read(path)?).map_err(|_| "c5_config")?,
                )
                .await;
            }
            // Physical ctid plans require a live exported snapshot. Keep its
            // read-only owner alive until every table has finished or failed.
            let ca = store.read(
                &source
                    .tls_ca
                    .as_ref()
                    .ok_or("source_tls_required")?
                    .secret_ref,
            )?;
            let options = PgConnectOptions::new()
                .host(&source.host)
                .port(source.port)
                .database(&source.database)
                .username(&source.user)
                .password(&store.read(&source.password.secret_ref)?)
                .ssl_mode(PgSslMode::VerifyFull)
                .ssl_root_cert_from_pem(ca.into_bytes())
                .disable_statement_logging();
            let mut owner = PgConnection::connect_with(&options)
                .await
                .map_err(|_| "snapshot_connect")?;
            sqlx::query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
                .execute(&mut owner)
                .await
                .map_err(|_| "snapshot_begin")?;
            let snapshot: String = sqlx::query_scalar("SELECT pg_export_snapshot()")
                .fetch_one(&mut owner)
                .await
                .map_err(|_| "snapshot_export")?;
            let mut engine = PostgresEngine::new(config)
                .with_snapshot(snapshot)
                .map_err(|e| e.to_string())?;
            let result = run_engine(&mut engine, source, &tables, &target, &store, &budget).await;
            owner.close().await.map_err(|_| "snapshot_close")?;
            result
        }
        "oracle" => {
            use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine};
            let mut config = OracleConnectConfig::new(
                &source.host,
                source.port,
                &source.database,
                &source.user,
                zeroize::Zeroizing::new(password),
            );
            config.scope = tables.clone();
            let mut engine = match opts.get("--oracle-worker") {
                Some(worker) => {
                    OracleEngine::connect_worker(config, &mut std::process::Command::new(worker))
                }
                None => OracleEngine::connect(config),
            }
            .map_err(|e| e.to_string())?;
            run_engine(&mut engine, source, &tables, &target, &store, &budget).await
        }
        "mysql" => {
            use oo_connectors::mysql::{
                MysqlConfig,
                engine::{EngineOptions, MysqlEngine},
            };
            let ca = source.tls_ca.as_ref().ok_or("source_tls_required")?;
            // Validate the owner-private reference before passing its path to the driver.
            store.read(&ca.secret_ref)?;
            let root = opts
                .get("--secrets-dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".config/open-ontology/secrets")
                });
            let config = MysqlConfig {
                mysql_host: source.host.clone(),
                mysql_port: source.port,
                mysql_database: source.database.clone(),
                mysql_user: source.user.clone(),
                mysql_ca_file: root.join(&ca.secret_ref),
                mysql_tls_server_name: None,
                secret_ref: SecretRef::new(format!("secret://{}", source.password.secret_ref))
                    .map_err(|e| e.to_string())?,
                connect_timeout_ms: 10000,
            };
            let mut engine = MysqlEngine::new(
                config,
                zeroize::Zeroizing::new(password),
                EngineOptions {
                    source_id: source.id.clone(),
                    ..Default::default()
                },
            );
            run_engine(&mut engine, source, &tables, &target, &store, &budget).await
        }
        _ => Err("explicit_supported_engine_required".into()),
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = execute().await {
        eprintln!("{}", serde_json::json!({"status":"failed","error":error}));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_override_cannot_escape_stack_scope() {
        let source: Source = serde_json::from_value(serde_json::json!({
            "id":"fixture","engine":"postgres","host":"localhost","port":5432,
            "database":"fixture","user":"reader","password":{"secret_ref":"reader"},
            "scope":{"include":["public.*"],"exclude":["public.private"]}
        }))
        .unwrap();
        assert_eq!(
            selected(&source, Some(&"public.orders".into()))
                .unwrap()
                .len(),
            1
        );
        for tables in [
            "other.orders",
            "public.private",
            "public.*",
            "public.orders,public.orders",
        ] {
            assert!(selected(&source, Some(&tables.into())).is_err());
        }
    }

    #[test]
    fn target_configuration_refuses_inline_passwords_and_unknown_fields() {
        let mut config = serde_json::json!({
            "host":"localhost","port":5432,"database":"agent","user":"loader",
            "password":{"secret_ref":"target"},"run_id":"r1"
        });
        assert!(serde_json::from_value::<TargetConfig>(config.clone()).is_ok());
        config["password"] = serde_json::json!("inline-value");
        assert!(serde_json::from_value::<TargetConfig>(config.clone()).is_err());
        config["password"] = serde_json::json!({"secret_ref":"target"});
        config["password_text"] = serde_json::json!("inline-value");
        assert!(serde_json::from_value::<TargetConfig>(config).is_err());
    }
}
