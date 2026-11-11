//! Feature-isolated helper: references on stdin, never credential values in argv.
use oo_connectors::engine::{ReadBudget, SourceEngine, TableRef};
use oo_connectors::mysql::{EngineOptions, MysqlConfig, MysqlEngine};
use oo_stack::{secret::SecretStore, stack::Source};
use serde_json::{Value, json};
use std::io::{self, Read};

#[path = "../../../../crates/oo-cli/src/admin/inspect.rs"]
mod inspection;

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
    let ca = source.tls_ca.as_ref().ok_or("SOURCE_TLS_CA_REQUIRED")?;
    let store = match value["secrets_dir"].as_str() {
        Some(path) => SecretStore::open(path)?,
        None => SecretStore::from_home()?,
    };
    // Validate the file using the private-store rules before handing its path
    // to the existing connector, whose TLS client verifies the hostname.
    store.read(&ca.secret_ref)?;
    let config = MysqlConfig {
        mysql_host: source.host.clone(),
        mysql_port: source.port,
        mysql_database: source.database.clone(),
        mysql_user: source.user.clone(),
        mysql_ca_file: store.path(&ca.secret_ref)?,
        mysql_tls_server_name: None,
        secret_ref: oo_connectors::SecretRef::new(format!(
            "secret://{}",
            source.password.secret_ref
        ))
        .map_err(|_| "SECRET_NAME")?,
        connect_timeout_ms: 10_000,
    };
    let options = EngineOptions {
        source_id: source.id.clone(),
        ..Default::default()
    };
    let mut engine = MysqlEngine::new(
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
