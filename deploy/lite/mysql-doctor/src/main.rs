//! Feature-isolated helper: references on stdin, never credential values in argv.
use oo_connectors::engine::TableRef;
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
    if source.ssh.is_some() {
        return Err("SOURCE_SSH_NOT_AVAILABLE");
    }
    let ca = source.tls_ca.as_ref().ok_or("SOURCE_TLS_CA_REQUIRED")?;
    let store = SecretStore::from_home()?;
    // Validate the file using the private-store rules before handing its path
    // to the existing connector, whose TLS client verifies the hostname.
    store.read(&ca.secret_ref)?;
    let home = std::env::var_os("HOME").ok_or("SECRET_HOME")?;
    let config = MysqlConfig {
        mysql_host: source.host,
        mysql_port: source.port,
        mysql_database: source.database,
        mysql_user: source.user,
        mysql_ca_file: std::path::Path::new(&home)
            .join(".config/open-ontology/secrets")
            .join(&ca.secret_ref),
        mysql_tls_server_name: None,
        secret_ref: oo_connectors::SecretRef::new(format!(
            "secret://{}",
            source.password.secret_ref
        ))
        .map_err(|_| "SECRET_NAME")?,
        connect_timeout_ms: 10_000,
    };
    let options = EngineOptions {
        source_id: source.id,
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
    Ok(runtime.block_on(inspection::inspect(&mut engine, &tables, &source.sync.mode)))
}

fn main() {
    let report = run().unwrap_or_else(|error| json!({"status":"blocked","error":error}));
    println!("{report}");
}
