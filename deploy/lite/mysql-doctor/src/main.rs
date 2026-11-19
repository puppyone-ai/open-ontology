//! Feature-isolated helper: references on stdin, never credential values in argv.
use oo_connectors::engine::{EngineFamily, ReadBudget, SourceEngine, TableRef};
use oo_connectors::mysql::{EngineOptions, MysqlConfig, MysqlEngine};
use oo_connectors::olap::{OlapConfig, OlapEngine, OlapTls};
use oo_stack::{secret::SecretStore, stack::Source};
use serde_json::{Value, json};
use std::io::{self, Read};

#[path = "../../../../crates/oo-cli/src/admin/inspect.rs"]
mod inspection;

fn family_of(engine: &str) -> Result<EngineFamily, &'static str> {
    match engine.split('-').next().unwrap_or("") {
        // PolarDB MySQL is a deployment variant of the MySQL family.
        "mysql" | "polardb" | "auto" => Ok(EngineFamily::Mysql),
        "mariadb" => Ok(EngineFamily::Mariadb),
        "tidb" => Ok(EngineFamily::Tidb),
        "oceanbase" => Ok(EngineFamily::Oceanbase),
        "clickhouse" => Ok(EngineFamily::Clickhouse),
        "doris" => Ok(EngineFamily::Doris),
        "starrocks" => Ok(EngineFamily::Starrocks),
        _ => Err("ENGINE_NOT_AVAILABLE"),
    }
}

fn is_olap(family: &EngineFamily) -> bool {
    matches!(
        family,
        EngineFamily::Clickhouse | EngineFamily::Doris | EngineFamily::Starrocks
    )
}

fn run() -> Result<Value, &'static str> {
    let mut body = String::new();
    io::stdin()
        .take(1_048_577)
        .read_to_string(&mut body)
        .map_err(|_| "MYSQL_DOCTOR_INPUT")?;
    if body.len() > 1_048_576 {
        return Err("MYSQL_DOCTOR_INPUT");
    }
    let value: Value = serde_json::from_str(&body).map_err(|_| "MYSQL_DOCTOR_INPUT")?;
    let source: Source =
        serde_json::from_value(value["source"].clone()).map_err(|_| "MYSQL_DOCTOR_INPUT")?;
    let tables: Vec<TableRef> =
        serde_json::from_value(value["tables"].clone()).map_err(|_| "MYSQL_DOCTOR_INPUT")?;
    let family = family_of(&source.engine)?;
    let loopback = matches!(source.host.as_str(), "localhost" | "127.0.0.1" | "::1" | "host.docker.internal");
    // Analytic engines may be plaintext only on this host's own loopback.
    let ca = match (&source.tls_ca, is_olap(&family) && loopback) {
        (Some(ca), _) => Some(ca),
        (None, true) => None,
        (None, false) => return Err("SOURCE_TLS_CA_REQUIRED"),
    };
    let store = match value["secrets_dir"].as_str() {
        Some(path) => SecretStore::open(path)?,
        None => SecretStore::from_home()?,
    };
    // Validate the file using the private-store rules before handing its path
    // to the existing connector, whose TLS client verifies the hostname.
    let ca_file = match ca {
        Some(ca) => {
            store.read(&ca.secret_ref)?;
            Some(store.path(&ca.secret_ref)?)
        }
        None => None,
    };
    if is_olap(&family) {
        let config = OlapConfig {
            host: source.host.clone(),
            port: source.port,
            database: source.database.clone(),
            user: source.user.clone(),
            tls: ca_file.map(|ca_file| OlapTls { ca_file, server_name: None }),
            connect_timeout_ms: 10_000,
        };
        let mut engine = OlapEngine::new(
            family,
            config,
            store.read(&source.password.secret_ref)?.into(),
            ReadBudget::default(),
        )
        .map_err(|_| "OLAP_CONFIG")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "MYSQL_DOCTOR_RUNTIME")?;
        return Ok(runtime.block_on(inspection::inspect(&mut engine, &tables, &source.sync.mode)));
    }
    let ca_file = ca_file.ok_or("SOURCE_TLS_CA_REQUIRED")?;
    let config = MysqlConfig {
        mysql_host: source.host.clone(),
        mysql_port: source.port,
        mysql_database: source.database.clone(),
        mysql_user: source.user.clone(),
        mysql_ca_file: ca_file,
        mysql_tls_server_name: None,
        // The connector receives resolved bytes, not a file:/env: locator.
        secret_ref: oo_connectors::SecretRef::new("secret://source-password")
            .map_err(|_| "SECRET_NAME")?,
        connect_timeout_ms: 10_000,
    };
    let options = EngineOptions {
        source_id: source.id.clone(),
        ..Default::default()
    };
    let mut engine = MysqlEngine::new_family(
        family,
        config,
        store.read(&source.password.secret_ref)?.into(),
        options,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "MYSQL_DOCTOR_RUNTIME")?;
    runtime.block_on(async {
        let tunnel = oo_connectors::transport::ssh::connect(&source, &store).await?;
        if let Some(tunnel) = tunnel {
            engine = engine.with_tunnel(tunnel).map_err(|_| "SSH_TARGET")?;
        }
        if value["operation"] == "declared_catalog" {
            let catalog = engine
                .declared_catalog(&tables, &ReadBudget::default())
                .await
                .map_err(|error| error.fact().unwrap_or("SOURCE_CATALOG_FAILED"))?;
            return Ok(json!({"status":"ok","catalog":catalog}));
        }
        if value["operation"] == "discover" {
            // `oo init`: sign in, then the databases and tables it can see.
            // Catalog views and statistics only; table discovery may fail on
            // its own (reported) without failing the sign-in.
            let budget = ReadBudget::default();
            // The database list is the sign-in: it needs a session and no grant.
            let databases = engine
                .list_databases(&budget)
                .await
                .map_err(|error| error.fact().unwrap_or("SOURCE_SIGN_IN_FAILED"))?;
            let version = engine.probe().await.ok().map(|p| p.version.raw);
            let mut report = json!({"status":"ok","version":version,"databases":databases});
            match engine.discover_catalog(&budget).await {
                Ok(tables) => report["tables"] = json!(tables),
                Err(error) => {
                    report["tables_error"] = json!(error.fact().unwrap_or("SOURCE_CATALOG_FAILED"))
                }
            }
            return Ok(report);
        }
        let mut report = inspection::inspect(&mut engine, &tables, &source.sync.mode).await;
        if let Some(ssh) = &source.ssh {
            report["tunnel"] = json!({"status":"ok","host_key_sha256":ssh.host_key_sha256,
                "transport":"ssh-direct-tcpip"});
        }
        Ok(report)
    })
}

fn main() {
    let report = run().unwrap_or_else(|error| {
        let mut report = json!({"status":"blocked","error":error});
        if error.starts_with("SSH_") {
            report["tunnel"] = json!({"status":"blocked","error":error});
        }
        report
    });
    println!("{report}");
}
