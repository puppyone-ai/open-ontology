use oo_connectors::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
    engine::{EngineFamily, ReadBudget, SourceEngine, TableRef},
    postgres::PostgresEngine,
};
use oo_connectors::{SourceError, engine};
use oo_pipeline::{engine_load, engine_load_pg::PgTarget};
use oo_stack::{secret::SecretStore, stack::Source};
use serde::Deserialize;
use sqlx::{
    ConnectOptions, Connection,
    postgres::{PgConnectOptions, PgConnection, PgSslMode},
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

mod c5;
mod documents;
mod db2_load;
mod epoch;
#[cfg(feature = "mysql-binlog")]
mod handoff;
mod log_sync;
mod olap;
mod sqlserver_load;
mod sync;
mod sync_command;
// The public connector export is coordinator-owned; compile the lane module here.
#[path = "../../../crates/oo-connectors/src/sqlserver/mod.rs"]
mod sqlserver;

fn sqlserver_config(
    source: &Source,
    tables: &[TableRef],
    password: String,
    ca_pem: Vec<u8>,
) -> Result<sqlserver::ConnectConfig, &'static str> {
    if source.engine != "sqlserver" || ca_pem.is_empty() || tables.is_empty() {
        return Err("sqlserver_tls_and_scope_required");
    }
    Ok(sqlserver::ConnectConfig {
        host: source.host.clone(),
        port: source.port,
        database: source.database.clone(),
        user: source.user.clone(),
        password: zeroize::Zeroizing::new(password),
        ca_pem,
        tables: tables.to_vec(),
        mode: None,
    })
}

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
    bounded(
        SOURCE_CONNECT_TIMEOUT,
        async {
            PgConnection::connect_with(&options)
                .await
                .map_err(|_| "sync_source_connect")
        },
        "sync_source_connect_timeout",
    )
    .await
    .map_err(String::from)
}

/// Client-side bounds for oo-load's own source control connection. The
/// server statement_timeout cannot fire when the network path is gone: a
/// connect or reply lost in a drop otherwise waits forever (T3 network drop,
/// PostgreSQL 13 C5 hung past 240 s in evidence n-s5-pg-01).
const SOURCE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SOURCE_CONTROL_TIMEOUT: Duration = Duration::from_secs(35);

async fn bounded<T>(
    limit: Duration,
    operation: impl std::future::Future<Output = Result<T, &'static str>>,
    code: &'static str,
) -> Result<T, &'static str> {
    tokio::time::timeout(limit, operation)
        .await
        .map_err(|_| code)?
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
        sink = documents::target(sink, source)?;
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

fn mysql_engine(
    family: EngineFamily,
    source: &Source,
    store: &SecretStore,
    opts: &BTreeMap<String, String>,
) -> Result<oo_connectors::mysql::engine::MysqlEngine, String> {
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
    let engine = MysqlEngine::new_family(
        family,
        config,
        zeroize::Zeroizing::new(store.read(&source.password.secret_ref)?),
        EngineOptions {
            source_id: source.id.clone(),
            ..Default::default()
        },
    );
    Ok(engine)
}

fn mysql_family(name: &str) -> Option<EngineFamily> {
    match name {
        "mysql" => Some(EngineFamily::Mysql),
        "mariadb" => Some(EngineFamily::Mariadb),
        "tidb" => Some(EngineFamily::Tidb),
        "oceanbase" => Some(EngineFamily::Oceanbase),
        _ => None,
    }
}

fn mysql_sync_family(family: EngineFamily) -> Result<(), &'static str> {
    match family {
        EngineFamily::Mysql | EngineFamily::Mariadb => Ok(()),
        EngineFamily::Tidb => Err("tidb_c1_unsupported_use_c5"),
        EngineFamily::Oceanbase => Err("oceanbase_c1_unsupported_use_c5"),
        _ => Err("explicit_supported_engine_required"),
    }
}

async fn execute() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1).peekable();
    let publish = arguments.peek().is_some_and(|a| a == "publish");
    let prune = arguments.peek().is_some_and(|a| a == "prune");
    if publish || prune {
        arguments.next();
    }
    let (sync_command, arguments) = sync_command::arguments(arguments);
    let mut args = arguments.into_iter();
    let mut opts = BTreeMap::new();
    while let Some(key) = args.next() {
        if key == "--help" {
            println!(
                "oo-load --stack FILE --source ID [--tables schema.table,...] --target FILE\n\
                [--secrets-dir DIR] [--chunk-rows N] [--oracle-worker EXECUTABLE] [--db2-worker EXECUTABLE]\n\
                [--sync FILE] Explicit epoch-bound C1; MySQL requires mysql-binlog; Oracle requires opt-in.\n\
                [--c5 FILE] PostgreSQL/MySQL/Oracle C5 range cycle over an existing snapshot.\n\
                [--handoff FILE] Atomic MySQL multi-source C5-to-C1 repair (mysql-binlog feature).\n\
                oo-load sync uses the same flags, requires an explicit sync mode and --run-id ID.\n\
                oo-load publish --generation ID --library ID uses --stack, --source and --target.\n\
                oo-load prune --library ID [--retain-seconds 86400] uses --stack, --source and --target.\n\
                It prepares a verified data cut; pending is NOT a Gateway-ready semantic publication.\n\
                --run-id ID overrides the target run_id; reuse it only to resume that cycle.\n\
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
            "--db2-worker",
            "--sync",
            "--c5",
            "--handoff",
            "--run-id",
            "--generation",
            "--library",
            "--retain-seconds",
        ]
        .contains(&key.as_str())
            || opts.contains_key(&key)
        {
            return Err("arguments".into());
        }
        opts.insert(key, args.next().ok_or("argument_value")?);
    }
    sync_command::validate(sync_command, &opts)?;
    if publish || prune {
        if [
            "--sync",
            "--c5",
            "--handoff",
            "--run-id",
            "--chunk-rows",
            "--oracle-worker",
            "--db2-worker",
        ]
        .iter()
        .any(|k| opts.contains_key(*k))
        {
            return Err("publication_arguments".into());
        }
        if (prune && opts.contains_key("--generation"))
            || (publish && opts.contains_key("--retain-seconds"))
        {
            return Err("publication_arguments".into());
        }
    } else if opts.contains_key("--generation") || opts.contains_key("--library") {
        return Err("publication_subcommand_required".into());
    } else if opts.contains_key("--retain-seconds") {
        return Err("prune_subcommand_required".into());
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
    sync_command::validate_engine(&source.engine, &opts)?;
    if ["--sync", "--c5", "--handoff"]
        .iter()
        .filter(|k| opts.contains_key(**k))
        .count()
        > 1
    {
        return Err("sync_modes_conflict".into());
    }
    if source.ssh.is_some()
        && ["--sync", "--c5", "--handoff"]
            .iter()
            .any(|key| opts.contains_key(*key))
    {
        return Err("ssh_incremental_control_not_integrated".into());
    }
    let tables = selected(source, opts.get("--tables"))?;
    let mut target: TargetConfig =
        serde_json::from_slice(&read(opts.get("--target").ok_or("target_required")?)?)
            .map_err(|_| "target_config")?;
    if let Some(run_id) = opts.get("--run-id") {
        target.run_id = run_id.clone();
    }
    let store = match opts.get("--secrets-dir") {
        Some(dir) => SecretStore::open(PathBuf::from(dir))?,
        None => SecretStore::from_home()?,
    };
    if prune {
        let age = opts
            .get("--retain-seconds")
            .map(|v| v.parse::<u32>())
            .transpose()
            .map_err(|_| "retention_policy")?
            .unwrap_or(86400);
        let mut conn = target_connection(&target, &store).await?;
        let result = oo_pipeline::engine_publication::release_expired(
            &mut conn,
            opts.get("--library").ok_or("library_required")?,
            age,
        )
        .await?;
        conn.close().await.map_err(|_| "publication_target_close")?;
        println!("{result}");
        return Ok(());
    }
    if publish {
        let request = oo_pipeline::engine_publication::PublicationRequest {
            library_id: opts.get("--library").ok_or("library_required")?.clone(),
            source_id: source.id.clone(),
            source_binding: binding(source)?,
            generation: opts
                .get("--generation")
                .ok_or("generation_required")?
                .clone(),
            tables,
        };
        let mut conn = target_connection(&target, &store).await?;
        let candidate =
            oo_pipeline::engine_publication::publish_generation(&mut conn, &request).await?;
        let result =
            oo_pipeline::engine_publication::activate_generation(&mut conn, &candidate).await;
        conn.close().await.map_err(|_| "publication_target_close")?;
        match result {
            Ok(result) => {
                println!("{result}");
                return Ok(());
            }
            Err(error) => {
                println!("{candidate}");
                return Err(error.into());
            }
        }
    }
    let mut budget = ReadBudget::default();
    if let Some(rows) = opts.get("--chunk-rows") {
        budget.max_chunk_rows = rows.parse().map_err(|_| "chunk_rows")?;
        if budget.max_chunk_rows == 0 || budget.max_chunk_rows > 100_000 {
            return Err("chunk_rows".into());
        }
    }
    if let Some(_path) = opts.get("--handoff") {
        #[cfg(feature = "mysql-binlog")]
        return handoff::run(
            &stack.sources,
            source,
            &target,
            &store,
            &budget,
            &opts,
            serde_json::from_slice(&read(_path)?).map_err(|_| "handoff_config")?,
        )
        .await;
        #[cfg(not(feature = "mysql-binlog"))]
        return Err("mysql_binlog_feature_required".into());
    }
    let password = store.read(&source.password.secret_ref)?;
    let tunnel = oo_connectors::transport::ssh::connect(source, &store).await?;
    match source.engine.as_str() {
        "db2" => db2_load::run(source, &tables, &target, &store, &budget, &opts).await,
        "sqlserver" => sqlserver_load::run(source, &tables, &target, &store, &budget, &opts).await,
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
            let config = match &tunnel {
                Some(tunnel) => config
                    .with_tunnel(tunnel.clone())
                    .map_err(|e| e.to_string())?,
                None => config,
            };
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
            let options = match &tunnel {
                Some(tunnel) => options.tcp_address(tunnel.address()),
                None => options,
            };
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
            if tunnel.is_some() && opts.contains_key("--oracle-worker") {
                return Err("ssh_external_worker_not_integrated".into());
            }
            let (host, port) = tunnel
                .as_ref()
                .map(|t| (t.address().ip().to_string(), t.address().port()))
                .unwrap_or_else(|| (source.host.clone(), source.port));
            let mut config = OracleConnectConfig::new(
                &host,
                port,
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
            if let Some(path) = opts.get("--sync") {
                return log_sync::oracle(
                    engine,
                    source,
                    &tables,
                    &target,
                    &store,
                    &budget,
                    serde_json::from_slice(&read(path)?).map_err(|_| "sync_config")?,
                    opts.get("--oracle-worker"),
                )
                .await;
            }
            if let Some(path) = opts.get("--c5") {
                return c5::run(
                    engine,
                    source,
                    &tables,
                    &target,
                    &store,
                    &budget,
                    serde_json::from_slice(&read(path)?).map_err(|_| "c5_config")?,
                )
                .await;
            }
            run_engine(&mut engine, source, &tables, &target, &store, &budget).await
        }
        family if mysql_family(family).is_some() => {
            let family = mysql_family(family).unwrap();
            if opts.contains_key("--sync") {
                mysql_sync_family(family)?;
            }
            let mut engine = mysql_engine(family, source, &store, &opts)?;
            if let Some(tunnel) = tunnel {
                engine = engine.with_tunnel(tunnel).map_err(|e| e.to_string())?;
            }
            if let Some(_path) = opts.get("--sync") {
                #[cfg(feature = "mysql-binlog")]
                return log_sync::mysql(
                    engine,
                    source,
                    &tables,
                    &target,
                    &store,
                    &budget,
                    serde_json::from_slice(&read(_path)?).map_err(|_| "sync_config")?,
                )
                .await;
                #[cfg(not(feature = "mysql-binlog"))]
                return Err("mysql_binlog_feature_required".into());
            }
            if let Some(path) = opts.get("--c5") {
                return c5::run(
                    engine,
                    source,
                    &tables,
                    &target,
                    &store,
                    &budget,
                    serde_json::from_slice(&read(path)?).map_err(|_| "c5_config")?,
                )
                .await;
            }
            let result = run_engine(&mut engine, source, &tables, &target, &store, &budget).await;
            engine.close().await;
            result
        }
        e if olap::family(e).is_some() => {
            olap::run(e, source, &tables, &target, &store, &budget, &opts).await
        }
        "mongodb" | "opensearch" | "elasticsearch" => {
            documents::run(source, &tables, &target, &store, &budget, &opts).await
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

    #[tokio::test]
    async fn source_control_operations_are_bounded_on_the_client() {
        let hung = bounded(
            Duration::from_millis(20),
            std::future::pending::<Result<(), &'static str>>(),
            "sync_lineage_timeout",
        );
        assert_eq!(hung.await, Err("sync_lineage_timeout"));
        let done = bounded(Duration::from_secs(1), async { Ok(7) }, "unused");
        assert_eq!(done.await, Ok(7));
        let failed = bounded(
            Duration::from_secs(1),
            async { Err::<(), _>("x") },
            "unused",
        );
        assert_eq!(failed.await, Err("x"));
    }

    #[test]
    fn sqlserver_arm_preserves_scope_and_requires_tls() {
        let source: Source = serde_json::from_value(serde_json::json!({
            "id":"fixture","engine":"sqlserver","host":"localhost","port":1433,
            "database":"fixture","user":"reader","password":{"secret_ref":"reader"},
            "scope":{"include":["dbo.fixture"],"exclude":[]}
        }))
        .unwrap();
        let tables = vec![TableRef {
            schema: "dbo".into(),
            name: "fixture".into(),
        }];
        assert!(sqlserver_config(&source, &tables, String::new(), vec![]).is_err());
        let config =
            sqlserver_config(&source, &tables, String::new(), b"test CA".to_vec()).unwrap();
        assert_eq!(config.tables, tables);
        assert_eq!(config.port, 1433);
        assert_eq!(config.mode, None);
    }

    #[test]
    fn family_sync_policy_is_explicit() {
        assert_eq!(mysql_sync_family(EngineFamily::Mysql), Ok(()));
        assert_eq!(mysql_sync_family(EngineFamily::Mariadb), Ok(()));
        assert_eq!(
            mysql_sync_family(EngineFamily::Tidb),
            Err("tidb_c1_unsupported_use_c5")
        );
        assert_eq!(
            mysql_sync_family(EngineFamily::Oceanbase),
            Err("oceanbase_c1_unsupported_use_c5")
        );
    }

    #[test]
    fn mysql_family_dispatch_is_explicit() {
        for (name, family) in [
            ("mysql", EngineFamily::Mysql),
            ("mariadb", EngineFamily::Mariadb),
            ("tidb", EngineFamily::Tidb),
            ("oceanbase", EngineFamily::Oceanbase),
        ] {
            assert_eq!(mysql_family(name), Some(family));
        }
        for name in ["auto", "postgres", "mysql-compatible", "MariaDB", ""] {
            assert_eq!(mysql_family(name), None);
        }
    }

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
