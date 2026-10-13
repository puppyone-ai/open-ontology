//! Calls the frozen connector `inspect_access` and writes its JSON.
//! The password is read from a file and is not printed.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::Duration;

use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef,
    SelectedTable, SourceId, TableSelection,
};
use zeroize::Zeroizing;

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let need = |name: &str| -> Option<String> {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let Some(source_id) = need("--source-id") else {
        return ExitCode::from(1);
    };
    let Some(host) = need("--host") else {
        return ExitCode::from(1);
    };
    let Some(port_s) = need("--port") else {
        return ExitCode::from(1);
    };
    let Some(database) = need("--database") else {
        return ExitCode::from(1);
    };
    let Some(username) = need("--username") else {
        return ExitCode::from(1);
    };
    let Some(password_file) = need("--password-file") else {
        return ExitCode::from(1);
    };
    let Some(ca_file) = need("--ca-file") else {
        return ExitCode::from(1);
    };
    let Some(secret_ref) = need("--secret-ref") else {
        return ExitCode::from(1);
    };
    let Some(schema) = need("--schema") else {
        return ExitCode::from(1);
    };
    let Some(table) = need("--table") else {
        return ExitCode::from(1);
    };
    let Some(out) = need("--out") else {
        return ExitCode::from(1);
    };
    let Ok(port) = port_s.parse::<u16>() else {
        return ExitCode::from(1);
    };
    let Ok(password_raw) = fs::read_to_string(&password_file) else {
        eprintln!("password file unreadable");
        return ExitCode::from(1);
    };
    let password = Zeroizing::new(password_raw.trim_end_matches(['\n', '\r']).to_string());
    let Ok(ca) = fs::read(&ca_file) else {
        return ExitCode::from(1);
    };
    let config = match build(&ReportArgs {
        source_id: &source_id,
        host: &host,
        port,
        database: &database,
        username: &username,
        password: &password,
        secret_ref: &secret_ref,
        ca: &ca,
        schema: &schema,
        table: &table,
    }) {
        Ok(config) => config,
        Err(()) => {
            eprintln!("config rejected");
            return ExitCode::from(1);
        }
    };
    let connector = PgConnector::new(config);
    let mut session = match connector.connect().await {
        Ok(session) => session,
        Err(_) => {
            eprintln!("connect_failed");
            return ExitCode::from(1);
        }
    };
    let report = match session.inspect_access().await {
        Ok(report) => report,
        Err(_) => {
            eprintln!("inspect_failed");
            return ExitCode::from(1);
        }
    };
    drop(password);
    match serde_json::to_vec_pretty(&report) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            if fs::write(&out, &bytes).is_err() {
                return ExitCode::from(1);
            }
            println!("wrote {out}");
            ExitCode::from(0)
        }
        Err(_) => ExitCode::from(1),
    }
}

struct ReportArgs<'a> {
    source_id: &'a str,
    host: &'a str,
    port: u16,
    database: &'a str,
    username: &'a str,
    password: &'a str,
    secret_ref: &'a str,
    ca: &'a [u8],
    schema: &'a str,
    table: &'a str,
}

fn build(args: &ReportArgs<'_>) -> Result<PgSourceConfig, ()> {
    let source_id = SourceId::new(args.source_id).map_err(|_| ())?;
    let endpoint = PgEndpoint::new(args.host, args.port, args.database).map_err(|_| ())?;
    let credentials = PgCredentials::new(args.username, args.password).map_err(|_| ())?;
    let secret_ref = SecretRef::new(args.secret_ref).map_err(|_| ())?;
    let tls = PgTlsConfig::from_ca_pem(args.ca).map_err(|_| ())?;
    let budget = PgBudget::new(
        Duration::from_secs(30),
        Duration::from_secs(30),
        Duration::from_secs(5),
        1000,
        1_048_576,
    )
    .map_err(|_| ())?;
    let selection =
        TableSelection::new(vec![SelectedTable::new(args.schema, args.table).map_err(|_| ())?])
            .map_err(|_| ())?;
    PgSourceConfig::try_new(
        source_id,
        endpoint,
        credentials,
        secret_ref,
        tls,
        budget,
        selection,
    )
    .map_err(|_| ())
}
